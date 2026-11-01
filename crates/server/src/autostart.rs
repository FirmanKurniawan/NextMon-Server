//! "Run when Windows starts": HKCU\Software\Microsoft\Windows\CurrentVersion\Run (per-user, no admin).
use crate::winproc;

const KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const NAME: &str = "NextMON Server";

pub fn command() -> String {
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    format!("\"{exe}\" --autostart")
}

pub fn is_enabled() -> bool {
    winproc::reg_get_string("HKCU", KEY, NAME).is_some()
}

pub fn set(enabled: bool) -> bool {
    if enabled {
        winproc::reg_set_string("HKCU", KEY, NAME, &command())
    } else {
        winproc::reg_delete_value("HKCU", KEY, NAME)
    }
}
