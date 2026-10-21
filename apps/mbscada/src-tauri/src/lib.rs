//! mbSCADA desktop shell (Tauri v2).
//!
//! Config-driven, Modbus Poll style. The UI owns connections / poll definitions
//! (your own addresses) / styling rules and pushes them here; `mb-engine` does
//! the polling. This crate only exposes commands — no polling logic.

use mb_engine::{Connection, Engine, Poll, Rule, Snapshot};use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::State;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Cfg {
    connections: Vec<Connection>,
    polls: Vec<Poll>,
    rules: Vec<Rule>,
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
    *state.engine.lock().unwrap() = Some(Engine::start(cfg.connections, cfg.polls, cfg.rules));
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
            import_paths,
        ])
        .run(tauri::generate_context!())
        .expect("error while running mbSCADA");
}
