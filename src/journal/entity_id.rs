use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Stable cross-peer entity identifier. Never changes once assigned.
/// Bevy `Entity` is a local generational index — this is the wire-safe equivalent.
#[derive(Component, Reflect, Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct WorldEntityId(pub Uuid);

impl WorldEntityId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl Default for WorldEntityId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for WorldEntityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Maps WorldEntityId → local Bevy Entity for a single world.
/// Lives as a component on the world root entity.
#[derive(Component, Default, Debug)]
pub struct WorldEntityRegistry {
    to_entity: HashMap<WorldEntityId, Entity>,
    to_world_id: HashMap<Entity, WorldEntityId>,
}

impl WorldEntityRegistry {
    pub fn register(&mut self, world_id: WorldEntityId, entity: Entity) {
        self.to_entity.insert(world_id, entity);
        self.to_world_id.insert(entity, world_id);
    }

    pub fn unregister(&mut self, world_id: &WorldEntityId) -> Option<Entity> {
        if let Some(entity) = self.to_entity.remove(world_id) {
            self.to_world_id.remove(&entity);
            Some(entity)
        } else {
            None
        }
    }

    pub fn unregister_entity(&mut self, entity: &Entity) -> Option<WorldEntityId> {
        if let Some(world_id) = self.to_world_id.remove(entity) {
            self.to_entity.remove(&world_id);
            Some(world_id)
        } else {
            None
        }
    }

    pub fn get_entity(&self, world_id: &WorldEntityId) -> Option<Entity> {
        self.to_entity.get(world_id).copied()
    }

    pub fn get_world_id(&self, entity: &Entity) -> Option<WorldEntityId> {
        self.to_world_id.get(entity).copied()
    }

    pub fn len(&self) -> usize {
        self.to_entity.len()
    }

    pub fn is_empty(&self) -> bool {
        self.to_entity.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_entity_id_unique() {
        let a = WorldEntityId::new();
        let b = WorldEntityId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn world_entity_id_serde_roundtrip() {
        let id = WorldEntityId::new();
        let bytes = rmp_serde::to_vec(&id).unwrap();
        let recovered: WorldEntityId = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(id, recovered);
    }

    #[test]
    fn registry_bidirectional() {
        let mut reg = WorldEntityRegistry::default();
        let wid = WorldEntityId::new();
        // Entity::from_raw is available for testing
        let entity = Entity::from_bits(42);

        reg.register(wid, entity);
        assert_eq!(reg.get_entity(&wid), Some(entity));
        assert_eq!(reg.get_world_id(&entity), Some(wid));
        assert_eq!(reg.len(), 1);

        reg.unregister(&wid);
        assert_eq!(reg.get_entity(&wid), None);
        assert_eq!(reg.get_world_id(&entity), None);
        assert!(reg.is_empty());
    }
}
