//! Minimal blocking Modbus servers for the simulator (std only).
//!
//! * [`serve`] — Modbus/TCP (MBAP framing).
//! * [`serve_rtu`] — Modbus RTU framing tunnelled over TCP (exercises the same
//!   RTU code path as a real serial port).
//! * [`serve_serial`] — Modbus RTU on a **physical serial port** (COM / ttyUSB
//!   via a USB-RS485 adapter): same RTU framing, resync on line garbage.
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
use std::time::{Duration, Instant};

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


// ------------------------------------------------------------ serial RTU --

/// Serial line settings for a physical RTU slave (USB-RS485 adapter, COM port).
#[derive(Debug, Clone)]
pub struct SerialParams {
    /// OS port name: `COM3`, `/dev/ttyUSB0`, `/dev/ttyS0`…
    pub port: String,
    /// Baud rate (9600 is the Modbus default).
    pub baud: u32,
    /// Parity: 'N' | 'E' | 'O'.
    pub parity: char,
    /// Data bits: 7 | 8.
    pub data_bits: u8,
    /// Stop bits: 1 | 2.
    pub stop_bits: u8,
}

impl Default for SerialParams {
    fn default() -> Self {
        Self { port: String::new(), baud: 9600, parity: 'E', data_bits: 8, stop_bits: 1 }
    }
}

/// A running serial-RTU slave. The read loop wakes at least every 20 ms, so
/// [`SerialServer::stop`] takes effect promptly and the OS port is released.
pub struct SerialServer {
    stop: Arc<AtomicBool>,
    /// Human-readable line description, e.g. `COM3 @ 9600 8E1` (status line).
    pub desc: String,
}

impl SerialServer {
    /// Stop the serving loop; the port handle is dropped shortly after.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

fn parity_of(c: char) -> serialport::Parity {
    match c {
        'E' => serialport::Parity::Even,
        'O' => serialport::Parity::Odd,
        _ => serialport::Parity::None,
    }
}

fn data_bits_of(n: u8) -> serialport::DataBits {
    match n {
        5 => serialport::DataBits::Five,
        6 => serialport::DataBits::Six,
        7 => serialport::DataBits::Seven,
        _ => serialport::DataBits::Eight,
    }
}

fn stop_bits_of(n: u8) -> serialport::StopBits {
    match n {
        2 => serialport::StopBits::Two,
        _ => serialport::StopBits::One,
    }
}

fn open_serial(p: &SerialParams) -> std::io::Result<Box<dyn serialport::SerialPort>> {
    serialport::new(&p.port, p.baud)
        .data_bits(data_bits_of(p.data_bits))
        .parity(parity_of(p.parity))
        .stop_bits(stop_bits_of(p.stop_bits))
        .timeout(Duration::from_millis(20))
        .open()
        .map_err(|e| std::io::Error::other(serial_open_error(&e, &p.port)))
}

/// Human-readable serial open failure (port busy / missing / wrong params).
/// Same shape as mb-engine's `serial_open_error` — the app shows this text
/// verbatim in the slave panel footer.
fn serial_open_error(e: &serialport::Error, port: &str) -> String {
    let hint = match &e.kind {
        serialport::ErrorKind::Io(std::io::ErrorKind::PermissionDenied) => {
            "порт занят другой программой — закройте её и нажмите ⟳ Обновить"
        }
        serialport::ErrorKind::NoDevice
        | serialport::ErrorKind::Io(std::io::ErrorKind::NotFound) => {
            "порт не найден — подключите адаптер и нажмите ⟳ Обновить"
        }
        serialport::ErrorKind::InvalidInput => "неверные параметры линии для этого порта",
        _ => "",
    };
    let base = format!("COM-порт {port} открыть не удалось: {e}");
    if hint.is_empty() {
        base
    } else {
        format!("{base} ({hint})")
    }
}

/// Serve Modbus RTU on a physical serial port (USB-RS485 / COM / ttyUSB).
///
/// Framing is byte-count based (same shapes as RTU-over-TCP) with a silence
/// resync: a buffer that stays incomplete for >300 ms, a frame with a bad CRC
/// or an unknown function byte never wedges the loop — the line keeps working.
/// The device answers any unit address, like the TCP variants.
pub fn serve_serial(sim: Sim, p: &SerialParams) -> std::io::Result<SerialServer> {
    serve_serial_shared(Arc::new(Mutex::new(sim)), p)
}

/// Like [`serve_serial`], but the device state is shared with the caller
/// (the mbSCADA slave panel edits registers while masters poll the line).
pub fn serve_serial_shared(
    sim: Arc<Mutex<Sim>>,
    p: &SerialParams,
) -> std::io::Result<SerialServer> {
    let port = open_serial(p)?; // fail fast: busy / missing port
    let stop = Arc::new(AtomicBool::new(false));
    let desc = format!("{} @ {} {}{}{}", p.port, p.baud, p.data_bits, p.parity, p.stop_bits);
    let stop2 = Arc::clone(&stop);
    thread::spawn(move || serial_loop(port, sim, stop2));
    Ok(SerialServer { stop, desc })
}

fn serial_loop(
    mut port: Box<dyn serialport::SerialPort>,
    sim: Arc<Mutex<Sim>>,
    stop: Arc<AtomicBool>,
) {
    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 512];
    let mut first_byte: Option<Instant> = None;
    // Last response we wrote; on 2-wire RS-485 the adapter may hear it back.
    let mut last_tx: Vec<u8> = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        match port.read(&mut tmp) {
            Ok(0) => {}
            Ok(n) => {
                if buf.is_empty() {
                    first_byte = Some(Instant::now());
                }
                buf.extend_from_slice(&tmp[..n]);
            }
            Err(ref e)
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => break, // adapter unplugged / port error — leave the loop
        }
        // Silence resync: an incomplete frame older than 300 ms is garbage.
        if !buf.is_empty() {
            match first_byte {
                Some(t0) if t0.elapsed() > Duration::from_millis(300) => {
                    buf.clear();
                    first_byte = None;
                    continue;
                }
                None => first_byte = Some(Instant::now()),
                _ => {}
            }
        }
        // Drop an echo of our own response before parsing (see strip_own_echo).
        strip_own_echo(&mut buf, &last_tx);
        let mut replies: Vec<Vec<u8>> = Vec::new();
        {
            let mut s = match sim.lock() {
                Ok(g) => g,
                Err(_) => break,
            };
            process_serial_buf(&mut s, &mut buf, &mut replies);
        }
        if buf.is_empty() {
            first_byte = None;
        }
        for out in replies {
            if port.write_all(&out).is_err() {
                break;
            }
            let _ = port.flush();
            last_tx = out;
        }
    }
}

/// Drop a leading echo of our own last response (2-wire RS-485 adapters may
/// loop TX back to RX, so the slave hears the reply it just wrote). The echo
/// is byte-identical to what we sent, hence the exact prefix match; at most
/// one copy is removed so a real request that happens to look like the reply
/// is never eaten. Anything else goes through the CRC resync as before.
fn strip_own_echo(buf: &mut Vec<u8>, last_tx: &[u8]) {
    if !last_tx.is_empty() && buf.starts_with(last_tx) {
        buf.drain(..last_tx.len());
    }
}

/// Serve every complete RTU frame buffered so far, pushing raw reply frames.
/// Resync is byte-wise: a candidate frame whose CRC does not verify is dropped
/// one byte in, so garbage before a valid frame never blocks it; the same
/// applies to unknown function bytes. Incomplete input stays for the next read
/// (and the 300 ms silence guard clears it if the master died mid-frame).
fn process_serial_buf(sim: &mut Sim, buf: &mut Vec<u8>, replies: &mut Vec<Vec<u8>>) {
    loop {
        if buf.is_empty() {
            return;
        }
        match rtu_frame_len(buf) {
            Some(need) if buf.len() >= need => {
                // Validate the candidate BEFORE draining: a wrong unit byte in
                // front of a valid frame shifts the boundaries, and only a CRC
                // check can tell where the real frame starts.
                if rtu::parse(&buf[..need]).is_ok() {
                    let frame: Vec<u8> = buf.drain(..need).collect();
                    if let Some(out) = handle_frame(sim, &frame, true) {
                        replies.push(out);
                    }
                } else {
                    buf.drain(..1); // bad CRC — skip one byte and rescan
                }
            }
            Some(_) => return, // incomplete frame — wait for more bytes
            None => {
                buf.drain(..1); // unknown function byte — skip and rescan
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mb_proto::pdu::{decode_response, Request, Response};

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
    fn serial_buf_garbage_then_valid_frame() {
        let mut sim = Sim::new(1);
        let mut buf = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let req = Request::ReadHoldingRegisters { addr: 0, qty: 2 }.encode();
        buf.extend(rtu::encode(1, &req));
        let mut replies = Vec::new();
        process_serial_buf(&mut sim, &mut buf, &mut replies);
        assert_eq!(replies.len(), 1, "garbage prefix skipped, frame served");
        let expect = mb_proto::pdu::decode_response(
            &rtu::parse(&replies[0]).unwrap().1,
        ).unwrap();
        assert_eq!(expect, Response::Registers(vec![0, 0]));
        assert!(buf.is_empty(), "buffer fully consumed");
    }

    #[test]
    fn serial_buf_bad_crc_dropped_next_frame_served() {
        let mut sim = Sim::new(1);
        let mut frame = rtu::encode(
            1,
            &Request::WriteSingleRegister { addr: 4, value: 77 }.encode(),
        );
        let last = frame.len() - 1;
        frame[last] ^= 0xFF; // corrupt CRC
        let good = rtu::encode(
            1,
            &Request::WriteSingleRegister { addr: 5, value: 88 }.encode(),
        );
        let mut buf = frame;
        buf.extend_from_slice(&good);
        let mut replies = Vec::new();
        process_serial_buf(&mut sim, &mut buf, &mut replies);
        assert_eq!(replies.len(), 1, "bad-CRC frame dropped, good one served");
        assert_eq!(sim.holding[5], 88, "write after the dropped frame applied");
        assert!(!sim.holding.iter().any(|&v| v == 77), "corrupted write never applied");
        assert!(buf.is_empty());
    }

    #[test]
    fn serial_buf_incomplete_frame_waits() {
        let mut sim = Sim::new(1);
        let frame = rtu::encode(1, &Request::ReadHoldingRegisters { addr: 0, qty: 1 }.encode());
        let (head, _tail) = frame.split_at(frame.len() - 1);
        let mut buf = head.to_vec();
        let mut replies = Vec::new();
        process_serial_buf(&mut sim, &mut buf, &mut replies);
        assert!(replies.is_empty(), "incomplete frame not served");
        assert_eq!(buf.len(), head.len(), "bytes kept for the next read");
        buf.push(*frame.last().unwrap());
        process_serial_buf(&mut sim, &mut buf, &mut replies);
        assert_eq!(replies.len(), 1, "frame served once complete");
    }

    #[test]
    fn serial_buf_two_frames_in_one_read() {
        let mut sim = Sim::new(1);
        let a = rtu::encode(1, &Request::WriteSingleRegister { addr: 0, value: 10 }.encode());
        let b = rtu::encode(1, &Request::WriteSingleRegister { addr: 1, value: 20 }.encode());
        let mut buf = a;
        buf.extend_from_slice(&b);
        let mut replies = Vec::new();
        process_serial_buf(&mut sim, &mut buf, &mut replies);
        assert_eq!(replies.len(), 2, "both frames served from one read");
        assert_eq!((sim.holding[0], sim.holding[1]), (10, 20));
    }

    #[test]
    fn serial_buf_strips_own_response_echo() {
        // 2-wire RS-485: the slave hears its own reply, then the next request.
        let mut sim = Sim::new(1);
        let req = Request::ReadHoldingRegisters { addr: 0, qty: 1 }.encode();
        let reply = rtu::encode(1, &sim.handle_pdu(&req));
        let next = rtu::encode(
            1,
            &Request::WriteSingleRegister { addr: 2, value: 9 }.encode(),
        );
        let mut buf = reply.clone();
        buf.extend_from_slice(&next);
        strip_own_echo(&mut buf, &reply);
        assert_eq!(buf, next, "echo removed, the following request intact");

        // A request that is NOT the echo must pass untouched.
        let mut buf = next.clone();
        strip_own_echo(&mut buf, &reply);
        assert_eq!(buf, next);
    }

    #[test]
    fn serve_serial_invalid_port_fails_fast() {
        let params = SerialParams {
            port: if cfg!(windows) { "\\\\.\\NOSUCHCOMPORT42" } else { "/dev/mbscada-no-such-port-xyz" }.into(),
            ..SerialParams::default()
        };
        assert!(serve_serial(Sim::new(1), &params).is_err(), "missing port must error, not panic");
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
