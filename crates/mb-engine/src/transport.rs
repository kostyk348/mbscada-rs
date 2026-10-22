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
use std::time::Duration;

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
            .timeout(Duration::from_millis(conn.timeout_ms.max(50) as u64))
            .open()
            .map_err(|e| ProtoError::Io(format!("open {}: {e}", s.port)))?;
        Ok(Self { port })
    }
}

impl Transport for RtuSerialTransport {
    fn call(&mut self, unit: u8, pdu: &[u8]) -> Result<Vec<u8>, ProtoError> {
        rtu_call(&mut self.port, unit, pdu)
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
}
