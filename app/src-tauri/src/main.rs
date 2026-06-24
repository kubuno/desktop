// Never show a console window — applies to all build profiles on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    kubuno_desktop_lib::run()
}
