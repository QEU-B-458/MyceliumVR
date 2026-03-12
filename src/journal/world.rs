use bevy_ecs::prelude::*;
use bevy_reflect::Reflect;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

use super::action::ActionRecord;

/// Marker component for a world root entity. Each loaded world has one.
/// Children of this entity are journaled/networked.
#[derive(Component, Reflect, Clone, Debug)]
pub struct WorldRoot {
    /// Human-readable world identifier (e.g. "lobby", "arena").
    pub world_id: String,
}

/// Marker component for the local root entity.
/// Children of this entity are NOT journaled or networked.
/// Used for: UI, debug gizmos, player rig, hand models, etc.
#[derive(Component, Reflect, Default, Clone, Debug)]
pub struct LocalRoot;

/// The journal component — attached to a world root entity.
/// Tracks all mutations, supports undo/redo, and holds the outbox for networking.
#[derive(Component, Debug)]
pub struct WorldJournal {
    /// Full action log for this world (append-only during a session).
    pub log: Vec<ActionRecord>,

    /// Undo stack: actions that can be undone (most recent on top).
    pub undo_stack: Vec<ActionRecord>,

    /// Redo stack: undone actions that can be redone (most recent on top).
    pub redo_stack: Vec<ActionRecord>,

    /// Actions waiting to be sent to peers.
    pub outbox: VecDeque<ActionRecord>,

    /// Next sequence number for locally-authored actions.
    pub next_sequence: u64,

    /// Maximum number of undo entries.
    pub max_undo_depth: usize,
}

impl Default for WorldJournal {
    fn default() -> Self {
        Self {
            log: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            outbox: VecDeque::new(),
            next_sequence: 1,
            max_undo_depth: 200,
        }
    }
}

impl WorldJournal {
    /// Record a locally-authored action. Pushes to log, undo stack, and outbox.
    /// Clears the redo stack (new action after undo invalidates redo chain).
    pub fn record_local(&mut self, mut record: ActionRecord) -> u64 {
        let seq = self.next_sequence;
        record.sequence = seq;
        record.received = false;
        self.next_sequence += 1;

        self.log.push(record.clone());
        self.undo_stack.push(record.clone());
        self.redo_stack.clear();
        self.outbox.push_back(record);

        // Enforce undo depth limit
        while self.undo_stack.len() > self.max_undo_depth {
            self.undo_stack.remove(0);
        }

        seq
    }

    /// Record an action received from a remote peer. Goes into log only (not outbox or undo).
    pub fn record_received(&mut self, mut record: ActionRecord) {
        record.received = true;
        self.log.push(record);
    }

    /// Pop the most recent undoable action from the undo stack.
    /// Returns the undo (reverse) action if available.
    pub fn undo(&mut self) -> Option<ActionRecord> {
        let action = self.undo_stack.pop()?;
        let reverse = action.make_undo()?;
        self.redo_stack.push(action);
        Some(reverse)
    }

    /// Pop the most recent redoable action from the redo stack.
    pub fn redo(&mut self) -> Option<ActionRecord> {
        let action = self.redo_stack.pop()?;
        self.undo_stack.push(action.clone());
        Some(action)
    }

    /// Drain the outbox, returning all pending actions for network send.
    pub fn drain_outbox(&mut self) -> Vec<ActionRecord> {
        self.outbox.drain(..).collect()
    }
}

/// Who can do what in a world.
#[derive(Component, Reflect, Clone, Debug, Serialize, Deserialize)]
pub struct WorldPermissions {
    /// Peer ID of the world creator.
    pub owner: String,

    /// Who can save the world.
    pub save_policy: SavePolicy,

    /// Default role for new joiners.
    pub default_role: WorldRole,

    /// Per-peer role overrides.
    pub peer_roles: HashMap<String, WorldRole>,
}

impl Default for WorldPermissions {
    fn default() -> Self {
        Self {
            owner: String::new(),
            save_policy: SavePolicy::OwnerOnly,
            default_role: WorldRole::Editor,
            peer_roles: HashMap::new(),
        }
    }
}

impl WorldPermissions {
    /// Get the effective role for a peer.
    pub fn role_for(&self, peer_id: &str) -> WorldRole {
        if peer_id == self.owner {
            return WorldRole::Owner;
        }
        self.peer_roles
            .get(peer_id)
            .copied()
            .unwrap_or(self.default_role)
    }

    /// Check if a peer can edit (add/remove/modify entities).
    pub fn can_edit(&self, peer_id: &str) -> bool {
        matches!(
            self.role_for(peer_id),
            WorldRole::Owner | WorldRole::Admin | WorldRole::Editor
        )
    }

    /// Check if a peer can save the world.
    pub fn can_save(&self, peer_id: &str) -> bool {
        match &self.save_policy {
            SavePolicy::OwnerOnly => peer_id == self.owner,
            SavePolicy::Permitted(list) => peer_id == self.owner || list.contains(&peer_id.to_string()),
            SavePolicy::Anyone => self.can_edit(peer_id),
        }
    }

    /// Check if a peer can modify permissions (owner/admin only).
    pub fn can_admin(&self, peer_id: &str) -> bool {
        matches!(
            self.role_for(peer_id),
            WorldRole::Owner | WorldRole::Admin
        )
    }
}

/// Who can save the world file.
#[derive(Clone, Debug, Serialize, Deserialize, Reflect)]
pub enum SavePolicy {
    OwnerOnly,
    Permitted(Vec<String>),
    Anyone,
}

/// Role within a world.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Reflect)]
pub enum WorldRole {
    Owner,
    Admin,
    Editor,
    Viewer,
    Blocked,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::entity_id::WorldEntityId;
    use crate::journal::action::ActionPayload;

    #[test]
    fn journal_record_and_undo() {
        let mut journal = WorldJournal::default();

        let record = ActionRecord {
            target: WorldEntityId::new(),
            payload: ActionPayload::SetProperty {
                component: "Transform".into(),
                field_path: "translation.x".into(),
                value: vec![1],
                previous_value: Some(vec![0]),
            },
            author: "peer_a".into(),
            timestamp: 0.0,
            sequence: 0, // will be overwritten
            received: false,
        };

        let seq = journal.record_local(record);
        assert_eq!(seq, 1);
        assert_eq!(journal.log.len(), 1);
        assert_eq!(journal.undo_stack.len(), 1);
        assert_eq!(journal.outbox.len(), 1);

        // Undo
        let undo_action = journal.undo().unwrap();
        match &undo_action.payload {
            ActionPayload::SetProperty { value, .. } => {
                assert_eq!(value, &vec![0]); // restored to previous
            }
            _ => panic!("Expected SetProperty"),
        }
        assert_eq!(journal.undo_stack.len(), 0);
        assert_eq!(journal.redo_stack.len(), 1);

        // Redo
        let redo_action = journal.redo().unwrap();
        assert_eq!(journal.undo_stack.len(), 1);
        assert_eq!(journal.redo_stack.len(), 0);
    }

    #[test]
    fn journal_received_does_not_go_to_outbox() {
        let mut journal = WorldJournal::default();

        let record = ActionRecord {
            target: WorldEntityId::new(),
            payload: ActionPayload::SetProperty {
                component: "Transform".into(),
                field_path: "translation.x".into(),
                value: vec![1],
                previous_value: None,
            },
            author: "remote_peer".into(),
            timestamp: 0.0,
            sequence: 5,
            received: false,
        };

        journal.record_received(record);
        assert_eq!(journal.log.len(), 1);
        assert!(journal.log[0].received);
        assert!(journal.outbox.is_empty());
        assert!(journal.undo_stack.is_empty());
    }

    #[test]
    fn permissions_role_resolution() {
        let perms = WorldPermissions {
            owner: "alice".into(),
            save_policy: SavePolicy::Permitted(vec!["bob".into()]),
            default_role: WorldRole::Viewer,
            peer_roles: {
                let mut m = HashMap::new();
                m.insert("bob".into(), WorldRole::Editor);
                m
            },
        };

        assert_eq!(perms.role_for("alice"), WorldRole::Owner);
        assert_eq!(perms.role_for("bob"), WorldRole::Editor);
        assert_eq!(perms.role_for("charlie"), WorldRole::Viewer);

        assert!(perms.can_edit("alice"));
        assert!(perms.can_edit("bob"));
        assert!(!perms.can_edit("charlie"));

        assert!(perms.can_save("alice"));
        assert!(perms.can_save("bob"));
        assert!(!perms.can_save("charlie"));

        assert!(perms.can_admin("alice"));
        assert!(!perms.can_admin("bob"));
    }

    #[test]
    fn drain_outbox() {
        let mut journal = WorldJournal::default();

        for i in 0..3 {
            let record = ActionRecord {
                target: WorldEntityId::new(),
                payload: ActionPayload::SetProperty {
                    component: "T".into(),
                    field_path: "x".into(),
                    value: vec![i],
                    previous_value: None,
                },
                author: "peer".into(),
                timestamp: 0.0,
                sequence: 0,
                received: false,
            };
            journal.record_local(record);
        }

        let drained = journal.drain_outbox();
        assert_eq!(drained.len(), 3);
        assert!(journal.outbox.is_empty());
        // Log and undo stack still have the records
        assert_eq!(journal.log.len(), 3);
    }
}
