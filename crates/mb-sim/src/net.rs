//! Minimal blocking Modbus servers for the simulator (std only).
//!
//! * [`serve`] — Modbus/TCP (MBAP framing).
//! * [`serve_rtu`] — Modbus RTU framing tunnelled over TCP (exercises the same
//!   RTU code path as a real serial port).
//!
//! Both servers are stoppable: [`SimServer::stop`] stops accepting new
//! connections and force-closes the live ones, freeing the bound port. A
//! client counter is kept for embedding UIs (the mbSCADA slave panel).

use crate::Sim;
use mb_proto::{mbap, rtu};
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// A running simulator: its bound address and shared device state.
pub struct SimServer {
    pub addr: SocketAddr,
    pub sim: Arc<Mutex<Sim>>,
    stop: Arc<AtomicBool>,
    clients: Arc<AtomicUsize>,
    /// Live client streams, force-closed by [`SimServer::stop`].
    streams: Arc<Mutex<Vec<TcpStream>>>,
}

impl SimServer {
    /// Stop accepting and drop all live client connections. The bound port
    /// is released shortly after (accept loop polls every 50 ms).
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Ok(mut v) = self.streams.lock() {
            for s in v.drain(..) {
                let _ = s.shutdown(Shutdown::Both);
            }
        }
    }

    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// Number of client connections currently served.
    pub fn clients(&self) -> usize {
        self.clients.load(Ordering::Relaxed)
    }
}

/// Decrements the client counter when a connection thread exits.
struct ClientGuard(Arc<AtomicUsize>);

impl Drop for ClientGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

fn accept_loop(
    listener: TcpListener,
    sim: Arc<Mutex<Sim>>,
    rtu_mode: bool,
    stop: Arc<AtomicBool>,
    clients: Arc<AtomicUsize>,
    streams: Arc<Mutex<Vec<TcpStream>>>,
) {
    thread::spawn(move || {
        let _ = listener.set_nonblocking(true);
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((s, _)) => {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Ok(cloned) = s.try_clone() {
                        if let Ok(mut v) = streams.lock() {
                            v.push(cloned);
                        }
                    }
                    let _ = s.set_nonblocking(true);
                    clients.fetch_add(1, Ordering::Relaxed);
                    let guard = ClientGuard(Arc::clone(&clients));
                    let stop2 = Arc::clone(&stop);
                    let sim2 = Arc::clone(&sim);
                    thread::spawn(move || {
                        let _guard = guard;
                        let _ = serve_conn(s, sim2, rtu_mode, stop2);
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(_) => break,
            }
        }
    });
}

/// Modbus/TCP (MBAP) server on `addr`.
pub fn serve(sim: Sim, addr: &str) -> std::io::Result<SimServer> {
    server(sim, addr, false)
}

/// RTU-over-TCP server on `addr`.
pub fn serve_rtu(sim: Sim, addr: &str) -> std::io::Result<SimServer> {
    server(sim, addr, true)
}

fn server(sim: Sim, addr: &str, rtu_mode: bool) -> std::io::Result<SimServer> {
    let listener = TcpListener::bind(addr)?;
    let local = listener.local_addr()?;
    let shared = Arc::new(Mutex::new(sim));
    let stop = Arc::new(AtomicBool::new(false));
    let clients = Arc::new(AtomicUsize::new(0));
    let streams: Arc<Mutex<Vec<TcpStream>>> = Arc::new(Mutex::new(Vec::new()));
    accept_loop(
        listener,
        Arc::clone(&shared),
        rtu_mode,
        Arc::clone(&stop),
        Arc::clone(&clients),
        Arc::clone(&streams),
    );
    Ok(SimServer {
        addr: local,
        sim: shared,
        stop,
        clients,
        streams,
    })
}

pub fn spawn_tcp(addr: &str, unit: u8) -> std::io::Result<SimServer> {
    serve(Sim::new(unit), addr)
}

/// Serve one non-blocking client connection until it disconnects or the
/// server is stopped. Frames are parsed incrementally from a byte buffer,
/// so partial reads never corrupt the stream.
fn serve_conn(
    mut stream: TcpStream,
    sim: Arc<Mutex<Sim>>,
    rtu_mode: bool,
    stop: Arc<AtomicBool>,
) -> std::io::Result<()> {
    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 2048];
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        match stream.read(&mut tmp) {
            Ok(0) => return Ok(()), // peer closed
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
                continue;
            }
            Err(_) => return Ok(()),
        }
        // Serve every complete frame the buffer holds.
        loop {
            let need = match if rtu_mode {
                rtu_frame_len(&buf)
            } else {
                mbap_frame_len(&buf)
            } {
                Some(n) => n,
                None => return Ok(()), // malformed — drop the connection
            };
            if buf.len() < need {
                break; // incomplete frame, read more
            }
            let frame: Vec<u8> = buf.drain(..need).collect();
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            let reply = {
                let mut s = match sim.lock() {
                    Ok(g) => g,
                    Err(_) => return Ok(()),
                };
                handle_frame(&mut s, &frame, rtu_mode)
            };
            match reply {
                Some(out) => {
                    if stream.write_all(&out).is_err() {
                        return Ok(());
                    }
                    let _ = stream.flush();
                }
                None => return Ok(()), // malformed — drop the connection
            }
        }
        if buf.len() > 65536 {
            return Ok(()); // runaway buffer guard
        }
    }
}

fn handle_frame(sim: &mut Sim, frame: &[u8], rtu_mode: bool) -> Option<Vec<u8>> {
    if rtu_mode {
        let (unit, pdu) = rtu::parse(frame).ok()?;
        let resp = sim.handle_pdu(&pdu);
        Some(rtu::encode(unit, &resp))
    } else {
        let parsed = mbap::parse(frame).ok()?;
        let out = sim.handle_mbap(&parsed).ok()?;
        Some(out.encode())
    }
}

/// Total MBAP frame length (None = header malformed, Some(n) = need n bytes).
fn mbap_frame_len(buf: &[u8]) -> Option<usize> {
    if buf.len() < 6 {
        return Some(6);
    }
    mbap::frame_len(&buf[..6]).ok()
}

/// Total RTU frame length (None = malformed, Some(n) = need n bytes).
/// Mirrors the frame shapes accepted by `read_rtu_request`.
fn rtu_frame_len(buf: &[u8]) -> Option<usize> {
    if buf.len() < 2 {
        return Some(2);
    }
    let fixed = match buf[1] {
        1 | 2 | 3 | 4 | 5 | 6 => 6, // addr + func + 4
        22 => 8,                    // addr + func + 6
        15 | 16 => {
            if buf.len() < 7 {
                return Some(7);
            }
            7 + buf[6] as usize // + byte count + data
        }
        23 => {
            if buf.len() < 11 {
                return Some(11);
            }
            11 + buf[10] as usize
        }
        _ => return None,
    };
    Some(fixed + 2) // CRC16
}

#[cfg(test)]
mod tests {
    use super::*;
    use mb_proto::pdu::{decode_response, Request, Response};
    use std::io::Write as _;

    #[test]
    fn tcp_roundtrip_clients_and_stop() {
        let server = serve(Sim::new(1), "127.0.0.1:0").expect("bind");
        let mut s = TcpStream::connect(server.addr).expect("connect");
        let req = Request::ReadHoldingRegisters { addr: 0, qty: 1 }.encode();
        let frame = mbap::MbapFrame {
            transaction: 7,
            unit: 1,
            pdu: req,
        }
        .encode();
        s.write_all(&frame).unwrap();
        s.flush().unwrap();
        thread::sleep(Duration::from_millis(150));
        assert_eq!(server.clients(), 1, "client counted");
        let mut head = vec![0u8; 6];
        s.read_exact(&mut head).unwrap();
        let total = mbap::frame_len(&head).unwrap();
        let mut rest = vec![0u8; total - 6];
        s.read_exact(&mut rest).unwrap();
        head.extend_from_slice(&rest);
        let f = mbap::parse(&head).unwrap();
        assert_eq!(
            decode_response(&f.pdu).unwrap(),
            Response::Registers(vec![0])
        );

        // stop() closes the client and frees the server
        server.stop();
        thread::sleep(Duration::from_millis(150));
        assert!(server.stopped());
        assert_eq!(server.clients(), 0, "client threads exited");
        let mut tmp = [0u8; 1];
        assert!(matches!(s.read(&mut tmp), Ok(0) | Err(_)), "client stream closed by stop()");
    }

    #[test]
    fn rtu_roundtrip_and_stop() {
        let server = serve_rtu(Sim::new(1), "127.0.0.1:0").expect("bind");
        let mut s = TcpStream::connect(server.addr).expect("connect");
        let req = Request::WriteSingleRegister { addr: 4, value: 1234 }.encode();
        s.write_all(&rtu::encode(1, &req)).unwrap();
        s.flush().unwrap();
        thread::sleep(Duration::from_millis(150));
        assert_eq!(server.clients(), 1);
        assert_eq!(server.sim.lock().unwrap().holding[4], 1234, "write applied");
        server.stop();
        thread::sleep(Duration::from_millis(150));
        assert_eq!(server.clients(), 0);
    }

    #[test]
    fn rtu_frame_len_shapes() {
        // func 3 request: addr, 03, hi, lo, qty-hi, qty-lo, crc-hi, crc-lo
        assert_eq!(rtu_frame_len(&[0x01, 0x03]), Some(8));
        // func 16 needs the byte count first
        assert_eq!(rtu_frame_len(&[0x01, 0x10, 0, 0, 0, 2, 4]), Some(13));
        assert_eq!(rtu_frame_len(&[0x01, 0x10]), Some(7));
        assert_eq!(rtu_frame_len(&[0x01, 0x99]), None);
    }
}
