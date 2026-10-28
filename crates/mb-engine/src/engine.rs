//! Multi-poll engine: one thread per connection, independent scan rates,
//! live row values, hi/hi-hi/lo/lo-lo alarms, styling rules, stats,
//! computed (virtual) tags, an optional event log file and a communication
//! log. Mirrors Modbus Poll's behaviour.

use crate::expr::Expr;
use crate::format::{decode, display, Fmt};
use crate::model::{func_is_read, ComputedTag, Connection, Poll};
use crate::rules::{self, Rule, Severity};
use crate::transport::{open_transport, Transport};
use mb_proto::error::ProtoError;
use mb_proto::{decode_response, Request, Response};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::io::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// ------------------------------------------------------------------ views

#[derive(Debug, Clone, Serialize, Default)]
pub struct ConnStats {
    pub tx: u64,
    pub rx: u64,
    pub errors: u64,
    pub timeouts: u64,
    pub last_ms: f64,
    pub avg_ms: f64,
    pub online: bool,
    pub last_error: String,
    /// Successful (re)opens of the transport after the first one.
    pub reconnects: u64,
    /// Epoch ms of the last successful exchange (0 = never).
    pub last_ok_ts: u128,
}

/// Per-poll diagnostics (success/error counters).
#[derive(Debug, Clone, Serialize, Default)]
pub struct PollStats {
    pub ok: u64,
    pub err: u64,
    /// Epoch ms of the last successful poll (0 = never).
    pub last_ok_ts: u128,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnView {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub describe: String,
    pub stats: ConnStats,
}

#[derive(Debug, Clone, Serialize)]
pub struct RowView {
    pub addr: u16,
    pub disp_addr: String,
    pub name: String,
    pub raw: u16,
    pub value: f64,
    /// Scaled value (mult/div + offset) the UI shows for numeric formats.
    pub scaled: f64,
    /// Raw registers this value is built from (1 for 16-bit formats,
    /// 2 for 32-bit, 4 for 64-bit) — the UI uses them for hex/bin views
    /// and for typed (float / 32-bit) writes.
    pub regs: Vec<u16>,
    pub display: String,
    pub unit: String,
    pub quality: String, // good | bad | off
    pub alarm_level: String,
    pub alarm_msg: String,
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub blink: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PollView {
    pub id: u32,
    pub name: String,
    pub conn_id: String,
    pub func: u8,
    pub slave: u8,
    pub start: u16,
    pub qty: u16,
    pub scan_ms: u64,
    pub fmt: Fmt,
    pub enabled: bool,
    pub last_poll_ms: f64,
    #[serde(default)]
    pub stats: PollStats,
    pub rows: Vec<RowView>,
}

/// One computed (virtual) tag as shown in the UI.
#[derive(Debug, Clone, Serialize)]
pub struct ComputedView {
    pub name: String,
    pub expr: String,
    pub unit: String,
    pub value: f64,
    pub display: String,
    /// false = expression/compile error, missing tag, or a source tag with
    /// bad quality — `err` explains it.
    pub ok: bool,
    pub err: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommEntry {
    pub time: String,
    pub conn: String,
    pub dir: String,
    pub text: String,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub running: bool,
    pub connections: Vec<ConnView>,
    pub polls: Vec<PollView>,
    #[serde(default)]
    pub computed: Vec<ComputedView>,
    pub log: Vec<CommEntry>,
    pub ts_ms: u128,
}

/// Everything `Engine::start_with` needs. Plain `start` fills the defaults
/// (no computed tags, no log file) for tests and simple embedders.
pub struct EngineCfg {
    pub connections: Vec<Connection>,
    pub polls: Vec<Poll>,
    pub rules: Vec<Rule>,
    pub computed: Vec<ComputedTag>,
    /// Event log file path; `None` disables file logging.
    pub log_path: Option<String>,
}

// ------------------------------------------------------------------ inner

enum Command {
    Write {
        poll_id: u32,
        addr: u16,
        value: u16,
        multi: bool,
        values: Vec<u16>,
    },
}

struct Inner {
    connections: HashMap<String, Connection>,
    polls: Vec<Poll>,
    rules: Vec<Rule>,
    rows: HashMap<u32, Vec<RowView>>,
    stats: HashMap<String, ConnStats>,
    poll_ms: HashMap<u32, f64>,
    poll_stats: HashMap<u32, PollStats>,
    log: VecDeque<CommEntry>,
    cmds: HashMap<String, Sender<Command>>,
    running: bool,
    /// Computed tags paired with their compiled expression
    /// (Err = compile error, surfaced verbatim in the snapshot).
    computed: Vec<(ComputedTag, Result<Expr, String>)>,
    /// Append-only event log file (None = disabled).
    log_file: Option<std::fs::File>,
}

pub struct Engine {
    inner: Arc<Mutex<Inner>>,
    stop: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
}

fn now_hms() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        % 86400;
    format!("{:02}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Days-since-epoch -> (year, month, day); Howard Hinnant's civil algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Full local timestamp "YYYY-MM-DD HH:MM:SS" for log lines.
fn now_stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, mo, d) = civil_from_days((secs / 86_400) as i64);
    let s = secs % 86_400;
    format!("{y:04}-{mo:02}-{d:02} {:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

impl Engine {
    /// Back-compat wrapper: no computed tags, no log file.
    pub fn start(connections: Vec<Connection>, polls: Vec<Poll>, rules: Vec<Rule>) -> Engine {
        Self::start_with(EngineCfg {
            connections,
            polls,
            rules,
            computed: vec![],
            log_path: None,
        })
    }

    pub fn start_with(cfg: EngineCfg) -> Engine {
        let mut conn_map = HashMap::new();
        for c in cfg.connections {
            conn_map.insert(c.id.clone(), c);
        }
        let mut stats = HashMap::new();
        for id in conn_map.keys() {
            stats.insert(id.clone(), ConnStats::default());
        }
        // Event log file: append; failures are non-fatal (logging stays off).
        let log_file = cfg.log_path.as_deref().and_then(|p| {
            if let Some(dir) = std::path::Path::new(p).parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
                .ok()
        });
        let computed = compile_computed(cfg.computed);
        let inner = Arc::new(Mutex::new(Inner {
            connections: conn_map,
            polls: cfg.polls,
            rules: cfg.rules,
            rows: HashMap::new(),
            stats,
            poll_ms: HashMap::new(),
            poll_stats: HashMap::new(),
            log: VecDeque::new(),
            cmds: HashMap::new(),
            running: true,
            computed,
            log_file,
        }));
        let stop = Arc::new(AtomicBool::new(false));

        let mut conn_ids: Vec<String> = {
            let g = inner.lock().unwrap();
            g.polls.iter().map(|p| p.conn_id.clone()).collect()
        };
        conn_ids.sort();
        conn_ids.dedup();

        let n_polls = inner.lock().unwrap().polls.len();
        engine_event(&inner, &format!("движок запущен: {} соединений, {} опросов", conn_ids.len(), n_polls));

        let mut handles = Vec::new();
        for cid in conn_ids {
            let (tx, rx) = channel::<Command>();
            inner.lock().unwrap().cmds.insert(cid.clone(), tx);
            let i2 = Arc::clone(&inner);
            let s2 = Arc::clone(&stop);
            handles.push(thread::spawn(move || conn_loop(i2, s2, cid, rx)));
        }
        Engine {
            inner,
            stop,
            handles,
        }
    }

    /// Replace computed tags at runtime (UI editor); recompiled immediately.
    pub fn set_computed(&self, tags: Vec<ComputedTag>) {
        if let Ok(mut g) = self.inner.lock() {
            g.computed = compile_computed(tags);
        }
    }

    pub fn computed(&self) -> Vec<ComputedTag> {
        self.inner
            .lock()
            .map(|g| g.computed.iter().map(|(t, _)| t.clone()).collect())
            .unwrap_or_default()
    }

    pub fn set_rules(&self, rules: Vec<Rule>) {
        if let Ok(mut g) = self.inner.lock() {
            g.rules = rules;
        }
    }

    pub fn rules(&self) -> Vec<Rule> {
        self.inner.lock().map(|g| g.rules.clone()).unwrap_or_default()
    }

    pub fn write(&self, poll_id: u32, addr: u16, value: u16) -> Result<(), String> {
        self.send_write(poll_id, addr, value, false, vec![])
    }

    pub fn write_multi(&self, poll_id: u32, addr: u16, values: Vec<u16>) -> Result<(), String> {
        self.send_write(poll_id, addr, 0, true, values)
    }

    fn send_write(
        &self,
        poll_id: u32,
        addr: u16,
        value: u16,
        multi: bool,
        values: Vec<u16>,
    ) -> Result<(), String> {
        let g = self.inner.lock().map_err(|_| "lock".to_string())?;
        let poll = g
            .polls
            .iter()
            .find(|p| p.id == poll_id)
            .ok_or("no such poll")?;
        let tx = g.cmds.get(&poll.conn_id).ok_or("no connection")?;
        tx.send(Command::Write {
            poll_id,
            addr,
            value,
            multi,
            values,
        })
        .map_err(|e| e.to_string())
    }

    pub fn snapshot(&self) -> Snapshot {
        let g = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => return Snapshot::empty(),
        };
        let connections = g
            .connections
            .values()
            .map(|c| ConnView {
                id: c.id.clone(),
                name: c.name.clone(),
                kind: format!("{:?}", c.kind).to_lowercase(),
                describe: c.describe(),
                stats: g.stats.get(&c.id).cloned().unwrap_or_default(),
            })
            .collect();

        let polls = g
            .polls
            .iter()
            .map(|p| {
                // Effective slave: a poll with slave = 0 inherits the
                // connection's default slave address.
                let conn = g.connections.get(&p.conn_id);
                let eff_slave = eff_slave(p.slave, conn);
                PollView {
                    id: p.id,
                    name: p.name.clone(),
                    conn_id: p.conn_id.clone(),
                    func: p.func,
                    slave: eff_slave,
                    start: p.start,
                    qty: p.qty,
                    scan_ms: p.scan_ms,
                    fmt: p.fmt,
                    enabled: p.enabled,
                    last_poll_ms: g.poll_ms.get(&p.id).copied().unwrap_or(0.0),
                    stats: g.poll_stats.get(&p.id).cloned().unwrap_or_default(),
                    rows: g.rows.get(&p.id).cloned().unwrap_or_default(),
                }
            })
            .collect();

        let computed = eval_computed(&g);

        Snapshot {
            running: g.running,
            connections,
            polls,
            computed,
            log: g.log.iter().cloned().collect(),
            ts_ms: now_ms(),
        }
    }
}

fn compile_computed(tags: Vec<ComputedTag>) -> Vec<(ComputedTag, Result<Expr, String>)> {
    tags.into_iter()
        .map(|t| {
            let e = Expr::compile(&t.expr);
            (t, e)
        })
        .collect()
}

/// Evaluate all computed tags against the latest row values.
/// Variables: the stable row label (`poll.label_of(addr)`) and a
/// `p<poll_id>:<addr>` alias; values are engineering units (mult/div/offset
/// applied), matching what alarms and the UI show.
fn eval_computed(g: &Inner) -> Vec<ComputedView> {
    if g.computed.is_empty() {
        return vec![];
    }
    // name -> (scaled value, quality_good)
    let mut vals: HashMap<String, (f64, bool)> = HashMap::new();
    for p in &g.polls {
        if let Some(rows) = g.rows.get(&p.id) {
            for r in rows {
                let scaled = r.value * p.mult
                    / (if p.div == 0.0 { 1.0 } else { p.div })
                    + p.offset;
                let good = r.quality == "good";
                vals.insert(p.label_of(r.addr), (scaled, good));
                vals.insert(format!("p{}:{}", p.id, r.addr), (scaled, good));
            }
        }
    }
    g.computed
        .iter()
        .map(|(t, e)| {
            let dec = (t.decimals.min(6)) as usize;
            let mut bad_quality = false;
            match e {
                Err(ce) => ComputedView {
                    name: t.name.clone(),
                    expr: t.expr.clone(),
                    unit: t.unit.clone(),
                    value: 0.0,
                    display: "—".into(),
                    ok: false,
                    err: ce.clone(),
                },
                Ok(ex) => {
                    let res = ex.eval(&mut |n: &str| match vals.get(n) {
                        Some((v, good)) => {
                            if !good {
                                bad_quality = true;
                            }
                            Some(*v)
                        }
                        None => None,
                    });
                    match res {
                        Ok(v) if v.is_finite() => ComputedView {
                            name: t.name.clone(),
                            expr: t.expr.clone(),
                            unit: t.unit.clone(),
                            value: v,
                            display: format!("{v:.dec$}"),
                            ok: !bad_quality,
                            err: if bad_quality { "нет качества у исходного тэга".into() } else { String::new() },
                        },
                        Ok(_) => ComputedView {
                            name: t.name.clone(),
                            expr: t.expr.clone(),
                            unit: t.unit.clone(),
                            value: 0.0,
                            display: "—".into(),
                            ok: false,
                            err: "результат не конечное число".into(),
                        },
                        Err(e) => ComputedView {
                            name: t.name.clone(),
                            expr: t.expr.clone(),
                            unit: t.unit.clone(),
                            value: 0.0,
                            display: "—".into(),
                            ok: false,
                            err: e,
                        },
                    }
                }
            }
        })
        .collect()
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut g) = self.inner.lock() {
            g.running = false;
        }
        engine_event(&self.inner, "движок остановлен");
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }
}

impl Snapshot {
    pub fn empty() -> Snapshot {
        Snapshot {
            running: false,
            connections: vec![],
            polls: vec![],
            computed: vec![],
            log: vec![],
            ts_ms: now_ms(),
        }
    }
}

/// Effective slave address of a poll: `0` means "follow the connection's
/// default" (falls back to 1 when the connection is unknown).
fn eff_slave(poll_slave: u8, conn: Option<&Connection>) -> u8 {
    if poll_slave == 0 {
        conn.map(|c| c.slave).unwrap_or(1)
    } else {
        poll_slave
    }
}

// ------------------------------------------------------------------ poller

fn read_poll(tr: &mut Box<dyn Transport>, poll: &Poll) -> Result<Vec<u16>, ProtoError> {
    let req = match poll.func {
        1 => Request::ReadCoils {
            addr: poll.start,
            qty: poll.qty,
        },
        2 => Request::ReadDiscreteInputs {
            addr: poll.start,
            qty: poll.qty,
        },
        3 => Request::ReadHoldingRegisters {
            addr: poll.start,
            qty: poll.qty,
        },
        4 => Request::ReadInputRegisters {
            addr: poll.start,
            qty: poll.qty,
        },
        _ => return Err(ProtoError::Io("unsupported read function".into())),
    };
    let pdu = tr.call(poll.slave, &req.encode())?;
    match decode_response(&pdu)? {
        Response::Registers(r) => Ok(r),
        Response::Bits(b) => {
            let mut v: Vec<u16> = b.iter().map(|x| *x as u16).collect();
            v.truncate(poll.qty as usize);
            Ok(v)
        }
        _ => Ok(vec![]),
    }
}

fn level_style(level: &str) -> (Option<String>, Option<String>, bool) {
    match level {
        "hihi" => (Some("#ffffff".into()), Some("#ef4444".into()), true),
        "hi" => (Some("#1f2937".into()), Some("#f59e0b".into()), false),
        "lo" => (Some("#0b1220".into()), Some("#38bdf8".into()), false),
        "lolo" => (Some("#ffffff".into()), Some("#2563eb".into()), true),
        _ => (None, None, false),
    }
}

fn build_rows(poll: &Poll, regs: &[u16], rules: &[Rule], quality: &str) -> Vec<RowView> {
    let step = poll.regs_per_value();
    let values = decode(regs, poll.fmt);
    let addrs = poll.row_addrs();
    let mut out = Vec::with_capacity(addrs.len());

    for (i, addr) in addrs.iter().enumerate() {
        let value = values.get(i).copied().unwrap_or(0.0);
        let raw = regs.get(i * step).copied().unwrap_or(0);
        let vregs: Vec<u16> = regs
            .get(i * step..(i + 1) * step)
            .map(|s| s.to_vec())
            .unwrap_or_default();
        let x = raw as i64;

        let disp = if poll.is_bits() {
            if raw != 0 {
                "ON".to_string()
            } else {
                "OFF".to_string()
            }
        } else {
            display(raw, value, poll.fmt, poll.mult, poll.div, poll.offset)
        };
        let scaled = value * poll.mult / (if poll.div == 0.0 { 1.0 } else { poll.div }) + poll.offset;

        let (mut level, mut msg) = poll
            .alarm_of(*addr)
            .map(|a| a.check(scaled))
            .unwrap_or_else(|| ("ok".into(), String::new()));

        let (mut fg, mut bg, mut blink) = level_style(&level);
        let mut name = poll.label_of(*addr);

        // Ordered branch evaluation: the first matching rule on the target
        // wins (Modbus Poll "rule 1 has precedence over rule 2", generalised
        // to up to MAX_BRANCHES branches per row).
        if let Some(r) = rules::first_matching(rules, poll.id, i as u16, x) {
            if let Some(f) = &r.fg {
                fg = Some(f.clone());
            }
            if let Some(b) = &r.bg {
                bg = Some(b.clone());
            }
            blink |= r.blink;
            if let Some(t) = r.text.as_ref().filter(|t| !t.is_empty()) {
                // Dynamic row text: short-lived value-bound description.
                name = t.clone();
            }
            if let Some(sev) = r.severity {
                level = match sev {
                    Severity::Critical => "hihi".into(),
                    Severity::Warning => "hi".into(),
                    Severity::Info => "lo".into(),
                };
                msg = r.message.clone().unwrap_or_else(|| r.name.clone());
                let (sfg, sbg, sbl) = level_style(&level);
                if r.fg.is_none() {
                    fg = sfg;
                }
                if r.bg.is_none() {
                    bg = sbg;
                }
                blink |= sbl;
            }
        }

        out.push(RowView {
            addr: *addr,
            disp_addr: poll.disp_addr(*addr),
            name,
            raw,
            value,
            scaled,
            regs: vregs,
            display: disp,
            unit: poll.unit.clone(),
            quality: quality.to_string(),
            alarm_level: level,
            alarm_msg: msg,
            fg,
            bg,
            blink,
        });
    }
    out
}

fn push_log(inner: &Arc<Mutex<Inner>>, conn: &str, dir: &str, text: String, ok: bool) {
    if let Ok(mut g) = inner.lock() {
        g.log.push_front(CommEntry {
            time: now_hms(),
            conn: conn.to_string(),
            dir: dir.to_string(),
            text: text.clone(),
            ok,
        });
        while g.log.len() > 500 {
            g.log.pop_back();
        }
        // Same line to the log file, if enabled: "2026-10-27 12:41:02 [RX] Conn: text"
        if let Some(f) = g.log_file.as_mut() {
            let mark = if ok { "" } else { "!" };
            let _ = writeln!(f, "{} [{}{}] {}: {}", now_stamp(), dir, mark, conn, text);
        }
    }
}

/// Engine lifecycle events (start/stop) into the comm log + log file.
fn engine_event(inner: &Arc<Mutex<Inner>>, text: &str) {
    push_log(inner, "engine", "SYS", text.to_string(), true);
}

fn update_stats<F: FnOnce(&mut ConnStats)>(inner: &Arc<Mutex<Inner>>, conn_id: &str, f: F) {
    if let Ok(mut g) = inner.lock() {
        if let Some(s) = g.stats.get_mut(conn_id) {
            f(s);
        }
    }
}

fn conn_loop(inner: Arc<Mutex<Inner>>, stop: Arc<AtomicBool>, conn_id: String, rx: Receiver<Command>) {
    let mut tr: Option<Box<dyn Transport>> = None;
    let mut next_due: HashMap<u32, Instant> = HashMap::new();
    let mut opens: u64 = 0; // successful transport opens (reconnect diagnostics)
    let mut pending: Vec<Command> = Vec::new(); // writes waiting for the link
    let mut last_open_err: Option<Instant> = None;

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }

        // Collect queued writes. They are flushed only while the link is up;
        // otherwise they wait in `pending` (bounded) instead of being dropped
        // silently while the transport is down / reopening.
        while let Ok(cmd) = rx.try_recv() {
            pending.push(cmd);
        }
        if pending.len() > 32 {
            let dropped = pending.len() - 32;
            pending.drain(..dropped);
            push_log(
                &inner,
                &polls_conn_name(&inner, &conn_id),
                "ERR",
                format!("очередь записи переполнена: {dropped} старых команд отброшено"),
                false,
            );
        }

        let (conn, polls) = {
            let g = match inner.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            (
                g.connections.get(&conn_id).cloned(),
                g.polls
                    .iter()
                    .filter(|p| p.conn_id == conn_id && p.enabled && func_is_read(p.func))
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        };
        let conn = match conn {
            Some(c) => c,
            None => break,
        };
        // Effective slave: a poll with slave = 0 inherits the connection's
        // default slave address ("slave ID" set on the connection itself).
        let polls: Vec<Poll> = polls
            .into_iter()
            .map(|mut p| {
                p.slave = eff_slave(p.slave, Some(&conn));
                p
            })
            .collect();
        if polls.is_empty() && pending.is_empty() {
            thread::sleep(Duration::from_millis(300));
            continue;
        }

        if tr.is_none() {
            match open_transport(&conn) {
                Ok(t) => {
                    tr = Some(t);
                    opens += 1;
                    update_stats(&inner, &conn_id, |s| {
                        s.online = true;
                        s.last_error.clear();
                        if opens > 1 {
                            s.reconnects += 1;
                        }
                    });
                    if opens > 1 {
                        push_log(&inner, &conn.name, "SYS", "связь восстановлена".into(), true);
                    }
                }
                Err(e) => {
                    update_stats(&inner, &conn_id, |s| {
                        s.online = false;
                        s.errors += 1;
                        s.last_error = e.to_string();
                    });
                    // Throttled: a dead device would otherwise spam the log on
                    // every retry. Queued writes keep waiting in `pending`.
                    if last_open_err
                        .map(|t| t.elapsed() > Duration::from_secs(2))
                        .unwrap_or(true)
                    {
                        push_log(&inner, &conn.name, "ERR", e.to_string(), false);
                        last_open_err = Some(Instant::now());
                    }
                    // Write-only pending work retries faster than scans.
                    thread::sleep(Duration::from_millis(if polls.is_empty() { 200 } else { 800 }));
                    continue;
                }
            }
        }

        // The link is up — flush queued writes oldest-first, before reads.
        if !pending.is_empty() {
            for cmd in pending.drain(..) {
                do_command(&inner, tr.as_mut().unwrap(), &conn_id, cmd);
            }
        }

        let now = Instant::now();
        let mut did = false;
        for poll in &polls {
            let due = next_due.get(&poll.id).copied().unwrap_or(now);
            if now < due {
                continue;
            }
            next_due.insert(
                poll.id,
                now + Duration::from_millis(poll.scan_ms.max(20)),
            );
            did = true;
            let t0 = Instant::now();
            match read_poll(tr.as_mut().unwrap(), poll) {
                Ok(regs) => {
                    let dt = t0.elapsed().as_secs_f64() * 1000.0;
                    let rows = {
                        let g = inner.lock().unwrap();
                        build_rows(poll, &regs, &g.rules, "good")
                    };
                    let n = rows.len();
                    let ts = now_ms();
                    if let Ok(mut g) = inner.lock() {
                        g.rows.insert(poll.id, rows);
                        g.poll_ms.insert(poll.id, dt);
                        let ps = g.poll_stats.entry(poll.id).or_default();
                        ps.ok += 1;
                        ps.last_ok_ts = ts;
                        let s = g.stats.get_mut(&conn_id);
                        if let Some(s) = s {
                            s.tx += 1;
                            s.rx += 1;
                            s.last_ms = dt;
                            s.avg_ms = if s.avg_ms == 0.0 {
                                dt
                            } else {
                                (s.avg_ms * 9.0 + dt) / 10.0
                            };
                            s.online = true;
                            s.last_ok_ts = ts;
                        }
                    }
                    push_log(
                        &inner,
                        &polls_conn_name(&inner, &conn_id),
                        "RX",
                        format!("{}: {} значений, {:.1} мс", poll.name, n, dt),
                        true,
                    );
                }
                Err(e) => {
                    let msg = e.to_string();
                    update_stats(&inner, &conn_id, |s| {
                        if matches!(e, ProtoError::Io(_)) {
                            s.timeouts += 1;
                        } else {
                            s.errors += 1;
                        }
                        s.online = false;
                        s.last_error = msg.clone();
                    });
                    if let Ok(mut g) = inner.lock() {
                        let bad = g
                            .rows
                            .get(&poll.id)
                            .cloned()
                            .map(|mut r| {
                                for row in &mut r {
                                    row.quality = "bad".into();
                                }
                                r
                            })
                            .unwrap_or_default();
                        g.rows.insert(poll.id, bad);
                        g.poll_stats.entry(poll.id).or_default().err += 1;
                    }
                    push_log(
                        &inner,
                        &polls_conn_name(&inner, &conn_id),
                        "ERR",
                        format!("{}: {}", poll.name, msg),
                        false,
                    );
                    tr = None;
                    break;
                }
            }
        }
        thread::sleep(Duration::from_millis(if did { 3 } else { 15 }));
    }
}

#[cfg(test)]
mod tests {
    use super::eff_slave;
    use crate::model::Connection;

    #[test]
    fn eff_slave_follows_connection() {
        let c: Connection = serde_json::from_str(
            r#"{"id":"c","name":"C","kind":"tcp","host":"1.2.3.4","port":502,"slave":9}"#,
        )
        .unwrap();
        assert_eq!(eff_slave(0, Some(&c)), 9, "0 = follow the connection");
        assert_eq!(eff_slave(3, Some(&c)), 3, "explicit poll address wins");
        assert_eq!(eff_slave(0, None), 1, "unknown connection falls back to 1");
    }
}

fn polls_conn_name(inner: &Arc<Mutex<Inner>>, conn_id: &str) -> String {
    inner
        .lock()
        .ok()
        .and_then(|g| g.connections.get(conn_id).map(|c| c.name.clone()))
        .unwrap_or_else(|| conn_id.to_string())
}

fn do_command(inner: &Arc<Mutex<Inner>>, tr: &mut Box<dyn Transport>, conn_id: &str, cmd: Command) {
    match cmd {
        Command::Write {
            poll_id,
            addr,
            value,
            multi,
            values,
        } => {
            let poll = inner
                .lock()
                .ok()
                .and_then(|g| g.polls.iter().find(|p| p.id == poll_id).cloned());
            let mut poll = match poll {
                Some(p) => p,
                None => return,
            };
            // Same slave inheritance as the read path: poll.slave = 0 means
            // "use the connection default".
            let conn_now = inner
                .lock()
                .ok()
                .and_then(|g| g.connections.get(conn_id).cloned());
            poll.slave = eff_slave(poll.slave, conn_now.as_ref());
            let req = if multi {
                Request::WriteMultipleRegisters {
                    addr,
                    values: values.clone(),
                }
            } else if poll.is_bits() {
                Request::WriteSingleCoil {
                    addr,
                    on: value != 0,
                }
            } else {
                Request::WriteSingleRegister { addr, value }
            };
            let conn_name = polls_conn_name(inner, conn_id);
            match tr.call(poll.slave, &req.encode()) {
                Ok(pdu) => match decode_response(&pdu) {
                    Ok(_) => push_log(
                        inner,
                        &conn_name,
                        "WR",
                        format!("{} @{} = {}", poll.name, addr, if multi { "multi".into() } else { value.to_string() }),
                        true,
                    ),
                    Err(e) => push_log(inner, &conn_name, "ERR", format!("write @{addr}: {e}"), false),
                },
                Err(e) => push_log(inner, &conn_name, "ERR", format!("write @{addr}: {e}"), false),
            }
        }
    }
}
