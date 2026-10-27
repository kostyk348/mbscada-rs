//! Modbus Poll-style configuration model: connections, polls, alarms.

use crate::format::Fmt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Transport kind, mirroring Modbus Poll connection types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnKind {
    /// Modbus/TCP (MBAP). "TCP/IP"
    Tcp,
    /// Modbus RTU on a serial port. "Serial Port"
    Rtu,
    /// Modbus RTU framing tunnelled over TCP. "RTU/ASCII over TCP/IP"
    RtuOverTcp,
    /// Modbus ASCII on a serial port.
    Ascii,
    /// Modbus ASCII tunnelled over TCP.
    AsciiOverTcp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerialCfg {
    pub port: String,
    pub baud: u32,
    pub data_bits: u8,
    /// "N" | "E" | "O"
    pub parity: String,
    pub stop_bits: u8,
}

impl Default for SerialCfg {
    fn default() -> Self {
        Self {
            port: "COM1".into(),
            baud: 9600,
            data_bits: 8,
            parity: "N".into(),
            stop_bits: 1,
        }
    }
}

fn def_timeout() -> u32 {
    1000
}

fn def_slave() -> u8 {
    1
}

/// One transport target (a "connection" in Modbus Poll).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub id: String,
    pub name: String,
    pub kind: ConnKind,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub serial: SerialCfg,
    #[serde(default = "def_timeout")]
    pub timeout_ms: u32,
    /// Default Modbus slave (unit) address for polls on this connection.
    /// Polls may override it with their own `slave`; a poll with `slave = 0`
    /// inherits this value.
    #[serde(default = "def_slave")]
    pub slave: u8,
}

impl Connection {
    pub fn describe(&self) -> String {
        let base = match self.kind {
            ConnKind::Tcp => format!("TCP {}:{}", self.host, self.port),
            ConnKind::RtuOverTcp => format!("RTU/TCP {}:{}", self.host, self.port),
            ConnKind::AsciiOverTcp => format!("ASCII/TCP {}:{}", self.host, self.port),
            ConnKind::Rtu => format!(
                "Serial {} {} {}{}{}",
                self.serial.port,
                self.serial.baud,
                self.serial.parity,
                self.serial.data_bits,
                self.serial.stop_bits
            ),
            ConnKind::Ascii => format!("ASCII {}", self.serial.port),
        };
        format!("{base} · slave {}", self.slave)
    }
}

/// Hi/HiHi/Lo/LoLo alarm thresholds for a row.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AlarmSpec {
    #[serde(default)]
    pub hi: Option<f64>,
    #[serde(default)]
    pub hihi: Option<f64>,
    #[serde(default)]
    pub lo: Option<f64>,
    #[serde(default)]
    pub lolo: Option<f64>,
    #[serde(default)]
    pub msg_hi: String,
    #[serde(default)]
    pub msg_hihi: String,
    #[serde(default)]
    pub msg_lo: String,
    #[serde(default)]
    pub msg_lolo: String,
}

impl AlarmSpec {
    /// Returns (level, message). level in {"ok","hi","hihi","lo","lolo"}.
    pub fn check(&self, v: f64) -> (String, String) {
        if let Some(t) = self.hihi {
            if v >= t {
                return ("hihi".into(), self.msg("hihi"));
            }
        }
        if let Some(t) = self.lolo {
            if v <= t {
                return ("lolo".into(), self.msg("lolo"));
            }
        }
        if let Some(t) = self.hi {
            if v >= t {
                return ("hi".into(), self.msg("hi"));
            }
        }
        if let Some(t) = self.lo {
            if v <= t {
                return ("lo".into(), self.msg("lo"));
            }
        }
        ("ok".into(), String::new())
    }

    fn msg(&self, level: &str) -> String {
        let (m, d) = match level {
            "hi" => (&self.msg_hi, "HIGH"),
            "hihi" => (&self.msg_hihi, "HIGH-HIGH"),
            "lo" => (&self.msg_lo, "LOW"),
            _ => (&self.msg_lolo, "LOW-LOW"),
        };
        if m.is_empty() {
            d.to_string()
        } else {
            m.clone()
        }
    }
}

fn one() -> u8 {
    1
}
fn onef() -> f64 {
    1.0
}
fn ms1000() -> u64 {
    1000
}
fn tru() -> bool {
    true
}

/// A poll definition: the equivalent of one Modbus Poll window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Poll {
    pub id: u32,
    pub name: String,
    pub conn_id: String,
    #[serde(default = "one")]
    pub slave: u8,
    /// Function code 1,2,3,4,5,6,15,16,22,23.
    pub func: u8,
    pub start: u16,
    #[serde(default)]
    pub raw_start: u32,
    pub qty: u16,
    #[serde(default = "ms1000")]
    pub scan_ms: u64,
    #[serde(default)]
    pub fmt: Fmt,
    #[serde(default = "onef")]
    pub mult: f64,
    #[serde(default = "onef")]
    pub div: f64,
    #[serde(default)]
    pub offset: f64,
    #[serde(default)]
    pub unit: String,
    #[serde(default = "tru")]
    pub enabled: bool,
    /// Address base: true => display 1-based addresses.
    #[serde(default)]
    pub base_one: bool,
    /// Display notation: "" / "dec" / "4x" / "3x" / "1x" / "4xxxx" / "3xxxx".
    #[serde(default)]
    pub notation: String,
    /// Row label per protocol address (keys are decimal strings).
    #[serde(default)]
    pub labels: HashMap<String, String>,
    /// Alarm spec per protocol address (keys are decimal strings).
    #[serde(default)]
    pub alarms: HashMap<String, AlarmSpec>,
}

impl Poll {
    pub fn is_bits(&self) -> bool {
        matches!(self.func, 1 | 2)
    }

    pub fn regs_per_value(&self) -> usize {
        self.fmt.regs_per_value()
    }

    /// Row protocol addresses (steps by regs-per-value for 32-bit formats).
    pub fn row_addrs(&self) -> Vec<u16> {
        let step = self.regs_per_value() as u16;
        let rows = (self.qty / step) as usize;
        (0..rows)
            .map(|i| self.start.wrapping_add(i as u16 * step))
            .collect()
    }

    pub fn label_of(&self, addr: u16) -> String {
        self.labels
            .get(&addr.to_string())
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("{}[{:04}]", self.name, addr))
    }

    pub fn alarm_of(&self, addr: u16) -> Option<&AlarmSpec> {
        self.alarms.get(&addr.to_string())
    }

    /// Display address honoring notation (PLC/Modicon) or the 1-based base.
    pub fn disp_addr(&self, addr: u16) -> String {
        let a = addr as u32;
        match self.notation.as_str() {
            "4x" => format!("4x{a:04X}"),
            "3x" => format!("3x{a:04X}"),
            "1x" => format!("1x{a:04X}"),
            "4xxxx" => format!("{}", 40001 + a),
            "3xxxx" => format!("{}", 30001 + a),
            _ => format!("{}", if self.base_one { a + 1 } else { a }),
        }
    }
}

fn def_decimals() -> u8 {
    2
}

/// A computed (virtual) tag: an arithmetic expression evaluated from other
/// tags' values each refresh (e.g. `{T2 jacket} - {T1 reactor}`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputedTag {
    /// Display name; also the variable name other expressions may reference
    /// (braces recommended: `{ΔT}`).
    pub name: String,
    /// Expression over tag labels, `p<poll>:<addr>` aliases, numbers and the
    /// functions min/max/avg/abs/sqrt/round/floor/ceil/pow.
    pub expr: String,
    #[serde(default)]
    pub unit: String,
    /// Digits after the decimal point in the UI (0..=6).
    #[serde(default = "def_decimals")]
    pub decimals: u8,
}

pub fn func_is_read(func: u8) -> bool {
    matches!(func, 1 | 2 | 3 | 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_slave_defaults_to_one_for_old_configs() {
        // Project JSON saved before the `slave` field existed must still load.
        let c: Connection = serde_json::from_str(
            r#"{"id":"c1","name":"C","kind":"tcp","host":"127.0.0.1","port":502}"#,
        )
        .unwrap();
        assert_eq!(c.slave, 1);
        assert_eq!(c.describe(), "TCP 127.0.0.1:502 · slave 1");
    }

    #[test]
    fn connection_slave_roundtrips_and_describe() {
        let c: Connection = serde_json::from_str(
            r#"{"id":"c1","name":"C","kind":"rtu_over_tcp","host":"10.0.0.5","port":502,"slave":7}"#,
        )
        .unwrap();
        assert_eq!(c.slave, 7);
        assert_eq!(c.describe(), "RTU/TCP 10.0.0.5:502 · slave 7");
        let back: Connection =
            serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back.slave, 7);
    }

    #[test]
    fn connection_serial_describe_includes_slave() {
        let c: Connection = serde_json::from_str(
            r#"{"id":"c2","name":"S","kind":"rtu","serial":{"port":"COM3","baud":115200,"data_bits":8,"parity":"N","stop_bits":1},"slave":3}"#,
        )
        .unwrap();
        assert_eq!(c.describe(), "Serial COM3 115200 N81 · slave 3");
    }

    #[test]
    fn computed_tag_defaults_and_roundtrip() {
        // Old project files: `unit` and `decimals` may be missing.
        let t: ComputedTag =
            serde_json::from_str(r#"{"name":"ΔT","expr":"{T2} - {T1}"}"#).unwrap();
        assert_eq!(t.unit, "");
        assert_eq!(t.decimals, 2);
        let t2: ComputedTag = serde_json::from_str(
            r#"{"name":"kW","expr":"P*1.5","unit":"кВт","decimals":1}"#,
        )
        .unwrap();
        let back: ComputedTag =
            serde_json::from_str(&serde_json::to_string(&t2).unwrap()).unwrap();
        assert_eq!(back.decimals, 1);
        assert_eq!(back.unit, "кВт");
    }
}
