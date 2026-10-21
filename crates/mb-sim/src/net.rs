//! Minimal blocking Modbus servers for the simulator (std only).
//!
//! * [`serve`] — Modbus/TCP (MBAP framing).
//! * [`serve_rtu`] — Modbus RTU framing tunnelled over TCP (exercises the same
//!   RTU code path as a real serial port).

use crate::Sim;
use mb_proto::{mbap, rtu};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

/// A running simulator: its bound address and shared device state.
pub struct SimServer {
    pub addr: SocketAddr,
    pub sim: Arc<Mutex<Sim>>,
}

fn accept_loop(listener: TcpListener, sim: Arc<Mutex<Sim>>, rtu_mode: bool) {
    thread::spawn(move || {
        for stream in listener.incoming() {
            if let Ok(s) = stream {
                let sim = Arc::clone(&sim);
                thread::spawn(move || {
                    let _ = if rtu_mode {
                        serve_rtu_conn(s, sim)
                    } else {
                        serve_conn(s, sim)
                    };
                });
            }
        }
    });
}

/// Modbus/TCP (MBAP) server on `addr`.
pub fn serve(sim: Sim, addr: &str) -> std::io::Result<SimServer> {
    let listener = TcpListener::bind(addr)?;
    let local = listener.local_addr()?;
    let shared = Arc::new(Mutex::new(sim));
    accept_loop(listener, Arc::clone(&shared), false);
    Ok(SimServer {
        addr: local,
        sim: shared,
    })
}

/// RTU-over-TCP server on `addr`.
pub fn serve_rtu(sim: Sim, addr: &str) -> std::io::Result<SimServer> {
    let listener = TcpListener::bind(addr)?;
    let local = listener.local_addr()?;
    let shared = Arc::new(Mutex::new(sim));
    accept_loop(listener, Arc::clone(&shared), true);
    Ok(SimServer {
        addr: local,
        sim: shared,
    })
}

pub fn spawn_tcp(addr: &str, unit: u8) -> std::io::Result<SimServer> {
    serve(Sim::new(unit), addr)
}

fn serve_conn(mut stream: TcpStream, sim: Arc<Mutex<Sim>>) -> std::io::Result<()> {
    let mut header = [0u8; 6];
    loop {
        if stream.read_exact(&mut header).is_err() {
            return Ok(());
        }
        let total = match mbap::frame_len(&header) {
            Ok(t) => t,
            Err(_) => return Ok(()),
        };
        let mut rest = vec![0u8; total - 6];
        stream.read_exact(&mut rest)?;
        let mut frame = header.to_vec();
        frame.extend_from_slice(&rest);
        let parsed = match mbap::parse(&frame) {
            Ok(f) => f,
            Err(_) => return Ok(()),
        };
        let reply = {
            let mut s = sim.lock().unwrap();
            s.handle_mbap(&parsed)
        };
        if let Ok(f) = reply {
            stream.write_all(&f.encode())?;
            stream.flush()?;
        }
    }
}

/// Read one RTU request frame (unit..CRC) from a stream.
fn read_rtu_request<R: Read>(io: &mut R) -> std::io::Result<Vec<u8>> {
    fn take<R: Read>(io: &mut R, n: usize, buf: &mut Vec<u8>) -> std::io::Result<()> {
        let mut t = vec![0u8; n];
        io.read_exact(&mut t)?;
        buf.extend_from_slice(&t);
        Ok(())
    }
    let mut head = [0u8; 2];
    io.read_exact(&mut head)?;
    let mut buf = vec![head[0], head[1]];
    match head[1] {
        1 | 2 | 3 | 4 | 5 | 6 => take(io, 4, &mut buf)?,
        22 => take(io, 6, &mut buf)?,
        15 | 16 | 23 => {
            // for 23 the leading fields are longer; read 8 then byte count
            if head[1] == 23 {
                take(io, 8, &mut buf)?;
            } else {
                take(io, 4, &mut buf)?;
            }
            let mut bc = [0u8; 1];
            io.read_exact(&mut bc)?;
            buf.push(bc[0]);
            take(io, bc[0] as usize, &mut buf)?;
        }
        _ => {}
    }
    take(io, 2, &mut buf)?; // CRC
    Ok(buf)
}

fn serve_rtu_conn(mut stream: TcpStream, sim: Arc<Mutex<Sim>>) -> std::io::Result<()> {
    loop {
        let frame = match read_rtu_request(&mut stream) {
            Ok(f) => f,
            Err(_) => return Ok(()),
        };
        let (unit, pdu) = match rtu::parse(&frame) {
            Ok(x) => x,
            Err(_) => return Ok(()),
        };
        let resp = {
            let mut s = sim.lock().unwrap();
            s.handle_pdu(&pdu)
        };
        stream.write_all(&rtu::encode(unit, &resp))?;
        stream.flush()?;
    }
}
