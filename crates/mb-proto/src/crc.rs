//! Modbus CRC-16 (poly 0xA001, init 0xFFFF), transmitted little-endian.

pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_self_check() {
        // Appending the CRC (LE) makes the CRC over the whole frame zero.
        let frame = [0x01u8, 0x03, 0x00, 0x00, 0x00, 0x0A];
        let c = crc16(&frame);
        let mut full = frame.to_vec();
        full.extend_from_slice(&c.to_le_bytes());
        assert_eq!(crc16(&full), 0);
    }

    #[test]
    fn crc_known_vector() {
        // Well-known RTU request 01 03 00 00 00 0A -> CRC 0xCDC5.
        assert_eq!(crc16(&[0x01, 0x03, 0x00, 0x00, 0x00, 0x0A]), 0xCDC5);
    }
}
