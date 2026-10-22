//! Modbus ASCII framing: `':' [addr][pdu][LRC] '\r' '\n'`, payload as hex.
//!
//! Per the Modbus Application Protocol V1.1b3 (§2.8 frame format): the frame
//! starts with the colon character `:` (3AH), then address+PDU+error-check
//! (LRC, sum-and-negate over all ADU bytes) transmitted as hex characters
//! (0-9 A-F), terminated by CR LF. Only ASCII hex digits between delimiters.

use crate::error::{ProtoError, Result};

/// LRC over raw ADU bytes: two's complement of the byte sum.
pub fn lrc(adu: &[u8]) -> u8 {
    let sum = adu.iter().fold(0u32, |a, &b| a + b as u32);
    (-(sum as i32)) as u8
}

fn hex_hi(b: u8) -> u8 {
    const T: [u8; 16] = *b"0123456789ABCDEF";
    T[(b >> 4) as usize]
}
fn hex_lo(b: u8) -> u8 {
    const T: [u8; 16] = *b"0123456789ABCDEF";
    T[(b & 0xF) as usize]
}
fn nib(c: u8) -> Result<u8> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        _ => Err(ProtoError::BadAsciiHex(c)),
    }
}

/// Wrap `unit` + `pdu` as a complete ASCII frame `:..xxx\r\n`.
pub fn encode(unit: u8, pdu: &[u8]) -> Vec<u8> {
    // ADU = [unit][pdu][lrc]; transmitted as `":" + hex(adu) + CRLF`.
    let mut adu = Vec::with_capacity(pdu.len() + 2);
    adu.push(unit);
    adu.extend_from_slice(pdu);
    adu.push(lrc(&adu));

    let mut out = Vec::with_capacity(1 + adu.len() * 2 + 2);
    out.push(b':');
    for &b in &adu {
        out.push(hex_hi(b));
        out.push(hex_lo(b));
    }
    out.extend_from_slice(b"\r\n");
    out
}

/// Parse one complete ASCII frame (`:` ... CRLF), verifying the LRC.
/// `frame` must contain only the frame (delimiters included).
pub fn parse(frame: &[u8]) -> Result<(u8, Vec<u8>)> {
    let body = frame.strip_prefix(b":").ok_or(ProtoError::NotAsciiFrame)?;
    if body.len() < 2 || &body[body.len() - 2..] != b"\r\n" {
        return Err(ProtoError::Truncated);
    }
    // Hex chars between ':' and CRLF must be even in length.
    let hex = &body[..body.len() - 2];
    if hex.is_empty() || hex.len() % 2 != 0 {
        return Err(ProtoError::Truncated);
    }
    let mut adu = Vec::with_capacity(hex.len() / 2);
    for p in hex.chunks_exact(2) {
        adu.push((nib(p[0])? << 4) | nib(p[1])?);
    }
    if adu.len() < 2 {
        return Err(ProtoError::Truncated);
    }
    let (payload, lrc_bytes) = adu.split_at(adu.len() - 1);
    let got = lrc_bytes[0];
    let expected = lrc(payload);
    if got != expected {
        return Err(ProtoError::Lrc { expected, got });
    }
    let unit = payload[0];
    Ok((unit, payload[1..].to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc_known_vector() {
        // ADU 11 03 00 6B 00 03: sum = 0x82, LRC = two's complement = 0x7E
        let adu = [0x11u8, 0x03, 0x00, 0x6B, 0x00, 0x03];
        assert_eq!(lrc(&adu), 0x7E);
        // wrap-around byte sum: 0xFF+0x02 = 0x101 -> sum byte 0x01 -> LRC 0xFF
        assert_eq!(lrc(&[0xFF, 0x02]), 0xFF);
    }

    #[test]
    fn ascii_roundtrip() {
        let pdu = [0x03u8, 0x00, 0x6B, 0x00, 0x03];
        let frame = encode(0x11, &pdu);
        assert_eq!(&frame[0..1], b":");
        assert_eq!(&frame[frame.len() - 2..], b"\r\n");
        assert!(frame[1..frame.len() - 2]
            .iter()
            .all(|c| c.is_ascii_alphanumeric()));
        let (unit, back) = parse(&frame).unwrap();
        assert_eq!(unit, 0x11);
        assert_eq!(back, pdu);
    }

    #[test]
    fn lrc_detects_corruption() {
        let mut f = encode(0x01, &[0x03, 0x00, 0x00, 0x00, 0x03]);
        // flip a hex char inside the payload
        let at = 5;
        if f[at] == b'0' { f[at] = b'1' } else { f[at] = b'0' };
        assert!(matches!(parse(&f), Err(ProtoError::Lrc { .. })));
    }

    #[test]
    fn reject_garbage() {
        assert!(matches!(parse(b"12AB\r\n"), Err(ProtoError::NotAsciiFrame)));
        let mut f = encode(0x01, &[0x03, 0x00, 0x00, 0x00, 0x03]);
        f[2] = b'Z'; // 'Z' is not hex
        assert!(matches!(parse(&f), Err(ProtoError::BadAsciiHex(b'Z'))));
    }
}
