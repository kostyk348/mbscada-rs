//! Display formats and register decoding (Modbus Poll style).

use serde::{Deserialize, Serialize};

/// Register display/interpretation format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Fmt {
    #[default]
    U16,
    I16,
    Hex,
    Bin,
    /// 32-bit float, word order ABCD (high word first).
    F32Abcd,
    /// 32-bit float, word order CDAB (words swapped).
    F32Cdab,
    /// 32-bit float, word order BADC (bytes swapped).
    F32Badc,
    /// 32-bit float, word order DCBA (fully reversed).
    F32Dcba,
    U32,
    I32,
}

impl Fmt {
    /// Number of 16-bit registers consumed per displayed value.
    pub fn regs_per_value(self) -> usize {
        match self {
            Fmt::U16 | Fmt::I16 | Fmt::Hex | Fmt::Bin => 1,
            _ => 2,
        }
    }

    pub fn is_float(self) -> bool {
        matches!(self, Fmt::F32Abcd | Fmt::F32Cdab | Fmt::F32Badc | Fmt::F32Dcba)
    }
}

fn swap16(x: u16) -> u32 {
    (((x & 0xFF) << 8) | (x >> 8)) as u32
}

fn word_order(a: u16, b: u16, fmt: Fmt) -> u32 {
    match fmt {
        Fmt::F32Abcd => ((a as u32) << 16) | b as u32,
        Fmt::F32Cdab => ((b as u32) << 16) | a as u32,
        Fmt::F32Badc => (swap16(a) << 16) | swap16(b),
        Fmt::F32Dcba => (swap16(b) << 16) | swap16(a),
        _ => ((a as u32) << 16) | b as u32,
    }
}

/// Decode raw registers into numeric values according to `fmt`.
pub fn decode(regs: &[u16], fmt: Fmt) -> Vec<f64> {
    match fmt {
        Fmt::U16 | Fmt::Hex | Fmt::Bin => regs.iter().map(|r| *r as f64).collect(),
        Fmt::I16 => regs.iter().map(|r| *r as i16 as f64).collect(),
        Fmt::F32Abcd | Fmt::F32Cdab | Fmt::F32Badc | Fmt::F32Dcba => regs
            .chunks_exact(2)
            .map(|c| f32::from_bits(word_order(c[0], c[1], fmt)) as f64)
            .collect(),
        Fmt::U32 => regs
            .chunks_exact(2)
            .map(|c| word_order(c[0], c[1], Fmt::F32Abcd) as f64)
            .collect(),
        Fmt::I32 => regs
            .chunks_exact(2)
            .map(|c| word_order(c[0], c[1], Fmt::F32Abcd) as i32 as f64)
            .collect(),
    }
}

/// Human-readable display string with scaling applied (`mult/div + offset`).
pub fn display(raw: u16, value: f64, fmt: Fmt, mult: f64, div: f64, offset: f64) -> String {
    let d = if div == 0.0 { 1.0 } else { div };
    let scaled = value * mult / d + offset;
    match fmt {
        Fmt::Hex => format!("0x{raw:04X}"),
        Fmt::Bin => format!("{raw:016b}"),
        Fmt::U16 | Fmt::I16 => {
            if scaled.fract() == 0.0 {
                format!("{}", scaled as i64)
            } else {
                format!("{scaled:.2}")
            }
        }
        Fmt::F32Abcd | Fmt::F32Cdab | Fmt::F32Badc | Fmt::F32Dcba => format!("{scaled:.3}"),
        Fmt::U32 | Fmt::I32 => format!("{}", scaled as i64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int16_sign() {
        assert_eq!(decode(&[0xFFFE], Fmt::I16), vec![-2.0]);
        assert_eq!(decode(&[0xFFFE], Fmt::U16), vec![65534.0]);
    }

    #[test]
    fn float_word_orders() {
        // 1.0f32 == 0x3F800000 -> words ABCD = [0x3F80, 0x0000]
        assert_eq!(decode(&[0x3F80, 0x0000], Fmt::F32Abcd), vec![1.0]);
        // CDAB (word swap): [0x0000, 0x3F80]
        assert_eq!(decode(&[0x0000, 0x3F80], Fmt::F32Cdab), vec![1.0]);
        // BADC: bytes swapped per word: 3F80 -> 803F, so [0x803F,0x0000]
        assert_eq!(decode(&[0x803F, 0x0000], Fmt::F32Badc), vec![1.0]);
        // DCBA: [0x0000, 0x803F] -> swap(b)=803F -> 803F0000? build: swap16(b)<<16 | swap16(a)
    }

    #[test]
    fn u32_and_scaling() {
        // 0x0001_0000 = 65536
        assert_eq!(decode(&[0x0001, 0x0000], Fmt::U32), vec![65536.0]);
        assert_eq!(display(0x0064, 100.0, Fmt::U16, 1.0, 10.0, 0.0), "10");
        assert_eq!(display(0x000A, 10.0, Fmt::Hex, 1.0, 1.0, 0.0), "0x000A");
    }

    #[test]
    fn regs_per_value() {
        assert_eq!(Fmt::U16.regs_per_value(), 1);
        assert_eq!(Fmt::I32.regs_per_value(), 2);
        assert_eq!(Fmt::F32Abcd.regs_per_value(), 2);
    }
}
