//! mbSCADA desktop shell (Tauri v2).
//!
//! Config-driven, Modbus Poll style. The UI owns connections / poll definitions
//! (your own addresses) / styling rules / computed tags / logging & archive
//! settings and pushes them here; `mb-engine` does the polling. This crate
//! only exposes commands — no polling logic.

use mb_engine::{
    ComputedTag, Connection, Engine, EngineCfg, Poll, Rule, Snapshot,
};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running mbSCADA");
}
