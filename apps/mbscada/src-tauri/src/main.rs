// Prevents an extra console window on Windows (always GUI subsystem).
#![windows_subsystem = "windows"]

fn main() {
    mbscada_lib::run()
}
