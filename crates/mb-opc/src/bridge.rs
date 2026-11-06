//! Bridge core: project file -> OPC UA address space, engine snapshots ->
//! variable values.

use crate::Project;
use mb_engine::model::{func_is_read, Poll};
use opcua::server::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

/// The subset of the desktop app's project JSON the bridge needs. Unknown
/// keys are ignored, `rules`/`computed` default to empty.
#[derive(Deserialize)]
pub struct Project {
    /// Modbus connections.
    #[serde(default)]
    pub connections: Vec<mb_engine::Connection>,
    /// Poll windows (rows are tags).
    #[serde(default)]
    pub polls: Vec<Poll>,
    /// Extra styling rules (accepted but not needed by the bridge).
    #[serde(default)]
    pub rules: Vec<mb_engine::Rule>,
    /// Computed (virtual) tags.
    #[serde(default)]
    pub computed: Vec<ComputedTag>,
}

/// Where a tag node finds its value in the snapshot.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct RowKey {
    pub conn: String,
    pub slave: u8,
    pub func: u8,
    pub addr: u16,
}

pub enum NodeKind {
    /// A poll row: value from the snapshot row map.
    Tag { key: RowKey, is_bits: bool },
    /// A computed tag, matched by name.
    Calc { name: String },
    /// Connection status variables.
    Online { conn: String },
    Tx { conn: String },
    Errors { conn: String },
}

pub struct NodeRef {
    pub id: NodeId,
    pub kind: NodeKind,
}

