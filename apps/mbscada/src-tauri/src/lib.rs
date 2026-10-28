//! mbSCADA desktop shell (Tauri v2).
//!
//! Config-driven, Modbus Poll style. The UI owns connections / poll definitions
//! (your own addresses) / styling rules / computed tags / logging & archive
//! settings and pushes them here; `mb-engine` does the polling. This crate
//! only exposes commands — no polling logic.

use mb_engine::{
    ComputedTag, Connection, Engine, EngineCfg, Poll, Rule, Snapshot,
};
use mb_sim::net::SimServer;
use mb_sim::Sim;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Cfg {
    connections: Vec<Connection>,
    polls: Vec<Poll>,
    rules: Vec<Rule>,
    #[serde(default)]
    computed: Vec<ComputedTag>,
    #[serde(default)]
    logfile: LogCfg,
    #[serde(default)]
    archive: ArchiveCfg,
}

/// Engine event log (journal) file settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LogCfg {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    path: String,
}

impl Default for LogCfg {
    fn default() -> Self {
        Self {
            enabled: false,
            path: String::new(),
        }
    }
}

/// CSV data archive (historian) settings; the frontend appends rows through
/// the `csv_log` command, these fields only persist the configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArchiveCfg {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    path: String,
    /// Write interval for "all values" mode, seconds.
    #[serde(default = "def_arc_interval")]
    interval_s: u32,
    /// true = write every tag every `interval_s`; false = only changed rows.
    #[serde(default)]
    all_values: bool,
}

fn def_arc_interval() -> u32 {
    5
}

impl Default for ArchiveCfg {
    fn default() -> Self {
        Self {
            enabled: false,
            path: String::new(),
            interval_s: def_arc_interval(),
            all_values: false,
        }
    }
}

#[derive(Default)]
struct AppState {
    cfg: Mutex<Cfg>,
    engine: Mutex<Option<Engine>>,
}

// ------------------------------------------------------------------ slave --
// In-app slave simulator: the app answers as a Modbus device so both sides
// of a setup can be emulated (the master polls devices; the slave panel
// serves registers that any master — including this app — can read/write).

/// Running in-app slave.
#[derive(Default)]
struct SlaveState {
    run: Mutex<Option<SlaveRun>>,
}

/// One active slave transport: a TCP listener (MBAP or RTU-over-TCP) or a
/// physical serial line. The device state (`sim`) is shared with the slave
/// panel editor and the auto-generator either way.
struct SlaveRun {
    sim: Arc<Mutex<Sim>>,
    tcp: Option<SimServer>,
    serial: Option<mb_sim::net::SerialServer>,
    /// "tcp" | "rtutcp" | "serial"
    mode: String,
    unit: u8,
    /// Status-line text: `0.0.0.0:10502` or `COM3 @ 9600 8E1`.
    desc: String,
}

#[derive(Debug, Clone, Serialize)]
struct SlaveStatus {
    running: bool,
    addr: String,
    unit: u8,
    /// Legacy mirror of `mode == "rtutcp"` (v1.3 UI used a bool).
    rtu: bool,
    mode: String,
    clients: usize,
}

/// Serial line settings coming from the slave panel UI.
#[derive(Debug, Clone, Deserialize)]
struct SlaveSerialDto {
    port: String,
    baud: u32,
    /// "N" | "E" | "O"
    parity: String,
    data_bits: u8,
    stop_bits: u8,
}

fn slave_status_of(run: Option<&SlaveRun>) -> SlaveStatus {
    match run {
        Some(r) => SlaveStatus {
            running: true,
            addr: r.desc.clone(),
            unit: r.unit,
            rtu: r.mode == "rtutcp",
            mode: r.mode.clone(),
            clients: r.tcp.as_ref().map(|s| s.clients()).unwrap_or(0),
        },
        None => SlaveStatus {
            running: false,
            addr: String::new(),
            unit: 1,
            rtu: false,
            mode: String::new(),
            clients: 0,
        },
    }
}

/// Start the in-app slave simulator (idempotent while running).
/// `mode`: "tcp" | "rtutcp" | "serial" (physical COM/RS-485 line).
#[tauri::command]
fn slave_start(
    slave: State<SlaveState>,
    port: u16,
    unit: u8,
    mode: String,
    serial: Option<SlaveSerialDto>,
) -> Result<SlaveStatus, String> {
    let mut g = slave.run.lock().map_err(|_| "lock".to_string())?;
    if let Some(r) = g.as_ref() {
        return Ok(slave_status_of(Some(r)));
    }
    let (shared, tcp, serialsrv, desc) = match mode.as_str() {
        "serial" => {
            let d = serial.ok_or("не заданы параметры COM-порта")?;
            if d.port.is_empty() {
                return Err("не выбран COM-порт".into());
            }
            let p = mb_sim::net::SerialParams {
                port: d.port.clone(),
                baud: d.baud,
                parity: d.parity.chars().next().unwrap_or('E'),
                data_bits: d.data_bits,
                stop_bits: d.stop_bits,
            };
            let sim = Arc::new(Mutex::new(Sim::new(unit)));
            let srv = mb_sim::net::serve_serial_shared(Arc::clone(&sim), &p)
                .map_err(|e| format!("COM-порт {} занять не удалось: {e}", d.port))?;
            let desc = srv.desc.clone();
            (sim, None, Some(srv), desc)
        }
        _ => {
            let bind = format!("0.0.0.0:{port}");
            let rtu = mode == "rtutcp";
            let server = if rtu {
                mb_sim::net::serve_rtu(Sim::new(unit), &bind)
            } else {
                mb_sim::net::serve(Sim::new(unit), &bind)
            }
            .map_err(|e| format!("порт {port} занять не удалось: {e}"))?;
            let desc = server.addr.to_string();
            (Arc::clone(&server.sim), Some(server), None, desc)
        }
    };
    let st = SlaveStatus {
        running: true,
        addr: desc.clone(),
        unit,
        rtu: mode == "rtutcp",
        mode: mode.clone(),
        clients: 0,
    };
    *g = Some(SlaveRun { sim: shared, tcp, serial: serialsrv, mode, unit, desc });
    Ok(st)
}

/// Stop the slave simulator and release the port.
#[tauri::command]
fn slave_stop(slave: State<SlaveState>) -> Result<(), String> {
    let mut g = slave.run.lock().map_err(|_| "lock".to_string())?;
    if let Some(r) = g.take() {
        if let Some(s) = r.tcp {
            s.stop();
        }
        if let Some(s) = r.serial {
            s.stop();
        }
    }
    Ok(())
}

#[tauri::command]
fn slave_status(slave: State<SlaveState>) -> SlaveStatus {
    let g = slave.run.lock().ok();
    let r = g.as_deref().and_then(|inner| inner.as_ref());
    slave_status_of(r)
}

/// Read raw registers/bits of the simulated device (bits come back as 0/1).
#[tauri::command]
fn slave_read(slave: State<SlaveState>, area: String, start: u16, qty: u16) -> Result<Vec<u16>, String> {
    let g = slave.run.lock().map_err(|_| "lock".to_string())?;
    let run = g.as_ref().ok_or("slave не запущен")?;
    let sim = run.sim.lock().map_err(|_| "lock".to_string())?;
    let a = start as usize;
    let n = qty as usize;
    let out = match area.as_str() {
        "holding" => sim.holding.get(a..a + n).ok_or("вне диапазона")?.to_vec(),
        "input" => sim.input.get(a..a + n).ok_or("вне диапазона")?.to_vec(),
        "coils" => sim
            .coils
            .get(a..a + n)
            .ok_or("вне диапазона")?
            .iter()
            .map(|b| *b as u16)
            .collect(),
        "discrete" => sim
            .discrete
            .get(a..a + n)
            .ok_or("вне диапазона")?
            .iter()
            .map(|b| *b as u16)
            .collect(),
        other => return Err(format!("неизвестная область {other}")),
    };
    Ok(out)
}

/// Write raw registers/bits of the simulated device (used by the slave panel
/// editor and the auto-generator; external masters write via Modbus itself).
#[tauri::command]
fn slave_write(slave: State<SlaveState>, area: String, start: u16, values: Vec<u16>) -> Result<(), String> {
    let g = slave.run.lock().map_err(|_| "lock".to_string())?;
    let run = g.as_ref().ok_or("slave не запущен")?;
    let mut sim = run.sim.lock().map_err(|_| "lock".to_string())?;
    let a = start as usize;
    let n = values.len();
    match area.as_str() {
        "holding" => {
            let dst = sim.holding.get_mut(a..a + n).ok_or("вне диапазона")?;
            dst.copy_from_slice(&values);
        }
        "input" => {
            let dst = sim.input.get_mut(a..a + n).ok_or("вне диапазона")?;
            dst.copy_from_slice(&values);
        }
        "coils" => {
            let dst = sim.coils.get_mut(a..a + n).ok_or("вне диапазона")?;
            for (d, v) in dst.iter_mut().zip(values.iter()) {
                *d = *v != 0;
            }
        }
        "discrete" => {
            let dst = sim.discrete.get_mut(a..a + n).ok_or("вне диапазона")?;
            for (d, v) in dst.iter_mut().zip(values.iter()) {
                *d = *v != 0;
            }
        }
        other => return Err(format!("неизвестная область {other}")),
    }
    Ok(())
}

#[tauri::command]
fn ping() -> String {
    "pong".into()
}

#[derive(Debug, Clone, Serialize)]
struct PortInfo {
    name: String,
    detail: String,
}

/// Real serial ports with a human-readable detail (USB VID:PID, product…).
#[tauri::command]
fn list_serial_ports() -> Vec<PortInfo> {
    serialport::available_ports()
        .map(|ports| {
            ports
                .into_iter()
                .map(|p| {
                    let detail = match p.port_type {
                        serialport::SerialPortType::UsbPort(info) => format!(
                            "USB {:04X}:{:04X}{}",
                            info.vid,
                            info.pid,
                            info.product.map(|s| format!(" {s}")).unwrap_or_default()
                        ),
                        serialport::SerialPortType::PciPort => "PCI".into(),
                        serialport::SerialPortType::BluetoothPort => "Bluetooth".into(),
                        serialport::SerialPortType::Unknown => String::new(),
                    };
                    PortInfo {
                        name: p.port_name,
                        detail,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Open the transport once to verify the settings (serial port / TCP reachable).
#[tauri::command]
fn test_connection(conn: Connection) -> Result<String, String> {
    match mb_engine::open_transport(&conn) {
        Ok(_) => Ok(format!("связь есть: {}", conn.describe())),
        Err(e) => Err(e.to_string()),
    }
}

fn restart(state: &AppState) {
    let cfg = state.cfg.lock().unwrap().clone();
    let log_path = if cfg.logfile.enabled && !cfg.logfile.path.trim().is_empty() {
        Some(cfg.logfile.path.trim().to_string())
    } else {
        None
    };
    *state.engine.lock().unwrap() = Some(Engine::start_with(EngineCfg {
        connections: cfg.connections,
        polls: cfg.polls,
        rules: cfg.rules,
        computed: cfg.computed,
        log_path,
    }));
}

#[tauri::command]
fn apply_config(state: State<AppState>, cfg: Cfg) -> Result<(), String> {
    *state.cfg.lock().unwrap() = cfg;
    // Restart only if polling is active; editing while stopped just stores it.
    let running = state.engine.lock().unwrap().is_some();
    if running {
        restart(state.inner());
    }
    Ok(())
}

#[tauri::command]
fn start(state: State<AppState>) {
    restart(state.inner());
}

#[tauri::command]
fn stop(state: State<AppState>) {
    *state.engine.lock().unwrap() = None;
}

#[tauri::command]
fn snapshot(state: State<AppState>) -> Snapshot {
    let g = state.engine.lock().unwrap();
    match g.as_ref() {
        Some(e) => e.snapshot(),
        None => Snapshot::empty(),
    }
}

#[tauri::command]
fn get_config(state: State<AppState>) -> Cfg {
    state.cfg.lock().unwrap().clone()
}

#[tauri::command]
fn write_value(state: State<AppState>, poll_id: u32, addr: u16, value: u16) -> Result<(), String> {
    let g = state.engine.lock().unwrap();
    match g.as_ref() {
        Some(e) => e.write(poll_id, addr, value),
        None => Err("опрос не запущен".into()),
    }
}

#[tauri::command]
fn write_multi(
    state: State<AppState>,
    poll_id: u32,
    addr: u16,
    values: Vec<u16>,
) -> Result<(), String> {
    let g = state.engine.lock().unwrap();
    match g.as_ref() {
        Some(e) => e.write_multi(poll_id, addr, values),
        None => Err("опрос не запущен".into()),
    }
}

#[tauri::command]
fn save_project(path: String, data: String) -> Result<(), String> {
    std::fs::write(&path, data).map_err(|e| e.to_string())
}

#[tauri::command]
fn load_project(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

/// Append one or many raw CSV lines to a log file (frontend builds the rows).
#[tauri::command]
fn csv_log(path: String, lines: Vec<String>) -> Result<usize, String> {
    use std::io::Write as _;
    if lines.is_empty() {
        return Ok(0);
    }
    if let Some(dir) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    let mut n = 0;
    for l in &lines {
        writeln!(f, "{l}").map_err(|e| e.to_string())?;
        n += 1;
    }
    Ok(n)
}

/// Append raw lines to the engine event log file (used by the frontend for
/// UI-side events like archive start/stop).
#[tauri::command]
fn log_lines(path: String, lines: Vec<String>) -> Result<usize, String> {
    use std::io::Write as _;
    if lines.is_empty() {
        return Ok(0);
    }
    if let Some(dir) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    let mut n = 0;
    for l in &lines {
        writeln!(f, "{l}").map_err(|e| e.to_string())?;
        n += 1;
    }
    Ok(n)
}

/// Suggested default file locations (app data dir): journal and CSV archive.
#[tauri::command]
fn suggest_paths(app: tauri::AppHandle) -> SuggestedPaths {
    let dir = app
        .path()
        .app_data_dir()
        .ok()
        .or_else(|| dirs_home().map(|h| h.join(".mbscada")))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    SuggestedPaths {
        log: dir.join("mbscada.log").to_string_lossy().into_owned(),
        csv: dir.join("mbscada-data.csv").to_string_lossy().into_owned(),
    }
}

#[derive(Debug, Clone, Serialize)]
struct SuggestedPaths {
    log: String,
    csv: String,
}

fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
}

/// Import Modbus Poll files (`<path>` may be a .mbw, a .mbp, or a folder).
#[tauri::command]
fn import_paths(paths: Vec<String>) -> mb_engine::import::ImportResult {
    mb_engine::import::import_paths(paths)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .manage(SlaveState::default())
        .invoke_handler(tauri::generate_handler![
            ping,
            list_serial_ports,
            test_connection,
            apply_config,
            start,
            stop,
            snapshot,
            get_config,
            write_value,
            write_multi,
            save_project,
            load_project,
            csv_log,
            log_lines,
            suggest_paths,
            import_paths,
            slave_start,
            slave_stop,
            slave_status,
            slave_read,
            slave_write,
        ])
        .run(tauri::generate_context!())
        .expect("error while running mbSCADA");
}
