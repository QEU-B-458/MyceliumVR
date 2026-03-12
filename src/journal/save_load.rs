use bevy_ecs::prelude::*;
use bevy_ecs::reflect::AppTypeRegistry;
use bevy_reflect::serde::{ReflectDeserializer, ReflectSerializer};
use bevy_reflect::{PartialReflect, TypeRegistry};
use serde::de::DeserializeSeed;

use super::descriptor::{ComponentData, NodeDescriptor};
use super::entity_id::{WorldEntityId, WorldEntityRegistry};

/// Errors that can occur during save/load operations.
#[derive(Debug, thiserror::Error)]
pub enum SaveLoadError {
    #[error("Serialization failed for component '{type_path}': {source}")]
    SerializeComponent {
        type_path: String,
        source: rmp_serde::encode::Error,
    },

    #[error("Deserialization failed for component '{type_path}': {source}")]
    DeserializeComponent {
        type_path: String,
        source: rmp_serde::decode::Error,
    },

    #[error("Type '{type_path}' not found in type registry")]
    TypeNotRegistered { type_path: String },

    #[error("Type '{type_path}' has no ReflectComponent data")]
    NoReflectComponent { type_path: String },

    #[error("Entity not found in world")]
    EntityNotFound,

    #[error("AppTypeRegistry resource not found in world")]
    NoTypeRegistry,
}

pub type Result<T> = std::result::Result<T, SaveLoadError>;

// -- Component-level serialization --

/// Serialize a single reflected component value to ComponentData.
pub fn serialize_component(
    reflected: &dyn PartialReflect,
    type_path: &str,
    registry: &TypeRegistry,
) -> Result<ComponentData> {
    let serializer = ReflectSerializer::new(reflected, registry);
    let data = rmp_serde::to_vec(&serializer).map_err(|e| SaveLoadError::SerializeComponent {
        type_path: type_path.to_string(),
        source: e,
    })?;
    Ok(ComponentData {
        type_path: type_path.to_string(),
        data,
    })
}

/// Deserialize ComponentData back to a reflected value.
pub fn deserialize_component(
    component_data: &ComponentData,
    registry: &TypeRegistry,
) -> Result<Box<dyn PartialReflect>> {
    let deserializer = ReflectDeserializer::new(registry);
    let mut de = rmp_serde::Deserializer::new(component_data.data.as_slice());
    deserializer
        .deserialize(&mut de)
        .map_err(|e| SaveLoadError::DeserializeComponent {
            type_path: component_data.type_path.clone(),
            source: e,
        })
}

// -- Type path skip list --

/// Components that should NOT be serialized into NodeDescriptor.
/// Hierarchy is captured by the descriptor's `children` field.
/// WorldEntityId is stored in the descriptor's `world_entity_id` field.
fn should_skip_component(type_path: &str) -> bool {
    type_path.contains("ChildOf")
        || type_path.contains("bevy_hierarchy")
        || type_path.contains("bevy_ecs::hierarchy")
        || type_path.contains("WorldEntityId")
}

// -- Entity tree serialization --

/// Serialize an entity and all its descendants into a NodeDescriptor tree.
///
/// Requires that the entity has a `WorldEntityId` component (or one will be generated).
/// Only components with registered `ReflectComponent` type data are included.
pub fn serialize_entity_tree(
    world: &World,
    entity: Entity,
    entity_registry: &WorldEntityRegistry,
    type_registry: &TypeRegistry,
) -> Result<NodeDescriptor> {
    let entity_ref = world.get_entity(entity).map_err(|_| SaveLoadError::EntityNotFound)?;

    // Get or generate WorldEntityId
    let world_entity_id = entity_ref
        .get::<WorldEntityId>()
        .copied()
        .or_else(|| entity_registry.get_world_id(&entity))
        .unwrap_or_else(WorldEntityId::new);

    // Serialize all reflected components
    let mut components = Vec::new();
    let archetype = entity_ref.archetype();

    for &component_id in archetype.components() {
        let Some(info) = world.components().get_info(component_id) else {
            continue;
        };
        let Some(type_id) = info.type_id() else {
            continue;
        };

        let type_name = info.name();

        if should_skip_component(&type_name) {
            continue;
        }

        // Check if this type has ReflectComponent registered
        let Some(registration) = type_registry.get(type_id) else {
            continue;
        };
        let Some(reflect_component) = registration.data::<bevy_ecs::reflect::ReflectComponent>()
        else {
            continue;
        };

        // Reflect the component value
        let Some(reflected) = reflect_component.reflect(entity_ref.clone()) else {
            continue;
        };

        let comp_type_path = registration.type_info().type_path();
        match serialize_component(reflected.as_partial_reflect(), comp_type_path, type_registry) {
            Ok(data) => components.push(data),
            Err(e) => {
                bevy_log::warn!("Skipping component {}: {}", type_name, e);
                continue;
            }
        }
    }

    // Recursively serialize children
    let children_descriptors = if let Some(children) = entity_ref.get::<Children>() {
        children
            .iter()
            .filter_map(|child| {
                serialize_entity_tree(world, child, entity_registry, type_registry).ok()
            })
            .collect()
    } else {
        Vec::new()
    };

    Ok(NodeDescriptor {
        world_entity_id,
        components,
        children: children_descriptors,
        metadata: Default::default(),
    })
}

// -- Entity tree deserialization (spawn) --

/// Spawn entities from a NodeDescriptor tree, returning the root Entity.
///
/// Parents children under the spawned root using Bevy hierarchy.
pub fn spawn_from_descriptor(
    world: &mut World,
    descriptor: &NodeDescriptor,
    parent: Option<Entity>,
) -> Result<Entity> {
    // Read type registry (need to clone the Arc to avoid borrow conflicts)
    let registry_arc = world
        .get_resource::<AppTypeRegistry>()
        .ok_or(SaveLoadError::NoTypeRegistry)?
        .clone();
    let type_registry = registry_arc.read();

    // Spawn the entity with WorldEntityId
    let entity = world.spawn(descriptor.world_entity_id).id();

    // Insert each deserialized component
    for comp_data in &descriptor.components {
        let reflected = deserialize_component(comp_data, &type_registry)?;

        let Some(registration) = type_registry.get_with_type_path(&comp_data.type_path) else {
            bevy_log::warn!(
                "Type '{}' not in registry, skipping",
                comp_data.type_path
            );
            continue;
        };

        let Some(reflect_component) =
            registration.data::<bevy_ecs::reflect::ReflectComponent>()
        else {
            bevy_log::warn!(
                "Type '{}' has no ReflectComponent, skipping",
                comp_data.type_path
            );
            continue;
        };

        let mut entity_mut = world.entity_mut(entity);
        reflect_component.insert(&mut entity_mut, reflected.as_ref(), &type_registry);
    }

    // Parent under the given parent entity using add_child
    if let Some(parent_entity) = parent {
        world.entity_mut(parent_entity).add_child(entity);
    }

    // Drop the registry lock before recursing (we'll re-acquire in recursive calls)
    drop(type_registry);

    // Recursively spawn children
    for child_desc in &descriptor.children {
        spawn_from_descriptor(world, child_desc, Some(entity))?;
    }

    Ok(entity)
}

/// Convenience: serialize an entire world root's children into a Vec of NodeDescriptors.
pub fn serialize_world(
    world: &World,
    world_root: Entity,
    entity_registry: &WorldEntityRegistry,
    type_registry: &TypeRegistry,
) -> Result<Vec<NodeDescriptor>> {
    let entity_ref = world
        .get_entity(world_root)
        .map_err(|_| SaveLoadError::EntityNotFound)?;

    let Some(children) = entity_ref.get::<Children>() else {
        return Ok(Vec::new());
    };

    // Collect child entities first to avoid borrow issues
    let child_entities: Vec<Entity> = children.to_vec();
    let mut descriptors = Vec::new();
    for child in child_entities {
        descriptors.push(serialize_entity_tree(
            world,
            child,
            entity_registry,
            type_registry,
        )?);
    }
    Ok(descriptors)
}

/// Convenience: spawn all entities from a Vec of NodeDescriptors under a world root.
/// Also registers them in the WorldEntityRegistry on the world root.
pub fn load_world(
    world: &mut World,
    world_root: Entity,
    descriptors: &[NodeDescriptor],
) -> Result<Vec<Entity>> {
    let mut spawned = Vec::new();
    for desc in descriptors {
        let entity = spawn_from_descriptor(world, desc, Some(world_root))?;
        spawned.push(entity);
    }

    // Register all spawned entities in the WorldEntityRegistry
    register_spawned_entities(world, world_root, descriptors);

    Ok(spawned)
}

/// Register WorldEntityId → Entity mappings for all spawned entities.
fn register_spawned_entities(
    world: &mut World,
    world_root: Entity,
    descriptors: &[NodeDescriptor],
) {
    // Collect all (WorldEntityId, Entity) pairs by querying the world
    let mut pairs = Vec::new();
    let mut query_state = world.query::<(Entity, &WorldEntityId)>();
    for (entity, wid) in query_state.iter(world) {
        pairs.push((*wid, entity));
    }

    // Register in the WorldEntityRegistry on the world root
    if let Some(mut registry) = world.get_mut::<WorldEntityRegistry>(world_root) {
        for (wid, entity) in pairs {
            registry.register(wid, entity);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::reflect::AppTypeRegistry;
    use bevy_reflect::Reflect;

    /// A test component that derives all the traits needed for save/load.
    #[derive(Component, Reflect, Default, Clone, Debug, PartialEq)]
    #[reflect(Component)]
    struct TestHealth {
        current: f32,
        max: f32,
    }

    #[derive(Component, Reflect, Default, Clone, Debug, PartialEq)]
    #[reflect(Component)]
    struct TestLabel {
        label: String,
    }

    /// Helper: create a World with the type registry set up.
    fn test_world() -> World {
        let mut world = World::new();
        let registry = AppTypeRegistry::default();
        {
            let mut reg = registry.write();
            reg.register::<TestHealth>();
            reg.register::<TestLabel>();
            reg.register::<WorldEntityId>();
        }
        world.insert_resource(registry);
        world
    }

    #[test]
    fn component_roundtrip() {
        let world = test_world();
        let registry = world.resource::<AppTypeRegistry>();
        let type_registry = registry.read();

        let original = TestHealth {
            current: 7.5,
            max: 10.0,
        };

        // Serialize
        let reflected: &dyn PartialReflect = &original;
        let data = serialize_component(
            reflected,
            "MyceliumVR::journal::save_load::tests::TestHealth",
            &type_registry,
        )
        .unwrap();

        // Deserialize
        let recovered_reflect = deserialize_component(&data, &type_registry).unwrap();
        let recovered = recovered_reflect
            .try_downcast_ref::<TestHealth>()
            .expect("Should downcast to TestHealth");

        assert_eq!(recovered.current, 7.5);
        assert_eq!(recovered.max, 10.0);
    }

    #[test]
    fn entity_tree_roundtrip() {
        let mut world = test_world();

        // Spawn a parent entity with children
        let parent_wid = WorldEntityId::new();
        let child1_wid = WorldEntityId::new();
        let child2_wid = WorldEntityId::new();

        let parent = world
            .spawn((
                parent_wid,
                TestHealth {
                    current: 100.0,
                    max: 100.0,
                },
                TestLabel {
                    label: "Parent".into(),
                },
            ))
            .id();

        let child1 = world
            .spawn((
                child1_wid,
                TestHealth {
                    current: 50.0,
                    max: 50.0,
                },
                TestLabel {
                    label: "Child1".into(),
                },
            ))
            .id();

        let child2 = world
            .spawn((
                child2_wid,
                TestLabel {
                    label: "Child2".into(),
                },
            ))
            .id();

        // Set up parent-child hierarchy using add_child
        world.entity_mut(parent).add_child(child1);
        world.entity_mut(parent).add_child(child2);

        // Build entity registry
        let mut entity_registry = WorldEntityRegistry::default();
        entity_registry.register(parent_wid, parent);
        entity_registry.register(child1_wid, child1);
        entity_registry.register(child2_wid, child2);

        // Serialize the tree
        let type_registry = world.resource::<AppTypeRegistry>().read();
        let descriptor =
            serialize_entity_tree(&world, parent, &entity_registry, &type_registry).unwrap();
        drop(type_registry);

        // Verify descriptor structure
        assert_eq!(descriptor.world_entity_id, parent_wid);
        assert_eq!(descriptor.children.len(), 2);
        assert!(descriptor.components.len() >= 2); // TestHealth + TestLabel at minimum

        // Serialize to MessagePack bytes (what would go to disk or wire)
        let bytes = rmp_serde::to_vec(&descriptor).unwrap();
        let recovered_descriptor: NodeDescriptor = rmp_serde::from_slice(&bytes).unwrap();

        // Despawn originals (recursive — removes children too)
        world.entity_mut(parent).despawn();

        // Spawn from descriptor
        let new_root = spawn_from_descriptor(&mut world, &recovered_descriptor, None).unwrap();

        // Verify the spawned entities
        let new_entity = world.entity(new_root);
        let health = new_entity.get::<TestHealth>().expect("Should have TestHealth");
        assert_eq!(health.current, 100.0);
        assert_eq!(health.max, 100.0);

        let label = new_entity.get::<TestLabel>().expect("Should have TestLabel");
        assert_eq!(label.label, "Parent");

        let wid = new_entity.get::<WorldEntityId>().expect("Should have WorldEntityId");
        assert_eq!(*wid, parent_wid);

        // Verify children were spawned
        let children = new_entity.get::<Children>().expect("Should have children");
        assert_eq!(children.len(), 2);

        // Check child component values
        let child_entities: Vec<Entity> = children.to_vec();
        let mut found_child1 = false;
        let mut found_child2 = false;

        for child_entity in child_entities {
            let child_ref = world.entity(child_entity);
            let child_label = child_ref.get::<TestLabel>().expect("Child should have TestLabel");

            if child_label.label == "Child1" {
                found_child1 = true;
                let child_health = child_ref.get::<TestHealth>().expect("Child1 should have health");
                assert_eq!(child_health.current, 50.0);
                assert_eq!(child_health.max, 50.0);
            } else if child_label.label == "Child2" {
                found_child2 = true;
                assert!(child_ref.get::<TestHealth>().is_none());
            }
        }

        assert!(found_child1, "Child1 not found");
        assert!(found_child2, "Child2 not found");
    }

    #[test]
    fn messagepack_size_is_compact() {
        let descriptor = NodeDescriptor {
            world_entity_id: WorldEntityId::new(),
            components: vec![ComponentData {
                type_path: "test::Component".into(),
                data: vec![0; 100],
            }],
            children: vec![],
            metadata: Default::default(),
        };

        let msgpack_bytes = rmp_serde::to_vec(&descriptor).unwrap();
        let json_bytes = serde_json::to_vec(&descriptor).unwrap();

        assert!(
            msgpack_bytes.len() < json_bytes.len(),
            "MessagePack ({} bytes) should be smaller than JSON ({} bytes)",
            msgpack_bytes.len(),
            json_bytes.len()
        );
    }
}
