//! Modbus/TCP framing (MBAP header + PDU).

use crate::error::{ProtoError, Result};

/// MBAP frame: transaction id, unit id, PDU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MbapFrame {
    pub transaction: u16,
    pub unit: u8,
    pub pdu: Vec<u8>,
}

/// Bytes needed to know a full frame's length (MBAP header is 6 bytes + unit).
pub const HEADER_LEN: usize = 7;

impl MbapFrame {
    pub fn encode(&self) -> Vec<u8> {
        let length = 1 + self.pdu.len();
        let mut v = Vec::with_capacity(HEADER_LEN + self.pdu.len());
        v.extend(self.transaction.to_be_bytes());
        v.extend([0x00, 0x00]); // protocol id
        v.extend((length as u16).to_be_bytes());
        v.push(self.unit);
        v.extend(&self.pdu);
        v
    }
}

/// Total frame length implied by an MBAP header buffer (>= 6 bytes).
pub fn frame_len(buf: &[u8]) -> Result<usize> {
    if buf.len() < 6 {
        return Err(ProtoError::Truncated);
    }
    let protocol = u16::from_be_bytes([buf[2], buf[3]]);
    if protocol != 0 {
        return Err(ProtoError::BadProtocolId(protocol));
    }
    let length = u16::from_be_bytes([buf[4], buf[5]]) as usize;
    Ok(6 + length)
}

/// Parse a full MBAP frame from `buf` (must contain exactly one frame).
pub fn parse(buf: &[u8]) -> Result<MbapFrame> {
    let total = frame_len(buf)?;
    if buf.len() < total {
        return Err(ProtoError::BadLength {
            expected: total,
            got: buf.len(),
        });
    }
    let transaction = u16::from_be_bytes([buf[0], buf[1]]);
    let unit = buf[6];
    let pdu = buf[7..total].to_vec();
    Ok(MbapFrame {
        transaction,
        unit,
        pdu,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mbap_roundtrip() {
        let f = MbapFrame {
            transaction: 0x1234,
            unit: 0x11,
            pdu: vec![0x03, 0x00, 0x6B, 0x00, 0x03],
        };
        let bytes = f.encode();
        assert_eq!(&bytes[0..2], &[0x12, 0x34]);
        assert_eq!(&bytes[2..4], &[0x00, 0x00]);
        assert_eq!(&bytes[4..6], &[0x00, 0x06]); // unit + 5 pdu bytes
        assert_eq!(frame_len(&bytes).unwrap(), bytes.len());
        assert_eq!(parse(&bytes).unwrap(), f);
    }

    #[test]
    fn mbap_incomplete() {
        let bytes = MbapFrame {
            transaction: 1,
            unit: 1,
            pdu: vec![0x03, 0x00, 0x6B, 0x00, 0x03],
        }
        .encode();
        assert!(parse(&bytes[..5]).is_err());
    }
}
