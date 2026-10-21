//! Modbus RTU framing: `[unit][pdu][crc16_le]`.

use crate::crc::crc16;
use crate::error::{ProtoError, Result};

/// Wrap a PDU as an RTU frame (appends the CRC).
pub fn encode(unit: u8, pdu: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(pdu.len() + 3);
    v.push(unit);
    v.extend(pdu);
    let c = crc16(&v);
    v.extend(c.to_le_bytes());
    v
}

/// Parse exactly one RTU frame, verifying the CRC.
pub fn parse(frame: &[u8]) -> Result<(u8, Vec<u8>)> {
    if frame.len() < 4 {
        return Err(ProtoError::Truncated);
    }
    let (body, crc_bytes) = frame.split_at(frame.len() - 2);
    let got = u16::from_le_bytes([crc_bytes[0], crc_bytes[1]]);
    let expected = crc16(body);
    if got != expected {
        return Err(ProtoError::Crc { expected, got });
    }
    Ok((body[0], body[1..].to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtu_roundtrip() {
        let pdu = [0x03u8, 0x00, 0x00, 0x00, 0x0A];
        let frame = encode(0x01, &pdu);
        assert_eq!(&frame[frame.len() - 2..], &[0xC5, 0xCD]); // CRC 0xCDC5 LE
        let (unit, back) = parse(&frame).unwrap();
        assert_eq!(unit, 0x01);
        assert_eq!(back, pdu);
    }

    #[test]
    fn rtu_crc_detects_corruption() {
        let mut frame = encode(0x01, &[0x03, 0x00, 0x00, 0x00, 0x0A]);
        frame[3] ^= 0x01;
        assert!(matches!(parse(&frame), Err(ProtoError::Crc { .. })));
    }
}
