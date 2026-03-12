use serde::{Deserialize, Serialize};

use super::descriptor::NodeDescriptor;
use super::entity_id::WorldEntityId;

/// A single journaled mutation to the world state.
/// Every user action that modifies the shared world becomes one (or a batch of) ActionRecord(s).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActionRecord {
    /// Which entity this action targets.
    pub target: WorldEntityId,

    /// What the action does.
    pub payload: ActionPayload,

    /// Who authored this action (peer identifier, e.g. public key or UUID).
    pub author: String,

    /// Wall-clock timestamp (seconds since UNIX epoch). Used for display, not conflict resolution.
    pub timestamp: f64,

    /// Monotonic sequence number from the authoring peer. Used for ordering.
    pub sequence: u64,

    /// Whether this action was received from a remote peer (true) or created locally (false).
    /// Received actions do NOT go into the outbox.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub received: bool,
}

/// The mutation payload — what kind of change this action represents.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ActionPayload {
    /// Set a single property on a component.
    SetProperty {
        /// Bevy type path of the component (e.g. "bevy_transform::components::Transform").
        component: String,
        /// Dot-separated field path within the component (e.g. "translation.x").
        field_path: String,
        /// New value, MessagePack-serialized.
        value: Vec<u8>,
        /// Previous value for undo. Populated locally, NOT trusted from the wire.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        previous_value: Option<Vec<u8>>,
    },

    /// Replace an entire component with new data.
    SetComponent {
        /// Bevy type path.
        component: String,
        /// Full component data, MessagePack-serialized via Reflect.
        data: Vec<u8>,
        /// Previous component data for undo.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        previous_data: Option<Vec<u8>>,
    },

    /// Add a component to an entity.
    AddComponent {
        /// Bevy type path.
        component: String,
        /// Initial component data, MessagePack-serialized via Reflect.
        data: Vec<u8>,
    },

    /// Remove a component from an entity.
    RemoveComponent {
        /// Bevy type path.
        component: String,
        /// Captured component data for undo.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        previous_data: Option<Vec<u8>>,
    },

    /// Spawn a new entity (with optional children) under a parent.
    AddNode {
        /// The full descriptor of the entity subtree to add.
        descriptor: NodeDescriptor,
        /// Parent entity (None = direct child of world root).
        parent: Option<WorldEntityId>,
    },

    /// Delete an entity and all its descendants.
    DeleteNode {
        /// Captured descriptor for undo — the full subtree that was deleted.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        snapshot: Option<NodeDescriptor>,
        /// Parent entity it was under (for undo re-parenting).
        parent: Option<WorldEntityId>,
    },

    /// Reparent an entity to a new parent.
    Reparent {
        /// New parent (None = direct child of world root).
        new_parent: Option<WorldEntityId>,
        /// Previous parent for undo.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        previous_parent: Option<Option<WorldEntityId>>,
    },

    /// A batch of actions applied atomically.
    Batch(Vec<ActionPayload>),
}

impl ActionRecord {
    /// Create a reverse action for undo. Returns None if the action has no previous state captured.
    pub fn make_undo(&self) -> Option<ActionRecord> {
        let reverse_payload = match &self.payload {
            ActionPayload::SetProperty {
                component,
                field_path,
                value,
                previous_value: Some(prev),
            } => Some(ActionPayload::SetProperty {
                component: component.clone(),
                field_path: field_path.clone(),
                value: prev.clone(),
                previous_value: Some(value.clone()),
            }),

            ActionPayload::SetComponent {
                component,
                data,
                previous_data: Some(prev),
            } => Some(ActionPayload::SetComponent {
                component: component.clone(),
                data: prev.clone(),
                previous_data: Some(data.clone()),
            }),

            ActionPayload::AddComponent { component, data } => {
                Some(ActionPayload::RemoveComponent {
                    component: component.clone(),
                    previous_data: Some(data.clone()),
                })
            }

            ActionPayload::RemoveComponent {
                component,
                previous_data: Some(prev),
            } => Some(ActionPayload::AddComponent {
                component: component.clone(),
                data: prev.clone(),
            }),

            ActionPayload::AddNode { descriptor, parent } => {
                Some(ActionPayload::DeleteNode {
                    snapshot: Some(descriptor.clone()),
                    parent: parent.clone(),
                })
            }

            ActionPayload::DeleteNode {
                snapshot: Some(desc),
                parent,
            } => Some(ActionPayload::AddNode {
                descriptor: desc.clone(),
                parent: parent.clone(),
            }),

            ActionPayload::Reparent {
                new_parent,
                previous_parent: Some(prev),
            } => Some(ActionPayload::Reparent {
                new_parent: prev.clone(),
                previous_parent: Some(new_parent.clone()),
            }),

            // Batch: reverse each sub-action in reverse order
            ActionPayload::Batch(actions) => {
                let reversed: Option<Vec<_>> = actions
                    .iter()
                    .rev()
                    .map(|a| {
                        // Wrap in a dummy record to reuse make_undo logic
                        let dummy = ActionRecord {
                            target: self.target,
                            payload: a.clone(),
                            author: self.author.clone(),
                            timestamp: self.timestamp,
                            sequence: self.sequence,
                            received: false,
                        };
                        dummy.make_undo().map(|r| r.payload)
                    })
                    .collect();
                reversed.map(ActionPayload::Batch)
            }

            _ => None, // No previous state captured — can't undo
        };

        reverse_payload.map(|payload| ActionRecord {
            target: self.target,
            payload,
            author: self.author.clone(),
            timestamp: self.timestamp,
            sequence: 0, // Will be assigned a new sequence on apply
            received: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_record_serde_roundtrip() {
        let record = ActionRecord {
            target: WorldEntityId::new(),
            payload: ActionPayload::SetProperty {
                component: "bevy_transform::components::Transform".into(),
                field_path: "translation.x".into(),
                value: rmp_serde::to_vec(&1.0f32).unwrap(),
                previous_value: Some(rmp_serde::to_vec(&0.0f32).unwrap()),
            },
            author: "peer_abc".into(),
            timestamp: 1710000000.0,
            sequence: 1,
            received: false,
        };

        let bytes = rmp_serde::to_vec(&record).unwrap();
        let recovered: ActionRecord = rmp_serde::from_slice(&bytes).unwrap();

        assert_eq!(record.target, recovered.target);
        assert_eq!(recovered.author, "peer_abc");
        assert_eq!(recovered.sequence, 1);
        assert!(!recovered.received);
    }

    #[test]
    fn undo_set_property() {
        let record = ActionRecord {
            target: WorldEntityId::new(),
            payload: ActionPayload::SetProperty {
                component: "Transform".into(),
                field_path: "translation.x".into(),
                value: vec![1],
                previous_value: Some(vec![0]),
            },
            author: "peer".into(),
            timestamp: 0.0,
            sequence: 1,
            received: false,
        };

        let undo = record.make_undo().unwrap();
        match &undo.payload {
            ActionPayload::SetProperty { value, previous_value, .. } => {
                assert_eq!(value, &vec![0]); // swapped
                assert_eq!(previous_value.as_ref().unwrap(), &vec![1]);
            }
            _ => panic!("Expected SetProperty"),
        }
    }

    #[test]
    fn undo_add_node_becomes_delete() {
        let desc = NodeDescriptor::new();
        let record = ActionRecord {
            target: WorldEntityId::new(),
            payload: ActionPayload::AddNode {
                descriptor: desc.clone(),
                parent: None,
            },
            author: "peer".into(),
            timestamp: 0.0,
            sequence: 1,
            received: false,
        };

        let undo = record.make_undo().unwrap();
        match &undo.payload {
            ActionPayload::DeleteNode { snapshot, parent } => {
                assert!(snapshot.is_some());
                assert!(parent.is_none());
            }
            _ => panic!("Expected DeleteNode"),
        }
    }

    use super::super::descriptor::NodeDescriptor;
}
