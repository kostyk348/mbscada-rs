//! Per-register styling rules and alarm conditions.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

impl Severity {
    pub fn color(self) -> &'static str {
        match self {
            Severity::Info => "#3b82f6",
            Severity::Warning => "#f59e0b",
            Severity::Critical => "#ef4444",
        }
    }
}

/// A condition on an integer register value (or a bit, as 0/1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Cond {
    Always,
    Eq { v: i64 },
    Ne { v: i64 },
    Gt { v: i64 },
    Ge { v: i64 },
    Lt { v: i64 },
    Le { v: i64 },
    InRange { lo: i64, hi: i64 },
    OutsideRange { lo: i64, hi: i64 },
    Bit { n: u8, set: bool },
}

impl Cond {
    pub fn matches(&self, x: i64) -> bool {
        match self {
            Cond::Always => true,
            Cond::Eq { v } => x == *v,
            Cond::Ne { v } => x != *v,
            Cond::Gt { v } => x > *v,
            Cond::Ge { v } => x >= *v,
            Cond::Lt { v } => x < *v,
            Cond::Le { v } => x <= *v,
            Cond::InRange { lo, hi } => x >= *lo && x <= *hi,
            Cond::OutsideRange { lo, hi } => x < *lo || x > *hi,
            Cond::Bit { n, set } => {
                if *n >= 64 {
                    false
                } else {
                    ((x >> n) & 1 == 1) == *set
                }
            }
        }
    }
}

/// Maximum number of value branches per row (poll+offset target).
///
/// Extends Modbus Poll (default + rule 1 + rule 2) to 32 value-dependent
/// stylistic branches per row.
pub const MAX_BRANCHES: usize = 32;

/// A styling rule optionally doubling as an alarm.
///
/// Rules on the same `(poll, offset)` target form an ordered branch list:
/// evaluation stops at the first match (Modbus Poll "Rule 1 has precedence
/// over Rule 2", generalised to branch order). Up to [`MAX_BRANCHES`]
/// branches may share one target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    #[serde(default)]
    pub id: u32,
    #[serde(default)]
    pub name: String,
    /// Target poll id.
    #[serde(default)]
    pub poll: u32,
    /// Target row index within the poll (0-based).
    #[serde(default)]
    pub offset: u16,
    pub cond: Cond,
    #[serde(default)]
    pub fg: Option<String>,
    #[serde(default)]
    pub bg: Option<String>,
    #[serde(default)]
    pub blink: bool,
    /// If set, a matching rule raises an alarm of this severity.
    #[serde(default)]
    pub severity: Option<Severity>,
    #[serde(default)]
    pub message: Option<String>,
    /// Dynamic row text: replaces the row label while this branch matches.
    #[serde(default)]
    pub text: Option<String>,
}

impl Rule {
    pub fn matches(&self, x: i64) -> bool {
        self.cond.matches(x)
    }
}

/// Number of branches already present on one `(poll, offset)` target.
pub fn branch_count(rules: &[Rule], poll: u32, offset: u16) -> usize {
    rules
        .iter()
        .filter(|r| r.poll == poll && r.offset == offset)
        .count()
}

/// First rule matching `x` on the target, in list order (branch precedence).
pub fn first_matching<'a>(rules: &'a [Rule], poll: u32, offset: u16, x: i64) -> Option<&'a Rule> {
    rules
        .iter()
        .find(|r| r.poll == poll && r.offset == offset && r.matches(x))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_conditions() {
        assert!(Cond::Eq { v: 5 }.matches(5));
        assert!(!Cond::Eq { v: 5 }.matches(6));
        assert!(Cond::Gt { v: 5 }.matches(6));
        assert!(Cond::InRange { lo: 10, hi: 20 }.matches(15));
        assert!(!Cond::InRange { lo: 10, hi: 20 }.matches(21));
        assert!(Cond::OutsideRange { lo: 10, hi: 20 }.matches(9));
    }

    #[test]
    fn bit_condition() {
        // value 0b0100 = 4, bit 2 is set
        assert!(Cond::Bit { n: 2, set: true }.matches(4));
        assert!(!Cond::Bit { n: 2, set: true }.matches(0));
        assert!(Cond::Bit { n: 2, set: false }.matches(0));
    }

    #[test]
    fn alarm_rule_carries_severity() {
        let r = Rule {
            id: 1,
            name: "overheat".into(),
            poll: 1,
            offset: 0,
            cond: Cond::Gt { v: 100 },
            fg: Some("#fff".into()),
            bg: Some("#f00".into()),
            blink: true,
            severity: Some(Severity::Critical),
            message: Some("Перегрев".into()),
            text: Some("Перегрев!".into()),
        };
        assert!(r.matches(101));
        assert_eq!(r.severity, Some(Severity::Critical));
    }

    #[test]
    fn first_matching_takes_branch_order() {
        let rules = vec![
            Rule {
                id: 1,
                name: "окно".into(),
                poll: 1,
                offset: 0,
                cond: Cond::InRange { lo: 398, hi: 402 },
                fg: Some("#0f0".into()),
                bg: None,
                blink: false,
                severity: None,
                message: None,
                text: None,
            },
            Rule {
                id: 2,
                name: "большой".into(),
                poll: 1,
                offset: 0,
                cond: Cond::Gt { v: 100 },
                fg: Some("#f00".into()),
                bg: None,
                blink: false,
                severity: None,
                message: None,
                text: None,
            },
        ];
        // Value falls into both window and threshold: the first branch wins,
        // mirroring Modbus Poll's "rule 1 beats rule 2" precedence.
        let hit = first_matching(&rules, 1, 0, 400).expect("hit");
        assert_eq!(hit.id, 1);
        // Only the second branch matches this value.
        assert_eq!(first_matching(&rules, 1, 0, 150).map(|r| r.id), Some(2));
        assert!(first_matching(&rules, 1, 0, 5).is_none());
    }

    #[test]
    fn branch_count_per_target() {
        let rules: Vec<Rule> = (0..MAX_BRANCHES)
            .map(|i| Rule {
                id: i as u32,
                name: format!("b{i}"),
                poll: if i % 2 == 0 { 1 } else { 2 },
                offset: 0,
                cond: Cond::Gt { v: i as i64 },
                fg: None,
                bg: None,
                blink: false,
                severity: None,
                message: None,
                text: None,
            })
            .collect();
        assert_eq!(branch_count(&rules, 1, 0), MAX_BRANCHES / 2);
        assert_eq!(branch_count(&rules, 3, 0), 0);
    }
}
