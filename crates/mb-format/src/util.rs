//! Low-level byte helpers for the Modbus Poll binary formats.
//!
//! All integers are little-endian. Strings are UTF-16LE, each prefixed by the
//! 3-byte marker `FF FE FF` and a 1-byte length (count of UTF-16 code units).

/// String marker used throughout .mbp/.mbw.
pub const MARKER: [u8; 3] = [0xFF, 0xFE, 0xFF];
/// Row padding constant.
pub const PAD: [u8; 4] = [0x55, 0x55, 0x55, 0x55];

/// Read a little-endian u32 at `off`, or `None` if out of bounds.
#[inline]
pub fn rd32(d: &[u8], off: usize) -> Option<u32> {
    d.get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[inline]
pub fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Decode a UTF-16LE byte slice, replacing invalid units (never panics).
pub fn utf16le_decode(b: &[u8]) -> String {
    let units: Vec<u16> = b
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    char::decode_utf16(units)
        .map(|r| r.unwrap_or('\u{FFFD}'))
        .collect()
}

/// Encode a string as marker + length + UTF-16LE (length capped at 100 units).
pub fn push_str_marker(out: &mut Vec<u8>, s: &str) {
    let units: Vec<u16> = s.encode_utf16().collect();
    out.extend_from_slice(&MARKER);
    out.push(units.len().min(100) as u8);
    for x in units.iter().take(100) {
        out.extend_from_slice(&x.to_le_bytes());
    }
}

/// A text marker found in the byte stream.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    pub offset: usize,
    pub len: usize,
    pub text: String,
}

/// Iterate marker-prefixed strings within `[start, end)`.
pub fn iter_markers(d: &[u8], start: usize, end: usize) -> Vec<Marker> {
    let mut out = Vec::new();
    let end = end.min(d.len());
    let mut i = start;
    while i + 4 <= end {
        if d[i] == MARKER[0] && d[i + 1] == MARKER[1] && d[i + 2] == MARKER[2] {
            let ln = d[i + 3] as usize;
            let mut text = String::new();
            if (1..=100).contains(&ln) {
                let a = i + 4;
                let b = (a + ln * 2).min(d.len());
                if b > a {
                    text = utf16le_decode(&d[a..b]);
                }
            }
            out.push(Marker {
                offset: i,
                len: ln,
                text,
            });
            i += 4;
        } else {
            i += 1;
        }
    }
    out
}

/// True for dotted-quad IPv4 strings with valid octets (0..255).
pub fn is_ip(s: &str) -> bool {
    let parts: Vec<&str> = s.trim_end_matches(',').split('.').collect();
    parts.len() == 4
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.len() <= 3
                && p.chars().all(|c| c.is_ascii_digit())
                && p.parse::<u16>().map(|n| n <= 255).unwrap_or(false)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_str_marker() {
        let mut b = Vec::new();
        push_str_marker(&mut b, "Привет");
        let m = iter_markers(&b, 0, b.len());
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].text, "Привет");
    }

    #[test]
    fn ip_detection() {
        assert!(is_ip("192.168.1.5"));
        assert!(is_ip("10.0.0.1,"));
        assert!(!is_ip("localhost"));
        assert!(!is_ip("1.2.3"));
        assert!(!is_ip("10.4.2.1930"), "octet > 255 is not an IP");
    }

    #[test]
    fn rd32_bounds() {
        assert_eq!(rd32(&[1, 0, 0, 0], 0), Some(1));
        assert_eq!(rd32(&[1, 0, 0], 0), None);
    }
}
