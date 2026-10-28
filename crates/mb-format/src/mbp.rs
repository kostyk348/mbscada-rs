//! `.mbp` (single table) parser and writer.

use crate::error::{Error, Result};
use crate::model::{decode_addr, notation_str, valid_func, Row, Table};
use crate::util::{iter_markers, push_str_marker, push_u32, rd32, PAD, MARKER};

pub const MAGIC_MBP_V4: u32 = 7000;
pub const MAGIC_MBP_V5: u32 = 10010;

/// The trailing metadata block: `[period][slave][1][0][0][qty]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tail {
    pub offset: usize,
    pub period: u32,
    pub slave: u32,
    pub qty: u32,
}

/// Scan backwards for the tail block (must sit at least 40 bytes before EOF).
pub fn find_tail(d: &[u8]) -> Option<Tail> {
    let n = d.len();
    if n < 24 {
        return None;
    }
    let start = n.saturating_sub(2000);
    let mut off = n as isize - 40;
    while off >= start as isize {
        let o = off as usize;
        if let (Some(p), Some(s)) = (rd32(d, o), rd32(d, o + 4)) {
            if (50..=60000).contains(&p)
                && (1..=247).contains(&s)
                && rd32(d, o + 8) == Some(1)
                && rd32(d, o + 12) == Some(0)
                && rd32(d, o + 16) == Some(0)
            {
                if let Some(q) = rd32(d, o + 20) {
                    if (1..=2000).contains(&q) {
                        return Some(Tail {
                            offset: o,
                            period: p,
                            slave: s,
                            qty: q,
                        });
                    }
                }
            }
        }
        off -= 4;
    }
    None
}

/// Parse a `.mbp` byte buffer. `path`/`name` are carried through for reporting.
pub fn parse_mbp(data: &[u8], path: &str, name: &str) -> Result<Table> {
    let n = data.len();
    if n < 64 {
        return Err(Error::Parse("file smaller than 64 bytes".into()));
    }
    let magic = rd32(data, 0).ok_or_else(|| Error::Parse("truncated header".into()))?;

    let (ip, p144) = match magic {
        MAGIC_MBP_V4 => (String::new(), 4usize),
        MAGIC_MBP_V5 => {
            if data.get(4..7) != Some(MARKER.as_slice()) {
                return Err(Error::Parse("v5: IP marker missing".into()));
            }
            let ln = data[7] as usize;
            if !(1..=40).contains(&ln) {
                return Err(Error::Parse("v5: bad IP length".into()));
            }
            let a = 8;
            let b = (a + ln * 2).min(n);
            let ip = crate::util::utf16le_decode(&data[a..b]);
            (ip, 4 + 4 + ln * 2)
        }
        other => return Err(Error::Parse(format!("unknown magic {other}"))),
    };

    let func = rd32(data, p144 + 4).ok_or_else(|| Error::Parse("truncated (func)".into()))? as u8;
    let raw_addr = rd32(data, p144 + 8).ok_or_else(|| Error::Parse("truncated (addr)".into()))?;
    // offset p144+12 is the quantity (verified against real files: q11->10, RS1->5).
    let qty_raw = rd32(data, p144 + 12).ok_or_else(|| Error::Parse("truncated (qty)".into()))?;
    if !valid_func(func) {
        return Err(Error::Parse(format!("unsupported function code {func}")));
    }
    // Addresses 0xFFF0..=0xFFFF are not register windows (they are address-list /
    // connection records that also use the .mbp extension).
    if raw_addr >= 0xFFF0 {
        return Err(Error::Parse(format!("не окно регистров (адрес 0x{raw_addr:04X})")));
    }
    let (addr, notation) = decode_addr(raw_addr, func);
    let qty = qty_raw.clamp(1, 125);

    // Row aliases: marker-prefixed UTF-16 strings after the header, in order.
    let hdr_end = p144 + 16;
    let mut rows: Vec<Row> = iter_markers(data, hdr_end, n)
        .into_iter()
        .take(qty as usize)
        .map(|m| Row {
            addr: 0,
            alias: m.text,
        })
        .collect();
    while rows.len() < qty as usize {
        rows.push(Row {
            addr: 0,
            alias: String::new(),
        });
    }
    for (i, r) in rows.iter_mut().enumerate() {
        // label keys must match the addresses the poll actually uses (raw)
        r.addr = raw_addr.wrapping_add(i as u32);
    }
    let named = rows.iter().filter(|r| !r.alias.is_empty()).count();
    let confidence = if named > 0 { 0.9 } else { 0.7 };

    // Trailing metadata block [period][slave][1][0][0][qty] — written by our
    // canonical writer and (in the same layout) by real Modbus Poll files.
    // Recover period + slave when the backward scan finds it; otherwise keep
    // the UI defaults. Before v1.4.3 the tail was never consulted, so every
    // imported poll silently got slave = 1 regardless of the file's device
    // address (the "polls keep adding unit 1" bug).
    let (period_ms, slave, tail_note) = match find_tail(data) {
        Some(t) => (
            t.period,
            t.slave as u8,
            format!("хвост: период {} мс, slave {}", t.period, t.slave),
        ),
        None => (
            1000,
            1,
            "хвост не найден — период 1000 мс, slave 1 (умолчания)".into(),
        ),
    };

    Ok(Table {
        path: path.to_string(),
        name: name.to_string(),
        magic,
        func,
        addr,
        raw_addr,
        qty,
        period_ms, // recovered from the tail block; 1000 when absent
        slave,     // recovered from the tail block; 1 when absent
        fmt16: 0,
        notation: notation_str(notation).to_string(),
        ip,
        rows,
        confidence,
        notes: format!("qty из заголовка; именованных строк {named}; {tail_note}"),
    })
}

/// Serialize a table to a canonical v4 `.mbp` buffer.
///
/// This is our own canonical layout (round-trips semantically through
/// [`parse_mbp`]); it is not guaranteed byte-identical to Modbus Poll output.
pub fn write_mbp(t: &Table) -> Vec<u8> {
    let mut out = Vec::new();
    push_u32(&mut out, MAGIC_MBP_V4);
    push_u32(&mut out, 144);
    push_u32(&mut out, t.func as u32);
    push_u32(&mut out, t.addr); // identity (decimal) encoding
    let hdr_qty = if !t.rows.is_empty() {
        t.rows.len() as u32
    } else {
        t.qty
    };
    push_u32(&mut out, hdr_qty);

    for r in &t.rows {
        push_str_marker(&mut out, &r.alias);
        out.extend_from_slice(&PAD);
        for _ in 0..6 {
            push_u32(&mut out, 0xFF00_0000); // colour block (unused on read)
        }
    }

    // Gap so the tail sits >= 40 bytes from EOF for the backward scan to find it.
    out.extend(std::iter::repeat(0u8).take(48));

    push_u32(&mut out, t.period_ms.max(1));
    push_u32(&mut out, (t.slave.max(1)) as u32);
    push_u32(&mut out, 1);
    push_u32(&mut out, 0);
    push_u32(&mut out, 0);
    let qty = if t.qty > 0 { t.qty } else { t.rows.len() as u32 };
    push_u32(&mut out, qty);

    // 16 trailing bytes, then the closing empty marker (real files end with
    // one; `MbpDoc::parse` relies on it), plus a 4-byte trailer like the originals.
    out.extend(std::iter::repeat(0u8).take(16));
    out.extend_from_slice(&MARKER);
    out.push(0);
    out.extend_from_slice(&[0u8; 4]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_roundtrip() {
        let mut b = Vec::new();
        push_u32(&mut b, 1000);
        push_u32(&mut b, 7);
        push_u32(&mut b, 1);
        push_u32(&mut b, 0);
        push_u32(&mut b, 0);
        push_u32(&mut b, 44);
        b.extend(std::iter::repeat(0u8).take(16));
        let t = find_tail(&b).expect("tail");
        assert_eq!((t.period, t.slave, t.qty), (1000, 7, 44));
    }

    #[test]
    fn reject_small_and_bad_magic() {
        assert!(parse_mbp(&[0u8; 10], "x", "x").is_err());
        let mut b = vec![0u8; 128];
        b[0..4].copy_from_slice(&1234u32.to_le_bytes());
        assert!(parse_mbp(&b, "x", "x").is_err());
    }
}
