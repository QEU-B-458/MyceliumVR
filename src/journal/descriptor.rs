use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::entity_id::WorldEntityId;

/// Portable description of a single entity and its children.
/// Used for: save/load, late join snapshots, AddNode actions, DeleteNode undo captures.
///
/// Components are stored as (type_name, reflect-serialized bytes) pairs.
/// The actual serialization/deserialization of component values uses Bevy Reflect
/// via the TypeRegistry — this struct just holds the bytes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeDescriptor {
    /// Stable cross-peer identifier for this entity.
    pub world_entity_id: WorldEntityId,

    /// Serialized components: type path → MessagePack bytes (Reflect-serialized).
    pub components: Vec<ComponentData>,

    /// Child entities, preserving hierarchy.
    pub children: Vec<NodeDescriptor>,

    /// Arbitrary metadata (e.g. display name, tags, asset references).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, Vec<u8>>,
}

/// A single serialized component within a NodeDescriptor.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComponentData {
    /// Bevy type path (e.g. "mycelium_vr::components::Health").
    pub type_path: String,

    /// MessagePack-serialized bytes from ReflectSerializer.
    pub data: Vec<u8>,
}

impl NodeDescriptor {
    /// Create a new empty descriptor with a fresh WorldEntityId.
    pub fn new() -> Self {
        Self {
            world_entity_id: WorldEntityId::new(),
            components: Vec::new(),
            children: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Create a descriptor with a specific WorldEntityId.
    pub fn with_id(id: WorldEntityId) -> Self {
        Self {
            world_entity_id: id,
            components: Vec::new(),
            children: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Total number of entities in this subtree (self + all descendants).
    pub fn entity_count(&self) -> usize {
        1 + self.children.iter().map(|c| c.entity_count()).sum::<usize>()
    }
}

impl Default for NodeDescriptor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_serde_roundtrip() {
        let mut root = NodeDescriptor::new();
        root.components.push(ComponentData {
            type_path: "test::FakeComponent".into(),
            data: vec![1, 2, 3, 4],
        });

        let child = NodeDescriptor::new();
        root.children.push(child);

        root.metadata.insert("name".into(), b"TestEntity".to_vec());

        let bytes = rmp_serde::to_vec(&root).unwrap();
        let recovered: NodeDescriptor = rmp_serde::from_slice(&bytes).unwrap();

        assert_eq!(root.world_entity_id, recovered.world_entity_id);
        assert_eq!(recovered.components.len(), 1);
        assert_eq!(recovered.components[0].type_path, "test::FakeComponent");
        assert_eq!(recovered.components[0].data, vec![1, 2, 3, 4]);
        assert_eq!(recovered.children.len(), 1);
        assert_eq!(recovered.entity_count(), 2);
        assert_eq!(recovered.metadata.get("name").unwrap(), b"TestEntity");
    }

    #[test]
    fn entity_count_nested() {
        let mut root = NodeDescriptor::new();
        let mut child = NodeDescriptor::new();
        child.children.push(NodeDescriptor::new());
        child.children.push(NodeDescriptor::new());
        root.children.push(child);
        root.children.push(NodeDescriptor::new());

        // root(1) + child(1) + grandchild(1) + grandchild(1) + child2(1) = 5
        assert_eq!(root.entity_count(), 5);
    }
}
