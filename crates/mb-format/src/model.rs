//! Domain model for Modbus Poll tables and workspaces.

/// Address notation as stored by Modbus Poll. Purely informational after decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notation {
    /// `4x0000`-style PLC notation (holding registers).
    PlcFourX,
    /// `3x0000`-style PLC notation (input registers).
    PlcThreeX,
    /// `1x0000`-style PLC notation (discrete inputs).
    PlcOneX,
    /// Modicon `40001..49999` (holding registers).
    ModiconFourX,
    /// Modicon `30001..39999` (input registers).
    ModiconThreeX,
    /// Plain decimal protocol address.
    Dec,
}

/// Decode a raw stored address into a protocol offset + notation.
///
/// Mirrors Modbus Poll behaviour: PLC notation only applies when it does not
/// collide with a plausible decimal address for the given function code.
pub fn decode_addr(raw: u32, func: u8) -> (u32, Notation) {
    if (0x4000..=0x4FFF).contains(&raw) {
        return (raw - 0x4000, Notation::PlcFourX);
    }
    if func == 4 && (0x3000..=0x3FFF).contains(&raw) {
        return (raw - 0x3000, Notation::PlcThreeX);
    }
    if func == 2 && (0x1000..=0x1FFF).contains(&raw) {
        return (raw - 0x1000, Notation::PlcOneX);
    }
    if func == 3 && (40001..=49999).contains(&raw) {
        return (raw - 40001, Notation::ModiconFourX);
    }
    if func == 4 && (30001..=39999).contains(&raw) {
        return (raw - 30001, Notation::ModiconThreeX);
    }
    (raw, Notation::Dec)
}

/// Human label for a decoded notation ("dec", "4x", "3x", "1x", "4xxxx", "3xxxx").
pub fn notation_str(n: Notation) -> &'static str {
    match n {
        Notation::PlcFourX => "4x",
        Notation::PlcThreeX => "3x",
        Notation::PlcOneX => "1x",
        Notation::ModiconFourX => "4xxxx",
        Notation::ModiconThreeX => "3xxxx",
        Notation::Dec => "dec",
    }
}

/// Function codes Modbus Poll accepts in a table header.
pub fn valid_func(func: u8) -> bool {
    matches!(func, 1 | 2 | 3 | 4 | 5 | 6 | 15 | 16 | 22 | 23)
}

/// Map the on-disk `fmt16` field to a display format name.
pub fn display_format(fmt16: u32) -> &'static str {
    match fmt16 {
        5 => "int16",
        6 => "int16",
        7 => "int32",
        8 => "float32",
        _ => "uint16",
    }
}

/// One named register row in a table.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Row {
    /// Protocol offset (`start_addr + index`).
    pub addr: u32,
    /// User alias / label.
    pub alias: String,
}

/// A single Modbus Poll table (one `.mbp` file).
#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub path: String,
    pub name: String,
    pub magic: u32,
    pub func: u8,
    pub addr: u32,
    /// Raw stored address (verbatim, no decoding).
    pub raw_addr: u32,
    pub qty: u32,
    pub period_ms: u32,
    pub slave: u8,
    pub fmt16: u32,
    /// Original address notation as stored in the file ("dec", "4x", "1x", …).
    pub notation: String,
    pub ip: String,
    pub rows: Vec<Row>,
    /// 0.0..1.0 — how confident the parse is (heuristic tail/marker match).
    pub confidence: f32,
    pub notes: String,
}

impl Table {
    pub fn display16(&self) -> &'static str {
        display_format(self.fmt16)
    }

    /// Address -> alias map for rows that carry an alias.
    pub fn aliases(&self) -> Vec<(u32, &str)> {
        self.rows
            .iter()
            .filter(|r| !r.alias.is_empty())
            .map(|r| (r.addr, r.alias.as_str()))
            .collect()
    }
}

/// Reference from a workspace to one `.mbp` table file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabRef {
    pub name: String,
    pub file: String,
    pub exists: bool,
}

/// A `.mbw` workspace (list of tables bound to a host/port).
#[derive(Debug, Clone, PartialEq)]
pub struct Workspace {
    pub path: String,
    pub host: String,
    pub port: u16,
    pub tables: Vec<TabRef>,
    pub notes: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_plc_and_modicon() {
        assert_eq!(decode_addr(0x4000, 3), (0, Notation::PlcFourX));
        assert_eq!(decode_addr(0x4074, 3), (0x74, Notation::PlcFourX));
        assert_eq!(decode_addr(0x3000, 4), (0, Notation::PlcThreeX));
        assert_eq!(decode_addr(0x1000, 2), (0, Notation::PlcOneX));
        assert_eq!(decode_addr(40004, 3), (3, Notation::ModiconFourX));
        assert_eq!(decode_addr(30001, 4), (0, Notation::ModiconThreeX));
    }

    #[test]
    fn decode_decimal_not_hijacked() {
        // 0x1388 = 5000 decimal, an ordinary user address for func 4 (not 1x).
        assert_eq!(decode_addr(5000, 4), (5000, Notation::Dec));
        // 5000 is inside 0x1000..0x1FFF but the 1x rule only fires for func 2.
        assert_eq!(decode_addr(5000, 1), (5000, Notation::Dec));
        assert_eq!(decode_addr(6000, 3), (6000, Notation::Dec));
    }
}
