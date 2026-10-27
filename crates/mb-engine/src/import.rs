//! Import of Modbus Poll files (`.mbp` tables, `.mbw` workspaces).
//!
//! UI-independent: converts parsed files into [`Connection`]s plus [`Poll`]
//! definitions (with row labels), so it can be unit-tested without Tauri.

use crate::format::Fmt;
use crate::model::{ConnKind, Connection, Poll, SerialCfg};
use mb_format::Table;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct FileReport {
    pub path: String,
    pub kind: String,
    pub ok: bool,
    pub message: String,
    pub func: u8,
    pub start: u16,
    pub qty: u16,
    pub slave: u8,
    pub rows: usize,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportResult {
    pub connections: Vec<Connection>,
    pub polls: Vec<Poll>,
    pub reports: Vec<FileReport>,
}

/// Expand inputs: a directory becomes its `.mbp`/`.mbw` files (sorted).
pub fn expand(paths: Vec<String>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        let pb = PathBuf::from(p.trim());
        if pb.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&pb) {
                let mut v: Vec<PathBuf> = rd
                    .flatten()
                    .map(|e| e.path())
                    .filter(|q| {
                        matches!(
                            q.extension()
                                .and_then(|s| s.to_str())
                                .map(|s| s.to_ascii_lowercase())
                                .as_deref(),
                            Some("mbp") | Some("mbw")
                        )
                    })
                    .collect();
                v.sort();
                out.extend(v);
            }
        } else if !p.trim().is_empty() {
            out.push(pb);
        }
    }
    out
}

fn ext_of(p: &Path) -> String {
    p.extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default()
}

fn key_of(p: &Path) -> String {
    p.canonicalize()
        .unwrap_or_else(|_| p.to_path_buf())
        .to_string_lossy()
        .to_lowercase()
}

fn report_from(path: &str, kind: &str, ok: bool, message: String, t: Option<&Table>) -> FileReport {
    FileReport {
        path: path.to_string(),
        kind: kind.to_string(),
        ok,
        message,
        func: t.map(|x| x.func).unwrap_or(0),
        start: t.map(|x| x.addr as u16).unwrap_or(0),
        qty: t.map(|x| x.qty as u16).unwrap_or(0),
        slave: t.map(|x| x.slave).unwrap_or(0),
        rows: t.map(|x| x.rows.len()).unwrap_or(0),
        confidence: t.map(|x| x.confidence).unwrap_or(0.0),
    }
}

/// Convert a parsed `.mbp` table into a poll definition.
pub fn table_to_poll(t: &Table, conn_id: &str, poll_id: u32) -> Poll {
    let func = match t.func {
        1 | 5 | 15 => 1,
        2 => 2,
        4 => 4,
        _ => 3,
    };
    let qty = (t.qty as u16).clamp(1, 125);
    let fmt = match mb_format::display_format(t.fmt16) {
        "int16" => Fmt::I16,
        "int32" => Fmt::I32,
        "float32" => Fmt::F32Abcd,
        _ => Fmt::U16,
    };
    let mut labels = HashMap::new();
    for r in &t.rows {
        if !r.alias.is_empty() {
            labels.insert(r.addr.to_string(), r.alias.clone());
        }
    }
    Poll {
        id: poll_id,
        name: t.name.clone(),
        conn_id: conn_id.to_string(),
        slave: if t.slave == 0 { 1 } else { t.slave },
        func,
        start: t.raw_addr as u16,
        raw_start: t.raw_addr,
        qty,
        scan_ms: if t.period_ms < 20 {
            1000
        } else {
            t.period_ms as u64
        },
        fmt,
        mult: 1.0,
        div: 1.0,
        offset: 0.0,
        unit: String::new(),
        enabled: true,
        base_one: false,
        notation: "raw".into(),
        labels,
        alarms: HashMap::new(),
    }
}

fn conn(id: String, name: String, host: String, port: u16, slave: u8) -> Connection {
    Connection {
        id,
        name,
        kind: ConnKind::Tcp,
        host,
        port,
        serial: SerialCfg::default(),
        timeout_ms: 1000,
        slave: if slave == 0 { 1 } else { slave },
    }
}

/// Import `.mbw`/`.mbp` files or folders.
///
/// * each `.mbw` becomes its own connection (its host/port) with its tables;
/// * standalone `.mbp` files go to a default `127.0.0.1:502` connection;
/// * a table referenced by a workspace is not imported twice.
pub fn import_paths(paths: Vec<String>) -> ImportResult {
    let files = expand(paths);
    let mut reports = Vec::new();
    let mut polls = Vec::new();
    let mut connections = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut cid_seq = 0u32;
    let mut pid: u32 = 1000;

    // pass 1: workspaces (each owns a connection)
    for f in &files {
        if ext_of(f) != "mbw" {
            continue;
        }
        let disp = f.display().to_string();
        match mb_format::parse_mbw_file(f) {
            Ok(ws) => {
                let cid = format!("imp{}", cid_seq);
                cid_seq += 1;
                let name = f
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Импорт")
                    .to_string();
                reports.push(report_from(
                    &disp,
                    "workspace",
                    true,
                    format!("{} таблиц · {}:{}", ws.tables.len(), ws.host, ws.port),
                    None,
                ));
                connections.push(conn(cid.clone(), name, ws.host.clone(), ws.port, 1));
                for t in &ws.tables {
                    if !t.exists {
                        reports.push(report_from(
                            &t.file,
                            "table",
                            false,
                            "файл не найден".into(),
                            None,
                        ));
                        continue;
                    }
                    let tp = PathBuf::from(&t.file);
                    if !seen.insert(key_of(&tp)) {
                        reports.push(report_from(
                            &t.file,
                            "table",
                            true,
                            "пропущен (дубликат)".into(),
                            None,
                        ));
                        continue;
                    }
                    match mb_format::parse_mbp_file(&tp) {
                        Ok(tab) => {
                            reports.push(report_from(
                                &t.file,
                                "table",
                                true,
                                String::new(),
                                Some(&tab),
                            ));
                            polls.push(table_to_poll(&tab, &cid, pid));
                            pid += 1;
                        }
                        Err(e) => {
                            reports.push(report_from(&t.file, "table", false, e.to_string(), None))
                        }
                    }
                }
            }
            Err(e) => reports.push(report_from(&disp, "workspace", false, e.to_string(), None)),
        }
    }

    // pass 2: standalone .mbp (default connection, created lazily)
    let mut default_cid: Option<String> = None;
    for f in &files {
        if ext_of(f) != "mbp" {
            continue;
        }
        let disp = f.display().to_string();
        if !seen.insert(key_of(f)) {
            reports.push(report_from(&disp, "table", true, "пропущен (дубликат)".into(), None));
            continue;
        }
        match mb_format::parse_mbp_file(f) {
            Ok(tab) => {
                if default_cid.is_none() {
                    let cid = format!("imp{}", cid_seq);
                    cid_seq += 1;
                    connections.push(conn(
                        cid.clone(),
                        "Импорт (по умолчанию)".into(),
                        "127.0.0.1".into(),
                        502,
                        1,
                    ));
                    default_cid = Some(cid);
                }
                let cid = default_cid.clone().unwrap();
                reports.push(report_from(&disp, "table", true, String::new(), Some(&tab)));
                polls.push(table_to_poll(&tab, &cid, pid));
                pid += 1;
            }
            Err(e) => reports.push(report_from(&disp, "table", false, e.to_string(), None)),
        }
    }

    // anything else in an explicitly-passed path
    for f in &files {
        let e = ext_of(f);
        if e != "mbp" && e != "mbw" {
            reports.push(report_from(
                &f.display().to_string(),
                "?",
                false,
                "не .mbp/.mbw".into(),
                None,
            ));
        }
    }

    ImportResult {
        connections,
        polls,
        reports,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_table(dir: &Path, name: &str, start: u16) {
        let mut rows = Vec::new();
        for i in 0..4u32 {
            rows.push(mb_format::Row {
                addr: i,
                alias: format!("Тег{i}"),
            });
        }
        let table = Table {
            path: String::new(),
            name: name.into(),
            magic: 7000,
            func: 3,
            addr: start as u32,
            raw_addr: start as u32,
            qty: 4,
            period_ms: 1000,
            slave: 3,
            fmt16: 1,
            notation: "dec".into(),
            ip: String::new(),
            rows,
            confidence: 1.0,
            notes: String::new(),
        };
        std::fs::write(dir.join(format!("{name}.mbp")), mb_format::write_mbp(&table)).unwrap();
    }

    #[test]
    fn import_folder_produces_polls_and_labels() {
        let dir = std::env::temp_dir().join(format!("mbscada_imp_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_table(&dir, "t1", 0);

        let res = import_paths(vec![dir.display().to_string()]);
        assert_eq!(res.polls.len(), 1);
        assert_eq!(res.connections.len(), 1);
        assert!(res.reports.iter().any(|r| r.ok));

        let p = &res.polls[0];
        assert_eq!((p.func, p.start, p.qty), (3, 0, 4));
        assert_eq!(p.name, "t1");
        assert_eq!(p.labels.get("0").map(String::as_str), Some("Тег0"));
        assert_eq!(p.conn_id, res.connections[0].id);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn workspaces_get_own_connection_and_tables_arent_duplicated() {
        // Simulate a folder where a table is both standalone and referenced by
        // a workspace: it must be imported once, and the workspace connection
        // must own its host/port.
        let dir = std::env::temp_dir().join(format!("mbscada_imp2_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_table(&dir, "shared", 10);
        write_table(&dir, "only", 200);

        // Build a .mbw referencing shared.mbp using our writer.
        let ws = mb_format::Workspace {
            path: String::new(),
            host: "10.9.8.7".into(),
            port: 502,
            tables: vec![mb_format::TabRef {
                name: "shared.mbp".into(),
                file: dir.join("shared.mbp").display().to_string(),
                exists: true,
            }],
            notes: String::new(),
        };
        std::fs::write(dir.join("plant.mbw"), mb_format::write_mbw(&ws)).unwrap();

        let res = import_paths(vec![dir.display().to_string()]);
        // two connections: the workspace (plant) and the default for `only`
        assert_eq!(res.connections.len(), 2);
        assert!(res.connections.iter().any(|c| c.host == "10.9.8.7"));
        // two polls: shared (once, via workspace) + only
        assert_eq!(res.polls.len(), 2, "shared must not be imported twice");
        let shared = res.polls.iter().find(|p| p.name == "shared").unwrap();
        let wsconn = res.connections.iter().find(|c| c.host == "10.9.8.7").unwrap();
        assert_eq!(shared.conn_id, wsconn.id, "workspace table uses workspace conn");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_path_reports_error_and_no_connection() {
        let res = import_paths(vec!["Z:\\nope\\nope.mbp".into()]);
        assert!(res.polls.is_empty());
        assert!(res.connections.is_empty());
        assert!(res.reports.iter().all(|r| !r.ok));
    }
}
