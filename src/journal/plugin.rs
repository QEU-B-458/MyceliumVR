use bevy_app::{App, Plugin, PreUpdate, Update};
use bevy_ecs::hierarchy::ChildOf;
use bevy_ecs::prelude::*;
use bevy::input::keyboard::KeyCode;
use bevy::input::ButtonInput;
use bevy_log::warn;

use super::action::ActionRecord;
use super::apply::{apply_and_record, apply_payload};
use super::entity_id::{WorldEntityId, WorldEntityRegistry};
use super::world::{WorldJournal, WorldRoot};

/// Queue of journal actions to be applied this frame.
/// Push actions here from any system; the journal processes them each frame.
#[derive(Resource, Default)]
pub struct JournalActionQueue(pub Vec<ActionRecord>);

/// Pending undo/redo requests. Set by input or other systems; processed each frame.
#[derive(Resource, Default)]
pub struct JournalUndoRedoQueue {
    pub undo_count: u32,
    pub redo_count: u32,
}

/// Wires the World Journal into the live app.
///
/// - Auto-registers entities with `WorldEntityId` into their `WorldRoot`'s registry
/// - Auto-unregisters despawned entities
/// - Processes queued journal actions (apply + record)
/// - Handles Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y for undo/redo
pub struct JournalPlugin;

impl Plugin for JournalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<JournalActionQueue>()
            .init_resource::<JournalUndoRedoQueue>()
            .add_systems(
                PreUpdate,
                (auto_register_entities, auto_unregister_entities),
            )
            .add_systems(Update, (handle_undo_redo_input, process_journal));
    }
}

/// Finds newly spawned entities with `WorldEntityId`, walks up the hierarchy
/// to find their `WorldRoot` ancestor, and registers them in the `WorldEntityRegistry`.
fn auto_register_entities(
    new_entities: Query<(Entity, &WorldEntityId), Added<WorldEntityId>>,
    parents: Query<&ChildOf>,
    world_roots: Query<Entity, With<WorldRoot>>,
    mut registries: Query<&mut WorldEntityRegistry, With<WorldRoot>>,
) {
    for (entity, wid) in &new_entities {
        // Walk up the hierarchy to find the WorldRoot ancestor
        let mut current = entity;
        let mut found_root = None;
        loop {
            if world_roots.get(current).is_ok() {
                found_root = Some(current);
                break;
            }
            match parents.get(current) {
                Ok(child_of) => current = child_of.parent(),
                Err(_) => break,
            }
        }

        if let Some(root) = found_root {
            if let Ok(mut registry) = registries.get_mut(root) {
                registry.register(*wid, entity);
            }
        }
    }
}

/// Cleans up the registry when entities with `WorldEntityId` are despawned.
fn auto_unregister_entities(
    mut removed: RemovedComponents<WorldEntityId>,
    mut registries: Query<&mut WorldEntityRegistry, With<WorldRoot>>,
) {
    for entity in removed.read() {
        for mut registry in registries.iter_mut() {
            registry.unregister_entity(&entity);
        }
    }
}

/// Reads keyboard input for undo/redo and queues requests.
fn handle_undo_redo_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut queue: ResMut<JournalUndoRedoQueue>,
) {
    let ctrl =
        keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !ctrl {
        return;
    }

    if keyboard.just_pressed(KeyCode::KeyZ) {
        if keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight) {
            queue.redo_count += 1;
        } else {
            queue.undo_count += 1;
        }
    }
    if keyboard.just_pressed(KeyCode::KeyY) {
        queue.redo_count += 1;
    }
}

/// Exclusive system that processes the journal action queue and undo/redo requests.
/// Runs with full world access since applying actions mutates entities via Reflect.
fn process_journal(world: &mut World) {
    // Drain the action queue
    let actions: Vec<ActionRecord> = world
        .resource_mut::<JournalActionQueue>()
        .0
        .drain(..)
        .collect();

    // Drain undo/redo counts
    let (undo_count, redo_count) = {
        let mut queue = world.resource_mut::<JournalUndoRedoQueue>();
        let counts = (queue.undo_count, queue.redo_count);
        queue.undo_count = 0;
        queue.redo_count = 0;
        counts
    };

    // Nothing to do?
    if actions.is_empty() && undo_count == 0 && redo_count == 0 {
        return;
    }

    // Find the first world root (single-world for now)
    let world_root = {
        let mut query = world.query_filtered::<Entity, With<WorldRoot>>();
        query.iter(world).next()
    };
    let Some(world_root) = world_root else {
        return;
    };

    // Apply queued actions
    for record in actions {
        if let Err(e) = apply_and_record(world, world_root, record) {
            warn!("Journal: failed to apply action: {e}");
        }
    }

    // Apply undos
    for _ in 0..undo_count {
        let undo_record = world
            .get_mut::<WorldJournal>(world_root)
            .and_then(|mut j| j.undo());
        if let Some(record) = undo_record {
            if let Err(e) = apply_payload(world, record.target, &record.payload, world_root) {
                warn!("Journal: failed to apply undo: {e}");
            }
        }
    }

    // Apply redos
    for _ in 0..redo_count {
        let redo_record = world
            .get_mut::<WorldJournal>(world_root)
            .and_then(|mut j| j.redo());
        if let Some(record) = redo_record {
            if let Err(e) = apply_payload(world, record.target, &record.payload, world_root) {
                warn!("Journal: failed to apply redo: {e}");
            }
        }
    }
}
