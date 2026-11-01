// Windows GUI subsystem: no console window when double-clicked. CLI use re-attaches to the parent console.
#![cfg_attr(windows, windows_subsystem = "windows")]

use nextmon_server::{cli, daemon, winproc};

fn main() {
    let mut json = false;
    let mut opts = daemon::Opts::default();
    let mut rest: Vec<String> = vec![];
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json = true,
            "--headless" => opts.headless = true,
            "--minimized" => opts.minimized = true,
            "--autostart" => opts.autostart = true,
            "--no-autostart" => opts.no_autostart = true,
            "--home" => {
                if let Some(h) = it.next() {
                    std::env::set_var("NEXTMON_HOME", h);
                }
            }
            _ => rest.push(a),
        }
    }
    let serve = rest.first().map(|s| s == "serve").unwrap_or(false);
    let daemon_mode = rest.is_empty() || serve || rest[0] == "run";
    if !daemon_mode || opts.headless || serve {
        winproc::attach_parent_console();
    }
    if daemon_mode {
        if serve {
            opts.headless = true;
        }
        std::process::exit(daemon::run(opts));
    }
    std::process::exit(cli::run(rest, json));
}
