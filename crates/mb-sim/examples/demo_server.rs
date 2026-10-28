//! Demo Modbus/TCP slave for UI screenshots and manual testing.
//!
//! Run: cargo run -p mb-sim --example demo_server -- [port]
//! Holding registers: [150, 7, 42, 0, 1234, 3] — designed so that a
//! "value > 100" style rule lights up row 0 (hot) while others stay calm.

fn main() {
    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1502);
    let mut sim = mb_sim::Sim::new(1);
    let seed: [u16; 6] = [150, 7, 42, 0, 1234, 3];
    for (i, v) in seed.iter().enumerate() {
        sim.holding[i] = *v;
    }
    let _server = mb_sim::net::serve(sim, &format!("127.0.0.1:{port}")).expect("bind");
    println!("demo slave on 127.0.0.1:{port} (Ctrl+C to stop)");
    loop {
        std::thread::park();
    }
}
