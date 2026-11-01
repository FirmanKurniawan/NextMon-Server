//! Tray icon + panel window (WebView2 via wry when installed, otherwise the default browser).
use crate::config::Edition;
use crate::core::{Core, UiCmd, UiHook};
use crate::supervisor::AppState;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopWindowTarget};
use tao::window::{Window, WindowBuilder, WindowId};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[derive(Debug, Clone)]
enum UserEvent {
    Cmd(UiCmd),
    Tick,
    Menu(MenuEvent),
    TrayClick,
}

struct Panel {
    window: Window,
    _webview: wry::WebView,
}

struct TrayItems {
    status: MenuItem,
    edition_menu: Submenu,
    ed_php: CheckMenuItem,
    ed_next: CheckMenuItem,
    open_panel: MenuItem,
    start: MenuItem,
    stop: MenuItem,
    restart: MenuItem,
    open_app: MenuItem,
    open_logs: MenuItem,
    open_data: MenuItem,
    autostart: CheckMenuItem,
    quit: MenuItem,
}

fn tr(lang: &str, id: &'static str, en: &'static str) -> &'static str {
    if lang == "en" {
        en
    } else {
        id
    }
}

fn load_icon() -> Option<Icon> {
    #[cfg(windows)]
    {
        Icon::from_resource(1, Some((32, 32))).ok()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn state_text(lang: &str, s: AppState) -> &'static str {
    match s {
        AppState::Stopped => tr(lang, "berhenti", "stopped"),
        AppState::Starting => tr(lang, "memulai...", "starting..."),
        AppState::Running => tr(lang, "berjalan", "running"),
        AppState::Stopping => tr(lang, "menghentikan...", "stopping..."),
        AppState::Backoff => tr(lang, "mencoba ulang...", "retrying..."),
        AppState::Failed => tr(lang, "gagal", "failed"),
    }
}

fn build_tray(core: &Core) -> Option<(TrayIcon, TrayItems)> {
    let lang = core.lang();
    let menu = Menu::new();
    let title = MenuItem::new(format!("NextMON Server v{}", crate::core::VERSION), false, None);
    let open_panel = MenuItem::new(tr(&lang, "Buka panel kontrol", "Open control panel"), true, None);
    let status = MenuItem::new("...", false, None);
    let edition_menu = Submenu::new(tr(&lang, "Edisi aktif", "Active edition"), true);
    let ed_php = CheckMenuItem::new("PHP", true, false, None);
    let ed_next = CheckMenuItem::new("Next.js", true, false, None);
    let _ = edition_menu.append(&ed_php);
    let _ = edition_menu.append(&ed_next);
    let start = MenuItem::new(tr(&lang, "Mulai", "Start"), true, None);
    let stop = MenuItem::new(tr(&lang, "Hentikan", "Stop"), true, None);
    let restart = MenuItem::new(tr(&lang, "Mulai ulang", "Restart"), true, None);
    let open_app = MenuItem::new(tr(&lang, "Buka aplikasi di browser", "Open app in browser"), true, None);
    let open_logs = MenuItem::new(tr(&lang, "Buka folder log", "Open logs folder"), true, None);
    let open_data = MenuItem::new(tr(&lang, "Buka folder data", "Open data folder"), true, None);
    let autostart = CheckMenuItem::new(tr(&lang, "Jalankan saat Windows start", "Run when Windows starts"), true, false, None);
    let quit = MenuItem::new(tr(&lang, "Keluar", "Quit"), true, None);
    let sep = || PredefinedMenuItem::separator();
    let _ = menu.append_items(&[
        &title,
        &open_panel,
        &sep(),
        &status,
        &edition_menu,
        &start,
        &stop,
        &restart,
        &open_app,
        &sep(),
        &open_logs,
        &open_data,
        &autostart,
        &sep(),
        &quit,
    ]);
    let mut b = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("NextMON Server").with_menu_on_left_click(false);
    if let Some(i) = load_icon() {
        b = b.with_icon(i);
    }
    match b.build() {
        Ok(t) => Some((t, TrayItems { status, edition_menu, ed_php, ed_next, open_panel, start, stop, restart, open_app, open_logs, open_data, autostart, quit })),
        Err(e) => {
            log::warn!("tray icon unavailable: {e}");
            None
        }
    }
}

fn refresh_tray(core: &Core, tray: &TrayIcon, it: &TrayItems) {
    let lang = core.lang();
    let cfg = core.config();
    let snap = core.active_snapshot();
    let port = cfg.app(cfg.edition).port;
    let line = format!("{} - {} :{}", cfg.edition.label(), state_text(&lang, snap.state), port);
    it.status.set_text(&line);
    it.edition_menu.set_text(format!("{}: {}", tr(&lang, "Edisi aktif", "Active edition"), cfg.edition.label()));
    it.ed_php.set_checked(cfg.edition == Edition::Php);
    it.ed_next.set_checked(cfg.edition == Edition::Next);
    let active = matches!(snap.state, AppState::Starting | AppState::Running | AppState::Backoff);
    it.start.set_enabled(!active && snap.state != AppState::Stopping);
    it.stop.set_enabled(active);
    it.restart.set_enabled(active);
    it.open_app.set_enabled(snap.state == AppState::Running);
    it.autostart.set_checked(crate::autostart::is_enabled());
    let _ = tray.set_tooltip(Some(format!("NextMON Server - {line}")));
    // labels follow the language setting
    it.open_panel.set_text(tr(&lang, "Buka panel kontrol", "Open control panel"));
    it.start.set_text(tr(&lang, "Mulai", "Start"));
    it.stop.set_text(tr(&lang, "Hentikan", "Stop"));
    it.restart.set_text(tr(&lang, "Mulai ulang", "Restart"));
    it.open_app.set_text(tr(&lang, "Buka aplikasi di browser", "Open app in browser"));
    it.open_logs.set_text(tr(&lang, "Buka folder log", "Open logs folder"));
    it.open_data.set_text(tr(&lang, "Buka folder data", "Open data folder"));
    it.autostart.set_text(tr(&lang, "Jalankan saat Windows start", "Run when Windows starts"));
    it.quit.set_text(tr(&lang, "Keluar", "Quit"));
}

fn open_panel(target: &EventLoopWindowTarget<UserEvent>, core: &Core, panel: &mut Option<Panel>) {
    let url = format!("http://127.0.0.1:{}/", core.config().control_port);
    if let Some(p) = panel.as_ref() {
        p.window.set_visible(true);
        p.window.set_minimized(false);
        p.window.set_focus();
        return;
    }
    if crate::diagnostics::webview2_version().is_none() {
        crate::util::shell_open(&url);
        return;
    }
    let build = || -> Result<Panel, String> {
        let window = WindowBuilder::new()
            .with_title("NextMON Server")
            .with_inner_size(LogicalSize::new(1120.0, 780.0))
            .with_min_inner_size(LogicalSize::new(420.0, 560.0))
            .build(target)
            .map_err(|e| e.to_string())?;
        let data_dir = core.paths.base.join(".webview2");
        let mut ctx = wry::WebContext::new(Some(data_dir));
        let webview = wry::WebViewBuilder::new_with_web_context(&mut ctx).with_url(&url).build(&window).map_err(|e| e.to_string())?;
        Ok(Panel { window, _webview: webview })
    };
    match build() {
        Ok(p) => *panel = Some(p),
        Err(e) => {
            log::warn!("WebView2 panel failed ({e}); opening the browser instead");
            crate::util::shell_open(&url);
        }
    }
}

/// Runs the tray + panel event loop on the calling (main) thread. Never returns.
pub fn run(core: Arc<Core>, rt: tokio::runtime::Handle, show_panel: bool) -> ! {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // UI hook for the API / tasks
    let hook_proxy = Mutex::new(proxy.clone());
    let hook: UiHook = Arc::new(move |c| {
        let _ = hook_proxy.lock().unwrap().send_event(UserEvent::Cmd(c));
    });
    *core.ui.lock().unwrap() = Some(hook);

    let p2 = Mutex::new(proxy.clone());
    MenuEvent::set_event_handler(Some(move |e| {
        let _ = p2.lock().unwrap().send_event(UserEvent::Menu(e));
    }));
    let p3 = Mutex::new(proxy.clone());
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::Click { button: tray_icon::MouseButton::Left, button_state: tray_icon::MouseButtonState::Up, .. } = e {
            let _ = p3.lock().unwrap().send_event(UserEvent::TrayClick);
        }
    }));
    let tick_proxy = proxy.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(2));
        if tick_proxy.send_event(UserEvent::Tick).is_err() {
            break;
        }
    });

    let mut tray: Option<(TrayIcon, TrayItems)> = None;
    let mut panel: Option<Panel> = None;
    let mut first = true;

    event_loop.run(move |event, target, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(tao::event::StartCause::Init) => {
                tray = build_tray(&core);
                if let Some((t, it)) = &tray {
                    refresh_tray(&core, t, it);
                }
                if first {
                    first = false;
                    if show_panel || tray.is_none() {
                        open_panel(target, &core, &mut panel);
                    }
                }
            }
            Event::UserEvent(UserEvent::Tick) | Event::UserEvent(UserEvent::Cmd(UiCmd::Refresh)) => {
                if let Some((t, it)) = &tray {
                    refresh_tray(&core, t, it);
                }
            }
            Event::UserEvent(UserEvent::Cmd(UiCmd::ShowPanel)) | Event::UserEvent(UserEvent::TrayClick) => open_panel(target, &core, &mut panel),
            Event::UserEvent(UserEvent::Cmd(UiCmd::Quit)) => {
                quit(&core, &rt, &mut tray);
                *flow = ControlFlow::Exit;
            }
            Event::UserEvent(UserEvent::Menu(e)) => {
                let Some((_, it)) = &tray else { return };
                let id = e.id;
                let c = core.clone();
                if id == it.quit.id() {
                    quit(&core, &rt, &mut tray);
                    *flow = ControlFlow::Exit;
                } else if id == it.open_panel.id() {
                    open_panel(target, &core, &mut panel);
                } else if id == it.start.id() {
                    let hook = c.ui.lock().unwrap().clone();
                    rt.spawn(async move {
                        if let Err(e) = c.app_start().await {
                            log::warn!("tray start failed: {}", e.en);
                            if let Some(h) = hook {
                                h(UiCmd::ShowPanel);
                            }
                        }
                    });
                } else if id == it.stop.id() {
                    rt.spawn(async move { c.app_stop().await });
                } else if id == it.restart.id() {
                    rt.spawn(async move {
                        let _ = c.app_restart().await;
                    });
                } else if id == it.open_app.id() {
                    let _ = c.open("app");
                } else if id == it.open_logs.id() {
                    let _ = c.open("folder:logs");
                } else if id == it.open_data.id() {
                    let _ = c.open("folder:data");
                } else if id == it.autostart.id() {
                    let on = !crate::autostart::is_enabled();
                    rt.spawn(async move {
                        let _ = c.config_patch(serde_json::json!({"autoStartWithWindows": on})).await;
                    });
                } else if id == it.ed_php.id() || id == it.ed_next.id() {
                    let ed = if id == it.ed_php.id() { Edition::Php } else { Edition::Next };
                    let hook = c.ui.lock().unwrap().clone();
                    let _guard = rt.enter();
                    let start = c.sup.is_active();
                    if c.config().edition != ed {
                        if let Err(e) = c.switch_edition(ed, start) {
                            log::warn!("edition switch refused: {}", e.en);
                            if let Some(h) = hook {
                                h(UiCmd::ShowPanel);
                            }
                        }
                    }
                }
                if let Some((t, it)) = &tray {
                    refresh_tray(&core, t, it);
                }
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, window_id, .. } => {
                if panel.as_ref().map(|p| p.window.id() == window_id).unwrap_or(false) {
                    panel = None; // destroys the WebView2 process
                }
            }
            _ => {}
        }
        let _: Option<WindowId> = None;
    })
}

fn quit(core: &Arc<Core>, rt: &tokio::runtime::Handle, tray: &mut Option<(TrayIcon, TrayItems)>) {
    log::info!("quit requested");
    *tray = None;
    rt.block_on(core.sup.stop());
    core.shutdown.notify_waiters();
}
