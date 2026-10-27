//! End-to-end engine tests against the simulator (TCP and RTU-over-TCP).

use mb_engine::{
    AlarmSpec, ComputedTag, Cond, ConnKind, Connection, Engine, EngineCfg, Fmt, Poll, Rule,
    Severity,
};
use mb_sim::net::{serve, serve_rtu};
use mb_sim::Sim;
use std::collections::HashMap;
use std::time::Duration;

fn conn(id: &str, kind: ConnKind, port: u16) -> Connection {
    Connection {
        id: id.into(),
        name: id.into(),
        kind,
        host: "127.0.0.1".into(),
        port,
        serial: Default::default(),
        timeout_ms: 500,
        slave: 1,
    }
}

fn poll(conn_id: &str) -> Poll {
    let mut alarms = HashMap::new();
    alarms.insert(
        "0".to_string(),
        AlarmSpec {
            hihi: Some(100.0),
            msg_hihi: "ПЕРЕГРЕВ".into(),
            ..Default::default()
        },
    );
    let mut labels = HashMap::new();
    labels.insert("0".to_string(), "Температура".into());
    Poll {
        id: 1,
        name: "HR".into(),
        conn_id: conn_id.into(),
        slave: 1,
        func: 3,
        start: 0,
        raw_start: 0,
        qty: 4,
        scan_ms: 50,
        fmt: Fmt::U16,
        mult: 1.0,
        div: 1.0,
        offset: 0.0,
        unit: "°C".into(),
        enabled: true,
        base_one: false,
        notation: String::new(),
        labels,
        alarms,
    }
}

fn hot_rule() -> Rule {
    Rule {
        id: 1,
        name: "Перегрев".into(),
        poll: 1,
        offset: 0,
        cond: Cond::Gt { v: 100 },
        fg: Some("#ffffff".into()),
        bg: Some("#ef4444".into()),
        blink: true,
        severity: Some(Severity::Critical),
        message: Some("T > 100".into()),
        text: None,
    }
}

fn seeded() -> Sim {
    let mut sim = Sim::new(1);
    sim.holding[0] = 150; // triggers alarm + rule
    sim.holding[1] = 7;
    sim
}

#[test]
fn tcp_poll_alarm_rule_and_write() {
    let server = serve(seeded(), "127.0.0.1:0").expect("bind");
    let eng = Engine::start(
        vec![conn("c1", ConnKind::Tcp, server.addr.port())],
        vec![poll("c1")],
        vec![hot_rule()],
    );

    std::thread::sleep(Duration::from_millis(400));
    let snap = eng.snapshot();

    assert!(snap.running);
    assert!(snap.connections[0].stats.online, "connected");
    assert!(snap.connections[0].stats.rx >= 1, "received responses");
    let p = &snap.polls[0];
    assert_eq!(p.rows[0].value, 150.0);
    assert_eq!(p.rows[0].name, "Температура");
    assert_eq!(p.rows[0].unit, "°C");
    assert_eq!(p.rows[0].alarm_level, "hihi", "hi-hi alarm fired");
    assert_eq!(p.rows[0].bg.as_deref(), Some("#ef4444"), "rule colours row");
    assert!(p.rows[0].blink);
    assert_eq!(p.rows[1].value, 7.0);

    // write a register through the engine
    eng.write(1, 1, 42).expect("queued");
    std::thread::sleep(Duration::from_millis(250));
    let snap2 = eng.snapshot();
    assert_eq!(snap2.polls[0].rows[1].value, 42.0, "write reflected");
}

#[test]
fn rtu_over_tcp_uses_same_framing() {
    // Serves RTU frames over TCP: exercises the exact code path used by a
    // real serial (COM) port.
    let server = serve_rtu(seeded(), "127.0.0.1:0").expect("bind");
    let eng = Engine::start(
        vec![conn("c1", ConnKind::RtuOverTcp, server.addr.port())],
        vec![poll("c1")],
        vec![],
    );
    std::thread::sleep(Duration::from_millis(400));
    let snap = eng.snapshot();
    assert!(snap.connections[0].stats.online, "RTU connection online");
    assert_eq!(snap.polls[0].rows[0].value, 150.0, "RTU value decoded");
    assert_eq!(snap.polls[0].rows[1].value, 7.0);
}

#[test]
fn disconnected_is_reported() {
    let eng = Engine::start(
        vec![conn("c1", ConnKind::Tcp, 1)],
        vec![poll("c1")],
        vec![],
    );
    std::thread::sleep(Duration::from_millis(1200));
    let snap = eng.snapshot();
    assert!(!snap.connections[0].stats.online, "must not claim online");
    assert!(snap.connections[0].stats.errors >= 1, "errors counted");
    assert!(!snap.connections[0].stats.last_error.is_empty(), "error surfaced");
}

/// Two branches on the same target: the first matching branch wins
/// (Modbus Poll precedence), and a branch may substitute dynamic text.
fn style_rule(id: u32, lo: i64, hi: i64, bg: &str, text: Option<&str>) -> Rule {
    Rule {
        id,
        name: format!("b{id}"),
        poll: 1,
        offset: 0,
        cond: Cond::InRange { lo, hi },
        fg: Some("#000000".into()),
        bg: Some(bg.into()),
        blink: false,
        severity: None,
        message: None,
        text: text.map(|t| t.into()),
    }
}

#[test]
fn branch_order_and_dynamic_text() {
    let server = serve(seeded(), "127.0.0.1:0").expect("bind");
    let eng = Engine::start(
        vec![conn("c1", ConnKind::Tcp, server.addr.port())],
        vec![poll("c1")],
        vec![
            // branch 1: 120..200 — the seeded 150 must match THIS one
            style_rule(1, 120, 200, "#22c55e", Some("ГОТОВ")),
            // branch 2 also matches 150, but branch 1 has precedence
            style_rule(2, 100, 300, "#ef4444", Some("ПЕРЕГРЕВ")),
        ],
    );
    std::thread::sleep(Duration::from_millis(400));
    let snap = eng.snapshot();
    let row = &snap.polls[0].rows[0];
    assert_eq!(row.value, 150.0);
    assert_eq!(row.bg.as_deref(), Some("#22c55e"), "first matching branch wins");
    assert_eq!(row.name, "ГОТОВ", "branch text replaces the row label");
    // The poll's AlarmSpec (hihi=100) is independent of style branches and
    // still reports its threshold alarm.
    assert_eq!(row.alarm_level, "hihi");
}

#[test]
fn thirty_two_branches_per_row() {
    let server = serve(seeded(), "127.0.0.1:0").expect("bind");
    let rules: Vec<Rule> = (0..32)
        .map(|i| Rule {
            id: i as u32,
            name: format!("b{i}"),
            poll: 1,
            offset: 0,
            cond: Cond::Gt { v: i as i64 - 1 },
            fg: None,
            bg: Some(format!("#{:06x}", 0x100000 + i * 0x2020)),
            blink: false,
            severity: None,
            message: None,
            text: None,
        })
        .collect();
    // Sanity: branch counting / cap semantics for the UI editor.
    assert_eq!(mb_engine::branch_count(&rules, 1, 0), 32);
    assert!(mb_engine::branch_count(&rules, 2, 0) == 0);

    let eng = Engine::start(
        vec![conn("c1", ConnKind::Tcp, server.addr.port())],
        vec![poll("c1")],
        rules,
    );
    std::thread::sleep(Duration::from_millis(400));
    let snap = eng.snapshot();
    let row = &snap.polls[0].rows[0];
    // value 150 matches branches with threshold <= 150; ordered evaluation
    // keeps the LOWEST threshold (id 0) in force.
    assert_eq!(row.bg.as_deref(), Some("#100000"), "branch order = list order");
}

// ------------------------------------------------- computed tags + log file

fn ctag(name: &str, expr: &str) -> ComputedTag {
    ComputedTag {
        name: name.into(),
        expr: expr.into(),
        unit: String::new(),
        decimals: 2,
    }
}

fn find<'a>(snap: &'a mb_engine::Snapshot, name: &str) -> &'a mb_engine::ComputedView {
    snap.computed.iter().find(|c| c.name == name).unwrap()
}

#[test]
fn computed_tags_evaluate_from_labels_and_aliases() {
    let server = serve(seeded(), "127.0.0.1:0").expect("bind");
    // sim.holding = [150, 7, ...]; label of addr 0 is "Температура"
    let eng = Engine::start_with(EngineCfg {
        connections: vec![conn("c1", ConnKind::Tcp, server.addr.port())],
        polls: vec![poll("c1")],
        rules: vec![],
        computed: vec![
            ctag("Двойная", "{Температура} * 2"),
            ctag("Разница", "{p1:1} - {Температура}"),
            ctag("НетТэга", "{Нет такого} + 1"),
            ctag("Деление", "1 / 0"),
            ctag("ПлохойСинтаксис", "2 +"),
        ],
        log_path: None,
    });
    std::thread::sleep(Duration::from_millis(400));
    let snap = eng.snapshot();

    let d = find(&snap, "Двойная");
    assert!(d.ok, "ok: {}", d.err);
    assert_eq!(d.value, 300.0, "scaled from the labeled row");

    let r = find(&snap, "Разница");
    assert!(r.ok);
    assert_eq!(r.value, 7.0 - 150.0, "p1:1 alias = 7, label = 150");

    let nt = find(&snap, "НетТэга");
    assert!(!nt.ok);
    assert!(nt.err.contains("Нет такого"), "err: {}", nt.err);

    let div = find(&snap, "Деление");
    assert!(!div.ok);
    assert!(div.err.contains("ноль"));

    let syn = find(&snap, "ПлохойСинтаксис");
    assert!(!syn.ok, "compile error surfaced");
    assert!(!syn.err.is_empty());
}

#[test]
fn event_log_file_receives_journal_lines() {
    let server = serve(seeded(), "127.0.0.1:0").expect("bind");
    let dir = std::env::temp_dir().join(format!("mbscada-log-it-{}", std::process::id()));
    let path = dir.join("journal.log");
    let _ = std::fs::remove_dir_all(&dir);

    let eng = Engine::start_with(EngineCfg {
        connections: vec![conn("c1", ConnKind::Tcp, server.addr.port())],
        polls: vec![poll("c1")],
        rules: vec![],
        computed: vec![],
        log_path: Some(path.to_string_lossy().into_owned()),
    });
    std::thread::sleep(Duration::from_millis(500));
    drop(eng); // flushes the "движок остановлен" line

    let s = std::fs::read_to_string(&path).expect("log file written");
    assert!(s.contains("движок запущен"), "start event: {s}");
    assert!(s.contains("[SYS] engine:"), "system events tagged: {s}");
    assert!(s.contains("[RX]"), "poll exchanges logged: {s}");
    assert!(s.contains("движок остановлен"), "stop event: {s}");
    // Timestamped lines, not just HH:MM:SS
    assert!(s.contains(" [RX] "), "format: {{stamp}} [DIR] conn: text");
    let _ = std::fs::remove_dir_all(&dir);
}
