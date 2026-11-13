//! Poll read coalescing: merge neighbouring polls into wider polls so the
//! engine fetches them with fewer Modbus transactions.
//!
//! Two polls may merge only when they share connection, slave, function
//! code, display format, scaling and scan rate, and their address grids
//! line up (for 32-bit formats the start address must be a multiple of the
//! registers-per-value). Ranges are merged when they touch or the gap
//! between them is at most [`gap`]; a merged range wider than the protocol
//! limit (125 registers for FC3/FC4, 2000 bits for FC1/FC2) is split back
//! into chunks on the value grid.
//!
//! Synthetic polls produced by chunking get ids from [`SYNTH_ID_BASE`] so
//! they never collide with user polls; the merged poll that replaces the
//! first member of a run keeps that member's id, so callers can map back.

use crate::format::Fmt;
use crate::model::{func_is_read, Poll};
use std::collections::HashMap;

/// Base for synthetic poll ids created when a merged run must be split into
/// protocol-sized chunks.
pub const SYNTH_ID_BASE: u32 = 0x4000_0000;

/// Max registers per FC3/FC4 request (protocol limit: 125).
const MAX_REGS: u16 = 125;
/// Max bits per FC1/FC2 request (protocol limit: 2000).
const MAX_BITS: u16 = 2000;

fn merge_key(p: &Poll) -> Option<(String, u8, u8, Fmt, u64, u64, u64, u64, u16)> {
    if !p.enabled || !func_is_read(p.func) {
        return None;
    }
    let step = p.regs_per_value() as u16;
    // Grids must line up on a common boundary or rows of a 32-bit member
    // would not be decoded by the merged poll.
    if p.start % step != 0 {
        return None;
    }
    Some((
        p.conn_id.clone(),
        p.slave,
        p.func,
        p.fmt,
        p.mult.to_bits(),
        p.div.to_bits(),
        p.offset.to_bits(),
        p.scan_ms,
        step,
    ))
}

/// Merge readable, enabled polls with matching keys. `gap` is the maximum
/// address distance between two ranges that still merges (0 = must touch).
/// Disabled polls, write polls and grid-misaligned polls pass through
/// unchanged. Returns the merged list; every input poll keeps at least the
/// coverage it had (merged ranges are supersets).
pub fn merge_polls(polls: &[Poll], gap: u16) -> Vec<Poll> {
    let mut out: Vec<Poll> = Vec::new();
    let mut groups: HashMap<(String, u8, u8, Fmt, u64, u64, u64, u64, u16), Vec<usize>> =
        HashMap::new();
    let mut order: Vec<_> = Vec::new();
    for (i, p) in polls.iter().enumerate() {
        match merge_key(p) {
            Some(k) => {
                if !groups.contains_key(&k) {
                    order.push(k.clone());
                }
                groups.entry(k).or_default().push(i);
            }
            None => out.push(p.clone()),
        }
    }
    for k in order {
        let idxs = &groups[&k];
        let max_chunk = if matches!(k.2, 1 | 2) {
            MAX_BITS
        } else {
            MAX_REGS
        };
        let mut members: Vec<&Poll> = idxs.iter().map(|&i| &polls[i]).collect();
        members.sort_by_key(|p| p.start);
        // Sweep: extend the current run while the next range starts within
        // `gap` of its end.
        let mut runs: Vec<(u16, u16, Vec<usize>)> = Vec::new(); // (lo, hi, member idxs)
        for (n, p) in members.iter().enumerate() {
            let lo = p.start;
            let hi = p.start.saturating_add(p.qty);
            match runs.last_mut() {
                Some((rlo, rhi, mem)) if lo <= *rhi + gap && hi >= *rlo => {
                    *rhi = (*rhi).max(hi);
                    mem.push(n);
                }
                _ => runs.push((lo, hi, vec![n])),
            }
        }
        for (lo, hi, mem) in runs {
            let first = members[mem[0]];
            // Split into protocol-sized chunks on the value grid.
            let mut s = lo;
            let mut chunk = 0;
            while s < hi {
                let room = max_chunk - (s - lo) % max_chunk;
                let take = room.min(hi - s);
                let e = s + take;
                let mut m = first.clone();
                m.start = s;
                m.qty = take;
                if chunk > 0 {
                    m.id = SYNTH_ID_BASE.wrapping_add(out.len() as u32);
                }
                m.name = merged_name(&members, &mem);
                m.labels.clear();
                m.alarms.clear();
                for &n in &mem {
                    let src = members[n];
                    for a in src.row_addrs() {
                        if a >= s && a < e {
                            m.labels.insert(a.to_string(), src.label_of(a));
                            if let Some(al) = src.alarm_of(a) {
                                m.alarms.insert(a.to_string(), al.clone());
                            }
                        }
                    }
                }
                out.push(m);
                s = e;
                chunk += 1;
            }
        }
    }
    out.sort_by_key(|p| (p.conn_id.clone(), p.slave, p.func, p.start));
    out
}

fn merged_name(members: &[&Poll], mem: &[usize]) -> String {
    let mut s = String::new();
    for &n in mem {
        if !s.is_empty() {
            s.push('+');
        }
        s.push_str(&members[n].name);
        if s.len() > 60 {
            s.push_str("+…");
            break;
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poll(id: u32, name: &str, func: u8, start: u16, qty: u16) -> Poll {
        Poll {
            id,
            name: name.into(),
            conn_id: "c1".into(),
            slave: 1,
            func,
            start,
            raw_start: 0,
            qty,
            scan_ms: 1000,
            fmt: Fmt::default(),
            mult: 1.0,
            div: 1.0,
            offset: 0.0,
            unit: String::new(),
            enabled: true,
            base_one: false,
            notation: String::new(),
            labels: HashMap::new(),
            alarms: HashMap::new(),
            source: String::new(),
        }
    }

    fn addr_set(p: &Poll) -> Vec<u16> {
        p.row_addrs()
    }

    #[test]
    fn adjacent_polls_merge_into_one() {
        let polls = vec![
            poll(1, "A", 3, 0, 10),
            poll(2, "B", 3, 10, 5),
            poll(3, "C", 3, 15, 2),
        ];
        let m = merge_polls(&polls, 0);
        assert_eq!(m.len(), 1);
        assert_eq!((m[0].start, m[0].qty), (0, 17));
        assert_eq!(m[0].id, 1, "keeps first member's id");
        assert_eq!(m[0].labels.len(), 17, "labels copied for member rows only");
        // every original row is covered on the merged grid
        for p in &polls {
            for a in addr_set(p) {
                assert!(addr_set(&m[0]).contains(&a));
            }
        }
    }

    #[test]
    fn gap_within_threshold_merges() {
        let polls = vec![poll(1, "A", 3, 0, 10), poll(2, "B", 3, 15, 5)];
        assert_eq!(merge_polls(&polls, 5).len(), 1);
        assert_eq!(merge_polls(&polls, 4).len(), 2);
    }

    #[test]
    fn different_scale_or_func_or_slave_never_merge() {
        // A gap of 50 would merge these if every key matched — so any test
        // below that returns 2 really is a key mismatch, not distance.
        let a = poll(1, "A", 3, 0, 10);
        let mut p2 = poll(2, "B", 3, 20, 5);
        assert_eq!(
            merge_polls(&[a.clone(), p2.clone()], 50).len(),
            1,
            "sanity: same key merges"
        );
        p2.func = 4;
        assert_eq!(
            merge_polls(&[a.clone(), p2.clone()], 50).len(),
            2,
            "func differs"
        );
        p2.func = 3;
        p2.mult = 2.0;
        assert_eq!(
            merge_polls(&[a.clone(), p2.clone()], 50).len(),
            2,
            "scale differs"
        );
        p2.mult = 1.0;
        p2.slave = 2;
        assert_eq!(
            merge_polls(&[a.clone(), p2.clone()], 50).len(),
            2,
            "slave differs"
        );
        p2.slave = 1;
        p2.scan_ms = 500;
        assert_eq!(merge_polls(&[a, p2], 50).len(), 2, "scan rate differs");
    }

    #[test]
    fn wide_range_splits_into_protocol_chunks() {
        let polls = vec![poll(1, "A", 3, 0, 300)];
        let m = merge_polls(&polls, 0);
        assert_eq!(m.len(), 3, "125-reg protocol cap forces chunks");
        assert_eq!((m[0].start, m[0].qty), (0, 125));
        assert_eq!((m[1].start, m[1].qty), (125, 125));
        assert_eq!((m[2].start, m[2].qty), (250, 50));
        assert_ne!(m[1].id, m[0].id, "chunk polls get distinct ids");
        assert_eq!(
            m[0].labels.len() + m[1].labels.len() + m[2].labels.len(),
            300
        );
        let bits = vec![poll(1, "B", 1, 0, 2500)];
        let mb = merge_polls(&bits, 0);
        assert_eq!(mb.len(), 2);
        assert_eq!(mb[0].qty, 2000);
        assert_eq!(mb[1].qty, 500);
    }

    #[test]
    fn misaligned_32bit_poll_passes_through() {
        let mut p = poll(1, "F", 3, 11, 6);
        p.fmt = Fmt::F32Abcd;
        let m = merge_polls(std::slice::from_ref(&p), 0);
        assert_eq!(m.len(), 1, "not merged, passes through unchanged");
        assert_eq!(m[0].start, 11);
        let mut ok = poll(2, "G", 3, 10, 4);
        ok.fmt = Fmt::F32Abcd;
        let m = merge_polls(&[p.clone(), ok.clone()], 0);
        assert_eq!(m.len(), 2, "aligned and misaligned stay separate");
        let mut p3 = poll(3, "H", 3, 14, 4);
        p3.fmt = Fmt::F32Abcd;
        let m = merge_polls(&[ok, p3], 0);
        assert_eq!(m.len(), 1);
        assert_eq!((m[0].start, m[0].qty), (10, 8));
        assert_eq!(addr_set(&m[0]), vec![10, 12, 14, 16]);
    }

    #[test]
    fn disabled_and_write_polls_pass_through() {
        let mut off = poll(1, "off", 3, 0, 10);
        off.enabled = false;
        let write = poll(2, "w", 6, 10, 1);
        let m = merge_polls(&[off, write], 0);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].func, 3);
        assert_eq!(m[1].func, 6);
    }

    #[test]
    fn duplicate_address_keeps_first_label() {
        let mut a = poll(1, "A", 3, 0, 4);
        a.labels.insert("2".into(), "first".into());
        let mut b = poll(2, "B", 3, 4, 4);
        b.labels.insert("2".into(), "second".into());
        let m = merge_polls(&[a, b], 0);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].labels.get("2").unwrap(), "first");
        assert_eq!(m[0].labels.get("6").unwrap(), "B[0006]");
    }
}
