# Future Improvements

## Mode Switching Enhancements

- **Mouse-look controller** — Currently flat mode uses arrow keys for looking. Adding cursor lock + mouse look (right-click held) would feel much better.
- **Smooth transition** — Fade-to-black when switching modes to avoid jarring visual jumps.
- **UI overlay** — A small "Enter VR" / "Exit VR" button in the window for flat mode users.
- **Gamepad support** — Map controller input in flat mode so you can test with a gamepad.
- **Persist mode preference** — Save last-used mode to a config file.
- **Spectator camera system** — A free-fly spectator camera that others can view on the desktop while someone is in VR (great for streaming/demos).

## Mirror Blit Improvements

The mirror now uses a GPU blit (fullscreen triangle shader) instead of a re-render camera. Remaining work:

- Aspect ratio correction when XR resolution differs from window resolution (currently stretches)
- Letterboxing instead of stretch for mismatched aspect ratios
- Option to show both eyes side-by-side
- Support switching eyes at runtime (already works via `DesktopMirror::Eye(1)`)

## Runtime World Editor (F7)

Press F7 to open a second OS window with an editor UI — like a game engine editor while running (VR or flat mode).

- Entity hierarchy tree (WorldRoot children)
- Select entities, inspect/edit components (Transform, materials, lights, custom)
- Add/remove entities and components
- All edits go through World Journal (undoable, networkable, saveable)
- Bevy multiple windows + `bevy_egui` for the UI
- Bevy `Reflect` + `TypeRegistry` for generic component field editing
- NodeDescriptor reuse for copy/paste, prefab saving

## Modular Entity Controller System (avian3d + bevy-tnua)

### Design philosophy

Every capability is a **self-contained component + system pair**. No component assumes what entity it's on. Components work independently and combine naturally.

### Architecture: 3 layers, fully decoupled

```
Layer 1: INPUT DRIVERS (write intent, know nothing about physics or abilities)
Layer 2: INTENT (pure data components, no logic)
Layer 3: ABILITY SYSTEMS (read intent, apply behavior via physics)
```

### Layer 1 — Input Drivers

- `VrInputDriver` — reads OpenXR actions + hand/head tracking
- `FlatInputDriver` — reads keyboard/mouse/gamepad
- `NetworkInputDriver` — receives intent from network
- `AiInputDriver` — NPC behavior tree writes intent

All drivers write to the same `MovementIntent` component.

### Layer 2 — Intent Components (pure data, no logic)

- **`MovementIntent`** — `direction: Vec3`, `jump: bool`, `sprint: bool`, `crouch: bool`
- **`LookIntent`** — `rotation: Quat`
- **`InteractIntent`** — `primary: bool`, `secondary: bool`, `target: Option<Entity>`
- **`GrabIntent`** — `left_grab: bool`, `right_grab: bool`, `grab_point: Vec3`

### Layer 3 — Ability Components (self-contained, snap-on)

| Component | What it does | Works on |
|-----------|-------------|----------|
| `Locomotion` | Reads `MovementIntent`, feeds `TnuaController` | Any entity with RigidBody + Collider |
| `HeadFollow` | Reads `LookIntent`, drives head/camera rotation | Any entity with Transform |
| `HandTargets` | IK target positions (VR tracking or procedural) | Any entity with a skeleton |
| `Grabbable` | Makes an entity pickable by `Grabber` | Props, tools, items |
| `Grabber` | Reads `GrabIntent`, raycasts, attaches via joints | Hands, robot arms, tractor beams |
| `Interactable` | Responds to `InteractIntent` within range | Buttons, doors, NPCs |
| `Interactor` | Reads `InteractIntent`, finds nearby `Interactable` | Any controlled entity |
| `Climbable` | Marks a surface as climbable | Walls, ladders, vines |
| `Climber` | Reads `GrabIntent` near `Climbable`, overrides `Locomotion` | Any entity with hands |
| `Inventory` | Stores grabbed entities, manages slots | Players, NPCs, chests |
| `Seat` | Makes an entity sittable, overrides `Locomotion` | Chairs, vehicles |

### Physics foundation

- `TnuaController` bridges `MovementIntent` ↔ physics
- bevy-tnua: walking, jumping, crouching, slopes, coyote time, air control
- avian3d: RigidBody, Collider, gravity, collision events, joints

### Key rules

1. No component knows about input sources. `Locomotion` reads `MovementIntent`, never `KeyCode`.
2. No component knows what entity it's on. A `Grabber` works the same on a player hand, NPC, or crane.
3. Components don't depend on each other unless explicit `require()`.
4. Each ability is one file — component, system, plugin.
5. Adding a new ability never touches existing code.

### Example

```rust
// Player avatar
commands.spawn((
    RigidBody::Dynamic, Collider::capsule(0.3, 1.0),
    MovementIntent::default(), LookIntent::default(),
    InteractIntent::default(), GrabIntent::default(),
    Locomotion { speed: 5.0, jump_force: 8.0 },
    HeadFollow, HandTargets::default(),
    Grabber, Interactor { range: 2.0 }, Climber, Inventory::new(8),
));

// NPC — same physics, AI-driven
commands.spawn((
    RigidBody::Dynamic, Collider::capsule(0.3, 1.0),
    MovementIntent::default(), AiInputDriver::new(behavior_tree),
    Locomotion { speed: 3.0, jump_force: 6.0 }, Interactor { range: 1.5 },
));

// Grabbable prop
commands.spawn((
    RigidBody::Dynamic, Collider::cuboid(0.5, 0.5, 0.5),
    Grabbable { mass: 2.0 },
));
```

---

## World Journal — Status & Remaining Work

### DONE (Layer 1 core)

- [x] WorldEntityId (UUID) + WorldEntityRegistry (bidirectional HashMap)
- [x] NodeDescriptor + Reflect-based component serialization (MessagePack)
- [x] WorldRoot, LocalRoot, WorldJournal, WorldPermissions components
- [x] ActionRecord + ActionPayload (SetComponent, AddComponent, RemoveComponent, SetProperty, AddNode, DeleteNode, Reparent, Batch)
- [x] apply_payload + apply_and_record (Reflect-based ECS mutation with previous value capture)
- [x] Undo/redo with reverse action generation
- [x] Save/load round-trip (serialize_world, load_world, serialize_entity_tree, spawn_from_descriptor)
- [x] JournalPlugin: auto-register/unregister entities, JournalActionQueue, Ctrl+Z/Y
- [x] Hybrid serialization decided & implemented (Reflect for components, NodeDescriptor for hierarchy)
- [x] Multi-world hierarchy (WorldRoot/LocalRoot separation)
- [x] WorldPermissions with roles (Owner/Admin/Editor/Viewer/Blocked) and SavePolicy
- [x] 20 tests passing

### Next up

- **Save/load to disk** — Wire save_load.rs to file I/O. Format: `worlds/<world_id>/world.bin` (MessagePack) + `meta.toml`.
- **Iroh-Bevy async bridge** — Networking prototype. JournalSync drains outbox → Iroh Gossip broadcast. Inbound → JournalActionQueue.
- **Hybrid logical clocks** — Replace wall-clock timestamps with HLC for conflict detection (VR headsets have unreliable clocks).
- **Late join** — Stream NodeDescriptor tree to joining peers (lightweight, references asset hashes via Iroh Blobs).
- **Undo memory budget** — Currently count-limited (200). Consider LZ4 compression or memory budget, since DeleteNode captures entire subtrees.
- **Wire format optimization** — Strip `previous_value` from network messages (peers read it locally).
- **Cross-peer undo UX** — Confirmation prompt when undoing an action that others have modified since.
- **AssetStore** — Content-addressed hashing + Iroh Blobs for asset transfer.

### Architecture reference

**Action flow:**
```
1. User drags cube → system looks up WorldEntityId
2. Creates ActionRecord with SetProperty payload
3. apply_and_record: resolves ID → Entity, mutates ECS via Reflect, captures previous value
4. Journal: pushes to log + undo stack + outbox
5. JournalSync (future): drains outbox → Iroh Gossip broadcast
6. Peer receives: deserializes, resolves to THEIR local Entity, applies, records (not outbox)
7. Undo: pops undo stack, creates reverse action, applies, broadcasts
```

**NodeDescriptor serves all serialization needs:**
```
NodeDescriptor ←── save/load to disk (world.bin)
      ↑
      ├──────── AddNode action payload (journal)
      ├──────── DeleteNode undo snapshot (journal)
      └──────── late join world snapshot (sent over Iroh)
```

**File formats (decided):**
```
Save to disk:    glTF .glb with extensions (complete, portable, standard)
Late join:       NodeDescriptor tree (lightweight, references asset hashes)
Journal actions: Individual component diffs (SetProperty, AddNode, etc.)
Avatars:         VRM (glTF-based)
Binary format:   MessagePack (rmp-serde)
```

### ECS overhead summary

| Category | Cost | When |
|----------|------|------|
| WorldEntityId component | 32 bytes/entity | Always |
| Registry lookup | ~20-50ns | On action apply only |
| Reflect derivation | Zero at runtime | Metadata at registration |
| Serialize previous_value | ~100-500ns | Per action fired |
| Undo stack clone | 1 alloc/action | Per action (bounded 200) |
| Outbox push | Negligible | Per action |
| Full world serialize | ~1-10ms | Save only (async) |
| Full world deserialize | ~5-50ms | Load only |

No overhead on normal component reads/writes, LocalRoot entities, or frames with no mutations.

### Open questions

- Components from third-party plugins that don't derive `Reflect` — exclude or wrapper?
- Schema migration for old saves when component fields change
- Visual scripting graph format — custom or adopt existing (Behavior Trees, Unreal-style dataflow)?
- How visual scripts interact with the journal (batch actions? local-only with outputs journaled?)

---

## Authoring Layers Roadmap

```
Layer 1: Engine core        ← IN PROGRESS (types/journal done, file I/O + networking next)
   ↓
Layer 2: WASM mods          ← Partially exists (Wasvy loads modules, WIT bindings)
   ↓
Layer 3: In-app composable  ← Future (VR UI, component catalog, entity inspector)
   ↓
Layer 4: Visual scripting   ← Future (graph editor, node types, VR canvas)
```

**Layer 1** done when: spawn world, place objects, save/load to disk, two peers see each other's changes over Iroh.

**Layer 2** done when: modder writes a component in Rust/WASM, compiles, loads in MyceliumVR, and it saves/loads/networks without engine changes.

**Layer 3** done when: VR user creates a world, imports models, configures components, sets permissions, saves and shares — no code.

**Layer 4** done when: VR user wires "Player enters TriggerZone → Set Door.rotation to open" visually.
