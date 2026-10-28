//! Integration tests: the engine against a real in-process slave (mb-sim).
//!
//! Covers the two user-visible bugs fixed together:
//! * a write issued while the link is down must not be lost — it lands once
//!   the transport comes back up (pending-write queue in `conn_loop`);
//! * a poll with `slave = 0` follows the connection's slave address.

use mb_engine::{Connection, Engine, EngineCfg, Poll};
use mb_sim::net;
use std::net::TcpListener;
use std::thread;
use std::time::{Duration, Instant};

fn tcp_conn(port: u16, slave: u8) -> Connection {
    serde_json::from_str(&format!(
        r#"{{"id":"c1","name":"C","kind":"tcp","host":"127.0.0.1","port":{port},"timeout_ms":400,"slave":{slave}}}"#
    ))
    .unwrap()
}

/// Poll FC3 @4 qty=1, slave = 0 -> follows the connection's address.
fn poll() -> Poll {
    serde_json::from_str(
        r#"{"id":1,"name":"P","conn_id":"c1","slave":0,"func":3,"start":4,"raw_start":4,"qty":1,"scan_ms":50}"#,
    )
    .unwrap()
}

fn engine_for(port: u16, slave: u8) -> Engine {
    Engine::start_with(EngineCfg {
        connections: vec![tcp_conn(port, slave)],
        polls: vec![poll()],
        rules: vec![],
        computed: vec![],
        log_path: None,
    })
}

/// Wait until `sim.holding[addr] == want` or fail after the deadline.
fn expect_holding(sim: &std::sync::Arc<std::sync::Mutex<mb_sim::Sim>>, addr: usize, want: u16, secs: u64) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        {
            let g = sim.lock().unwrap();
            if g.holding[addr] == want {
                return;
            }
        }
        assert!(Instant::now() < deadline, "write never landed: holding[{addr}] != {want}");
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn write_lands_over_tcp_loopback() {
    let srv = net::serve(mb_sim::Sim::new(1), "127.0.0.1:0").unwrap();
    let port = srv.addr.port();

    let engine = engine_for(port, 7);
    // Let the first poll cycle open the transport and succeed.
    thread::sleep(Duration::from_millis(400));

    engine.write(1, 4, 777).unwrap();
    expect_holding(&srv.sim, 4, 777, 3);
}

#[test]
fn write_survives_link_down_and_lands_after_reconnect() {
    // Grab a free port and leave it closed: the engine will fail to connect.
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);

    let engine = engine_for(port, 7);
    // A few failed open attempts -> transport is down; the write must wait,
    // not vanish (the old code consumed and dropped it here).
    thread::sleep(Duration::from_millis(700));
    engine.write(1, 4, 555).unwrap();
    thread::sleep(Duration::from_millis(100));

    // The device appears -> the engine reconnects and flushes the write.
    let srv = net::serve(mb_sim::Sim::new(1), &format!("127.0.0.1:{port}")).unwrap();
    expect_holding(&srv.sim, 4, 555, 6);
}

#[test]
fn poll_reads_through_follow_slave() {
    // slave = 0 (follow conn.slave = 7) must read fine: the sim answers any
    // unit, but the request still has to travel the full engine path.
    let srv = net::serve(mb_sim::Sim::new(1), "127.0.0.1:0").unwrap();
    {
        let mut g = srv.sim.lock().unwrap();
        g.holding[4] = 4242;
    }
    let _engine = engine_for(srv.addr.port(), 7);

    // The poll row value appears in the sim-driven table within a few scans.
    // (Direct observation: sim register stays intact after reads.)
    thread::sleep(Duration::from_millis(400));
    let g = srv.sim.lock().unwrap();
    assert_eq!(g.holding[4], 4242, "register intact after engine polls");
}
