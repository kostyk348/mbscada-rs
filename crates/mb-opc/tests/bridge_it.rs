//! End-to-end bridge test: Modbus simulator -> `mb-engine` -> bridge address
//! space -> a real OPC UA client session (in-process, opc.tcp over loopback).
//!
//! Verifies scaled values, bit tags, coalesced polls (neighbouring polls
//! collapse into fewer engine transactions), computed tags and connection
//! status variables.

use mb_engine::coalesce::merge_polls;
use mb_sim::Sim;
use mb_engine::{Engine, EngineCfg};
use mb_opc::bridge::{build_server_and_space, pump, Project};
use opcua::client::prelude::*;
use opcua::server::prelude::*;
use opcua::types::service_types::{ReadValueId, TimestampsToReturn};
use opcua::types::AttributeId;
use opcua::sync::RwLock;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// 3 connection, 4 polls: p1 and p2 are neighbouring f32 ranges (they must
/// coalesce into one transaction), p3 is a distant u32 range, p4 reads coils.
fn project_json(mb_port: u16) -> String {
    format!(
        r#"{{
  "connections": [
    {{"id": "c1", "name": "Reactor PLC", "kind": "tcp", "host": "127.0.0.1", "port": {mb_port}, "timeout_ms": 1000, "slave": 1}}
  ],
  "polls": [
    {{"id": 1, "name": "Temps", "conn_id": "c1", "slave": 1, "func": 3, "start": 0, "qty": 2, "scan_ms": 200, "fmt": "f32_abcd", "mult": 1.0, "div": 1.0, "labels": {{"0": "T reactor"}}, "unit": "°C"}},
    {{"id": 2, "name": "Pressures", "conn_id": "c1", "slave": 1, "func": 3, "start": 2, "qty": 2, "scan_ms": 200, "fmt": "f32_abcd", "mult": 1.0, "div": 1.0, "labels": {{"2": "P inlet"}}}},
    {{"id": 3, "name": "Counter", "conn_id": "c1", "slave": 1, "func": 3, "start": 20, "qty": 2, "scan_ms": 200, "fmt": "u32", "labels": {{"20": "N batches"}}}},
    {{"id": 4, "name": "Valves", "conn_id": "c1", "slave": 1, "func": 1, "start": 0, "qty": 10, "scan_ms": 200, "fmt": "u16", "labels": {{"5": "V5 open"}}}}
  ],
  "computed": [
    {{"name": "T-P index", "expr": "{{T reactor}} / max({{P inlet}}, 0.001)", "unit": "", "decimals": 2}}
  ]
}}"#
    )
}

fn read_value(session: &Arc<RwLock<Session>>, id: &NodeId) -> DataValue {
    let rv = ReadValueId {
        node_id: id.clone(),
        attribute_id: AttributeId::Value as u32,
        index_range: UAString::null(),
        data_encoding: QualifiedName::new(0, ""),
    };
    let s = session.write();
    s.read(&[rv], TimestampsToReturn::Both, 0.0).expect("read ok").remove(0)
}

fn as_f64(dv: &DataValue) -> f64 {
    match dv.value.as_ref().expect("value present") {
        Variant::Double(v) => *v,
        Variant::Float(v) => *v as f64,
        other => panic!("expected a number, got {other:?}"),
    }
}

fn as_bool(dv: &DataValue) -> bool {
    match dv.value.as_ref().expect("value present") {
        Variant::Boolean(v) => *v,
        other => panic!("expected a boolean, got {other:?}"),
    }
}

fn as_u64(dv: &DataValue) -> u64 {
    match dv.value.as_ref().expect("value present") {
        Variant::UInt64(v) => *v,
        Variant::UInt32(v) => *v as u64,
        other => panic!("expected a counter, got {other:?}"),
    }
}

#[test]
fn bridge_end_to_end() {
    opcua::console_logging::init();
    // --- Modbus side: a simulator with known register contents -------------
    let mut sim = Sim::new(1);
    {
        // f32 25.5 = 0x41CC0000 (ABCD, high word first) at 0..1
        sim.holding[0] = 0x41CC;
        sim.holding[1] = 0x0000;
        // f32 12.3 at 2..3 (0x4144CCCC)
        sim.holding[2] = 0x4144;
        sim.holding[3] = 0xCCCC;
        // u32 10000 = 0x00002710 at 20..21
        sim.holding[20] = 0x0000;
        sim.holding[21] = 0x2710;
    }
    sim.coils[5] = true;
    let mb_srv = mb_sim::net::serve(sim, "127.0.0.1:0").expect("sim server");
    let mb_port = mb_srv.addr.port();

    // --- Project + coalescing ---------------------------------------------
    let raw = project_json(mb_port);
    let project: Project = serde_json::from_str(&raw).expect("project parses");
    let merged = merge_polls(&project.polls, 0);
    let fc3: Vec<_> = merged.iter().filter(|p| p.func == 3).collect();
    assert_eq!(fc3.len(), 2, "p1+p2 coalesce into one range, p3 stays alone");
    assert_eq!((fc3[0].start, fc3[0].qty), (0, 4));
    assert_eq!(merged.len(), 3, "3 transactions per cycle instead of 4");

    // --- Engine + bridge ----------------------------------------------------
    let engine = Arc::new(Engine::start_with(EngineCfg {
        connections: project.connections.clone(),
        polls: merged,
        rules: project.rules.clone(),
        computed: project.computed.clone(),
        log_path: None,
    }));

    let opc_port = free_port();
    let (server, nodes, asp) =
        build_server_and_space(&project, &format!("127.0.0.1:{opc_port}"));
    let server = Arc::new(RwLock::new(server));
    std::thread::spawn(move || Server::run_server(server));
    std::thread::sleep(Duration::from_millis(800));

    // --- OPC UA client ------------------------------------------------------
    let mut client = ClientBuilder::new()
        .application_name("bridge-test")
        .application_uri("urn:bridge-test")
        .endpoints(vec![(
            "none",
            ClientEndpoint {
                url: format!("opc.tcp://127.0.0.1:{opc_port}/"),
                security_policy: String::from(SecurityPolicy::None.to_str()),
                security_mode: String::from(MessageSecurityMode::None),
                user_token_id: "ANONYMOUS".to_string(),
            },
        )])
        .client()
        .expect("client config valid");

    let session = client
        .connect_to_endpoint_id(Some("none"))
        .expect("client connects to the bridge");

    let ns2 = |s: &str| NodeId::new(2, s.to_string());

    // --- Pump snapshots until the tags go good (or fail after 15 s) ---------
    let tag_ids: Vec<NodeId> = ["p1:0", "p2:2", "p3:20", "p4:5"]
        .iter()
        .map(|s| ns2(s))
        .collect();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut values: Vec<DataValue> = Vec::new();
    loop {
        pump(&engine, &asp, &nodes);
        values = tag_ids.iter().map(|id| read_value(&session, id)).collect();
        if values.iter().all(|v| v.status == Some(StatusCode::Good)) {
            break;
        }
        assert!(Instant::now() < deadline, "tags never went good: {values:?}");
        std::thread::sleep(Duration::from_millis(250));
    }

    assert!((as_f64(&values[0]) - 25.5).abs() < 1e-4, "T reactor = 25.5, got {}", values[0].value.clone().unwrap());
    assert!((as_f64(&values[1]) - 12.3).abs() < 1e-4, "P inlet = f32 12.3, got {}", values[1].value.clone().unwrap());
    assert!((as_f64(&values[2]) - 10000.0).abs() < 1e-9, "N batches = 10000, got {}", values[2].value.clone().unwrap());
    assert!(as_bool(&values[3]), "V5 open = true");

    // Live update: change a register, the value must follow.
    mb_srv.sim.lock().unwrap().holding[0] = 0x41D0; // f32 26.0
    mb_srv.sim.lock().unwrap().holding[1] = 0x0000;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        pump(&engine, &asp, &nodes);
        let v = read_value(&session, &ns2("p1:0"));
        if v.status == Some(StatusCode::Good) && (as_f64(&v) - 26.0).abs() < 1e-4 {
            break;
        }
        assert!(Instant::now() < deadline, "value never updated to 26.0");
        std::thread::sleep(Duration::from_millis(250));
    }

    // Computed tag tracks its sources.
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        pump(&engine, &asp, &nodes);
        let v = read_value(&session, &ns2("calc:0"));
        if v.status == Some(StatusCode::Good) && as_f64(&v) > 26.0 / 12.4 {
            // 26.0 / 12.3 ≈ 2.11; guard against the initial 0/0 state
            break;
        }
        assert!(Instant::now() < deadline, "computed tag never updated: {v:?}");
        std::thread::sleep(Duration::from_millis(250));
    }
    let calc = read_value(&session, &ns2("calc:0"));
    assert!((as_f64(&calc) - 26.0 / 12.3).abs() < 1e-4, "calc = T/P, got {}", calc.value.unwrap());

    // Status variables: online, and tx grows (engine is polling).
    let online = read_value(&session, &ns2("conn:c1:online"));
    assert_eq!(online.status, Some(StatusCode::Good));
    assert!(as_bool(&online), "connection is online");
    let tx1 = as_u64(&read_value(&session, &ns2("conn:c1:tx")));
    std::thread::sleep(Duration::from_millis(1200));
    pump(&engine, &asp, &nodes);
    let tx2 = as_u64(&read_value(&session, &ns2("conn:c1:tx")));
    assert!(tx2 >= tx1 + 3, "tx grows over time ({} -> {})", tx1, tx2);

    mb_srv.stop();
}
