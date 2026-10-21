//! `mb-format` — parser/writer for Modbus Poll project files.
//!
//! * `.mbp` — a single polling table (function code, start address, quantity,
//!   scan period, slave id, row aliases).
//! * `.mbw` — a workspace: host/port plus references to `.mbp` table files.
//!
//! The format is proprietary and undocumented; this crate implements the
//! reverse-engineered layout (v4/v5 `.mbp`, `.mbw`) used by the Python
//! `mbscada` prototype, now as a dependency-free Rust crate.

pub mod error;
pub mod mbp;
pub mod mbw;
pub mod model;
pub mod util;

pub use error::{Error, Result};
pub use mbp::{find_tail, parse_mbp, write_mbp, Tail};
pub use mbw::{parse_mbw, write_mbw};
pub use model::{
    decode_addr, display_format, notation_str, valid_func, Notation, Row, TabRef, Table, Workspace,
};
use std::path::Path;

/// Read and parse a `.mbp` file from disk.
pub fn parse_mbp_file(path: &Path) -> Result<Table> {
    let data = std::fs::read(path)?;
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    parse_mbp(&data, &path.to_string_lossy(), &name)
}

/// Read and parse a `.mbw` file, resolving referenced `.mbp` paths relative to
/// the workspace directory and recording whether each table exists.
pub fn parse_mbw_file(path: &Path) -> Result<Workspace> {
    let data = std::fs::read(path)?;
    let mut ws = parse_mbw(&data, &path.to_string_lossy())?;
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    for t in &mut ws.tables {
        let direct = base.join(&t.name);
        let (file, exists) = if direct.exists() {
            (direct, true)
        } else {
            let alt = base.join(Path::new(&t.name).file_name().unwrap_or_default());
            if alt.exists() {
                (alt, true)
            } else {
                (direct, false)
            }
        };
        t.file = file.to_string_lossy().to_string();
        t.exists = exists;
    }
    Ok(ws)
}
