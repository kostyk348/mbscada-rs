//! Integration tests: semantic round-trip through our own writer.

use mb_format::{parse_mbp, parse_mbw, write_mbp, write_mbw, Row, TabRef, Table, Workspace};

fn sample_table() -> Table {
    Table {
        path: "t.mbp".into(),
        name: "t".into(),
        magic: mb_format::mbp::MAGIC_MBP_V4,
        func: 3,
        addr: 0,
        raw_addr: 0,
        qty: 3,
        period_ms: 1500,
        slave: 7,
        fmt16: 1,
        notation: "dec".into(),
        ip: String::new(),
        rows: vec![
            Row {
                addr: 0,
                alias: "Напряжение L1".into(),
            },
            Row {
                addr: 1,
                alias: "Напряжение L2".into(),
            },
            Row {
                addr: 2,
                alias: "Ток".into(),
            },
        ],
        confidence: 1.0,
        notes: String::new(),
    }
}

#[test]
fn mbp_semantic_roundtrip() {
    let t = sample_table();
    let bytes = write_mbp(&t);
    let back = parse_mbp(&bytes, "t.mbp", "t").expect("parse canonical output");

    assert_eq!(back.func, t.func);
    assert_eq!(back.addr, t.addr);
    assert_eq!(back.period_ms, t.period_ms);
    assert_eq!(back.slave, t.slave);
    assert_eq!(back.qty, t.qty);
    assert_eq!(back.rows, t.rows, "row aliases + addresses must survive");
}

#[test]
fn mbp_write_is_idempotent() {
    let t = sample_table();
    let a = write_mbp(&t);
    let parsed = parse_mbp(&a, "t", "t").unwrap();
    let b = write_mbp(&parsed);
    assert_eq!(a, b, "write -> parse -> write must be stable");
}

#[test]
fn mbw_semantic_roundtrip() {
    let w = Workspace {
        path: "w.mbw".into(),
        host: "10.1.2.3".into(),
        port: 502,
        tables: vec![TabRef {
            name: "a.mbp".into(),
            file: "a.mbp".into(),
            exists: false,
        }],
        notes: String::new(),
    };
    let bytes = write_mbw(&w);
    let back = parse_mbw(&bytes, "w.mbw").unwrap();
    assert_eq!(back.host, "10.1.2.3");
    assert_eq!(back.tables.len(), 1);
}
