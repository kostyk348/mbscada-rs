//! `mb-engine` — waiting-free polling engine in the shape of Modbus Poll.
//!
//! * [`transport`] — TCP / RTU serial / RTU-over-TCP transports.
//! * [`model`] — connections, poll definitions, alarm specs.
//! * [`format`] — register display formats (u16/i16/hex/bin/u32/i32/floats).
//! * [`rules`] — extra styling rules (colour, blink, severity).
//! * [`engine`] — the multi-poll engine producing snapshots for the UI.

pub mod engine;
pub mod format;
pub mod import;
pub mod model;
pub mod rules;
pub mod transport;

pub use engine::{CommEntry, ConnStats, ConnView, Engine, PollView, RowView, Snapshot};
pub use format::{decode, display, Fmt};
pub use import::{import_paths, ImportResult};
pub use model::{func_is_read, AlarmSpec, ConnKind, Connection, Poll, SerialCfg};
pub use rules::{branch_count, first_matching, Cond, MAX_BRANCHES, Rule, Severity};
pub use transport::{open_transport, Transport};
