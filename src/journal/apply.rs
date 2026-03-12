use bevy_ecs::prelude::*;
use bevy_ecs::reflect::AppTypeRegistry;
use bevy_reflect::{PartialReflect, Reflect, TypeRegistry};

use super::action::{ActionPayload, ActionRecord};
use super::descriptor::ComponentData;
use super::entity_id::{WorldEntityId, WorldEntityRegistry};
use super::save_load::{
    deserialize_component, serialize_component, serialize_entity_tree, spawn_from_descriptor,
    SaveLoadError,
};
use super::world::{WorldJournal, WorldRoot};

/// Errors that can occur when applying an action.
#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("Target entity {0} not found in WorldEntityRegistry")]
    TargetNotFound(WorldEntityId),

    #[error("Entity {0} not found in ECS world")]
    EntityMissing(Entity),

    #[error("World root entity not found")]
    WorldRootMissing,

    #[error(transparent)]
    SaveLoad(#[from] SaveLoadError),

    #[error("Component '{0}' not found in type registry")]
    TypeNotRegistered(String),

    #[error("Component '{0}' has no ReflectComponent type data")]
    NoReflectComponent(String),

    #[error("Component '{0}' not present on entity")]
    ComponentMissing(String),

    #[error("Field path '{path}' not found on component '{component}'")]
    FieldNotFound { component: String, path: String },
}

pub type Result<T> = std::result::Result<T, ApplyError>;

/// Apply an ActionPayload to the ECS world, mutating the target entity.
///
/// This is the core function that turns a journal entry into an actual ECS mutation.
/// It captures previous values for undo and returns an ActionPayload with those values filled in.
///
/// Does NOT record into the journal — the caller handles that.
pub fn apply_payload(
    world: &mut World,
    target: WorldEntityId,
    payload: &ActionPayload,
    world_root: Entity,
) -> Result<ActionPayload> {
    match payload {
        ActionPayload::SetComponent {
            component,
            data,
            ..
        } => apply_set_component(world, target, component, data, world_root),

        ActionPayload::AddComponent { component, data } => {
            apply_add_component(world, target, component, data, world_root)
        }

        ActionPayload::RemoveComponent { component, .. } => {
            apply_remove_component(world, target, component, world_root)
        }

        ActionPayload::SetProperty {
            component,
            field_path,
            value,
            ..
        } => apply_set_property(world, target, component, field_path, value, world_root),

        ActionPayload::AddNode { descriptor, parent } => {
            apply_add_node(world, descriptor, parent.as_ref(), world_root)
        }

        ActionPayload::DeleteNode { .. } => apply_delete_node(world, target, world_root),

        ActionPayload::Reparent { new_parent, .. } => {
            apply_reparent(world, target, new_parent.as_ref(), world_root)
        }

        ActionPayload::Batch(actions) => {
            let mut applied = Vec::new();
            for action in actions {
                applied.push(apply_payload(world, target, action, world_root)?);
            }
            Ok(ActionPayload::Batch(applied))
        }
    }
}

// -- Helper: resolve WorldEntityId to Entity --

fn resolve_entity(world: &World, target: WorldEntityId, world_root: Entity) -> Result<Entity> {
    let registry = world
        .get::<WorldEntityRegistry>(world_root)
        .ok_or(ApplyError::WorldRootMissing)?;
    registry
        .get_entity(&target)
        .ok_or(ApplyError::TargetNotFound(target))
}

fn get_type_registry(world: &World) -> std::result::Result<AppTypeRegistry, ApplyError> {
    world
        .get_resource::<AppTypeRegistry>()
        .cloned()
        .ok_or(ApplyError::SaveLoad(SaveLoadError::NoTypeRegistry))
}

// -- SetComponent: replace an entire component --

fn apply_set_component(
    world: &mut World,
    target: WorldEntityId,
    component_path: &str,
    new_data: &[u8],
    world_root: Entity,
) -> Result<ActionPayload> {
    let entity = resolve_entity(world, target, world_root)?;
    let registry_arc = get_type_registry(world)?;
    let type_registry = registry_arc.read();

    // Look up the type
    let registration = type_registry
        .get_with_type_path(component_path)
        .ok_or_else(|| ApplyError::TypeNotRegistered(component_path.into()))?;
    let reflect_component = registration
        .data::<bevy_ecs::reflect::ReflectComponent>()
        .ok_or_else(|| ApplyError::NoReflectComponent(component_path.into()))?;

    // Capture previous value for undo
    let entity_ref = world
        .get_entity(entity)
        .map_err(|_| ApplyError::EntityMissing(entity))?;
    let previous_data = if let Some(reflected) = reflect_component.reflect(entity_ref) {
        let comp_data =
            serialize_component(reflected.as_partial_reflect(), component_path, &type_registry)?;
        Some(comp_data.data)
    } else {
        None
    };

    // Deserialize new value and insert
    let comp_data = ComponentData {
        type_path: component_path.to_string(),
        data: new_data.to_vec(),
    };
    let reflected = deserialize_component(&comp_data, &type_registry)?;
    drop(type_registry);

    let type_registry = registry_arc.read();
    let registration = type_registry.get_with_type_path(component_path).unwrap();
    let reflect_component = registration
        .data::<bevy_ecs::reflect::ReflectComponent>()
        .unwrap();
    let mut entity_mut = world.entity_mut(entity);
    reflect_component.insert(&mut entity_mut, reflected.as_ref(), &type_registry);

    Ok(ActionPayload::SetComponent {
        component: component_path.to_string(),
        data: new_data.to_vec(),
        previous_data,
    })
}

// -- AddComponent: add a new component to an entity --

fn apply_add_component(
    world: &mut World,
    target: WorldEntityId,
    component_path: &str,
    data: &[u8],
    world_root: Entity,
) -> Result<ActionPayload> {
    let entity = resolve_entity(world, target, world_root)?;
    let registry_arc = get_type_registry(world)?;
    let type_registry = registry_arc.read();

    let registration = type_registry
        .get_with_type_path(component_path)
        .ok_or_else(|| ApplyError::TypeNotRegistered(component_path.into()))?;
    let reflect_component = registration
        .data::<bevy_ecs::reflect::ReflectComponent>()
        .ok_or_else(|| ApplyError::NoReflectComponent(component_path.into()))?;

    let comp_data = ComponentData {
        type_path: component_path.to_string(),
        data: data.to_vec(),
    };
    let reflected = deserialize_component(&comp_data, &type_registry)?;

    let mut entity_mut = world.entity_mut(entity);
    reflect_component.insert(&mut entity_mut, reflected.as_ref(), &type_registry);

    Ok(ActionPayload::AddComponent {
        component: component_path.to_string(),
        data: data.to_vec(),
    })
}

// -- RemoveComponent: remove a component, capturing it for undo --

fn apply_remove_component(
    world: &mut World,
    target: WorldEntityId,
    component_path: &str,
    world_root: Entity,
) -> Result<ActionPayload> {
    let entity = resolve_entity(world, target, world_root)?;
    let registry_arc = get_type_registry(world)?;
    let type_registry = registry_arc.read();

    let registration = type_registry
        .get_with_type_path(component_path)
        .ok_or_else(|| ApplyError::TypeNotRegistered(component_path.into()))?;
    let reflect_component = registration
        .data::<bevy_ecs::reflect::ReflectComponent>()
        .ok_or_else(|| ApplyError::NoReflectComponent(component_path.into()))?;

    // Capture current value for undo before removing
    let entity_ref = world
        .get_entity(entity)
        .map_err(|_| ApplyError::EntityMissing(entity))?;
    let previous_data = if let Some(reflected) = reflect_component.reflect(entity_ref) {
        let comp_data =
            serialize_component(reflected.as_partial_reflect(), component_path, &type_registry)?;
        Some(comp_data.data)
    } else {
        None
    };

    drop(type_registry);

    // Remove the component
    let type_registry = registry_arc.read();
    let registration = type_registry.get_with_type_path(component_path).unwrap();
    let reflect_component = registration
        .data::<bevy_ecs::reflect::ReflectComponent>()
        .unwrap();
    let mut entity_mut = world.entity_mut(entity);
    reflect_component.remove(&mut entity_mut);

    Ok(ActionPayload::RemoveComponent {
        component: component_path.to_string(),
        previous_data,
    })
}

// -- SetProperty: mutate a single field on a component --

fn apply_set_property(
    world: &mut World,
    target: WorldEntityId,
    component_path: &str,
    field_path: &str,
    new_value: &[u8],
    world_root: Entity,
) -> Result<ActionPayload> {
    let entity = resolve_entity(world, target, world_root)?;
    let registry_arc = get_type_registry(world)?;
    let type_registry = registry_arc.read();

    let registration = type_registry
        .get_with_type_path(component_path)
        .ok_or_else(|| ApplyError::TypeNotRegistered(component_path.into()))?;
    let reflect_component = registration
        .data::<bevy_ecs::reflect::ReflectComponent>()
        .ok_or_else(|| ApplyError::NoReflectComponent(component_path.into()))?;

    // Read current component, navigate to field, capture previous value
    let entity_ref = world
        .get_entity(entity)
        .map_err(|_| ApplyError::EntityMissing(entity))?;
    let reflected = reflect_component
        .reflect(entity_ref)
        .ok_or_else(|| ApplyError::ComponentMissing(component_path.into()))?;

    // Navigate to the field via the path
    let field_ref = navigate_field_path(reflected.as_partial_reflect(), field_path).ok_or_else(
        || ApplyError::FieldNotFound {
            component: component_path.into(),
            path: field_path.into(),
        },
    )?;

    // Serialize previous field value
    let previous_value = rmp_serde::to_vec(
        &bevy_reflect::serde::ReflectSerializer::new(field_ref, &type_registry),
    )
    .ok();

    // Deserialize the new value
    let deserializer = bevy_reflect::serde::ReflectDeserializer::new(&type_registry);
    let mut de = rmp_serde::Deserializer::new(new_value);
    let new_reflected = serde::de::DeserializeSeed::deserialize(deserializer, &mut de)
        .map_err(|e| ApplyError::SaveLoad(SaveLoadError::DeserializeComponent {
            type_path: component_path.into(),
            source: e,
        }))?;

    drop(type_registry);

    // Now get a mutable reference and apply
    let type_registry = registry_arc.read();
    let registration = type_registry.get_with_type_path(component_path).unwrap();
    let reflect_component = registration
        .data::<bevy_ecs::reflect::ReflectComponent>()
        .unwrap();

    let mut entity_mut = world.entity_mut(entity);
    if let Some(mut reflected_mut) = reflect_component.reflect_mut(&mut entity_mut) {
        if let Some(field_mut) =
            navigate_field_path_mut(reflected_mut.as_partial_reflect_mut(), field_path)
        {
            field_mut.apply(new_reflected.as_ref());
        }
    }

    Ok(ActionPayload::SetProperty {
        component: component_path.to_string(),
        field_path: field_path.to_string(),
        value: new_value.to_vec(),
        previous_value,
    })
}

// -- AddNode: spawn an entity subtree --

fn apply_add_node(
    world: &mut World,
    descriptor: &super::descriptor::NodeDescriptor,
    parent: Option<&WorldEntityId>,
    world_root: Entity,
) -> Result<ActionPayload> {
    // Resolve parent entity
    let parent_entity = match parent {
        Some(parent_wid) => Some(resolve_entity(world, *parent_wid, world_root)?),
        None => Some(world_root), // No parent specified = direct child of world root
    };

    let entity = spawn_from_descriptor(world, descriptor, parent_entity)?;

    // Register in WorldEntityRegistry
    register_descriptor_recursive(world, world_root, descriptor);

    Ok(ActionPayload::AddNode {
        descriptor: descriptor.clone(),
        parent: parent.copied(),
    })
}

fn register_descriptor_recursive(
    world: &mut World,
    world_root: Entity,
    descriptor: &super::descriptor::NodeDescriptor,
) {
    // Find the entity with this WorldEntityId
    let mut found_entity = None;
    let mut query = world.query::<(Entity, &WorldEntityId)>();
    for (entity, wid) in query.iter(world) {
        if *wid == descriptor.world_entity_id {
            found_entity = Some(entity);
            break;
        }
    }

    if let Some(entity) = found_entity {
        if let Some(mut registry) = world.get_mut::<WorldEntityRegistry>(world_root) {
            registry.register(descriptor.world_entity_id, entity);
        }
    }

    for child in &descriptor.children {
        register_descriptor_recursive(world, world_root, child);
    }
}

// -- DeleteNode: remove an entity subtree, capturing snapshot for undo --

fn apply_delete_node(
    world: &mut World,
    target: WorldEntityId,
    world_root: Entity,
) -> Result<ActionPayload> {
    let entity = resolve_entity(world, target, world_root)?;

    // Figure out the parent for undo
    let parent_wid = {
        let entity_ref = world
            .get_entity(entity)
            .map_err(|_| ApplyError::EntityMissing(entity))?;
        entity_ref
            .get::<bevy_ecs::hierarchy::ChildOf>()
            .and_then(|child_of| {
                let parent = child_of.parent();
                if parent == world_root {
                    None // Direct child of world root = no explicit parent
                } else {
                    let registry = world.get::<WorldEntityRegistry>(world_root)?;
                    registry.get_world_id(&parent)
                }
            })
    };

    // Capture snapshot before deletion
    let registry_arc = get_type_registry(world)?;
    let type_registry = registry_arc.read();
    let entity_registry = world
        .get::<WorldEntityRegistry>(world_root)
        .ok_or(ApplyError::WorldRootMissing)?;
    let snapshot = serialize_entity_tree(world, entity, entity_registry, &type_registry).ok();
    drop(type_registry);

    // Unregister from WorldEntityRegistry (recursively)
    unregister_recursive(world, entity, world_root);

    // Despawn entity and children
    world.entity_mut(entity).despawn();

    Ok(ActionPayload::DeleteNode {
        snapshot,
        parent: parent_wid,
    })
}

fn unregister_recursive(world: &mut World, entity: Entity, world_root: Entity) {
    // Collect children first
    let children: Vec<Entity> = world
        .get_entity(entity)
        .ok()
        .and_then(|e| e.get::<Children>())
        .map(|c| c.to_vec())
        .unwrap_or_default();

    for child in children {
        unregister_recursive(world, child, world_root);
    }

    // Unregister this entity
    if let Some(mut registry) = world.get_mut::<WorldEntityRegistry>(world_root) {
        registry.unregister_entity(&entity);
    }
}

// -- Reparent: move an entity to a new parent --

fn apply_reparent(
    world: &mut World,
    target: WorldEntityId,
    new_parent: Option<&WorldEntityId>,
    world_root: Entity,
) -> Result<ActionPayload> {
    let entity = resolve_entity(world, target, world_root)?;

    // Capture previous parent for undo
    let previous_parent = {
        let entity_ref = world
            .get_entity(entity)
            .map_err(|_| ApplyError::EntityMissing(entity))?;
        entity_ref
            .get::<bevy_ecs::hierarchy::ChildOf>()
            .and_then(|child_of| {
                let parent = child_of.parent();
                if parent == world_root {
                    None
                } else {
                    let registry = world.get::<WorldEntityRegistry>(world_root)?;
                    registry.get_world_id(&parent)
                }
            })
    };

    // Resolve new parent
    let new_parent_entity = match new_parent {
        Some(parent_wid) => resolve_entity(world, *parent_wid, world_root)?,
        None => world_root,
    };

    // Reparent
    world.entity_mut(new_parent_entity).add_child(entity);

    Ok(ActionPayload::Reparent {
        new_parent: new_parent.copied(),
        previous_parent: Some(previous_parent),
    })
}

// -- Field path navigation --

/// Navigate a dot-separated field path on a reflected value.
/// e.g. "translation.x" on a Transform → the x field of the translation Vec3.
fn navigate_field_path<'a>(
    value: &'a dyn PartialReflect,
    path: &str,
) -> Option<&'a dyn PartialReflect> {
    let mut current = value;
    for segment in path.split('.') {
        let reflected = current.try_as_reflect()?;
        let s = reflected.reflect_ref().as_struct().ok()?;
        current = s.field(segment)?.as_partial_reflect();
    }
    Some(current)
}

/// Mutable version of field path navigation.
fn navigate_field_path_mut<'a>(
    value: &'a mut dyn PartialReflect,
    path: &str,
) -> Option<&'a mut dyn PartialReflect> {
    let segments: Vec<&str> = path.split('.').collect();
    let mut current = value;
    for segment in &segments {
        let as_reflect = current.try_as_reflect_mut()?;
        let s = as_reflect.reflect_mut().as_struct().ok()?;
        current = s.field_mut(segment)?.as_partial_reflect_mut();
    }
    Some(current)
}

// -- High-level apply + journal record --

/// Apply an action to the world AND record it in the appropriate journal.
///
/// For local actions: applies the mutation, records in journal (log + undo + outbox).
/// For received actions: applies the mutation, records in journal (log only).
pub fn apply_and_record(
    world: &mut World,
    world_root: Entity,
    mut record: ActionRecord,
) -> Result<()> {
    // Apply the payload, which fills in previous values
    let applied_payload = apply_payload(world, record.target, &record.payload, world_root)?;
    record.payload = applied_payload;

    // Record in journal
    if let Some(mut journal) = world.get_mut::<WorldJournal>(world_root) {
        if record.received {
            journal.record_received(record);
        } else {
            journal.record_local(record);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::reflect::AppTypeRegistry;
    use bevy_reflect::Reflect;

    #[derive(Component, Reflect, Default, Clone, Debug, PartialEq)]
    #[reflect(Component)]
    struct Position {
        x: f32,
        y: f32,
    }

    #[derive(Component, Reflect, Default, Clone, Debug, PartialEq)]
    #[reflect(Component)]
    struct Velocity {
        dx: f32,
        dy: f32,
    }

    fn test_world_with_entity() -> (World, Entity, Entity, WorldEntityId) {
        let mut world = World::new();
        let registry = AppTypeRegistry::default();
        {
            let mut reg = registry.write();
            reg.register::<Position>();
            reg.register::<Velocity>();
            reg.register::<WorldEntityId>();
        }
        world.insert_resource(registry);

        let wid = WorldEntityId::new();

        // Spawn world root with registry
        let world_root = world
            .spawn((
                WorldRoot {
                    world_id: "test".into(),
                },
                WorldJournal::default(),
                WorldEntityRegistry::default(),
            ))
            .id();

        // Spawn target entity as child of world root
        let entity = world
            .spawn((wid, Position { x: 1.0, y: 2.0 }))
            .id();
        world.entity_mut(world_root).add_child(entity);

        // Register in entity registry
        world
            .get_mut::<WorldEntityRegistry>(world_root)
            .unwrap()
            .register(wid, entity);

        (world, world_root, entity, wid)
    }

    #[test]
    fn apply_set_component() {
        let (mut world, world_root, _entity, wid) = test_world_with_entity();

        let registry_arc = world.resource::<AppTypeRegistry>().clone();
        let type_registry = registry_arc.read();

        // Serialize new Position value
        let new_pos = Position { x: 10.0, y: 20.0 };
        let new_data = serialize_component(
            &new_pos as &dyn PartialReflect,
            "MyceliumVR::journal::apply::tests::Position",
            &type_registry,
        )
        .unwrap();
        drop(type_registry);

        let result = apply_payload(
            &mut world,
            wid,
            &ActionPayload::SetComponent {
                component: "MyceliumVR::journal::apply::tests::Position".into(),
                data: new_data.data.clone(),
                previous_data: None,
            },
            world_root,
        )
        .unwrap();

        // Verify the entity was updated
        let entity = world
            .get::<WorldEntityRegistry>(world_root)
            .unwrap()
            .get_entity(&wid)
            .unwrap();
        let pos = world.entity(entity).get::<Position>().unwrap();
        assert_eq!(pos.x, 10.0);
        assert_eq!(pos.y, 20.0);

        // Verify previous_data was captured
        match result {
            ActionPayload::SetComponent { previous_data, .. } => {
                assert!(previous_data.is_some(), "Should capture previous value");
            }
            _ => panic!("Expected SetComponent"),
        }
    }

    #[test]
    fn apply_add_and_remove_component() {
        let (mut world, world_root, entity, wid) = test_world_with_entity();

        let registry_arc = world.resource::<AppTypeRegistry>().clone();
        let type_registry = registry_arc.read();

        // Serialize Velocity
        let vel = Velocity { dx: 5.0, dy: -3.0 };
        let vel_data = serialize_component(
            &vel as &dyn PartialReflect,
            "MyceliumVR::journal::apply::tests::Velocity",
            &type_registry,
        )
        .unwrap();
        drop(type_registry);

        // Add Velocity component
        apply_payload(
            &mut world,
            wid,
            &ActionPayload::AddComponent {
                component: "MyceliumVR::journal::apply::tests::Velocity".into(),
                data: vel_data.data,
            },
            world_root,
        )
        .unwrap();

        // Verify it was added
        let vel = world.entity(entity).get::<Velocity>().unwrap();
        assert_eq!(vel.dx, 5.0);
        assert_eq!(vel.dy, -3.0);

        // Remove it
        let result = apply_payload(
            &mut world,
            wid,
            &ActionPayload::RemoveComponent {
                component: "MyceliumVR::journal::apply::tests::Velocity".into(),
                previous_data: None,
            },
            world_root,
        )
        .unwrap();

        // Verify it was removed
        assert!(world.entity(entity).get::<Velocity>().is_none());

        // Verify previous data was captured for undo
        match result {
            ActionPayload::RemoveComponent { previous_data, .. } => {
                assert!(previous_data.is_some());
            }
            _ => panic!("Expected RemoveComponent"),
        }
    }

    #[test]
    fn apply_set_property_field() {
        let (mut world, world_root, entity, wid) = test_world_with_entity();

        let registry_arc = world.resource::<AppTypeRegistry>().clone();
        let type_registry = registry_arc.read();

        // Serialize just the new x value
        let new_x: f32 = 42.0;
        let new_x_bytes = rmp_serde::to_vec(
            &bevy_reflect::serde::ReflectSerializer::new(
                &new_x as &dyn PartialReflect,
                &type_registry,
            ),
        )
        .unwrap();
        drop(type_registry);

        let result = apply_payload(
            &mut world,
            wid,
            &ActionPayload::SetProperty {
                component: "MyceliumVR::journal::apply::tests::Position".into(),
                field_path: "x".into(),
                value: new_x_bytes,
                previous_value: None,
            },
            world_root,
        )
        .unwrap();

        // Verify only x changed, y stayed the same
        let pos = world.entity(entity).get::<Position>().unwrap();
        assert_eq!(pos.x, 42.0);
        assert_eq!(pos.y, 2.0); // unchanged

        // Verify previous value captured
        match result {
            ActionPayload::SetProperty { previous_value, .. } => {
                assert!(previous_value.is_some());
            }
            _ => panic!("Expected SetProperty"),
        }
    }

    #[test]
    fn apply_and_record_local() {
        let (mut world, world_root, entity, wid) = test_world_with_entity();

        let registry_arc = world.resource::<AppTypeRegistry>().clone();
        let type_registry = registry_arc.read();
        let new_pos = Position { x: 99.0, y: 99.0 };
        let new_data = serialize_component(
            &new_pos as &dyn PartialReflect,
            "MyceliumVR::journal::apply::tests::Position",
            &type_registry,
        )
        .unwrap();
        drop(type_registry);

        let record = ActionRecord {
            target: wid,
            payload: ActionPayload::SetComponent {
                component: "MyceliumVR::journal::apply::tests::Position".into(),
                data: new_data.data,
                previous_data: None,
            },
            author: "local_peer".into(),
            timestamp: 0.0,
            sequence: 0,
            received: false,
        };

        apply_and_record(&mut world, world_root, record).unwrap();

        // Verify ECS was mutated
        let pos = world.entity(entity).get::<Position>().unwrap();
        assert_eq!(pos.x, 99.0);

        // Verify journal recorded it
        let journal = world.get::<WorldJournal>(world_root).unwrap();
        assert_eq!(journal.log.len(), 1);
        assert_eq!(journal.undo_stack.len(), 1);
        assert_eq!(journal.outbox.len(), 1);
    }

    #[test]
    fn apply_and_record_received() {
        let (mut world, world_root, entity, wid) = test_world_with_entity();

        let registry_arc = world.resource::<AppTypeRegistry>().clone();
        let type_registry = registry_arc.read();
        let new_pos = Position { x: 77.0, y: 77.0 };
        let new_data = serialize_component(
            &new_pos as &dyn PartialReflect,
            "MyceliumVR::journal::apply::tests::Position",
            &type_registry,
        )
        .unwrap();
        drop(type_registry);

        let record = ActionRecord {
            target: wid,
            payload: ActionPayload::SetComponent {
                component: "MyceliumVR::journal::apply::tests::Position".into(),
                data: new_data.data,
                previous_data: None,
            },
            author: "remote_peer".into(),
            timestamp: 0.0,
            sequence: 5,
            received: true, // from network
        };

        apply_and_record(&mut world, world_root, record).unwrap();

        // Verify ECS was mutated
        let pos = world.entity(entity).get::<Position>().unwrap();
        assert_eq!(pos.x, 77.0);

        // Verify journal: log only, NOT in undo/outbox
        let journal = world.get::<WorldJournal>(world_root).unwrap();
        assert_eq!(journal.log.len(), 1);
        assert!(journal.undo_stack.is_empty());
        assert!(journal.outbox.is_empty());
    }
}
