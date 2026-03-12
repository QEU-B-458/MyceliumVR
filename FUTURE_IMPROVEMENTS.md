# Future Improvements

## Mode Switching Enhancements

- **Mouse-look controller** — Currently flat mode uses arrow keys for looking. Adding cursor lock + mouse look (right-click held) would feel much better.
- **Smooth transition** — Fade-to-black when switching modes to avoid jarring visual jumps.
- **UI overlay** — A small "Enter VR" / "Exit VR" button in the window for flat mode users.
- **Mirror resolution scaling** — Render the mirror camera at half-res to save GPU when running 3 cameras in VR.
- **Gamepad support** — Map controller input in flat mode so you can test with a gamepad.
- **Persist mode preference** — Save last-used mode to a config file.
- **Spectator camera system** — A free-fly spectator camera that others can view on the desktop while someone is in VR (great for streaming/demos).
- **Multi-window support** — Open a second window for the mirror view instead of sharing the primary window.

## Modular Entity Controller System (avian3d + bevy-tnua)

### Design philosophy

Every capability is a **self-contained component + system pair**. No component assumes what entity it's on — an NPC, a player avatar, a physics prop, a vehicle, anything. Components work independently and combine naturally. No "god controller" that knows about everything.

### Architecture: 3 layers, fully decoupled

```
Layer 1: INPUT DRIVERS (write intent, know nothing about physics or abilities)
Layer 2: INTENT (pure data components, no logic)
Layer 3: ABILITY SYSTEMS (read intent, apply behavior via physics)
```

### Layer 1 — Input Drivers

Each driver is a standalone plugin. Only one runs per controlled entity. Swapped by `AppMode` or per-entity override.

- `VrInputDriver` — reads OpenXR actions (thumbstick, grip, trigger) + hand/head tracking
- `FlatInputDriver` — reads keyboard/mouse/gamepad
- `NetworkInputDriver` — receives intent from network (multiplayer remote entities)
- `AiInputDriver` — NPC behavior tree writes intent (same system, AI-controlled)

All drivers write to the same `MovementIntent` component. The entity doesn't know or care where the intent came from.

### Layer 2 — Intent Components (pure data, no logic)

These go on any entity that needs to be controlled:

- **`MovementIntent`** — `direction: Vec3`, `jump: bool`, `sprint: bool`, `crouch: bool`
- **`LookIntent`** — `rotation: Quat` (where the entity wants to face)
- **`InteractIntent`** — `primary: bool`, `secondary: bool`, `target: Option<Entity>`
- **`GrabIntent`** — `left_grab: bool`, `right_grab: bool`, `grab_point: Vec3`

### Layer 3 — Ability Components (self-contained, snap-on)

Each is its own component + system. Add it to an entity to give it that ability. Remove it to take it away. No dependencies between abilities unless explicitly opted in.

| Component | What it does | Works on |
|-----------|-------------|----------|
| `Locomotion` | Reads `MovementIntent`, feeds `TnuaController` for physics walking/jumping | Any entity with RigidBody + Collider |
| `HeadFollow` | Reads `LookIntent`, positions a camera or drives head bone rotation | Any entity with Transform |
| `HandTargets` | Provides IK target positions (from VR tracking or procedural) | Any entity with a skeleton |
| `Grabbable` | Makes an entity pickable by anything with `Grabber` | Props, tools, items |
| `Grabber` | Reads `GrabIntent`, raycasts for `Grabbable` entities, attaches via joints | Hands, robot arms, tractor beams |
| `Interactable` | Responds to `InteractIntent` within range | Buttons, doors, NPCs |
| `Interactor` | Reads `InteractIntent`, finds nearby `Interactable` entities | Any controlled entity |
| `Climbable` | Marks a surface as climbable | Walls, ladders, vines |
| `Climber` | Reads `GrabIntent` near `Climbable` surfaces, overrides `Locomotion` | Any entity with hands |
| `Inventory` | Stores grabbed entities, manages slots | Players, NPCs, chests |
| `Seat` | Makes an entity sittable, overrides `Locomotion` while seated | Chairs, vehicles |

### Physics foundation (avian3d + bevy-tnua)

- `TnuaController` is the bridge between `MovementIntent` and physics
- bevy-tnua handles: walking, jumping, crouching, slopes, coyote time, air control
- avian3d handles: `RigidBody`, `Collider`, gravity, collision events, joints for grabbing
- Each ability system uses avian3d APIs directly (raycasts, overlap queries, joints) — no shared physics wrapper

### Key rules

1. **No component knows about input sources.** `Locomotion` reads `MovementIntent`, never `KeyCode`.
2. **No component knows what entity it's on.** A `Grabber` works the same on a player hand, an NPC, or a crane.
3. **Components don't depend on each other** unless there's an explicit `require()`. `Climber` can check for `Locomotion` to disable it while climbing, but works without it too.
4. **Each ability is one file** — component definition, system, plugin. Self-contained.
5. **Adding a new ability never touches existing code.** New file, new component, new system, register the plugin.

### Example: spawning a fully controllable entity

```rust
// Player avatar
commands.spawn((
    RigidBody::Dynamic,
    Collider::capsule(0.3, 1.0),
    MovementIntent::default(),
    LookIntent::default(),
    InteractIntent::default(),
    GrabIntent::default(),
    Locomotion { speed: 5.0, jump_force: 8.0 },
    HeadFollow,
    HandTargets::default(),
    Grabber,
    Interactor { range: 2.0 },
    Climber,
    Inventory::new(8),
));

// NPC with same physics but AI-driven
commands.spawn((
    RigidBody::Dynamic,
    Collider::capsule(0.3, 1.0),
    MovementIntent::default(),
    AiInputDriver::new(behavior_tree),
    Locomotion { speed: 3.0, jump_force: 6.0 },
    Interactor { range: 1.5 },
));

// Physics prop that can be grabbed
commands.spawn((
    RigidBody::Dynamic,
    Collider::cuboid(0.5, 0.5, 0.5),
    Grabbable { mass: 2.0 },
));
```

## World Journal System — Design Review Notes

Findings from reviewing the World Journal spec (March 2026).

### Critical: Entity ID mapping across peers

Bevy `Entity` is a local generational index — not stable across instances. The spec says `assigned_id` "must match on all peers" but doesn't define how. Need a `WorldEntityId` (UUID or `peer_id:counter`) with a local `HashMap<WorldEntityId, Entity>` mapping on each peer. This is load-bearing for every action type.

### Wire format: strip `previous_value`

Section 4.1 correctly says peers must read `previous_value` locally at receive time. But `ActionRecord` still serializes it over the wire. Either strip it from the wire format (add `#[serde(skip)]`) or document it as "hint only, never trusted." Sending stale `previous_value` from the author is misleading and wastes bandwidth.

### Timestamps: use hybrid logical clocks

Last-write-wins by wall clock is fragile — consumer VR headsets can have clock skew well beyond 500ms. Adding a Lamport or hybrid logical clock alongside the wall clock is cheap now and painful to retrofit. This affects conflict detection (section 10.3) too.

### Late join scalability

Full `WorldSnapshot` for late join won't scale for asset-heavy worlds. Consider streaming the NodeDescriptor tree incrementally or using delta snapshots. At minimum, document a size budget and compression strategy.

### Undo UX: cross-peer side effects

If peer A places an object and peer B moves it, then A undoes the place, the object is deleted — including B's move. Technically correct but will surprise users. Worth a UX note or confirmation prompt when undoing an action that other peers have modified since.

### Implementation order: prototype Iroh bridge first

The spec lists the Iroh-Bevy async bridge as step 4 but flags it as "the one real unknown." Recommend reordering to: types → Iroh bridge prototype → journal resource → sync → Bevy systems → WorldTree → persistence → assets. If the async bridge is awkward, it could change outbox and inbound feed design.

### Persistence: consider MessagePack over JSON

`journal.jsonl` is human-readable but slow for large sessions. Since the wire format already uses MessagePack, using it for persistence too would be more consistent and faster. JSON could be a debug/export option instead of the primary format.

### Multi-world hierarchy and local/networked split

The ECS scene tree should separate local (non-networked) entities from shared (journaled) world entities. Multiple worlds can be loaded simultaneously, each with its own journal.

**Hierarchy:**

```
SceneRoot (Bevy root)
├── LocalRoot              ← unjournaled: UI, debug gizmos, player rig, hand models
├── World("lobby")         ← own WorldJournal, own Iroh session/topic
│   ├── networked entity A
│   └── networked entity B
├── World("arena")         ← separate WorldJournal, separate session
│   ├── networked entity C
│   └── networked entity D
└── ...
```

**Design consequences:**

- `WorldJournal` is a **component on world root entities**, not a global resource. Systems query `(Entity, &mut WorldJournal)` and operate per-world.
- `LocalRoot` children never go through the journal. Exact contents TBD — likely candidates include user dashboard, debug menus, settings UI. The key point is the separation exists so non-networked entities have a home.
- Each world root has its own: `WorldJournal`, `WorldTree`, undo/redo stacks, outbox, Iroh Gossip topic, peer list, and `WorldEntityId` registry.
- **World switching** toggles visibility/activity on world sub-roots — no destroy/rebuild. A peer can be connected to multiple worlds simultaneously (e.g., lobby + game world during transition).
- Marker components on world roots: `WorldRoot { world_id: String, session_id: String }`. Children inherit world membership via Bevy's parent hierarchy.
- Actions must reference which world they belong to (add `world_id` to `ActionRecord` or scope by the Iroh topic).
- `AssetStore` remains global — assets are content-addressed and shared across worlds to avoid duplicate storage/transfers.

### World representation, save/load — solve this first

The serialization format is the keystone. It determines how worlds are saved to disk, sent to late-joining peers, described in AddNode/DeleteNode actions, and captured for undo snapshots. Get this right and everything else builds on it.

**Core problem:** given a world root entity and all its descendants, how do we serialize every entity's components into a portable format, and reconstruct them on another peer (or from disk)?

**What needs answering:**

1. **Component registry** — which components are serializable/networkable? Not everything should be (local render state, physics cache, etc.). Need a way to mark components as "world-serializable" — either a trait, a registration list, or Bevy's built-in reflection.
2. **Entity descriptors** — the `NodeDescriptor` from the spec. Each entity becomes: type name + serialized components + children + metadata. This is the atom of save/load and of AddNode actions.
3. **Asset references** — components that reference assets (meshes, textures, audio) store content hashes, not file paths. On load, the AssetStore resolves hashes to local cache paths.
4. **World file format** — a world on disk is a directory:
   ```
   worlds/<world_id>/
   ├── world.bin          ← serialized NodeDescriptor tree (MessagePack)
   ├── world.json         ← same tree as human-readable JSON (debug/export)
   ├── journal.bin        ← action log (MessagePack, optional for replay)
   └── meta.toml          ← world name, author, created date, schema version
   ```
5. **Load flow** — read `world.bin` → walk the descriptor tree → spawn entities with deserialized components → register each in `WorldEntityRegistry` → parent under the world root.
6. **Save flow** — walk all children of a world root → for each entity, serialize all registered components → build descriptor tree preserving hierarchy → write to `world.bin`.

**Bevy reflection as the backbone:**

Bevy's `Reflect` trait + `TypeRegistry` already solve a lot of this. Components that derive `Reflect` can be serialized/deserialized generically. The registry knows how to construct them from reflected data. This avoids hand-writing serialize/deserialize for every component type.

```rust
// Any component that should be saved/networked just needs:
#[derive(Component, Reflect, Serialize, Deserialize)]
#[reflect(Component)]
struct MyCustomThing {
    value: f32,
    color: Color,
}
```

Users making things in worlds would follow the same pattern — derive the right traits and their components automatically participate in save/load/journaling.

**Action flow walkthrough — one format, three jobs:**

A single user action (move a cube from origin to (1,2,3)) flows through the entire system:

```
1. INPUT: User drags cube. System looks up WorldEntityId: "peer_abc:42"

2. JOURNAL: Creates ActionRecord {
     target: "peer_abc:42",         ← WorldEntityId, not Bevy Entity
     payload: SetProperty { component: "Transform", prop_name: "translation",
              value: [1,2,3], previous_value: [0,0,0] (read from ECS now) }
   }

3. APPLY: Journal resolves "peer_abc:42" → Entity(47) via WorldEntityRegistry
   Sets transform, pushes to undo stack, pushes to outbox

4. NETWORK: JournalSync drains outbox → serializes → Iroh Gossip broadcast

5. PEER RECEIVES: Deserializes, resolves "peer_abc:42" → Entity(83) (THEIR local entity)
   Reads THEIR previous_value, applies, appends to journal (NOT outbox — received=true)

6. UNDO: Pops from undo stack, creates REVERSE action (value=[0,0,0]),
   applies locally, puts reverse in outbox → broadcasts as NEW action

7. SAVE: Walk world root children, serialize each entity's Reflect components →
   NodeDescriptor { world_entity_id: "peer_abc:42", components: [...], children: [] }

8. LOAD: Read descriptors, spawn entities, deserialize components,
   register WorldEntityId mappings. New local Entity, same WorldEntityId.
```

**WorldEntityId** is the glue — the one stable identifier in actions, descriptors, registries, and undo snapshots. **NodeDescriptor** is the one serialization format used for save files, late join snapshots, AddNode actions, and DeleteNode undo captures.

```
NodeDescriptor ←── save/load to disk (world.bin)
      ↑
      ├──────── AddNode action payload (journal)
      ├──────── DeleteNode undo snapshot (journal)
      └──────── late join world snapshot (sent over Iroh)
```

### ECS overhead analysis

The journal system adds runtime cost on top of vanilla Bevy ECS. Here's what that looks like and where it matters.

**Per-entity cost (constant, always present):**

| Overhead | Cost | Notes |
|---|---|---|
| `WorldEntityId` component | 32 bytes per entity (UUID) | Stored as a regular ECS component. No cache impact on queries that don't include it. |
| `WorldEntityRegistry` HashMap lookup | ~20-50ns per lookup | Only on action apply/receive, not every frame. `HashMap<WorldEntityId, Entity>` with typical FxHashMap. |
| `Reflect` trait derivation | Zero runtime cost when not serializing | Reflect adds type metadata at registration time, not per-frame. No impact on normal component access. |

**Per-action cost (only when mutations happen):**

| Overhead | Cost | Notes |
|---|---|---|
| Serialize previous_value | ~100-500ns depending on component size | Only on the frame the action fires, not every frame. Uses rmp-serde (MessagePack). |
| Clone action into undo stack | One allocation per action | Bounded by MAX_UNDO_DEPTH (200). Old entries dropped. |
| Outbox push | Negligible (VecDeque push) | Drained each network tick, not accumulated. |
| Action coalescing scan | O(outbox size) per SetProperty | Outbox is typically small (< 50 pending). Linear scan is fine. |

**Per-frame cost (systems that run every tick):**

| Overhead | Cost | Notes |
|---|---|---|
| JournalSync outbox drain | Proportional to actions this frame | Typically 0-5 actions per frame. Serialization + Iroh send. |
| WorldTree change detection | Bevy's built-in change detection | Nearly free — Bevy already tracks Added/Changed/Removed. Just reads the flags. |
| Inbound action processing | Proportional to received actions | Typically 0-10 per frame. Deserialize + apply + registry lookup. |

**Save/load cost (infrequent, can be async):**

| Overhead | Cost | Notes |
|---|---|---|
| Full world serialize | ~1-10ms for hundreds of entities | Walks hierarchy, serializes all Reflect components. Run on IoTaskPool, doesn't block frame. |
| Full world deserialize + spawn | ~5-50ms depending on entity count | Spawn is the bottleneck (Commands are deferred). Can spread across frames if needed. |
| Autosave (every 60s) | Atomic write: temp file → rename | Background IO, no frame impact. |

**Where it does NOT add overhead:**

- Normal component reads/writes in gameplay systems — no cost. Querying `&Transform` or `&mut Velocity` is the same as vanilla Bevy.
- Entities under `LocalRoot` — completely untouched by the journal system. No WorldEntityId, no registry, no overhead.
- Frames with no mutations — the journal systems early-out. If nobody moves anything, cost is effectively zero.

**The real concern:** not per-frame CPU, but **memory**. The undo stack stores serialized snapshots. A DeleteNode on a complex hierarchy captures the entire subtree. With MAX_UNDO_DEPTH=200 and large entities, this could be 10-50MB. Consider compressing undo snapshots or using a memory budget instead of a count limit.

**Memory concern — undo snapshots:** The undo stack stores serialized snapshots. A DeleteNode on a complex hierarchy captures the entire subtree. With MAX_UNDO_DEPTH=200 and large entities, this could be 10-50MB. Consider compressing undo snapshots (LZ4) or using a memory budget instead of a count limit.

### File formats: glTF for worlds, VRM for avatars

Rather than inventing a custom binary format, lean on existing standards:

**World save/load — glTF with extensions:**
- glTF (.glb) is already Bevy's native scene format. Meshes, materials, transforms, hierarchy — all handled.
- Custom components and journal metadata go in glTF extras/extensions (e.g. `"MYCELIUM_world"` extension for WorldEntityId, custom components, world metadata).
- Save = export world root subtree to .glb with extensions. Load = import .glb, parse extensions, register WorldEntityIds, parent under world root.
- Bonus: worlds are viewable in any glTF viewer even without MyceliumVR.

**Avatar format — VRM:**
- VRM is glTF-based, purpose-built for humanoid avatars. Includes bone mapping, blend shapes, first-person settings, material constraints.
- Well-supported in the VR/social ecosystem. Users likely already have VRM avatars.
- Load via Bevy's glTF loader + VRM extension parsing.

**Live world transfer (late join) — NOT the same as file save:**
- Saving to disk uses glTF (structured, complete, includes mesh data).
- Sending to a joining peer uses the NodeDescriptor snapshot format — just serialized component data + hierarchy. Mesh/texture assets are already in AssetStore and transferred separately via Iroh Blobs by content hash.
- The snapshot is lighter than a full glTF because it doesn't embed geometry — it references it by hash. The joining peer already has (or will request) the asset blobs.

```
Save to disk:    glTF .glb with extensions (complete, portable, standard)
Late join:       NodeDescriptor tree (lightweight, references asset hashes)
Journal actions: Individual component diffs (SetProperty, AddNode, etc.)
```

Three levels of granularity, same underlying WorldEntityId + component data, different packaging for different needs.

### World permissions — component on world root

Permissions attach to the world root as a component, same as WorldJournal and WorldTree. Controls who can modify, save, invite, and administer the world.

```rust
#[derive(Component, Reflect, Serialize, Deserialize)]
struct WorldPermissions {
    owner: String,                              // peer_id of world creator
    save_policy: SavePolicy,                    // who can save
    default_role: WorldRole,                    // role for new joiners
    peer_roles: HashMap<String, WorldRole>,     // per-peer overrides
}

enum SavePolicy {
    OwnerOnly,              // only owner can save
    Permitted(Vec<String>), // owner + listed peer_ids
    Anyone,                 // any editor can save their own copy
}

enum WorldRole {
    Owner,      // full control: edit, delete, save, grant roles, kick
    Admin,      // edit, delete, save, grant editor/viewer
    Editor,     // edit properties, add/remove entities, save if permitted
    Viewer,     // read-only, can look around but not modify
    Blocked,    // cannot join
}
```

**World root with all components:**
```rust
commands.spawn((
    WorldRoot { world_id: "lobby".into() },
    WorldJournal::default(),
    WorldEntityRegistry::default(),
    WorldTree::default(),
    WorldPermissions {
        owner: local_peer_id.clone(),
        save_policy: SavePolicy::OwnerOnly,
        default_role: WorldRole::Editor,
        peer_roles: HashMap::new(),
    },
    // Networking (added when connecting):
    // JournalSync { topic: ... },
    // PeerList::default(),
));
```

**Enforcement:**
- Permissions are checked locally before applying any action. If a peer sends an action they don't have rights for, the receiving peer discards it and logs a warning.
- `WorldPermissions` itself is synced via `SetMeta` actions — but only owner/admin can modify it. Other peers' attempts to change permissions are discarded.
- "Save" for a non-owner with `SavePolicy::Anyone` creates a **fork** — a new world file with a new `world_id` and the saving peer as owner. The original is unmodified.

### Authoring layers roadmap

The goal is a platform where users can build worlds at whatever depth they're comfortable with — from snapping together pre-made components in VR to writing full WASM mods. Each layer builds on the one below it.

**Layer 1 — Engine core (bare minimum, build first)**

The Rust components and systems that make worlds function. Everything else depends on these.

What to build:
- `WorldRoot`, `WorldJournal`, `WorldEntityRegistry`, `WorldTree`, `WorldPermissions`
- `NodeDescriptor` + Bevy `Reflect`-based component serialization
- Save/load (glTF with extensions)
- `ActionRecord` + `ActionPayload` types, apply/undo/redo
- Iroh-Bevy bridge, `JournalSync`, outbox, inbound processing
- `AssetStore` with content-addressed hashing + Iroh Blobs
- Built-in components: `Transform`, `Mesh`, `Material`, `Grabbable`, `Interactable`, `Collider`, etc.

Done when: you can spawn a world, place objects, save to disk, load from disk, and two peers can see each other's changes over Iroh.

**Layer 2 — Modder-friendly (WASM via Wasvy)**

Power users write components and systems in Rust (or any WASM-targeting language), compile to `.wasm`, and drop them into a mod folder. Already partially working — Wasvy loads guest modules and WIT bindings expose ECS.

What to build on top of Layer 1:
- Extend WIT interfaces so WASM guests can define new component types (not just access built-in ones like `Health`)
- WASM-defined components must participate in Reflect/serialize so they work with journal, save/load, and networking automatically
- Component metadata from WASM: display name, category, editable fields + types, default values — so the in-app UI (Layer 3) can show them without hardcoding
- Mod manifest format: name, version, component list, required permissions
- Sandboxing: WASM guests can't access filesystem, network, or other worlds directly

Done when: a modder can write a new component (e.g. `Scoreboard { scores: Vec<(String, u32)> }`), compile to WASM, load it in MyceliumVR, attach it to entities, and it saves/loads/networks correctly without any engine changes.

**Layer 3 — In-app composable (no code, VR UI)**

Users inside VR can build worlds by selecting entities and attaching/configuring components from a catalog. Like a Unity inspector but on your wrist or as a floating panel.

What to build on top of Layer 2:
- **Component catalog** — a registry of all available components (built-in + WASM mods) with metadata: name, description, category, editable fields, field types, default values, icon
- **Entity inspector UI** — select an entity in-world → see its components → add/remove components → edit field values. All changes go through the journal (so they're undoable, networkable, saveable)
- **Prefab system** — save an entity (with its components and children) as a reusable template. Drag from a library to stamp copies. Prefabs are just NodeDescriptors stored in AssetStore.
- **World settings panel** — configure world root components (permissions, physics settings, lighting, etc.) from UI
- **Asset browser** — browse AssetStore, import new assets (drag & drop or file picker), preview thumbnails

Done when: a user in VR can create a world from scratch, import a .glb model, make it grabbable and interactable, set permissions, save the world, and share it with friends — all without writing any code.

**Layer 4 — Visual scripting (future)**

Wire logic between components visually. "When this trigger fires, do that action." Builds on everything below — the script graph references components (Layer 3 catalog), compiles to something the journal can serialize and network, and gets saved with the world.

What to build on top of Layer 3:
- Graph data model: nodes (events, conditions, actions) + edges (data flow, execution flow)
- Built-in node types: component value read/write, math ops, comparisons, timers, events (on grab, on enter trigger, on collide, etc.)
- Graph editor UI in VR (floating node canvas, grab to connect wires)
- Script execution engine: interprets the graph each frame (or event-driven)
- Scripts are components too — `VisualScript { graph: ScriptGraph }` on the entity. Serializable, undoable, networkable like everything else.
- WASM mods can register custom node types for the visual scripting graph

Done when: a user in VR can make a door that opens when you walk near it, without writing code — just wiring "Player enters TriggerZone" → "Set Door.rotation to open" in a visual graph.

**Layer dependency chain:**
```
Layer 1: Engine core
   ↓ (components exist and can be serialized/networked/saved)
Layer 2: WASM mods
   ↓ (new components can be added without engine changes, with metadata for UI)
Layer 3: In-app composable
   ↓ (components can be attached and configured from VR UI)
Layer 4: Visual scripting
   (logic can be wired between components visually)
```

Each layer is usable on its own — you don't need Layer 4 to ship. Layer 1 + 2 is already a functional multiplayer world-building platform for technical users. Layer 3 opens it up to everyone.

### DECIDED: Hybrid serialization — Reflect for components, custom NodeDescriptor for hierarchy

**Decision:** Option C — use Bevy's `Reflect` + `TypeRegistry` for serializing individual component values, wrap them in our own `NodeDescriptor` struct for hierarchy, WorldEntityId, asset references, and metadata.

**Why not pure Bevy `DynamicScene` (Option A):**
- Can't control the format — adding WorldEntityId, asset hashes, metadata means fighting the scene system
- Hierarchy control is limited — parent/child is Bevy's internal `ChildOf`, no per-node metadata
- Can't easily filter non-networkable components (render cache, physics state)
- Coupled to Bevy's internal scene format across versions — saved worlds could break on Bevy upgrades

**Why not pure custom descriptors (Option B):**
- Requires hand-writing serialize/deserialize for every component — kills the modding story
- Loses the "derive Reflect and it just works" promise that makes Layer 2-4 possible

**How Option C works:**

```rust
// Serializing a component (uses Bevy Reflect — works for ANY Reflect component):
let type_registry = world.resource::<AppTypeRegistry>().read();
let serializer = ReflectSerializer::new(component_ref, &type_registry);
let bytes = rmp_serde::to_vec(&serializer)?;

// Deserializing:
let deserializer = ReflectDeserializer::new(&type_registry);
let reflected = deserializer.deserialize(&mut rmp_serde::Deserializer::new(&bytes[..]))?;
// Use ReflectComponent to insert onto entity

// NodeDescriptor owns the hierarchy — our code, our control:
struct NodeDescriptor {
    world_entity_id: WorldEntityId,
    components: Vec<(String, Vec<u8>)>,  // type name → Reflect-serialized bytes
    children: Vec<NodeDescriptor>,
    metadata: HashMap<String, Vec<u8>>,
}
```

**What this gives us:**
- Components just derive `Reflect` and participate automatically (preserves modding story)
- We own hierarchy format — WorldEntityId, asset hashes, filtering are first-class
- We control the binary format (MessagePack) and version it independently of Bevy
- Same format for save, network, journal, and undo — different levels of the tree
- ~200-300 lines of tree walking code to write (manageable)

**Remaining open questions:**
- How do we handle components from third-party plugins that don't derive `Reflect`? Exclude them? Wrapper components?
- Schema migration — when a component's fields change between versions, how do we load old saves? The `schema_version` + migration chain from the spec applies here too.
- Visual scripting graph format — custom or adopt an existing standard (e.g. Behavior Trees, or a dataflow graph like Unreal Blueprints)?
- How do visual scripts interact with the journal? Is each script tick's side effects a Batch action? Or are scripts local-only with their outputs journaled?
