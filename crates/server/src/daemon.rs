//! Long-running mode: config + logging + single instance + control API + supervisor + tray/panel.
use crate::client::Client;
use crate::config;
use crate::core::Core;
use crate::paths::Paths;
use crate::runtime_api::{FsRuntimes, NmRuntimes, RuntimeApi};
use crate::supervisor::AppState;
use crate::{api, util, winproc};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct Opts {
    /// No tray / window; logs to stderr; Ctrl+C stops.
    pub headless: bool,
    /// Start hidden (no panel window, no browser).
    pub minimized: bool,
    /// Started from the Windows Run key.
    pub autostart: bool,
    /// Do not start the active edition at launch (used when the CLI spawns the daemon for other commands).
    pub no_autostart: bool,
}

/// Single-instance mutex name, unique per install folder so portable copies do not collide.
pub fn mutex_name(paths: &Paths) -> String {
    let s = paths.base.to_string_lossy().to_ascii_lowercase();
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("Local\\NextMONServer-{h:016x}")
}

pub fn make_runtime_api(paths: &Paths, min_node: &str) -> Arc<dyn RuntimeApi> {
    match NmRuntimes::new(paths.runtimes(), min_node) {
        Ok(r) => Arc::new(r),
        Err(e) => {
            log::warn!("runtime manager unavailable ({}), using folder scan only", e.message);
            Arc::new(FsRuntimes { root: paths.runtimes() })
        }
    }
}

#[cfg(windows)]
fn message_box(title: &str, text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let (t, x) = (util::wide(std::ffi::OsStr::new(title)), util::wide(std::ffi::OsStr::new(text)));
    unsafe {
        MessageBoxW(std::ptr::null_mut(), x.as_ptr(), t.as_ptr(), MB_OK | MB_ICONERROR);
    }
}
#[cfg(not(windows))]
fn message_box(_t: &str, _x: &str) {}

pub fn run(opts: Opts) -> i32 {
    let paths = Paths::discover();
    paths.ensure_dirs();
    let (cfg, note) = config::load_or_create(&paths);
    crate::logger::init(&paths, opts.headless);
    if let Some(n) = note {
        log::warn!("{n}");
    }
    api::LANG_EN.store(cfg.language == "en", std::sync::atomic::Ordering::Relaxed);

    // Single instance: a second launch asks the first to show its panel.
    let (_mutex_keepalive, already) = winproc::single_instance(&mutex_name(&paths));
    if already {
        let c = Client::new(cfg.control_port, &cfg.control_token);
        if c.request("POST", "/api/panel/open", Some(&serde_json::json!({})), Duration::from_secs(3)).is_err() {
            util::shell_open(&format!("http://127.0.0.1:{}/", cfg.control_port));
        }
        eprintln!("NextMON Server is already running.");
        return 0;
    }

    log::info!("NextMON Server {} starting in {}", crate::core::VERSION, paths.base.display());
    let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(4).thread_name("nextmon").enable_all().build() {
        Ok(r) => r,
        Err(e) => {
            log::error!("cannot start async runtime: {e}");
            return 1;
        }
    };
    // Bind the control port first. If the configured port is unavailable (in use, or inside a Windows excluded
    // range -> EACCES) fall back to a free port and persist it so the CLI and the next start find it.
    let mut cfg = cfg;
    let listener = match std::net::TcpListener::bind(("127.0.0.1", cfg.control_port)) {
        Ok(l) => l,
        Err(e) => {
            log::warn!("control port {} unavailable ({e}); picking a free port", cfg.control_port);
            match std::net::TcpListener::bind(("127.0.0.1", 0)) {
                Ok(l) => {
                    cfg.control_port = l.local_addr().map(|a| a.port()).unwrap_or(cfg.control_port);
                    if let Err(e) = config::save(&paths, &cfg) {
                        log::warn!("could not persist the new controlPort: {e}");
                    }
                    log::info!("control panel moved to port {}", cfg.control_port);
                    l
                }
                Err(e2) => {
                    let msg = format!("Cannot open a local control port ({e2}).");
                    log::error!("{msg}");
                    eprintln!("{msg}");
                    if !opts.headless {
                        message_box("NextMON Server", &msg);
                    }
                    return 1;
                }
            }
        }
    };
    let rtapi = make_runtime_api(&paths, &cfg.min_node);
    let core = Core::new(paths.clone(), cfg.clone(), rtapi);
    {
        let c = core.clone();
        rt.spawn(async move {
            if let Err(e) = api::serve(c, listener).await {
                log::error!("control API stopped: {e}");
            }
        });
    }
    if opts.no_autostart {
        core.cfg.write().unwrap().auto_start_edition = false;
    }
    rt.block_on(core.boot());
    if opts.no_autostart {
        // keep the persisted setting intact: we only suppressed it for this process
        core.cfg.write().unwrap().auto_start_edition = cfg.auto_start_edition;
    }

    let minimized = opts.minimized || cfg.start_minimized;
    if cfg.open_browser_on_start && !minimized && !opts.no_autostart && cfg.auto_start_edition {
        let c = core.clone();
        rt.spawn(async move {
            if c.sup.wait_settled(Duration::from_secs(45)).await == AppState::Running {
                let _ = c.open("app");
            }
        });
    }

    if opts.headless {
        rt.block_on(async {
            tokio::select! {
                _ = core.shutdown.notified() => {}
                _ = tokio::signal::ctrl_c() => {}
            }
            log::info!("shutting down");
            core.sup.stop().await;
        });
        return 0;
    }
    // GUI: tray + panel on this thread
    let show_panel = !minimized && !opts.no_autostart;
    crate::gui::run(core, rt.handle().clone(), show_panel)
}
