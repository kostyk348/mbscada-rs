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
    /// 32-bit hex view over two registers (ABCD), e.g. 0xDEADBEEF.
    Hex32,
    /// Two ASCII chars per register (0x4142 -> "AB").
    Ascii16,
    /// Byte-swapped u16 (0x1234 -> 0x3412), Modbus word-swapped ints.
    // Alias: older project files / UI builds sent the name without the
    // underscore, which serde rejects — keep them loadable.
    #[serde(alias = "u16swapped")]
    U16Swapped,
    /// 64-bit unsigned over four registers ABCD.
    U64,
    /// 64-bit signed over four registers ABCD.
    I64,
    /// 64-bit float over four registers ABCD.
    F64,
}

impl Fmt {
    /// Number of 16-bit registers consumed per displayed value.
    pub fn regs_per_value(self) -> usize {
        match self {
            Fmt::U64 | Fmt::I64 | Fmt::F64 => 4,
            Fmt::U32 | Fmt::I32 | Fmt::Hex32 => 2,
            Fmt::F32Abcd | Fmt::F32Cdab | Fmt::F32Badc | Fmt::F32Dcba => 2,
            _ => 1,
        }
    }

    pub fn is_float(self) -> bool {
        matches!(
            self,
            Fmt::F32Abcd | Fmt::F32Cdab | Fmt::F32Badc | Fmt::F32Dcba | Fmt::F64
        )
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
        Fmt::U16 | Fmt::Hex | Fmt::Bin | Fmt::Ascii16 => regs.iter().map(|r| *r as f64).collect(),
        Fmt::I16 => regs.iter().map(|r| *r as i16 as f64).collect(),
        Fmt::U16Swapped => regs.iter().map(|r| swap16(*r) as f64).collect(),
        Fmt::F32Abcd | Fmt::F32Cdab | Fmt::F32Badc | Fmt::F32Dcba => regs
            .chunks_exact(2)
            .map(|c| f32::from_bits(word_order(c[0], c[1], fmt)) as f64)
            .collect(),
        Fmt::U32 | Fmt::Hex32 => regs
            .chunks_exact(2)
            .map(|c| word_order(c[0], c[1], Fmt::F32Abcd) as f64)
            .collect(),
        Fmt::I32 => regs
            .chunks_exact(2)
            .map(|c| word_order(c[0], c[1], Fmt::F32Abcd) as i32 as f64)
            .collect(),
        Fmt::U64 | Fmt::I64 | Fmt::F64 => regs
            .chunks_exact(4)
            .map(|c| {
                let bits = ((c[0] as u64) << 48)
                    | ((c[1] as u64) << 32)
                    | ((c[2] as u64) << 16)
                    | (c[3] as u64);
                match fmt {
                    Fmt::F64 => f64::from_bits(bits),
                    Fmt::I64 => bits as i64 as f64,
                    _ => bits as f64,
                }
            })
            .collect(),
    }
}

/// Human-readable display string with scaling applied (`mult/div + offset`).
pub fn display(raw: u16, value: f64, fmt: Fmt, mult: f64, div: f64, offset: f64) -> String {
    let d = if div == 0.0 { 1.0 } else { div };
    let scaled = value * mult / d + offset;
    match fmt {
        Fmt::Hex => format!("0x{raw:04X}"),
        Fmt::Hex32 => format!("0x{:08X}", scaled as i64 as u32),
        Fmt::Bin => format!("{raw:016b}"),
        Fmt::Ascii16 => {
            let hi = (raw >> 8) as u8 as char;
            let lo = (raw & 0xFF) as u8 as char;
            format!("{hi}{lo}")
        }
        Fmt::U16 | Fmt::I16 | Fmt::U16Swapped => {
            if scaled.fract() == 0.0 {
                format!("{}", scaled as i64)
            } else {
                format!("{scaled:.2}")
            }
        }
        Fmt::F32Abcd | Fmt::F32Cdab | Fmt::F32Badc | Fmt::F32Dcba => format!("{scaled:.3}"),
        Fmt::F64 => format!("{scaled:.5}"),
        Fmt::U32 | Fmt::I32 | Fmt::I64 => format!("{}", scaled as i64),
        Fmt::U64 => format!("{}", scaled as u64),
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
        assert_eq!(Fmt::U64.regs_per_value(), 4);
        assert_eq!(Fmt::Hex32.regs_per_value(), 2);
    }

    #[test]
    fn mbrs_style_formats() {
        // hex32: 0xDEADBEEF over words ABCD
        assert_eq!(decode(&[0xDEAD, 0xBEEF], Fmt::Hex32), vec![0xDEADBEEFu32 as f64]);
        assert_eq!(display(0xDEAD, 0xDEADBEEFu32 as f64, Fmt::Hex32, 1.0, 1.0, 0.0), "0xDEADBEEF");
        // ascii16: 0x4142 -> "AB"
        assert_eq!(display(0x4142, 0x4142 as f64, Fmt::Ascii16, 1.0, 1.0, 0.0), "AB");
        // u16 swapped: 0x1234 -> 0x3412
        assert_eq!(decode(&[0x1234], Fmt::U16Swapped), vec![0x3412 as f64]);
        // u64: 0x0000000100000002 -> words [0x0000, 0x0001, 0x0000, 0x0002]
        assert_eq!(decode(&[0, 0x0001, 0, 0x0002], Fmt::U64), vec![0x0000000100000002u64 as f64]);
        // i64: -1 = 0xFFFFFFFFFFFFFFFF
        assert_eq!(decode(&[0xFFFF; 4], Fmt::I64), vec![-1.0]);
        // f64: 1.0 = 0x3FF0000000000000 -> words 0x3FF0,0,0,0
        assert_eq!(decode(&[0x3FF0, 0, 0, 0], Fmt::F64), vec![1.0]);
        assert_eq!(display(0x3FF0, 1.0, Fmt::F64, 1.0, 1.0, 0.0), "1.00000");
    }
}
