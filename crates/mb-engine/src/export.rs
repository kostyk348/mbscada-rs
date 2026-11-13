//! Export polls back to Modbus Poll files (`.mbp`).
//!
//! Two paths, chosen automatically:
//!
//! 1. **Byte-preserving** — the poll came from an imported `.mbp` and the
//!    original file is still readable. The document is re-parsed
//!    losslessly ([`MbpDoc`]), the current aliases are edited into their
//!    row markers, and every other byte — header, colours, settings blob,
//!    IP string for v5 — stays exactly as Modbus Poll wrote it.
//! 2. **Synthetic** — no usable original (polls created in the app, or a
//!    moved/deleted file). A canonical v4 table is generated with
//!    [`write_mbp`]: function code, address (notation-encoded), quantity,
//!    aliases, scan period and slave ID.
//!
//! What is *not* stored in the synthetic file: scaling (mult/div/offset),
//! units, alarms and computed tags — `.mbp` has no fields for them, and
//! Modbus Poll keeps its own scaling in the per-row settings blob. The
//! byte-preserving path keeps whatever the original file had.

use crate::model::Poll;
use mb_format::mbp::write_mbp;
use mb_format::mbp_doc::MbpDoc;
use mb_format::model::{valid_func, Row, Table};
use std::path::Path;

use crate::Fmt;

/// Result of an export: the bytes to write and which path produced them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedMbp {
    pub bytes: Vec<u8>,
    /// true — rebuilt from the original file with only alias markers edited;
    /// false — generated synthetically from the poll.
    pub from_original: bool,
}

/// Inverse of `decode_addr`: protocol offset + notation → raw stored address.
fn encode_raw_addr(addr: u32, notation: &str, func: u8) -> u32 {
    let raw = match notation {
        "4x" => addr + 0x4000,
        "3x" if func == 4 => addr + 0x3000,
        "1x" if func == 2 => addr + 0x1000,
        "4xxxx" if func == 3 => addr + 40001,
        "3xxxx" if func == 4 => addr + 30001,
        _ => addr,
    };
    // The parser rejects stored addresses of 0xFFF0 and above; fall back to
    // decimal when an encoded notation would land outside the window.
    if raw >= 0xFFF0 {
        addr
    } else {
        raw
    }
}

/// Poll → canonical `.mbp` table.
fn poll_to_table(p: &Poll) -> Table {
    let func = if valid_func(p.func) { p.func } else { 3 };
    let raw = encode_raw_addr(p.start as u32, &p.notation, func);
    let rows: Vec<Row> = p
        .row_addrs()
        .into_iter()
        .map(|a| Row {
            addr: a as u32,
            alias: p.labels.get(&a.to_string()).cloned().unwrap_or_default(),
        })
        .collect();
    Table {
        path: p.name.clone(),
        name: p.name.clone(),
        magic: mb_format::mbp::MAGIC_MBP_V4,
        func,
        addr: raw,
        raw_addr: raw,
        qty: p.qty.clamp(1, 2000) as u32,
        // `find_tail` accepts 50..=60000; a faster app-side scan rate still
        // round-trips as a legal Modbus Poll period.
        period_ms: p.scan_ms.clamp(50, 60000) as u32,
        slave: p.slave.clamp(1, 247),
        fmt16: fmt16_for(p.fmt),
        notation: p.notation.clone(),
        ip: String::new(),
        rows,
        confidence: 1.0,
        notes: String::new(),
    }
}

/// The `fmt16` code Modbus Poll uses for a display format family.
fn fmt16_for(fmt: Fmt) -> u32 {
    match fmt {
        Fmt::I16 => 5,
        Fmt::I32 => 7,
        Fmt::F32Abcd | Fmt::F32Cdab | Fmt::F32Badc | Fmt::F32Dcba => 8,
        _ => 0,
    }
}

/// Apply the poll's current labels to a losslessly parsed original document.
///
/// Aliases are edited through [`MbpDoc::set_alias`], so only the changed row
/// markers are rebuilt; everything else in the file is preserved verbatim.
fn apply_aliases(
    doc: &mut MbpDoc,
    raw_addr: u32,
    labels: &std::collections::HashMap<String, String>,
) {
    for (i, row) in doc.rows.clone().iter().enumerate() {
        let want = labels
            .get(&(raw_addr.wrapping_add(i as u32)).to_string())
            .cloned()
            .unwrap_or_default();
        let have = row.alias();
        if want.is_empty() || want == have || want.encode_utf16().count() > 100 {
            continue;
        }
        let _ = doc.set_alias(i, &want);
    }
}

/// Export a poll to `.mbp` bytes: byte-preserving when the poll remembers an
/// original `.mbp` that is still on disk, synthetic otherwise.
pub fn export_poll_mbp(p: &Poll) -> ExportedMbp {
    if !p.source.is_empty() && p.source.ends_with(".mbp") {
        if let Ok(bytes) = std::fs::read(&p.source) {
            if let (Ok(mut doc), Ok(t)) = (
                MbpDoc::parse(&bytes),
                mb_format::mbp::parse_mbp(&bytes, &p.source, &p.name),
            ) {
                apply_aliases(&mut doc, t.raw_addr, &p.labels);
                return ExportedMbp {
                    bytes: doc.write(),
                    from_original: true,
                };
            }
        }
    }
    ExportedMbp {
        bytes: write_mbp(&poll_to_table(p)),
        from_original: false,
    }
}

/// Export and write the file atomically (temp file + rename).
pub fn export_poll_mbp_to_file(p: &Poll, path: &Path) -> Result<ExportedMbp, String> {
    let out = export_poll_mbp(p);
    let tmp = path.with_extension("mbp.tmp");
    std::fs::write(&tmp, &out.bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .map_err(|e| format!("{} → {}: {e}", tmp.display(), path.display()))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn poll_with_labels(func: u8, start: u16, qty: u16, labels: &[(&str, &str)]) -> Poll {
        let mut l = HashMap::new();
        for (k, v) in labels {
            l.insert(k.to_string(), v.to_string());
        }
        Poll {
            id: 7,
            name: "Boiler".into(),
            conn_id: "c1".into(),
            slave: 2,
            func,
            start,
            raw_start: 0,
            qty,
            scan_ms: 500,
            fmt: if func == 3 { Fmt::U16 } else { Fmt::U16 },
            mult: 1.0,
            div: 1.0,
            offset: 0.0,
            unit: String::new(),
            enabled: true,
            base_one: false,
            notation: String::new(),
            labels: l,
            alarms: HashMap::new(),
            source: String::new(),
        }
    }

    #[test]
    fn synthetic_export_round_trips_through_parse() {
        let p = poll_with_labels(3, 20, 4, &[("20", "T boiler"), ("22", "P pump")]);
        let out = export_poll_mbp(&p);
        assert!(!out.from_original);

        let t = mb_format::mbp::parse_mbp(&out.bytes, "syn", "syn").expect("parses back");
        assert_eq!(t.func, 3);
        assert_eq!(t.raw_addr, 20);
        assert_eq!(t.qty, 4);
        assert_eq!(t.period_ms, 500);
        assert_eq!(t.slave, 2);
        let aliases = t.aliases();
        assert_eq!(aliases, vec![(20, "T boiler"), (22, "P pump")]);
    }

    #[test]
    fn synthetic_export_encodes_plc_notation() {
        let mut p = poll_with_labels(3, 0, 2, &[]);
        p.notation = "4x".into();
        let out = export_poll_mbp(&p);
        let t = mb_format::mbp::parse_mbp(&out.bytes, "s", "s").expect("parses");
        assert_eq!(t.raw_addr, 0x4000);
        assert_eq!(t.addr, 0);
    }

    #[test]
    fn bits_poll_exports_func_one() {
        let p = poll_with_labels(1, 0, 8, &[]);
        let out = export_poll_mbp(&p);
        let t = mb_format::mbp::parse_mbp(&out.bytes, "s", "s").expect("parses");
        assert_eq!(t.func, 1);
    }

    #[test]
    fn export_to_file_writes_and_names_atomically() {
        let p = poll_with_labels(3, 0, 2, &[("0", "first")]);
        let dir = std::env::temp_dir().join(format!("mbscada-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("poll.mbp");
        let out = export_poll_mbp_to_file(&p, &path).expect("written");
        assert!(!out.from_original);
        assert!(
            !dir.join("poll.mbp.tmp").exists(),
            "temp file must be renamed away"
        );
        let t = mb_format::mbp::parse_mbp(&std::fs::read(&path).unwrap(), "x", "x").unwrap();
        assert_eq!(t.aliases(), vec![(0, "first")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn original_file_is_preserved_and_aliases_reapplied() {
        // A synthetic source file (canonical layout) plays the role of an
        // original Modbus Poll file here.
        let p0 = poll_with_labels(3, 10, 3, &[("10", "old name")]);
        let original = export_poll_mbp(&p0).bytes;

        let dir = std::env::temp_dir().join(format!("mbscada-export-o-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("orig.mbp");
        std::fs::write(&src, &original).unwrap();

        // The user renamed the alias in the app.
        let mut p1 = poll_with_labels(3, 10, 3, &[("10", "new name"), ("12", "added")]);
        p1.source = src.display().to_string();
        let out = export_poll_mbp(&p1);
        assert!(out.from_original);
        // Byte-identical except the edited alias markers.
        assert_ne!(out.bytes, original);
        let t = mb_format::mbp::parse_mbp(&out.bytes, "x", "x").unwrap();
        assert_eq!(t.aliases(), vec![(10, "new name"), (12, "added")]);

        // Unchanged poll → byte-exact export.
        let mut same = poll_with_labels(3, 10, 3, &[("10", "old name")]);
        same.source = src.display().to_string();
        let out2 = export_poll_mbp(&same);
        assert!(out2.from_original);
        assert_eq!(out2.bytes, original, "unchanged export must be byte-exact");

        // Missing original → synthetic fallback.
        let mut p3 = poll_with_labels(3, 10, 3, &[]);
        p3.source = dir.join("nope.mbp").display().to_string();
        let out3 = export_poll_mbp(&p3);
        assert!(!out3.from_original);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn long_alias_is_left_unedited_not_corrupted() {
        let p0 = poll_with_labels(3, 0, 1, &[("0", "x")]);
        let original = export_poll_mbp(&p0).bytes;
        let dir = std::env::temp_dir().join(format!("mbscada-export-l-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("o.mbp");
        std::fs::write(&src, &original).unwrap();

        let long = "я".repeat(110); // 110 utf16 units — over the 100-unit limit
        let mut p1 = poll_with_labels(3, 0, 1, &[("0", long.as_str())]);
        p1.source = src.display().to_string();
        let out = export_poll_mbp(&p1);
        assert!(out.from_original);
        let t = mb_format::mbp::parse_mbp(&out.bytes, "x", "x").unwrap();
        assert_eq!(
            t.aliases(),
            vec![(0, "x")],
            "over-long alias must be skipped"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
