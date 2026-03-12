# MyceliumVR

A social VR platform built with [Bevy](https://bevyengine.org/) and OpenXR. Create, share, and inhabit worlds together.

## What is MyceliumVR?

MyceliumVR is an open-source social VR application where users can build and explore shared 3D worlds. It uses a journaled ECS architecture — every mutation is tracked, undoable, and networkable — so worlds can be collaboratively edited in real time and saved/loaded reliably.

### Key features

- **VR + flat mode** — Run in VR with OpenXR or in flat-screen mode (`--flat` flag) for development and non-VR users
- **World Journal** — All entity mutations go through a journal system with full undo/redo, network sync readiness, and save/load
- **WASM modding** — Extend the engine with WebAssembly mods via [Wasvy](https://github.com/nickkuk/wasvy) (WIT component model)
- **Modular architecture** — Components are self-contained and composable on any entity
- **Desktop mirror** — GPU blit of VR eye texture to desktop window

## Getting started

### Prerequisites

- Rust (stable)
- An OpenXR runtime (SteamVR, Oculus, etc.) for VR mode
- Windows (primary target), Linux support planned

### Running

```bash
# VR mode (requires OpenXR runtime)
cargo run

# Flat-screen mode (no headset needed)
cargo run -- --flat
```

### Development

```bash
# Fast compile with dynamic linking
cargo run --features dev
```

## Architecture

```
src/
├── main.rs          # App setup, plugin registration, entity spawning
├── app_mode.rs      # VR/flat mode switching, desktop mirror config
├── oxr/             # OpenXR integration (session, rendering, hand tracking)
├── xr/              # XR abstractions (session state, render systems)
├── xr_utils/        # XR utilities (actions, tracking, transforms)
├── journal/          # World Journal system
│   ├── entity_id.rs  # WorldEntityId (UUID) + WorldEntityRegistry
│   ├── descriptor.rs # NodeDescriptor + component serialization
│   ├── action.rs     # ActionRecord + ActionPayload types
│   ├── world.rs      # WorldRoot, LocalRoot, WorldJournal, WorldPermissions
│   ├── apply.rs      # Apply actions to ECS via Reflect
│   ├── save_load.rs  # Serialize/deserialize entity trees
│   └── plugin.rs     # JournalPlugin (auto-register, undo/redo, action queue)
├── components.rs     # Game components (Health, etc.)
└── ...
```

### ECS hierarchy

```
SceneRoot
├── WorldRoot("default")     # Journaled/networked entities
│   ├── entity (ground)
│   ├── entity (cube)
│   └── entity (light)
└── LocalRoot                # Unjournaled (UI, cameras, debug)
    └── FlatModeCamera
```

### World Journal

Every entity under a `WorldRoot` gets a stable `WorldEntityId` (UUID) and is tracked in the `WorldJournal`. Actions (SetComponent, SetProperty, AddNode, DeleteNode, Reparent, etc.) are applied via Bevy's `Reflect` system, with automatic previous-value capture for undo.

- **Ctrl+Z** — Undo
- **Ctrl+Shift+Z / Ctrl+Y** — Redo

## Modding

WASM mods go in the `mods/` directory. See [crates/guest_wit_example](crates/guest_wit_example/) for an example guest module.

Components defined in mods use WIT interfaces and automatically participate in the ECS.

### Building WAT files into WASM modules (experimental)

```bash
wasm-tools component wit ./wit -o metadata.wasm
wasm-tools component embed metadata.wasm ./guest_wit_example.wat --world host -o ./guest_wit_example.wasm
wasm-tools component new ./guest_wit_example.wasm -o ./guest_wit_example2.wasm
```

## License

Copyright (C) 2026 UNTAMEDSTARBLOB

This program is free software: you can redistribute it and/or modify it under the terms of the [GNU General Public License v3.0](LICENSE) as published by the Free Software Foundation.

See [LICENSE](LICENSE) for the full license text.
