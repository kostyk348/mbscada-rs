//! `mb-opc` binary: CLI over the [`mb_opc::bridge`] library.

use mb_engine::coalesce::merge_polls;
use mb_engine::model::func_is_read;
use mb_engine::{Engine, EngineCfg};
use mb_opc::bridge::{
    build_server_and_space, check_port_free, parse_listen, pump, NodeKind, Project,
};
use std::io::BufRead as _;
use std::sync::Arc;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("mb-opc {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "mb-opc — OPC UA bridge for mbSCADA projects\n\n\
             USAGE: mb-opc --project <project.json> [options]\n\n\
             OPTIONS:\n  \
             --version             print version and exit\n  \
             --project <path>      project JSON (the file the mbSCADA app saves)\n  \
             --listen <host:port>  OPC UA TCP bind address [default: 0.0.0.0:4840]\n  \
             --coalesce-gap <n>    max register gap that still merges polls [default: 0]\n  \
             --no-coalesce         poll every range separately\n  \
             --snapshot-ms <n>     how often snapshots are pushed into the address space [default: 500]\n  \
             --log <path>          engine event log file (optional)\n\n\
             SECURITY: endpoint is None/anonymous — put it on a trusted network."
        );
        return;
    }
    let arg = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1).cloned())
    };

    // Ошибки запуска печатаются в одну строку и ждут Enter: окно консоли после
    // двойного клика иначе закрывается раньше, чем человек успеет прочитать
    // причину. В скриптах stdin в EOF — read_line не блокирует, паузы нет.
    fn die(msg: &str) -> ! {
        eprintln!("error: {msg}");
        let mut line = String::new();
        let _ = std::io::stdin().lock().read_line(&mut line);
        std::process::exit(2);
    }

    let project_path = match arg("--project") {
        Some(p) => p,
        None => die("--project <project.json> is required (see --help); the project file is what the mbSCADA app saves"),
    };
    let listen = arg("--listen").unwrap_or_else(|| "0.0.0.0:4840".into());
    let (host, port) = match parse_listen(&listen) {
        Ok(hp) => hp,
        Err(e) => die(&e),
    };
    let gap: u16 = match arg("--coalesce-gap") {
        Some(v) => match v.parse() {
            Ok(g) => g,
            Err(_) => die("--coalesce-gap must be a number"),
        },
        None => 0,
    };
    let coalesce = arg("--no-coalesce").is_none();
    let snapshot_ms: u64 = match arg("--snapshot-ms") {
        Some(v) => match v.parse() {
            Ok(g) => g,
            Err(_) => die("--snapshot-ms must be a number"),
        },
        None => 500,
    };
    let log_path = arg("--log");

    // Ранний чек порта: opcua при занятом адресе паникует из run() — человек
    // видит мгновенный вылет без причины. Сообщение печатается с подсказкой
    // альтернативного порта; сам слушатель чек освобождает сразу.
    if let Err(e) = check_port_free(&host, port) {
        let alt = if port == 4840 { 4841 } else { 4840 };
        die(&format!("{e}; try --listen 0.0.0.0:{alt}"));
    }

    let raw = match std::fs::read_to_string(&project_path) {
        Ok(r) => r,
        Err(e) => die(&format!("cannot read project {project_path}: {e}")),
    };
    let project: Project = match serde_json::from_str(&raw) {
        Ok(p) => p,
        Err(e) => die(&format!("cannot parse project {project_path}: {e}")),
    };

    let readable = |p: &&mb_engine::Poll| p.enabled && func_is_read(p.func);
    let n_before = project.polls.iter().filter(readable).count();

    // Coalesce polls for the engine; the address space keeps referring to
    // the original poll ids, so clients never see the merge.
    let engine_polls = if coalesce {
        let merged = merge_polls(&project.polls, gap);
        println!(
            "coalescing: {n_before} read polls -> {} transactions per cycle ({} saved)",
            merged.len(),
            n_before.saturating_sub(merged.len())
        );
        merged
    } else {
        project.polls.clone()
    };

    let n_rows: usize = project
        .polls
        .iter()
        .filter(readable)
        .map(|p| p.row_addrs().len())
        .sum();
    println!(
        "project: {} — {} connections, {} polls ({} rows), {} computed tags",
        project_path,
        project.connections.len(),
        project.polls.len(),
        n_rows,
        project.computed.len()
    );

    let engine = Arc::new(Engine::start_with(EngineCfg {
        connections: project.connections.clone(),
        polls: engine_polls,
        rules: project.rules.clone(),
        computed: project.computed.clone(),
        log_path,
    }));

    let (server, nodes, asp, _ns) = build_server_and_space(&project, &listen, &engine);
    println!(
        "address space: {} tag nodes, {} computed tags, {} status variables",
        nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Tag { .. }))
            .count(),
        project.computed.len(),
        project.connections.len() * 3
    );
    let n_writable = nodes
        .iter()
        .filter(|n| matches!(&n.kind, NodeKind::Tag { wr: Some(_), .. }))
        .count();
    println!(
        "listening on opc.tcp://{listen}/ (security: None, anonymous; {} of {} tags writable)",
        n_writable,
        nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Tag { .. }))
            .count()
    );

    // Snapshot pump: engine snapshots -> address space.
    let pump_nodes = nodes;
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(snapshot_ms));
        pump(&engine, &asp, &pump_nodes);
    });

    server.run();
    // unreachable in practice: run() blocks until the process is killed
}
