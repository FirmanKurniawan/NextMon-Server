//! Command line interface. Talks to the running daemon over the control API; a few commands also work offline.
use crate::client::{Client, Reply};
use crate::config::{self, Config, Edition};
use crate::paths::Paths;
use crate::{autostart, backup, diagnostics};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub const HELP: &str = "NextMON Server - tray launcher, supervisor and control panel for NextMON

USAGE: nextmon-server [--json] [--home <dir>] <command>

  (no command)               start the launcher (tray + panel); also: run, serve [--headless]
  start [php|next]           start the app (switches edition first if one is given)
  stop | restart             stop / restart the active edition
  status                     state of the launcher and the app (exit 0 = running, 3 = not running)
  edition [php|next]         show or change the active edition (only one runs at a time)
  php list|available|install <v>|use <v>|upgrade
  node list|available|install <v|lts|latest>|use <v>|upgrade
  backup create|list|restore <name>
  update check|apply         application updates (GitHub Releases)
  doctor                     environment checks (VC++, ports, PHP extensions, Node version, WebView2)
  open [panel|app|data|logs] open in browser / Explorer
  autostart on|off|status    run when Windows starts (per-user)
  quit                       stop the app and exit the launcher
  version

Flags: --json machine-readable output; --home <dir> use another install folder; --headless (run/serve) no tray.
";

struct Ctx {
    json: bool,
    paths: Paths,
    cfg: Config,
}

impl Ctx {
    fn en(&self) -> bool {
        self.cfg.language == "en"
    }
    fn t<'a>(&self, id: &'a str, en: &'a str) -> &'a str {
        if self.en() {
            en
        } else {
            id
        }
    }
    fn client(&self) -> Client {
        Client::new(self.cfg.control_port, &self.cfg.control_token)
    }
    fn print(&self, v: &Value) {
        println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
    }
}

fn fail(ctx: &Ctx, code: &str, msg: &str) -> i32 {
    if ctx.json {
        ctx.print(&json!({"error": {"code": code, "message": msg}}));
    } else {
        eprintln!("error: {msg}");
    }
    1
}

fn fail_reply(ctx: &Ctx, r: &Reply) -> i32 {
    if ctx.json {
        println!("{}", r.body);
    } else {
        eprintln!("error: {}", r.error_message(ctx.en()));
    }
    1
}

/// Start the daemon (hidden, no autostart of the app) if it is not running; returns a ready client.
fn ensure_daemon(ctx: &Ctx) -> Result<Client, String> {
    let c = ctx.client();
    if c.alive() {
        return Ok(c);
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--minimized").arg("--no-autostart").env("NEXTMON_HOME", &ctx.paths.base);
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0008 | 0x0000_0200 | 0x0800_0000); // DETACHED_PROCESS | NEW_PROCESS_GROUP | NO_WINDOW
    }
    cmd.spawn().map_err(|e| format!("cannot start the launcher: {e}"))?;
    let end = Instant::now() + Duration::from_secs(15);
    while Instant::now() < end {
        if c.alive() {
            return Ok(c);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err("the launcher did not become ready (is the control port in use?)".into())
}

fn wait_job(ctx: &Ctx, c: &Client, id: &str) -> Result<Value, String> {
    let mut last = String::new();
    loop {
        let r = c.get(&format!("/api/jobs/{id}"))?;
        let j = r.json();
        if !ctx.json {
            let line = format!("[{:>3.0}%] {} {}", j["progress"].as_f64().unwrap_or(0.0) * 100.0, j["step"].as_str().unwrap_or(""), j["message"].as_str().unwrap_or(""));
            if line != last {
                eprintln!("{line}");
                last = line;
            }
        }
        match j["state"].as_str() {
            Some("running") => std::thread::sleep(Duration::from_millis(400)),
            _ => return Ok(j),
        }
    }
}

fn finish_job(ctx: &Ctx, j: &Value, ok_msg: &str) -> i32 {
    if j["state"] == "done" {
        if ctx.json {
            ctx.print(j);
        } else {
            println!("{ok_msg}");
        }
        0
    } else {
        if ctx.json {
            ctx.print(j);
        } else {
            eprintln!("error: {}", j["error"]["message"].as_str().unwrap_or("failed"));
        }
        1
    }
}

pub fn run(args: Vec<String>, json: bool) -> i32 {
    let paths = Paths::discover();
    let (cfg, _) = config::load_or_create(&paths);
    let ctx = Ctx { json, paths, cfg };
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    match a.as_slice() {
        ["version"] | ["--version"] | ["-V"] => {
            if ctx.json {
                ctx.print(&json!({"name": "NextMON Server", "version": crate::core::VERSION, "appVersion": crate::update::current_version(&ctx.paths)}));
            } else {
                println!("NextMON Server {} (app {})", crate::core::VERSION, crate::update::current_version(&ctx.paths));
            }
            0
        }
        ["help"] | ["--help"] | ["-h"] => {
            print!("{HELP}");
            0
        }
        ["status"] => cmd_status(&ctx),
        ["start"] | ["start", "all"] => cmd_start(&ctx, None),
        ["start", e] => match Edition::parse(e) {
            Some(ed) => cmd_start(&ctx, Some(ed)),
            None => usage(&ctx, "edition must be php, next or all"),
        },
        ["stop"] => cmd_simple(&ctx, "stop"),
        ["restart"] => cmd_simple(&ctx, "restart"),
        ["edition"] => {
            if ctx.json {
                ctx.print(&json!({"edition": ctx.cfg.edition.as_str()}));
            } else {
                println!("{}", ctx.cfg.edition.as_str());
            }
            0
        }
        ["edition", e] => match Edition::parse(e) {
            Some(ed) => cmd_edition(&ctx, ed),
            None => usage(&ctx, "edition must be php or next"),
        },
        [k @ ("php" | "node"), rest @ ..] => cmd_runtime(&ctx, k, rest),
        ["backup", rest @ ..] => cmd_backup(&ctx, rest),
        ["update", "check"] => cmd_update_check(&ctx),
        ["update", "apply"] => cmd_update_apply(&ctx),
        ["doctor"] => cmd_doctor(&ctx),
        ["open"] => cmd_open(&ctx, "panel"),
        ["open", what] => cmd_open(&ctx, what),
        ["autostart", v] => cmd_autostart(&ctx, v),
        ["quit"] | ["exit"] => {
            let c = ctx.client();
            if !c.alive() {
                println!("{}", ctx.t("NextMON Server tidak berjalan", "NextMON Server is not running"));
                return 0;
            }
            let _ = c.post("/api/shutdown", &json!({}));
            println!("{}", ctx.t("Meminta NextMON Server berhenti...", "Asking NextMON Server to quit..."));
            0
        }
        [] => usage(&ctx, "missing command"),
        other => usage(&ctx, &format!("unknown command: {}", other.join(" "))),
    }
}

fn usage(ctx: &Ctx, msg: &str) -> i32 {
    if ctx.json {
        ctx.print(&json!({"error": {"code": "usage", "message": msg}}));
    } else {
        eprintln!("error: {msg}\n\n{HELP}");
    }
    2
}

fn cmd_status(ctx: &Ctx) -> i32 {
    let c = ctx.client();
    if !c.alive() {
        if ctx.json {
            ctx.print(&json!({"daemon": false, "edition": ctx.cfg.edition.as_str(), "app": {"state": "stopped"}, "ports": {"control": ctx.cfg.control_port}}));
        } else {
            println!("{}: {}", ctx.t("Launcher", "Launcher"), ctx.t("tidak berjalan (jalankan: NextMON Server.exe)", "not running (start: NextMON Server.exe)"));
            println!("{}: {}", ctx.t("Edisi aktif", "Active edition"), ctx.cfg.edition.label());
        }
        return 3;
    }
    let r = match c.get("/api/status") {
        Ok(r) if r.ok() => r,
        Ok(r) => return fail_reply(ctx, &r),
        Err(e) => return fail(ctx, "network", &e),
    };
    let s = r.json();
    let running = s["app"]["state"] == "running";
    if ctx.json {
        let mut s2 = s.clone();
        s2["daemon"] = json!(true);
        if let Some(o) = s2.as_object_mut() {
            o.remove("config");
        }
        ctx.print(&s2);
    } else {
        let app = &s["app"];
        println!("NextMON Server v{}  (app {})", s["version"].as_str().unwrap_or("?"), s["appVersion"].as_str().unwrap_or("?"));
        println!("{:<14}: {}", ctx.t("Edisi aktif", "Active edition"), s["editions"][s["edition"].as_str().unwrap_or("php")]["label"].as_str().unwrap_or("?"));
        println!(
            "{:<14}: {}{}{}",
            ctx.t("Status", "State"),
            app["state"].as_str().unwrap_or("?"),
            app["pid"].as_u64().map(|p| format!("  pid {p}")).unwrap_or_default(),
            if running { format!("  uptime {}s", app["uptime"].as_u64().unwrap_or(0)) } else { String::new() }
        );
        println!("{:<14}: {}{}", "Runtime", app["runtime"].as_str().unwrap_or("-"), app["ramMB"].as_f64().map(|m| format!("  ({m} MB RAM)")).unwrap_or_default());
        println!("{:<14}: {}", "URL", app["url"].as_str().unwrap_or("-"));
        if let Some(l) = app["lanUrl"].as_str() {
            println!("{:<14}: {}", "LAN", l);
        }
        println!("{:<14}: http://127.0.0.1:{}/", ctx.t("Panel", "Panel"), ctx.cfg.control_port);
        if let Some(e) = app["lastError"].as_str() {
            println!("{:<14}: {}", ctx.t("Catatan", "Note"), e);
        }
    }
    if running {
        0
    } else {
        3
    }
}

fn cmd_start(ctx: &Ctx, ed: Option<Edition>) -> i32 {
    let c = match ensure_daemon(ctx) {
        Ok(c) => c,
        Err(e) => return fail(ctx, "daemon", &e),
    };
    if let Some(ed) = ed {
        if ed != ctx.cfg.edition {
            // refresh config in case the daemon changed it
            return cmd_edition_with(ctx, &c, ed, true);
        }
    }
    match c.post("/api/app/start", &json!({})) {
        Ok(r) if r.ok() => {
            let _ = wait_running(&c, 45);
            let st = c.get("/api/status").map(|r| r.json()).unwrap_or(Value::Null);
            if ctx.json {
                ctx.print(&st["app"]);
            } else {
                println!("{} {}", ctx.t("Berjalan:", "Running:"), st["app"]["url"].as_str().unwrap_or(""));
            }
            if st["app"]["state"] == "running" {
                0
            } else {
                eprintln!("{}: {}", ctx.t("Aplikasi belum siap", "App is not ready"), st["app"]["lastError"].as_str().unwrap_or(st["app"]["state"].as_str().unwrap_or("?")));
                1
            }
        }
        Ok(r) => fail_reply(ctx, &r),
        Err(e) => fail(ctx, "network", &e),
    }
}

fn wait_running(c: &Client, secs: u64) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if let Ok(r) = c.get("/api/status") {
            match r.json()["app"]["state"].as_str() {
                Some("running") => return true,
                Some("failed") | Some("stopped") => return false,
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    false
}

fn cmd_simple(ctx: &Ctx, action: &str) -> i32 {
    let c = ctx.client();
    if !c.alive() {
        if action == "stop" {
            println!("{}", ctx.t("Tidak berjalan.", "Not running."));
            return 0;
        }
        return cmd_start(ctx, None);
    }
    match c.post(&format!("/api/app/{action}"), &json!({})) {
        Ok(r) if r.ok() => {
            if action == "restart" {
                wait_running(&c, 45);
            }
            if ctx.json {
                println!("{}", c.get("/api/status").map(|r| r.json()["app"].to_string()).unwrap_or_default());
            } else {
                println!("{}", if action == "stop" { ctx.t("Dihentikan.", "Stopped.") } else { ctx.t("Dimulai ulang.", "Restarted.") });
            }
            0
        }
        Ok(r) => fail_reply(ctx, &r),
        Err(e) => fail(ctx, "network", &e),
    }
}

fn cmd_edition(ctx: &Ctx, ed: Edition) -> i32 {
    // Without a running launcher just change the setting (nothing can be running).
    let c = ctx.client();
    if !c.alive() {
        let mut cfg = ctx.cfg.clone();
        cfg.edition = ed;
        return match config::save(&ctx.paths, &cfg) {
            Ok(()) => {
                println!("{} {}", ctx.t("Edisi aktif:", "Active edition:"), ed.label());
                0
            }
            Err(e) => fail(ctx, "config", &e),
        };
    }
    let start = c.get("/api/status").map(|r| r.json()["app"]["state"] != "stopped").unwrap_or(false);
    cmd_edition_with(ctx, &c, ed, start)
}

fn cmd_edition_with(ctx: &Ctx, c: &Client, ed: Edition, start: bool) -> i32 {
    let r = match c.post("/api/edition", &json!({"edition": ed.as_str(), "start": start})) {
        Ok(r) => r,
        Err(e) => return fail(ctx, "network", &e),
    };
    if !r.ok() {
        return fail_reply(ctx, &r);
    }
    let id = r.json()["jobId"].as_str().unwrap_or("").to_string();
    match wait_job(ctx, c, &id) {
        Ok(j) => {
            if j["state"] == "done" && start {
                wait_running(c, 45);
            }
            finish_job(ctx, &j, &format!("{} {}", ctx.t("Edisi aktif sekarang:", "Active edition is now:"), ed.label()))
        }
        Err(e) => fail(ctx, "network", &e),
    }
}

fn cmd_runtime(ctx: &Ctx, kind: &str, rest: &[&str]) -> i32 {
    let c = match ensure_daemon(ctx) {
        Ok(c) => c,
        Err(e) => return fail(ctx, "daemon", &e),
    };
    let net = |e: String| fail(ctx, "network", &e);
    match rest {
        ["list"] | ["available"] => {
            let refresh = if rest[0] == "available" { "?refresh=1" } else { "" };
            let r = match c.get(&format!("/api/runtimes/{kind}{refresh}")) {
                Ok(r) => r,
                Err(e) => return net(e),
            };
            if !r.ok() {
                return fail_reply(ctx, &r);
            }
            let v = r.json();
            if ctx.json {
                ctx.print(&v);
                return 0;
            }
            if rest[0] == "list" {
                let inst = v["installed"].as_array().cloned().unwrap_or_default();
                if inst.is_empty() {
                    println!("{}", ctx.t("Belum ada runtime terpasang.", "No runtime installed."));
                }
                for i in inst {
                    println!(
                        "{} {:<12} {:>8.1} MB  {}  {}",
                        if i["active"] == true { "*" } else { " " },
                        i["version"].as_str().unwrap_or("?"),
                        i["sizeMB"].as_f64().unwrap_or(0.0),
                        i["tsnts"].as_str().unwrap_or(""),
                        i["usedBy"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(",")).unwrap_or_default()
                    );
                }
            } else {
                for a in v["available"].as_array().cloned().unwrap_or_default().iter().take(25) {
                    println!(
                        "  {:<12} {:<8} {}{}",
                        a["version"].as_str().unwrap_or("?"),
                        a["channel"].as_str().unwrap_or(""),
                        a["date"].as_str().unwrap_or(""),
                        if a["isNewer"] == true { "  (new)" } else { "" }
                    );
                }
                if v["offline"] == true {
                    eprintln!("{}", ctx.t("(offline: daftar dari cache)", "(offline: list from cache)"));
                }
            }
            0
        }
        ["install", v] => post_job(ctx, &c, &format!("/api/runtimes/{kind}/install"), json!({"version": v}), &format!("{kind} {v} {}", ctx.t("terpasang", "installed"))),
        ["upgrade"] => {
            let r = match c.post(&format!("/api/runtimes/{kind}/upgrade"), &json!({})) {
                Ok(r) => r,
                Err(e) => return net(e),
            };
            if !r.ok() {
                return fail_reply(ctx, &r);
            }
            let id = r.json()["jobId"].as_str().unwrap_or("").to_string();
            match wait_job(ctx, &c, &id) {
                Ok(j) => {
                    let up = j["result"]["upgraded"] == true;
                    let msg = if up {
                        format!("{kind} -> {}", j["result"]["to"].as_str().unwrap_or("?"))
                    } else {
                        ctx.t("Sudah versi terbaru.", "Already up to date.").to_string()
                    };
                    finish_job(ctx, &j, &msg)
                }
                Err(e) => net(e),
            }
        }
        ["use", v] => match c.post(&format!("/api/runtimes/{kind}/activate"), &json!({"version": v})) {
            Ok(r) if r.ok() => {
                if ctx.json {
                    println!("{}", r.body);
                } else {
                    println!("{} {kind} {v}", ctx.t("Dipakai:", "Now using:"));
                }
                0
            }
            Ok(r) => fail_reply(ctx, &r),
            Err(e) => net(e),
        },
        _ => usage(ctx, &format!("usage: {kind} list|available|install <v>|use <v>|upgrade")),
    }
}

fn post_job(ctx: &Ctx, c: &Client, path: &str, body: Value, ok_msg: &str) -> i32 {
    let r = match c.post(path, &body) {
        Ok(r) => r,
        Err(e) => return fail(ctx, "network", &e),
    };
    if !r.ok() {
        return fail_reply(ctx, &r);
    }
    let id = r.json()["jobId"].as_str().unwrap_or("").to_string();
    match wait_job(ctx, c, &id) {
        Ok(j) => finish_job(ctx, &j, ok_msg),
        Err(e) => fail(ctx, "network", &e),
    }
}

fn cmd_backup(ctx: &Ctx, rest: &[&str]) -> i32 {
    let c = ctx.client();
    let daemon = c.alive();
    let data_root = ctx.paths.data_root(&ctx.cfg.data_dir);
    match rest {
        ["create"] => {
            let res: Result<Value, String> = if daemon {
                c.post("/api/backup/create", &json!({})).and_then(|r| if r.ok() { Ok(r.json()["jobId"].as_str().unwrap_or("").to_string()) } else { Err(r.error_message(ctx.en())) }).and_then(|id| wait_job(ctx, &c, &id)).and_then(|j| if j["state"] == "done" { Ok(j["result"]["backup"].clone()) } else { Err(j["error"]["message"].as_str().unwrap_or("failed").to_string()) })
            } else {
                std::fs::create_dir_all(&data_root).ok();
                backup::create(&ctx.paths, &data_root, "data").map(|b| serde_json::to_value(b).unwrap_or(Value::Null)).map_err(|e| e.to_string())
            };
            match res {
                Ok(b) => {
                    if ctx.json {
                        ctx.print(&b);
                    } else {
                        println!("{} {} ({:.2} MB)", ctx.t("Backup dibuat:", "Backup created:"), b["name"].as_str().unwrap_or("?"), b["sizeMB"].as_f64().unwrap_or(0.0));
                    }
                    0
                }
                Err(e) => fail(ctx, "backup", &e),
            }
        }
        ["list"] => {
            let list = serde_json::to_value(backup::list(&ctx.paths)).unwrap_or(Value::Null);
            if ctx.json {
                ctx.print(&json!({"backups": list}));
            } else if list.as_array().map(|a| a.is_empty()).unwrap_or(true) {
                println!("{}", ctx.t("Belum ada backup.", "No backups yet."));
            } else {
                for b in list.as_array().unwrap() {
                    println!("{:<40} {:>8.2} MB  {}  {}", b["name"].as_str().unwrap_or(""), b["sizeMB"].as_f64().unwrap_or(0.0), b["date"].as_str().unwrap_or(""), b["kind"].as_str().unwrap_or(""));
                }
            }
            0
        }
        ["restore", name] => {
            let res: Result<Value, String> = if daemon {
                c.post("/api/backup/restore", &json!({"name": name})).and_then(|r| if r.ok() { Ok(r.json()["jobId"].as_str().unwrap_or("").to_string()) } else { Err(r.error_message(ctx.en())) }).and_then(|id| wait_job(ctx, &c, &id)).and_then(|j| if j["state"] == "done" { Ok(j["result"]["result"].clone()) } else { Err(j["error"]["message"].as_str().unwrap_or("failed").to_string()) })
            } else {
                backup::restore(&ctx.paths, &data_root, name).map(|r| serde_json::to_value(r).unwrap_or(Value::Null)).map_err(|e| e.to_string())
            };
            match res {
                Ok(r) => {
                    if ctx.json {
                        ctx.print(&r);
                    } else {
                        println!("{} {name}  ({}: {})", ctx.t("Dipulihkan:", "Restored:"), ctx.t("salinan pengaman", "safety copy"), r["preRestore"].as_str().unwrap_or("-"));
                    }
                    0
                }
                Err(e) => fail(ctx, "backup", &e),
            }
        }
        _ => usage(ctx, "usage: backup create|list|restore <name>"),
    }
}

fn cmd_update_check(ctx: &Ctx) -> i32 {
    let c = match ensure_daemon(ctx) {
        Ok(c) => c,
        Err(e) => return fail(ctx, "daemon", &e),
    };
    match c.get("/api/update/check") {
        Ok(r) if r.ok() => {
            let v = r.json();
            if ctx.json {
                ctx.print(&v);
            } else if v["available"] == true {
                println!("{} {} -> {}", ctx.t("Pembaruan tersedia:", "Update available:"), v["current"].as_str().unwrap_or("?"), v["latest"].as_str().unwrap_or("?"));
            } else {
                println!("{} ({})", ctx.t("Sudah terbaru", "Up to date"), v["current"].as_str().unwrap_or("?"));
            }
            0
        }
        Ok(r) => fail_reply(ctx, &r),
        Err(e) => fail(ctx, "network", &e),
    }
}

fn cmd_update_apply(ctx: &Ctx) -> i32 {
    let c = match ensure_daemon(ctx) {
        Ok(c) => c,
        Err(e) => return fail(ctx, "daemon", &e),
    };
    post_job(ctx, &c, "/api/update/apply", json!({}), ctx.t("Pembaruan selesai.", "Update finished."))
}

fn cmd_doctor(ctx: &Ctx) -> i32 {
    let c = ctx.client();
    let v: Value = if c.alive() {
        match c.get("/api/diagnostics") {
            Ok(r) if r.ok() => r.json(),
            Ok(r) => return fail_reply(ctx, &r),
            Err(e) => return fail(ctx, "network", &e),
        }
    } else {
        let rt = crate::daemon::make_runtime_api(&ctx.paths, &ctx.cfg.min_node);
        let checks = diagnostics::run(&ctx.paths, &ctx.cfg, &rt, None);
        let fails = checks.iter().filter(|c| c.status == "fail").count();
        let warns = checks.iter().filter(|c| c.status == "warn").count();
        json!({"checks": checks, "summary": {"fail": fails, "warn": warns, "ok": checks.len() - fails - warns}})
    };
    if ctx.json {
        ctx.print(&v);
    } else {
        for ch in v["checks"].as_array().cloned().unwrap_or_default() {
            let mark = match ch["status"].as_str() {
                Some("pass") => "[ OK ]",
                Some("warn") => "[WARN]",
                _ => "[FAIL]",
            };
            println!("{mark} {} - {}", ch["title"].as_str().unwrap_or(""), ch["detail"].as_str().unwrap_or(""));
            if let Some(h) = ch["hint"].as_str() {
                println!("       {h}");
            }
            if let Some(u) = ch["hintUrl"].as_str() {
                println!("       -> {u}");
            }
        }
    }
    if v["summary"]["fail"].as_u64().unwrap_or(0) > 0 {
        1
    } else {
        0
    }
}

fn cmd_open(ctx: &Ctx, what: &str) -> i32 {
    let what = match what {
        "panel" => "panel",
        "app" => "app",
        "data" => "folder:data",
        "logs" => "folder:logs",
        "backups" => "folder:backups",
        _ => return usage(ctx, "usage: open panel|app|data|logs"),
    };
    let c = match ensure_daemon(ctx) {
        Ok(c) => c,
        Err(e) => return fail(ctx, "daemon", &e),
    };
    let r = if what == "panel" { c.post("/api/panel/open", &json!({})) } else { c.post("/api/open", &json!({"what": what})) };
    match r {
        Ok(r) if r.ok() => 0,
        Ok(r) => fail_reply(ctx, &r),
        Err(e) => fail(ctx, "network", &e),
    }
}

fn cmd_autostart(ctx: &Ctx, v: &str) -> i32 {
    match v {
        "status" => {
            let on = autostart::is_enabled();
            if ctx.json {
                ctx.print(&json!({"autostart": on}));
            } else {
                println!("{}", if on { "on" } else { "off" });
            }
            0
        }
        "on" | "off" => {
            let on = v == "on";
            let c = ctx.client();
            if c.alive() {
                match c.post("/api/config", &json!({"autoStartWithWindows": on})) {
                    Ok(r) if r.ok() => {}
                    Ok(r) => return fail_reply(ctx, &r),
                    Err(e) => return fail(ctx, "network", &e),
                }
            } else {
                let mut cfg = ctx.cfg.clone();
                cfg.auto_start_with_windows = on;
                let _ = config::save(&ctx.paths, &cfg);
                autostart::set(on);
            }
            let now = autostart::is_enabled();
            if ctx.json {
                ctx.print(&json!({"autostart": now}));
            } else {
                println!("{}: {}", ctx.t("Jalankan saat Windows start", "Run when Windows starts"), if now { "on" } else { "off" });
            }
            if now == on {
                0
            } else {
                1
            }
        }
        _ => usage(ctx, "usage: autostart on|off|status"),
    }
}

