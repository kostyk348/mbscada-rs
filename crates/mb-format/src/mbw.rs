//! `.mbw` (workspace) parser and writer.

use crate::error::{Error, Result};
use crate::model::{TabRef, Workspace};
use crate::util::{is_ip, iter_markers, push_str_marker, push_u32, rd32};

pub const MAGIC_MBW: u32 = 6400;
pub const MAGIC_MBW_V2: u32 = 10020;

/// Parse a `.mbw` byte buffer. File existence of referenced tables is left
/// `false` here; use [`crate::parse_mbw_file`] to resolve paths on disk.
pub fn parse_mbw(data: &[u8], path: &str) -> Result<Workspace> {
    if data.len() < 40 {
        return Err(Error::Parse("workspace smaller than 40 bytes".into()));
    }

    let markers = iter_markers(data, 0, data.len());
    let texts: Vec<String> = markers
        .iter()
        .filter(|m| m.len > 0)
        .map(|m| m.text.trim_end_matches(',').to_string())
        .collect();

    let host = texts
        .iter()
        .find(|t| is_ip(t))
        .cloned()
        .unwrap_or_else(|| "127.0.0.1".to_string());

    let names: Vec<String> = texts
        .iter()
        .filter(|t| t.to_lowercase().ends_with(".mbp"))
        .cloned()
        .collect();

    // TCP port: first occurrence of 502 in the first 200 bytes (default 502).
    let mut port: u16 = 502;
    let lim = data.len().min(200);
    let mut off = 0;
    while off + 4 <= lim {
        if rd32(data, off) == Some(502) {
            port = 502;
            break;
        }
        off += 4;
    }

    let tables = names
        .into_iter()
        .map(|name| TabRef {
            file: name.clone(),
            name,
            exists: false,
        })
        .collect();

    Ok(Workspace {
        path: path.to_string(),
        host,
        port,
        tables,
        notes: String::new(),
    })
}

/// Serialize a workspace to a canonical `.mbw` buffer (semantic round-trip).
pub fn write_mbw(w: &Workspace) -> Vec<u8> {
    let mut out = Vec::new();
    push_u32(&mut out, MAGIC_MBW);
    out.extend_from_slice(&[0, 0, 0, 0]);
    push_str_marker(&mut out, &w.host);
    push_u32(&mut out, w.port as u32);
    for t in &w.tables {
        push_str_marker(&mut out, &t.name);
    }
    while out.len() < 64 {
        out.push(0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mbw_roundtrip() {
        let w = Workspace {
            path: "x.mbw".into(),
            host: "192.168.1.10".into(),
            port: 502,
            tables: vec![
                TabRef {
                    name: "line1.mbp".into(),
                    file: "line1.mbp".into(),
                    exists: false,
                },
                TabRef {
                    name: "line2.mbp".into(),
                    file: "line2.mbp".into(),
                    exists: false,
                },
            ],
            notes: String::new(),
        };
        let bytes = write_mbw(&w);
        let back = parse_mbw(&bytes, "x.mbw").expect("parse");
        assert_eq!(back.host, "192.168.1.10");
        assert_eq!(back.port, 502);
        let names: Vec<_> = back.tables.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["line1.mbp", "line2.mbp"]);
    }

    #[test]
    fn reject_small() {
        assert!(parse_mbw(&[0u8; 8], "x").is_err());
    }
}
