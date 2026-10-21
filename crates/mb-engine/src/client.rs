//! Blocking Modbus/TCP client (one outstanding request, like a poller).

use mb_proto::error::ProtoError;
use mb_proto::{decode_response, mbap, Request, Response};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

pub struct ModbusTcpClient {
    stream: TcpStream,
    transaction: u16,
}

impl ModbusTcpClient {
    pub fn connect(host: &str, port: u16, timeout: Duration) -> Result<Self, ProtoError> {
        let addr = (host, port)
            .to_socket_addrs()
            .map_err(|e| ProtoError::Io(e.to_string()))?
            .next()
            .ok_or_else(|| ProtoError::Io("no address resolved".into()))?;
        let stream = TcpStream::connect_timeout(&addr, timeout)
            .map_err(|e| ProtoError::Io(e.to_string()))?;
        stream
            .set_read_timeout(Some(timeout))
            .and_then(|_| stream.set_write_timeout(Some(timeout)))
            .and_then(|_| stream.set_nodelay(true))
            .map_err(|e| ProtoError::Io(e.to_string()))?;
        Ok(Self {
            stream,
            transaction: 0,
        })
    }

    /// Send a request and read exactly one response PDU.
    pub fn call(&mut self, unit: u8, req: &Request) -> Result<Response, ProtoError> {
        self.transaction = self.transaction.wrapping_add(1);
        let frame = mbap::MbapFrame {
            transaction: self.transaction,
            unit,
            pdu: req.encode(),
        };
        self.stream
            .write_all(&frame.encode())
            .and_then(|_| self.stream.flush())
            .map_err(|e| ProtoError::Io(e.to_string()))?;

        let mut header = [0u8; 6];
        self.stream
            .read_exact(&mut header)
            .map_err(|e| ProtoError::Io(e.to_string()))?;
        let total = mbap::frame_len(&header)?;
        if total < 6 {
            return Err(ProtoError::BadLength {
                expected: 6,
                got: total,
            });
        }
        let mut rest = vec![0u8; total - 6];
        self.stream
            .read_exact(&mut rest)
            .map_err(|e| ProtoError::Io(e.to_string()))?;
        let mut full = header.to_vec();
        full.extend_from_slice(&rest);
        let resp = mbap::parse(&full)?;
        decode_response(&resp.pdu)
    }
}
