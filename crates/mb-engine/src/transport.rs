//! Transports: Modbus/TCP (MBAP), RTU over serial, RTU over TCP,
//! Modbus ASCII over serial / TCP.
//!
//! One `call` = one request PDU -> one response PDU. All framing differences
//! (MBAP vs RTU+CRC vs ':'+hex+LRC) live here; the engine stays
//! transport-agnostic.

use crate::model::{ConnKind, Connection};
use mb_proto::error::ProtoError;
use mb_proto::{ascii, mbap, rtu};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::thread;
use std::time::{Duration, Instant};

pub trait Transport: Send {
    fn call(&mut self, unit: u8, pdu: &[u8]) -> Result<Vec<u8>, ProtoError>;
}

fn io_err(e: std::io::Error) -> ProtoError {
    ProtoError::Io(e.to_string())
}

// ----------------------------------------------------------------- Modbus/TCP

pub struct TcpTransport {
    stream: TcpStream,
    tx: u16,
}

impl TcpTransport {
    pub fn connect(host: &str, port: u16, timeout: Duration) -> Result<Self, ProtoError> {
        let addr = (host, port)
            .to_socket_addrs()
            .map_err(io_err)?
            .next()
            .ok_or_else(|| ProtoError::Io("no address".into()))?;
        let stream = TcpStream::connect_timeout(&addr, timeout).map_err(io_err)?;
        stream
            .set_read_timeout(Some(timeout))
            .and_then(|_| stream.set_write_timeout(Some(timeout)))
            .and_then(|_| stream.set_nodelay(true))
            .map_err(io_err)?;
        Ok(Self { stream, tx: 0 })
    }
}

impl Transport for TcpTransport {
    fn call(&mut self, unit: u8, pdu: &[u8]) -> Result<Vec<u8>, ProtoError> {
        self.tx = self.tx.wrapping_add(1);
        let frame = mbap::MbapFrame {
            transaction: self.tx,
            unit,
            pdu: pdu.to_vec(),
        };
        self.stream
            .write_all(&frame.encode())
            .and_then(|_| self.stream.flush())
            .map_err(io_err)?;
        let mut header = [0u8; 6];
        self.stream.read_exact(&mut header).map_err(io_err)?;
        let total = mbap::frame_len(&header)?;
        let mut rest = vec![0u8; total.saturating_sub(6)];
        self.stream.read_exact(&mut rest).map_err(io_err)?;
        let mut full = header.to_vec();
        full.extend_from_slice(&rest);
        Ok(mbap::parse(&full)?.pdu)
    }
}

// ------------------------------------------------------------ RTU over TCP

pub struct RtuOverTcpTransport {
    stream: TcpStream,
}

impl RtuOverTcpTransport {
    pub fn connect(host: &str, port: u16, timeout: Duration) -> Result<Self, ProtoError> {
        let addr = (host, port)
            .to_socket_addrs()
            .map_err(io_err)?
            .next()
            .ok_or_else(|| ProtoError::Io("no address".into()))?;
        let stream = TcpStream::connect_timeout(&addr, timeout).map_err(io_err)?;
        stream
            .set_read_timeout(Some(timeout))
            .and_then(|_| stream.set_write_timeout(Some(timeout)))
            .map_err(io_err)?;
        Ok(Self { stream })
    }
}

impl Transport for RtuOverTcpTransport {
    fn call(&mut self, unit: u8, pdu: &[u8]) -> Result<Vec<u8>, ProtoError> {
        rtu_call(&mut self.stream, unit, pdu)
    }
}

// ----------------------------------------------------------------- RTU serial

pub struct RtuSerialTransport {
    port: Box<dyn serialport::SerialPort>,
    /// Overall response wait used by the echo-tolerant read loop.
    timeout: Duration,
}

fn parity_of(s: &str) -> serialport::Parity {
    match s.to_ascii_uppercase().as_str() {
        "E" => serialport::Parity::Even,
        "O" => serialport::Parity::Odd,
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
    if n == 2 {
        serialport::StopBits::Two
    } else {
        serialport::StopBits::One
    }
}

impl RtuSerialTransport {
    pub fn open(conn: &Connection) -> Result<Self, ProtoError> {
        let s = &conn.serial;
        let port = serialport::new(&s.port, s.baud)
            .data_bits(data_bits_of(s.data_bits))
            .parity(parity_of(&s.parity))
            .stop_bits(stop_bits_of(s.stop_bits))
            // Short read slices keep the echo-tolerant loop responsive; the
            // overall wait is bounded by conn.timeout_ms inside `call`.
            .timeout(Duration::from_millis(50))
            .open()
            .map_err(|e| ProtoError::Io(serial_open_error(&e, &s.port)))?;
        Ok(Self {
            port,
            timeout: Duration::from_millis(conn.timeout_ms.max(50) as u64),
        })
    }
}

impl Transport for RtuSerialTransport {
    fn call(&mut self, unit: u8, pdu: &[u8]) -> Result<Vec<u8>, ProtoError> {
        rtu_call_echo(&mut self.port, unit, pdu, self.timeout)
    }
}

/// Human-readable serial open failure (port busy / missing / wrong params).
/// Shared shape with mb-sim so both sides of the app speak the same text.
fn serial_open_error(e: &serialport::Error, port: &str) -> String {
    let hint = match &e.kind {
        serialport::ErrorKind::Io(std::io::ErrorKind::PermissionDenied) => {
            "порт занят другой программой — закройте её и повторите"
        }
        serialport::ErrorKind::NoDevice
        | serialport::ErrorKind::Io(std::io::ErrorKind::NotFound) => {
            "порт не найден — подключите адаптер и обновите список портов"
        }
        serialport::ErrorKind::InvalidInput => "неверные параметры линии для этого порта",
        _ => "",
    };
    let base = format!("последовательный порт {port} открыть не удалось: {e}");
    if hint.is_empty() {
        base
    } else {
        format!("{base} ({hint})")
    }
}

// ------------------------------------------------------------- ASCII serial/TCP

pub struct AsciiSerialTransport {
    port: Box<dyn serialport::SerialPort>,
}

impl AsciiSerialTransport {
    pub fn open(conn: &Connection) -> Result<Self, ProtoError> {
        let s = &conn.serial;
        let port = serialport::new(&s.port, s.baud)
            .data_bits(data_bits_of(s.data_bits))
            .parity(parity_of(&s.parity))
            .stop_bits(stop_bits_of(s.stop_bits))
            .timeout(Duration::from_millis(conn.timeout_ms.max(50) as u64))
            .open()
            .map_err(|e| ProtoError::Io(format!("open {}: {e}", s.port)))?;
        Ok(Self { port })
    }
}

impl Transport for AsciiSerialTransport {
    fn call(&mut self, unit: u8, pdu: &[u8]) -> Result<Vec<u8>, ProtoError> {
        ascii_call(&mut self.port, unit, pdu)
    }
}

pub struct AsciiOverTcpTransport {
    stream: TcpStream,
}

impl AsciiOverTcpTransport {
    pub fn connect(host: &str, port: u16, timeout: Duration) -> Result<Self, ProtoError> {
        let addr = (host, port)
            .to_socket_addrs()
            .map_err(io_err)?
            .next()
            .ok_or_else(|| ProtoError::Io("no address".into()))?;
        let stream = TcpStream::connect_timeout(&addr, timeout).map_err(io_err)?;
        stream
            .set_read_timeout(Some(timeout))
            .and_then(|_| stream.set_write_timeout(Some(timeout)))
            .map_err(io_err)?;
        Ok(Self { stream })
    }
}

impl Transport for AsciiOverTcpTransport {
    fn call(&mut self, unit: u8, pdu: &[u8]) -> Result<Vec<u8>, ProtoError> {
        ascii_call(&mut self.stream, unit, pdu)
    }
}

// ---------------------------------------------------------------- RTU framing

/// Shared RTU request/response over any blocking duplex (serial or TCP).
pub fn rtu_call<R: Read + Write>(io: &mut R, unit: u8, pdu: &[u8]) -> Result<Vec<u8>, ProtoError> {
    let frame = rtu::encode(unit, pdu);
    io.write_all(&frame).and_then(|_| io.flush()).map_err(io_err)?;

    let mut buf = vec![0u8; 2];
    io.read_exact(&mut buf).map_err(io_err)?;
    let func = buf[1];

    if func & 0x80 != 0 {
        read_into(io, &mut buf, 1)?; // exception code
    } else if matches!(func, 1 | 2 | 3 | 4 | 23) {
        let mut bc = [0u8; 1];
        io.read_exact(&mut bc).map_err(io_err)?;
        buf.push(bc[0]);
        read_into(io, &mut buf, bc[0] as usize)?;
    } else if matches!(func, 5 | 6 | 15 | 16) {
        read_into(io, &mut buf, 4)?; // addr + value/qty
    } else if func == 22 {
        read_into(io, &mut buf, 6)?; // addr + and + or
    }
    read_into(io, &mut buf, 2)?; // CRC

    let (_unit_rx, out) = rtu::parse(&buf)?;
    Ok(out)
}

fn read_into<R: Read>(io: &mut R, buf: &mut Vec<u8>, n: usize) -> Result<(), ProtoError> {
    let mut tmp = vec![0u8; n];
    io.read_exact(&mut tmp).map_err(io_err)?;
    buf.extend_from_slice(&tmp);
    Ok(())
}

// ------------------------------------------------- RTU over half-duplex line

/// Expected RTU **response** frame length from its first bytes
/// (None = unknown function — keep reading until the deadline).
/// Shapes: [unit][func]…[crc16], exception = 5 bytes total.
fn rtu_resp_frame_len(buf: &[u8]) -> Option<usize> {
    if buf.len() < 2 {
        return Some(2);
    }
    let f = buf[1];
    if f & 0x80 != 0 {
        return Some(5); // [unit][f|0x80][exc][crc16]
    }
    match f {
        1 | 2 | 3 | 4 | 23 => {
            if buf.len() < 3 {
                return Some(3);
            }
            Some(3 + buf[2] as usize + 2) // header + byte count + data + CRC
        }
        5 | 6 | 15 | 16 => Some(8), // [unit][f][addr][val/qty][crc16]
        22 => Some(10),             // [unit][f][addr][and][or][crc16]
        _ => None,
    }
}

/// RTU request/response over a **half-duplex serial line** (2-wire RS-485).
///
/// Unlike [`rtu_call`] (full-duplex TCP), many RS-485 adapters loop our own
/// transmission back to RX, so the bytes read may be
/// `<echo of the request><response>`. Both layouts are tried and
/// CRC-verified, so line noise can never pose as a valid frame:
///
/// * Candidate A — the buffer starts with our request, a valid response
///   follows it → the echo is stripped and the response returned.
/// * Candidate B — the buffer itself is a valid response (no echo).
///
/// FC5/6 responses repeat the request verbatim, so a buffer equal to the
/// request is ambiguous: a short grace window waits for a late real response
/// (any new byte resets it) before accepting the request-shaped frame.
pub fn rtu_call_echo<R: Read + Write>(
    io: &mut R,
    unit: u8,
    pdu: &[u8],
    timeout: Duration,
) -> Result<Vec<u8>, ProtoError> {
    let frame = rtu::encode(unit, pdu);
    io.write_all(&frame).and_then(|_| io.flush()).map_err(io_err)?;

    let deadline = Instant::now() + timeout;
    let mut rx: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 512];
    let mut grace: Option<Instant> = None;
    // Only FC5/6 responses are byte-identical to the request (see doc).
    let write_resp = matches!(pdu.first(), Some(5 | 6));
    loop {
        if Instant::now() >= deadline {
            return Err(ProtoError::Io("timeout waiting for RTU response".into()));
        }
        match io.read(&mut tmp) {
            Ok(0) => thread::sleep(Duration::from_millis(2)),
            Ok(n) => {
                rx.extend_from_slice(&tmp[..n]);
                grace = None; // the line is still talking — reset the window
            }
            Err(ref e)
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(io_err(e)),
        }
        if rx.len() > 8192 {
            return Err(ProtoError::Truncated);
        }
        // Candidate A: our echo followed by the real response.
        if rx.starts_with(&frame) && rx.len() > frame.len() {
            let tail = &rx[frame.len()..];
            if let Some(need) = rtu_resp_frame_len(tail) {
                if tail.len() >= need {
                    if let Ok((_u, out)) = rtu::parse(&tail[..need]) {
                        return Ok(out);
                    }
                }
            }
        }
        // Candidate B: plain response (no echo on the line).
        if let Some(need) = rtu_resp_frame_len(&rx) {
            if rx.len() >= need {
                if let Ok((_u, out)) = rtu::parse(&rx[..need]) {
                    if write_resp && rx.starts_with(&frame) {
                        // Ambiguous FC5/6 frame — wait out the grace window.
                        let g = grace.get_or_insert(Instant::now() + Duration::from_millis(50));
                        if Instant::now() >= *g {
                            return Ok(out);
                        }
                    } else {
                        return Ok(out);
                    }
                }
            }
        }
    }
}

// --------------------------------------------------------------- ASCII framing

/// Shared Modbus ASCII request/response over any blocking duplex.
///
/// Responses have no length field: read until the terminating `\r\n` is seen
/// (bounded, so a dead line can't fill memory).
pub fn ascii_call<R: Read + Write>(io: &mut R, unit: u8, pdu: &[u8]) -> Result<Vec<u8>, ProtoError> {
    let frame = ascii::encode(unit, pdu);
    io.write_all(&frame).and_then(|_| io.flush()).map_err(io_err)?;

    let mut buf = Vec::with_capacity(64);
    let mut one = [0u8; 1];
    loop {
        io.read_exact(&mut one).map_err(io_err)?;
        buf.push(one[0]);
        if buf.len() >= 2 && &buf[buf.len() - 2..] == b"\r\n" {
            break;
        }
        if buf.len() > 1024 {
            return Err(ProtoError::Truncated);
        }
    }
    let (unit_rx, out) = ascii::parse(&buf)?;
    if unit_rx != unit {
        return Err(ProtoError::BadProtocolId(unit_rx as u16));
    }
    Ok(out)
}

// ------------------------------------------------------------------ factory

pub fn open_transport(conn: &Connection) -> Result<Box<dyn Transport>, ProtoError> {
    let timeout = Duration::from_millis(conn.timeout_ms.max(50) as u64);
    match conn.kind {
        ConnKind::Tcp => Ok(Box::new(TcpTransport::connect(&conn.host, conn.port, timeout)?)),
        ConnKind::RtuOverTcp => Ok(Box::new(RtuOverTcpTransport::connect(
            &conn.host,
            conn.port,
            timeout,
        )?)),
        ConnKind::Rtu => Ok(Box::new(RtuSerialTransport::open(conn)?)),
        ConnKind::Ascii => Ok(Box::new(AsciiSerialTransport::open(conn)?)),
        ConnKind::AsciiOverTcp => Ok(Box::new(AsciiOverTcpTransport::connect(
            &conn.host,
            conn.port,
            timeout,
        )?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mb_proto::pdu::{decode_response, Request, Response};
    use std::io::Cursor;

    /// Duplex backed by a canned response, capturing what was written.
    struct MockIo {
        rx: Cursor<Vec<u8>>,
        tx: Vec<u8>,
    }
    impl Read for MockIo {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            self.rx.read(b)
        }
    }
    impl Write for MockIo {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.tx.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn rtu_call_frames_and_parses() {
        // A slave reply for FC3, unit 1: [unit][fc][bc=4][10 0 20 0][crc]
        let payload = [0x10u16, 0x0014];
        let mut resp_pdu = vec![0x03u8, 0x04];
        for w in payload {
            resp_pdu.extend(w.to_be_bytes());
        }
        let resp_frame = rtu::encode(0x01, &resp_pdu);

        let mut io = MockIo {
            rx: Cursor::new(resp_frame),
            tx: Vec::new(),
        };
        let req = Request::ReadHoldingRegisters {
            addr: 0,
            qty: 2,
        };
        let out = rtu_call(&mut io, 0x01, &req.encode()).expect("rtu call");
        assert_eq!(decode_response(&out).unwrap(), Response::Registers(vec![16, 20]));
        // the bytes written must be a valid RTU frame for our request
        assert_eq!(io.tx, rtu::encode(0x01, &req.encode()));
    }

    #[test]
    fn rtu_call_decodes_exception() {
        // [unit][fc|0x80][exc=2][crc]
        let resp = rtu::encode(0x01, &[0x83, 0x02]);
        let mut io = MockIo {
            rx: Cursor::new(resp),
            tx: Vec::new(),
        };
        let req = Request::ReadHoldingRegisters {
            addr: 0,
            qty: 2,
        };
        let out = rtu_call(&mut io, 0x01, &req.encode()).unwrap();
        assert!(matches!(
            decode_response(&out),
            Err(ProtoError::Exception(_))
        ));
    }

    #[test]
    fn ascii_call_frames_and_parses() {
        // A slave reply for FC3, unit 0x11: [unit][fc][bc=6][regs][lrc] as ASCII.
        let resp_pdu: Vec<u8> = {
            let mut v = vec![0x03u8, 0x04];
            v.extend(0x0306u16.to_be_bytes()); // reg 774
            v.extend(0x876Bu16.to_be_bytes()); // reg 34667
            v
        };
        let resp = ascii::encode(0x11, &resp_pdu);
        assert_eq!(&resp[0..1], b":");

        let mut io = MockIo {
            rx: Cursor::new(resp),
            tx: Vec::new(),
        };
        let req = Request::ReadHoldingRegisters {
            addr: 0x6B,
            qty: 2,
        };
        let out = ascii_call(&mut io, 0x11, &req.encode()).expect("ascii call");
        assert_eq!(
            decode_response(&out).unwrap(),
            Response::Registers(vec![774, 34667])
        );
        // the bytes written must be a valid ASCII frame for our request
        assert_eq!(io.tx, ascii::encode(0x11, &req.encode()));
    }

    #[test]
    fn ascii_call_dead_line_is_bounded() {
        // runs out of bytes before CRLF -> Truncated, not a hang/growth
        let mut io = MockIo {
            rx: Cursor::new(vec![b':' ; 2048]), // no CRLF ever
            tx: Vec::new(),
        };
        let req = Request::ReadHoldingRegisters { addr: 0, qty: 1 };
        let out = ascii_call(&mut io, 0x01, &req.encode());
        assert!(out.is_err());
    }

    // ------------------------------------------------ half-duplex RTU echo --

    fn fc3_response(regs: &[u16]) -> Vec<u8> {
        let mut pdu = vec![0x03u8, regs.len() as u8 * 2];
        for w in regs {
            pdu.extend(w.to_be_bytes());
        }
        rtu::encode(0x01, &pdu)
    }

    #[test]
    fn rtu_resp_frame_len_shapes() {
        assert_eq!(rtu_resp_frame_len(&[]), Some(2));
        // exception: [unit][83][exc][crc][crc]
        assert_eq!(rtu_resp_frame_len(&rtu::encode(1, &[0x83, 0x02])), Some(5));
        // FC3 with byte count 4: [unit][03][04][d d d d][crc][crc]
        assert_eq!(rtu_resp_frame_len(&fc3_response(&[10, 20])), Some(9));
        assert_eq!(rtu_resp_frame_len(&[0x01, 0x03, 0x04]), Some(9));
        // write-single-register echo shape
        assert_eq!(rtu_resp_frame_len(&[0x01, 0x06]), Some(8));
        assert_eq!(rtu_resp_frame_len(&[0x01, 0x16]), Some(10));
        // unknown function without the exception bit -> unknowable length
        assert_eq!(rtu_resp_frame_len(&[0x01, 0x11]), None);
        // any high-bit function byte reads as an exception shape (CRC still
        // guards the parse, so garbage can never pass)
        assert_eq!(rtu_resp_frame_len(&[0x01, 0x99]), Some(5));
    }

    #[test]
    fn rtu_echo_serial_strips_half_duplex_echo() {
        // 2-wire RS-485 adapter loops our request back; the slave reply follows.
        let req = Request::ReadHoldingRegisters { addr: 0, qty: 2 };
        let frame = rtu::encode(0x01, &req.encode());
        let resp = fc3_response(&[16, 20]);
        let mut io = MockIo {
            rx: Cursor::new([&frame[..], &resp[..]].concat()),
            tx: Vec::new(),
        };
        let out = rtu_call_echo(&mut io, 0x01, &req.encode(), Duration::from_millis(400))
            .expect("echo must be stripped");
        assert_eq!(decode_response(&out).unwrap(), Response::Registers(vec![16, 20]));
        assert_eq!(io.tx, frame, "the request on the wire is a valid RTU frame");
    }

    #[test]
    fn rtu_echo_serial_plain_response_no_echo() {
        let req = Request::ReadHoldingRegisters { addr: 0, qty: 2 };
        let resp = fc3_response(&[7, 8]);
        let mut io = MockIo {
            rx: Cursor::new(resp),
            tx: Vec::new(),
        };
        let out = rtu_call_echo(&mut io, 0x01, &req.encode(), Duration::from_millis(400))
            .expect("plain response");
        assert_eq!(decode_response(&out).unwrap(), Response::Registers(vec![7, 8]));
    }

    #[test]
    fn rtu_echo_serial_fc6_response_is_the_request() {
        // No echo: the FC6 response repeats the request verbatim; the grace
        // window must not break the call.
        let req = Request::WriteSingleRegister { addr: 4, value: 1234 };
        let frame = rtu::encode(0x01, &req.encode());
        let mut io = MockIo {
            rx: Cursor::new(frame.clone()),
            tx: Vec::new(),
        };
        let out = rtu_call_echo(&mut io, 0x01, &req.encode(), Duration::from_millis(400))
            .expect("FC6 response == request must still parse");
        assert_eq!(out, req.encode());

        // With echo: request + identical response -> the real reply is served.
        let mut io = MockIo {
            rx: Cursor::new([&frame[..], &frame[..]].concat()),
            tx: Vec::new(),
        };
        let out = rtu_call_echo(&mut io, 0x01, &req.encode(), Duration::from_millis(400))
            .expect("echo + FC6 response");
        assert_eq!(out, req.encode());
    }

    #[test]
    fn rtu_echo_serial_dead_line_times_out() {
        let req = Request::ReadHoldingRegisters { addr: 0, qty: 1 };
        let mut io = MockIo {
            rx: Cursor::new(Vec::new()),
            tx: Vec::new(),
        };
        let out = rtu_call_echo(&mut io, 0x01, &req.encode(), Duration::from_millis(120));
        assert!(out.is_err(), "silent line must end in a timeout");
    }

    #[test]
    fn rtu_echo_serial_noise_is_never_a_response() {
        // Garbage that never forms a CRC-valid frame -> timeout, not junk data.
        let req = Request::ReadHoldingRegisters { addr: 0, qty: 1 };
        let mut io = MockIo {
            rx: Cursor::new(vec![0xDE; 64]),
            tx: Vec::new(),
        };
        let out = rtu_call_echo(&mut io, 0x01, &req.encode(), Duration::from_millis(120));
        assert!(out.is_err());
    }
}
