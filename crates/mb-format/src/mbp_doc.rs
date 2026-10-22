//! Lossless container for `.mbp` files (byte-exact round-trip).
//!
//! Verified layout of real Modbus Poll 10.5 files (v4, magic 7000):
//!
//! ```text
//! [head .. first row marker]      magic (+ v5: IP marker), header fields
//! [row record] × qty              marker + alias string + gap block
//!                                 (mid-list rows: PAD + 6×colours + zeros,
//!                                  last row: + settings blob with tail)
//! [closing marker]                final empty marker `FF FE FF 00` + tail bytes
//! ```
//!
//! Invariant observed on the corpus: exactly `qty + 1` markers, the last one
//! is the empty closer. A document stores every segment verbatim, so
//! `write(parse(bytes)) == bytes` — a safe edit-in-place compatibility layer.
//! Semantic access: aliases are editable via [`MbpDoc::set_alias`] (rebuilds
//! only that row's marker bytes).

use crate::error::{Error, Result};
use crate::util::{MARKER, iter_markers, utf16le_decode};

/// One row record: the full marker bytes (marker + len + UTF-16LE) plus the
/// gap block that follows it (padding/colours, and settings for the last row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MbpRow {
    pub marker: Vec<u8>,
    pub gap: Vec<u8>,
}

impl MbpRow {
    pub fn alias(&self) -> String {
        // marker layout: [FF FE FF][len][len × UTF-16 units]
        if self.marker.len() < 4 {
            return String::new();
        }
        let ln = self.marker[3] as usize;
        let a = 4;
        let b = (a + ln * 2).min(self.marker.len());
        utf16le_decode(&self.marker[a..b])
    }
}

/// Byte-exact document: head + rows + closing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MbpDoc {
    /// Everything before the first row marker (magic, header fields, v5 IP).
    pub head: Vec<u8>,
    /// Exactly `qty` row records (the file's registered rows).
    pub rows: Vec<MbpRow>,
    /// Starting at the final empty marker and running to EOF.
    pub closing: Vec<u8>,
}

impl MbpRow {
    /// Rebuild the marker bytes for a new alias string (1..=100 UTF-16 units).
    fn set_marker_alias_text(text: &str) -> Result<Vec<u8>> {
        let units: Vec<u16> = text.encode_utf16().collect();
        if units.is_empty() || units.len() > 100 {
            return Err(Error::Parse(format!(
                "alias must be 1..=100 utf16 units, got {}",
                units.len()
            )));
        }
        let mut m = Vec::with_capacity(4 + units.len() * 2);
        m.extend_from_slice(&MARKER);
        m.push(units.len() as u8);
        for u in &units {
            m.extend_from_slice(&u.to_le_bytes());
        }
        Ok(m)
    }
}

impl MbpDoc {
    /// Parse a `.mbp` buffer losslessly.
    pub fn parse(d: &[u8]) -> Result<MbpDoc> {
        if d.len() < 64 {
            return Err(Error::Parse("file smaller than 64 bytes".into()));
        }
        crate::util::rd32(d, 0).ok_or_else(|| Error::Parse("no magic".into()))?;

        // Markers: for v5 the first marker is the IP string at offset 4 and is
        // part of the head. Find the scan start accordingly.
        let magic = crate::util::rd32(d, 0);
        let scan_from = if magic == Some(10010) {
            // marker is at 4, length byte at 7 (validated by parse_mbp)
            4 + 4 + (d.get(7).copied().unwrap_or(0) as usize) * 2
        } else {
            0
        };
        let markers = iter_markers(d, scan_from, d.len());
        if markers.len() < 2 {
            return Err(Error::Parse("mbp_doc: no row markers".into()));
        }
        // The final marker is the empty closer.
        if markers[markers.len() - 1].len != 0 {
            return Err(Error::Parse("mbp_doc: last marker is not the closer".into()));
        }

        let head = d[..markers[0].offset].to_vec();
        let mut rows = Vec::with_capacity(markers.len() - 1);
        for w in markers.windows(2) {
            let start = w[0].offset;
            let end = w[1].offset;
            let mk = d[start..start + 4 + (w[0].len as usize) * 2].to_vec();
            let gaplen = end - (start + mk.len());
            let gap = d[start + mk.len()..start + mk.len() + gaplen].to_vec();
            rows.push(MbpRow { marker: mk, gap });
        }
        let closing = d[markers[markers.len() - 1].offset..].to_vec();
        Ok(MbpDoc { head, rows, closing })
    }

    /// Serialize back; must equal the original input byte for byte.
    pub fn write(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.head);
        for r in &self.rows {
            out.extend_from_slice(&r.marker);
            out.extend_from_slice(&r.gap);
        }
        out.extend_from_slice(&self.closing);
        out
    }

    pub fn set_alias(&mut self, idx: usize, text: &str) -> Result<()> {
        let r = self
            .rows
            .get_mut(idx)
            .ok_or_else(|| Error::Parse("row index out of range".into()))?;
        r.marker = MbpRow::set_marker_alias_text(text)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corpus() -> Vec<(std::path::PathBuf, Vec<u8>)> {
        let mut out = Vec::new();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/mbp");
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().map(|x| x == "mbp").unwrap_or(false) {
                    if let Ok(b) = std::fs::read(&p) {
                        out.push((p, b));
                    }
                }
            }
        }
        out
    }

    #[test]
    fn byte_exact_roundtrip_corpus() {
        let files = corpus();
        if files.is_empty() {
            // public/CI checkout: synthetic corpus generated at sync time or absent
            eprintln!("no .mbp corpus at crates/mb-format/testdata/mbp — skipping");
            return;
        }
        for (p, bytes) in files {
            let doc = MbpDoc::parse(&bytes)
                .unwrap_or_else(|e| panic!("parse {:?}: {e}", p.file_name().unwrap()));
            assert_eq!(doc.write(), bytes, "byte-exact round-trip: {:?}", p.file_name());
        }
    }

    #[test]
    fn alias_edit_semantic_roundtrip() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/mbp");
        let files: Vec<_> = corpus();
        if files.is_empty() {
            eprintln!("no corpus — skipping alias edit test");
            return;
        }
        let (p, bytes) = files
            .iter()
            .find(|(p, _)| p.starts_with(&dir))
            .expect("corpus");
        let _ = p;
        let mut doc = MbpDoc::parse(bytes).unwrap();
        let before = doc.rows.len();
        doc.set_alias(0, "Тест себя").unwrap();
        let re = doc.write();
        let doc2 = MbpDoc::parse(&re).unwrap();
        assert_eq!(doc2.rows.len(), before);
        assert_eq!(doc2.rows[0].alias(), "Тест себя");
    }

    #[test]
    fn reject_bad_last_marker() {
        let b = vec![0u8; 80];
        assert!(MbpDoc::parse(&b).is_err());
    }
}
