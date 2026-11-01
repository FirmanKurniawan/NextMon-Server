//! Application core: shared state and the orchestration used by the HTTP API, CLI and tray.
use crate::config::{self, Config, Edition, SharedConfig};
use crate::jobs::{JobHandle, Jobs};
use crate::launcher::{AppLauncher, LaunchError, Launcher};
use crate::paths::Paths;
use crate::runtime_api::{InstalledRt, RtError, RuntimeApi};
use crate::supervisor::{AppState, StartOutcome, Supervisor, SwitchError};
use crate::{api, autostart, backup, diagnostics, update, util};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const HOMEPAGE: &str = "https://github.com/FirmanKurniawan/NextMon";

#[derive(Debug, Clone)]
pub struct ApiErr {
    pub status: u16,
    pub code: String,
    pub id: String,
    pub en: String,
    pub extra: Option<Value>,
}

impl ApiErr {
    pub fn new(status: u16, code: &str, id: impl Into<String>, en: impl Into<String>) -> ApiErr {
        ApiErr { status, code: code.into(), id: id.into(), en: en.into(), extra: None }
    }
    pub fn with(mut self, extra: Value) -> ApiErr {
        self.extra = Some(extra);
        self
    }
    pub fn msg(&self, lang: &str) -> &str {
        if lang == "en" {
            &self.en
        } else {
            &self.id
        }
    }
}

impl From<LaunchError> for ApiErr {
    fn from(e: LaunchError) -> ApiErr {
        let status = match e.code.as_str() {
            "runtime_missing" | "port_in_use" | "app_missing" | "node_too_old" => 409,
            _ => 500,
        };
        let mut a = ApiErr::new(status, &e.code, e.id, e.en);
        if let Some(k) = e.runtime_kind {
            a = a.with(json!({"runtime": {"kind": k, "installable": true}}));
        }
        a
    }
}

/// Error codes the panel knows (see ui/): normalise what the runtime manager reports.
fn ui_code(code: &str) -> String {
    match code {
        "checksum_mismatch" => "sha256_mismatch",
        "in_use" => "runtime_in_use",
        other => other,
    }
    .to_string()
}

impl From<RtError> for ApiErr {
    fn from(e: RtError) -> ApiErr {
        let status = match e.code.as_str() {
            "not_found" => 404,
            "in_use" | "busy" | "already_installed" => 409,
            "network" => 502,
            "unavailable" => 503,
            "bad_kind" | "no_checksum" | "checksum_mismatch" | "unsupported" => 422,
            _ => 500,
        };
        ApiErr::new(status, &ui_code(&e.code), e.message_id, e.message)
    }
}

impl From<backup::BackupError> for ApiErr {
    fn from(e: backup::BackupError) -> ApiErr {
        let status = match e.code {
            "bad_name" | "unsafe_archive" | "bad_archive" => 422,
            "not_found" => 404,
            "in_use" => 409,
            _ => 500,
        };
        let code = if e.code == "bad_archive" { "backup_corrupt" } else { e.code };
        ApiErr::new(status, code, e.message.clone(), e.message)
    }
}

impl From<config::ConfigError> for ApiErr {
    fn from(e: config::ConfigError) -> ApiErr {
        let code = if e.field.ends_with(".port") { "invalid_port" } else { "invalid_config" };
        ApiErr::new(422, code, format!("Konfigurasi tidak valid - {e}"), format!("Invalid configuration - {e}")).with(json!({"field": e.field}))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiCmd {
    ShowPanel,
    Refresh,
    Quit,
}

pub type UiHook = Arc<dyn Fn(UiCmd) + Send + Sync>;

pub struct Core {
    pub paths: Paths,
    pub cfg: SharedConfig,
    pub sup: Supervisor,
    pub jobs: Jobs,
    pub rt: Arc<dyn RuntimeApi>,
    pub launcher: Arc<AppLauncher>,
    pub http: reqwest::Client,
    pub shutdown: tokio::sync::Notify,
    pub ui: Mutex<Option<UiHook>>,
    pub update_cache: Mutex<Option<Value>>,
    cfg_lock: Mutex<()>,
}

/// Config as exposed to the panel: never contains the token.
pub fn public_config(c: &Config) -> Value {
    let mut v = serde_json::to_value(c).unwrap_or(Value::Null);
    if let Some(o) = v.as_object_mut() {
        o.remove("controlToken");
    }
    v
}

impl Core {
    pub fn new(paths: Paths, config: Config, rt: Arc<dyn RuntimeApi>) -> Arc<Core> {
        api::LANG_EN.store(config.language == "en", std::sync::atomic::Ordering::Relaxed);
        let cfg = config::shared(config);
        let launcher = Arc::new(AppLauncher { paths: paths.clone(), cfg: cfg.clone(), rt: rt.clone() });
        let sup = Supervisor::new(paths.clone(), cfg.clone(), launcher.clone() as Arc<dyn Launcher>);
        Arc::new(Core {
            paths,
            cfg,
            sup,
            jobs: Jobs::new(),
            rt,
            launcher,
            http: update::http_client().unwrap_or_default(),
            shutdown: tokio::sync::Notify::new(),
            ui: Mutex::new(None),
            update_cache: Mutex::new(None),
            cfg_lock: Mutex::new(()),
        })
    }

    pub fn config(&self) -> Config {
        self.cfg.read().unwrap().clone()
    }

    pub fn lang(&self) -> String {
        self.cfg.read().unwrap().language.clone()
    }

    /// Pick the Indonesian or English text by the configured language.
    pub fn tr(&self, id: &str, en: &str) -> String {
        if self.lang() == "en" {
            en.to_string()
        } else {
            id.to_string()
        }
    }

    fn step(&self, j: &JobHandle, p: f64, id: &str, en: &str) {
        j.set(p, &self.tr(id, en), "");
    }

    pub fn ui_cmd(&self, c: UiCmd) {
        if let Some(h) = self.ui.lock().unwrap().clone() {
            h(c);
        }
    }

    pub fn data_root(&self) -> std::path::PathBuf {
        self.paths.data_root(&self.cfg.read().unwrap().data_dir)
    }

    /// Startup housekeeping + autostart of the active edition.
    pub async fn boot(self: &Arc<Self>) {
        let n = Supervisor::cleanup_orphans(&self.paths);
        if n > 0 {
            log::warn!("cleaned {n} orphaned process(es)");
        }
        let c = self.rt.cleanup_stale();
        if c > 0 {
            log::info!("removed {c} stale runtime folder(s)");
        }
        let (days, auto, ed, check) = {
            let c = self.cfg.read().unwrap();
            (c.log_retention_days, c.auto_start_edition, c.edition, c.update.check_on_start)
        };
        crate::logger::cleanup(&self.paths, days);
        if auto {
            match self.sup.start(ed).await {
                Ok(_) => {}
                Err(e) => log::error!("autostart of {} failed: {}", ed.as_str(), e.en),
            }
        }
        if check {
            let me = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(5)).await;
                match me.update_check().await {
                    Ok(v) if v["available"].as_bool() == Some(true) => log::info!("application update available: {}", v["latest"]),
                    Ok(_) => {}
                    Err(e) => log::info!("update check skipped: {}", e.en),
                }
            });
        }
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
                let d = me.cfg.read().unwrap().log_retention_days;
                crate::logger::cleanup(&me.paths, d);
            }
        });
    }

    // ------------------------------------------------------------------ status

    pub fn active_snapshot(&self) -> crate::supervisor::Snapshot {
        let mut s = self.sup.snapshot();
        let active = self.cfg.read().unwrap().edition;
        if s.edition != Some(active) {
            // old edition's leftovers (e.g. failed state) are not the active app's state
            s.state = AppState::Stopped;
            s.pid = None;
            s.uptime_secs = 0;
            s.restarts = 0;
            s.last_error = None;
            s.retry_in_secs = None;
            s.edition = Some(active);
        }
        s
    }

    /// Resolved runtime version for an edition (what is installed and would be used), else the configured string.
    fn runtime_version(&self, ed: Edition) -> (String, bool) {
        match self.launcher.resolve_runtime(ed) {
            Ok((_, v)) => (v, true),
            Err(_) => (self.cfg.read().unwrap().app(ed).runtime.clone(), false),
        }
    }

    pub fn status_json(&self) -> Value {
        let cfg = self.config();
        let s = self.active_snapshot();
        let port = cfg.app(cfg.edition).port;
        let mem = if s.state == AppState::Running { s.pid.map(crate::winproc::working_set_mb) } else { None };
        let (runtime, _) = self.runtime_version(cfg.edition);
        let lan = if cfg.bind == "0.0.0.0" { crate::net::lan_ip().map(|ip| format!("http://{ip}:{port}")) } else { None };
        let mut editions = serde_json::Map::new();
        for ed in Edition::ALL {
            let (rt_v, rt_ok) = self.runtime_version(ed);
            let marker = match ed {
                Edition::Php => self.paths.app_dir(ed).join("public").join("index.php"),
                Edition::Next => self.paths.app_dir(ed).join("server.js"),
            };
            editions.insert(
                ed.as_str().into(),
                json!({
                    "port": cfg.app(ed).port, "runtime": rt_v, "runtimeKind": ed.runtime_kind(),
                    "installed": rt_ok, "appPresent": marker.is_file(), "label": ed.label(),
                }),
            );
        }
        json!({
            "edition": cfg.edition.as_str(),
            "app": {
                "state": s.state, "pid": s.pid, "port": port, "url": cfg.app_url(cfg.edition), "lanUrl": lan,
                "uptime": s.uptime_secs, "runtime": runtime, "restarts": s.restarts, "ramMB": mem,
                "lastError": s.last_error, "retryInSecs": s.retry_in_secs, "bind": cfg.bind,
            },
            "editions": editions,
            "version": VERSION,
            "appVersion": update::current_version(&self.paths),
            "config": public_config(&cfg),
            "system": {"os": os_label(), "arch": std::env::consts::ARCH, "ramMB": util::ram_mb(), "diskFreeGB": util::disk_free_gb(&self.paths.base)},
            "ports": {"control": cfg.control_port, "php": cfg.apps.php.port, "next": cfg.apps.next.port},
            "paths": {"root": self.paths.base, "data": self.data_root(), "logs": self.paths.logs(), "backups": self.paths.backups()},
            "about": {"license": "MIT", "commit": option_env!("NEXTMON_COMMIT").unwrap_or(""), "buildDate": option_env!("NEXTMON_BUILD_DATE").unwrap_or(""), "homepage": HOMEPAGE},
            "autostart": autostart::is_enabled(),
            "updateAvailable": self.update_cache.lock().unwrap().as_ref().and_then(|v| v["available"].as_bool()).unwrap_or(false),
        })
    }

    // ------------------------------------------------------------------ app control (always the active edition)

    pub async fn app_start(&self) -> Result<StartOutcome, ApiErr> {
        let ed = self.config().edition;
        Ok(self.sup.start(ed).await?)
    }

    pub async fn app_stop(&self) {
        self.sup.stop().await;
    }

    pub async fn app_restart(&self) -> Result<(), ApiErr> {
        let ed = self.config().edition;
        Ok(self.sup.restart(ed).await?)
    }

    async fn restart_if_running(&self, ed: Edition) -> bool {
        let s = self.sup.snapshot();
        if s.edition == Some(ed) && self.sup.is_active() {
            let _ = self.sup.restart(ed).await;
            true
        } else {
            false
        }
    }

    /// Switch edition (one at a time): validates, stops the old edition, persists, optionally starts the new one. Runs as a job.
    pub fn switch_edition(self: &Arc<Self>, ed: Edition, start: bool) -> Result<JobHandle, ApiErr> {
        // refuse early (runtime/app missing) without touching the running edition
        self.launcher.resolve_runtime(ed)?;
        if start {
            self.launcher.spec(ed)?;
        }
        let job = self.jobs.create("edition");
        let me = self.clone();
        let j = job.clone();
        tokio::spawn(async move {
            if me.config().edition == ed {
                // already the active edition: only (re)start when asked
                let r = if start { me.sup.start(ed).await.map(|_| ()) } else { Ok(()) };
                match r {
                    Ok(()) => j.done(json!({"edition": ed.as_str(), "started": start, "changed": false})),
                    Err(e) => j.fail(&e.code, &e.en, None),
                }
                me.ui_cmd(UiCmd::Refresh);
                return;
            }
            me.step(&j, 0.1, "Menghentikan edisi lama", "Stopping the current edition");
            let paths = me.paths.clone();
            let cfg = me.cfg.clone();
            let lock_me = me.clone();
            let persist = move || -> Result<(), String> {
                let _g = lock_me.cfg_lock.lock().unwrap();
                let mut c = cfg.read().unwrap().clone();
                c.edition = ed;
                config::save(&paths, &c)?;
                *cfg.write().unwrap() = c;
                Ok(())
            };
            match me.sup.switch(ed, start, &persist).await {
                Ok(()) => {
                    if start {
                        me.step(&j, 0.7, "Menjalankan edisi baru", "Starting the new edition");
                        let st = me.sup.wait_settled(Duration::from_secs(45)).await;
                        if st == AppState::Failed {
                            let e = me.sup.snapshot().last_error.unwrap_or_default();
                            j.fail("start_failed", &format!("{} could not start: {e}", ed.label()), Some(json!({"edition": ed.as_str()})));
                            me.ui_cmd(UiCmd::Refresh);
                            return;
                        }
                    }
                    log::info!("active edition is now {}", ed.as_str());
                    j.done(json!({"edition": ed.as_str(), "started": start}));
                }
                Err(SwitchError::Launch(e)) => j.fail(&e.code, if me.lang() == "en" { &e.en } else { &e.id }, None),
                Err(SwitchError::Persist(m)) => j.fail("config_save_failed", &m, None),
            }
            me.ui_cmd(UiCmd::Refresh);
        });
        Ok(job)
    }

    // ------------------------------------------------------------------ config

    /// Panel/CLI settings update: partial, validated, atomically saved. `apps`, `edition`, `controlToken`, `controlPort`
    /// and `version` are not settable here (use /api/app/config, /api/edition, /api/config/token/regenerate).
    pub async fn config_patch(&self, patch: Value) -> Result<Value, ApiErr> {
        let mut patch = patch;
        if let Some(o) = patch.as_object_mut() {
            for k in ["apps", "edition", "controlToken", "controlPort", "version"] {
                o.remove(k);
            }
        }
        let (new, restart_required) = self.apply_config(patch).await?;
        let mut v = public_config(&new);
        if restart_required {
            v["restartRequired"] = json!(true);
        }
        Ok(v)
    }

    /// Internal: apply any validated patch (including apps.*), restart the app when its settings changed.
    async fn apply_config(&self, patch: Value) -> Result<(Config, bool), ApiErr> {
        let cur = self.config();
        let new = {
            let _g = self.cfg_lock.lock().unwrap();
            let new = config::apply_patch(&cur, &patch)?;
            self.check_ports(&cur, &new)?;
            config::save(&self.paths, &new).map_err(|e| ApiErr::new(500, "config_save_failed", e.clone(), e))?;
            *self.cfg.write().unwrap() = new.clone();
            new
        };
        api::LANG_EN.store(new.language == "en", std::sync::atomic::Ordering::Relaxed);
        let restart_required = new.control_port != cur.control_port || new.control_token != cur.control_token;
        if new.auto_start_with_windows != cur.auto_start_with_windows {
            autostart::set(new.auto_start_with_windows);
        }
        if new.bind != cur.bind || new.apps.php != cur.apps.php || new.apps.next != cur.apps.next {
            self.restart_if_running(new.edition).await;
        }
        self.ui_cmd(UiCmd::Refresh);
        Ok((new, restart_required))
    }

    /// Owner description of a port that is not free ("name.exe (PID n)").
    fn port_user(port: u16) -> String {
        crate::winproc::port_owner(port)
            .map(|p| format!("{} (PID {p})", crate::winproc::process_name(p).unwrap_or_else(|| "?".into())))
            .unwrap_or_else(|| "?".into())
    }

    fn port_in_use_err(port: u16) -> ApiErr {
        let by = Self::port_user(port);
        ApiErr::new(409, "port_in_use", format!("Port {port} dipakai oleh {by}."), format!("Port {port} is used by {by}.")).with(json!({"port": port, "usedBy": by}))
    }

    /// Port-free validation for ports that changed (a port held by our own running app is fine).
    fn check_ports(&self, old: &Config, new: &Config) -> Result<(), ApiErr> {
        let snap = self.sup.snapshot();
        let running_port = if self.sup.is_active() { snap.port } else { None };
        for ed in Edition::ALL {
            let (o, n) = (old.app(ed), new.app(ed));
            if n.port != o.port || new.bind != old.bind {
                if Some(n.port) == running_port {
                    // our own running app already holds it (or the other edition's port, which never runs together)
                    continue;
                }
                if !crate::net::port_free(&new.bind, n.port) {
                    return Err(Self::port_in_use_err(n.port));
                }
            }
            if n.runtime != o.runtime && !n.runtime.is_empty() {
                let kind = ed.runtime_kind();
                if !self.rt.list_installed(kind).iter().any(|i| i.dir_name == n.runtime || i.version == n.runtime) {
                    return Err(ApiErr::new(409, "runtime_not_installed", format!("Runtime {kind} {} belum terpasang.", n.runtime), format!("{kind} runtime {} is not installed.", n.runtime))
                        .with(json!({"runtime": {"kind": kind, "version": n.runtime, "installable": true}})));
                }
            }
        }
        if new.control_port != old.control_port && !crate::net::port_free("127.0.0.1", new.control_port) {
            return Err(Self::port_in_use_err(new.control_port));
        }
        Ok(())
    }

    /// Flat body `{port?, runtime?, opcache?, extraIni?, env?}` for one edition; returns that edition's app config.
    pub async fn app_config_patch(&self, ed: Option<Edition>, patch: Value) -> Result<Value, ApiErr> {
        let ed = ed.unwrap_or_else(|| self.config().edition);
        let Some(obj) = patch.as_object() else {
            return Err(ApiErr::new(422, "bad_request", "Body harus berupa objek JSON", "Body must be a JSON object"));
        };
        let allowed = ["port", "runtime", "opcache", "extraIni", "env"];
        let mut clean = serde_json::Map::new();
        for (k, v) in obj {
            if !allowed.contains(&k.as_str()) {
                continue; // unknown keys (and `edition`, handled by the caller) are ignored, like the panel's mock does
            }
            if k == "runtime" && v.as_str() == Some("") {
                continue; // empty = unchanged
            }
            if ed == Edition::Next && (k == "opcache" || k == "extraIni") {
                continue; // PHP-only options
            }
            if k == "port" && !v.as_u64().map(|p| (1..=65535).contains(&p)).unwrap_or(false) {
                return Err(ApiErr::new(422, "invalid_port", "Port harus antara 1 dan 65535.", "Port must be between 1 and 65535."));
            }
            clean.insert(k.clone(), v.clone());
        }
        let body = json!({"apps": { ed.as_str(): Value::Object(clean) }});
        let (new, _) = self.apply_config(body).await?;
        Ok(serde_json::to_value(new.app(ed)).unwrap_or(Value::Null))
    }

    /// `GET /api/ports/check`: is `port` usable for `edition`?
    pub fn ports_check(&self, port: u16, edition: Option<Edition>) -> Value {
        let cfg = self.config();
        let ed = edition.unwrap_or(cfg.edition);
        let snap = self.sup.snapshot();
        let running = self.sup.is_active().then_some(()).and(snap.port.zip(snap.edition));
        if let Some((rp, re)) = running {
            if rp == port {
                // Our own app holds it: fine for that edition, conflicting only for the "other" one that could never run alongside... which is allowed too.
                if re == ed {
                    return json!({"free": true});
                }
                return json!({"free": true, "note": "shared with the other edition (they never run together)"});
            }
        }
        if port == cfg.control_port {
            return json!({"free": false, "usedBy": "NextMON Server (panel)"});
        }
        if crate::net::port_free(&cfg.bind, port) {
            json!({"free": true})
        } else {
            json!({"free": false, "usedBy": Self::port_user(port)})
        }
    }

    pub fn token_regenerate(&self) -> Result<String, ApiErr> {
        let _g = self.cfg_lock.lock().unwrap();
        let mut c = self.config();
        c.control_token = util::random_token();
        config::save(&self.paths, &c).map_err(|e| ApiErr::new(500, "config_save_failed", e.clone(), e))?;
        let t = c.control_token.clone();
        *self.cfg.write().unwrap() = c;
        log::info!("control token regenerated");
        Ok(t)
    }

    // ------------------------------------------------------------------ runtimes

    fn used_by(&self, kind: &str, rt: &InstalledRt) -> Vec<&'static str> {
        let cfg = self.config();
        let mut v = vec![];
        for ed in Edition::ALL {
            if ed.runtime_kind() != kind {
                continue;
            }
            let want = &cfg.app(ed).runtime;
            let uses = if want.is_empty() {
                self.launcher.resolve_runtime(ed).map(|(p, _)| p == std::path::absolute(&rt.path).unwrap_or(rt.path.clone())).unwrap_or(false)
            } else {
                &rt.dir_name == want || &rt.version == want
            };
            if uses {
                v.push(ed.as_str());
            }
        }
        v
    }

    pub async fn runtime_overview(&self, kind: &str, force: bool, variant: Option<&str>, channel: Option<&str>) -> Result<Value, ApiErr> {
        let cfg = self.config();
        let channel = channel.map(String::from).unwrap_or_else(|| if kind == "node" { cfg.update.channel_node.clone() } else { cfg.update.channel_php.clone() });
        let mut v = self.rt.overview(kind, &channel, variant.unwrap_or("nts"), force).await?;
        let installed = self.rt.list_installed(kind);
        if let Some(arr) = v["installed"].as_array_mut() {
            for item in arr.iter_mut() {
                let path = item["path"].as_str().unwrap_or("").to_string();
                let found = installed.iter().find(|i| i.path.to_string_lossy() == path);
                if let Some(i) = found {
                    let used = self.used_by(kind, i);
                    item["dirName"] = json!(i.dir_name);
                    item["active"] = json!(!used.is_empty());
                    item["usedBy"] = json!(used);
                }
            }
        }
        Ok(v)
    }

    pub fn runtime_install(self: &Arc<Self>, kind: &str, version: &str, variant: Option<String>) -> Result<JobHandle, ApiErr> {
        if kind != "php" && kind != "node" {
            return Err(ApiErr::new(404, "not_found", "Jenis runtime tidak dikenal", "Unknown runtime kind"));
        }
        if version.trim().is_empty() || version.len() > 40 {
            return Err(ApiErr::new(422, "bad_request", "Versi wajib diisi", "version is required"));
        }
        if self.jobs.is_running(&format!("runtime-install-{kind}")) {
            return Err(ApiErr::new(409, "busy", format!("Pemasangan {kind} lain masih berjalan"), format!("Another {kind} install is still running")));
        }
        let job = self.jobs.create(&format!("runtime-install-{kind}"));
        let (me, j, kind, version) = (self.clone(), job.clone(), kind.to_string(), version.to_string());
        tokio::spawn(async move {
            let jj = j.clone();
            let cb: crate::runtime_api::ProgressCb = Arc::new(move |p, msg| jj.set(p, &msg, ""));
            match me.rt.install(&kind, &version, variant, cb).await {
                Ok(r) => {
                    log::info!("installed {kind} {}", r.version);
                    j.done(json!({"kind": kind, "version": r.version, "dirName": r.dir_name}));
                }
                Err(e) => {
                    log::warn!("install {kind} {version} failed: {}", e.message);
                    let msg = if me.lang() == "en" { e.message.clone() } else { e.message_id.clone() };
                    let code = if e.code == "not_found" { "version_not_found".to_string() } else { ui_code(&e.code) };
                    j.fail(&code, &msg, None);
                }
            }
            me.ui_cmd(UiCmd::Refresh);
        });
        Ok(job)
    }

    fn find_installed(&self, kind: &str, version: &str) -> Option<InstalledRt> {
        let list = self.rt.list_installed(kind);
        // an exact folder name wins; a bare version prefers the NTS build
        list.iter().find(|i| i.dir_name == version).or_else(|| list.iter().find(|i| i.version == version && i.tsnts != "ts")).or_else(|| list.iter().find(|i| i.version == version)).cloned()
    }

    fn edition_for(kind: &str, app: Option<&str>) -> Result<Edition, ApiErr> {
        let default = if kind == "node" { Edition::Next } else { Edition::Php };
        let ed = match app {
            Some(a) => Edition::parse(a).ok_or_else(|| ApiErr::new(422, "invalid_app", "app tidak dikenal", "unknown app"))?,
            None => default,
        };
        if ed.runtime_kind() != kind {
            return Err(ApiErr::new(422, "invalid_app", "Runtime tidak cocok dengan edisi.", "Runtime does not match the edition."));
        }
        Ok(ed)
    }

    pub async fn runtime_activate(&self, kind: &str, version: &str, app: Option<&str>) -> Result<Value, ApiErr> {
        let ed = Self::edition_for(kind, app)?;
        let inst = self
            .find_installed(kind, version)
            .ok_or_else(|| ApiErr::new(409, "runtime_not_installed", format!("Runtime {version} belum terpasang."), "Runtime is not installed."))?;
        let before = self.config();
        let was = self.sup.is_active() && before.edition == ed;
        self.app_config_patch(Some(ed), json!({"runtime": inst.dir_name})).await?;
        Ok(json!({"ok": true, "edition": ed.as_str(), "runtime": inst.version, "restarted": was}))
    }

    pub fn runtime_uninstall(&self, kind: &str, version: &str) -> Result<(), ApiErr> {
        let inst = self
            .find_installed(kind, version)
            .ok_or_else(|| ApiErr::new(404, "not_found", "Runtime tidak ditemukan.", "Runtime not found."))?;
        let cfg = self.config();
        let snap = self.sup.snapshot();
        let running_here = self.sup.is_active() && snap.edition.map(|e| e.runtime_kind() == kind).unwrap_or(false) && !self.used_by(kind, &inst).is_empty();
        let configured = Edition::ALL.iter().any(|e| e.runtime_kind() == kind && (cfg.app(*e).runtime == inst.dir_name || cfg.app(*e).runtime == inst.version));
        if running_here || configured {
            return Err(ApiErr::new(
                409,
                "runtime_in_use",
                format!("{kind} {} sedang dipakai; pilih versi lain dulu (Pakai) lalu hapus.", inst.version),
                format!("{kind} {} is in use; activate another version first, then remove it.", inst.version),
            ));
        }
        self.rt.uninstall(kind, &inst.dir_name, &|_| false)?;
        Ok(())
    }

    /// Install the newest version of the channel (or `to`), activate it for the app, roll back when the app does not come up.
    pub fn runtime_upgrade(self: &Arc<Self>, kind: &str, to: Option<String>, app: Option<String>) -> Result<JobHandle, ApiErr> {
        let ed = Self::edition_for(kind, app.as_deref())?;
        if self.jobs.is_running(&format!("runtime-upgrade-{kind}")) {
            return Err(ApiErr::new(409, "busy", "Upgrade lain masih berjalan", "Another upgrade is still running"));
        }
        let job = self.jobs.create(&format!("runtime-upgrade-{kind}"));
        let (me, j, kind) = (self.clone(), job.clone(), kind.to_string());
        tokio::spawn(async move {
            let res = me.do_runtime_upgrade(&j, &kind, ed, to).await;
            match res {
                Ok(v) => j.done(v),
                Err(e) => {
                    let m = if me.lang() == "en" { e.en.clone() } else { e.id.clone() };
                    j.fail(&e.code, &m, e.extra.clone());
                }
            }
            me.ui_cmd(UiCmd::Refresh);
        });
        Ok(job)
    }

    async fn do_runtime_upgrade(&self, j: &JobHandle, kind: &str, ed: Edition, to: Option<String>) -> Result<Value, ApiErr> {
        let cfg = self.config();
        self.step(j, 0.02, "Memeriksa versi", "Checking versions");
        let prev_cfg_runtime = cfg.app(ed).runtime.clone();
        let prev = self.launcher.resolve_runtime(ed).ok().map(|(_, v)| v);
        let target = match &to {
            Some(t) => t.clone(),
            None => {
                let ch = if kind == "node" { cfg.update.channel_node.clone() } else { cfg.update.channel_php.clone() };
                let latest = self.rt.resolve_latest(kind, &ch).await?;
                let Some(l) = latest else { return Ok(json!({"upgraded": false, "reason": "no_release"})) };
                let v = l["version"].as_str().unwrap_or("").to_string();
                if let Some(p) = &prev {
                    if util::cmp_version(&v, p).is_le() {
                        return Ok(json!({"upgraded": false, "reason": "up_to_date", "current": p, "latest": v}));
                    }
                }
                v
            }
        };
        let inst = if let Some(i) = self.find_installed(kind, &target) {
            i
        } else {
            let jj = j.clone();
            let cb: crate::runtime_api::ProgressCb = Arc::new(move |p, m| jj.set(0.05 + p * 0.65, &m, ""));
            self.rt.install(kind, &target, None, cb).await?
        };
        self.step(j, 0.72, "Health-check", "Health check");
        let health = self.rt.health_check(kind, &inst.path).await;
        if health["ok"].as_bool() == Some(false) {
            let p = prev.clone().unwrap_or_default();
            return Err(ApiErr::new(
                422,
                "health_check_failed",
                format!("Health-check gagal; tetap memakai {p} (rollback)."),
                format!("Health check failed; staying on {p} (rolled back)."),
            )
            .with(json!({"health": health, "rolledBack": true})));
        }
        self.step(j, 0.78, "Backup data", "Backing up data");
        let (paths, root) = (self.paths.clone(), self.data_root());
        if root.is_dir() {
            let _ = tokio::task::spawn_blocking(move || backup::create(&paths, &root, "pre-runtime")).await;
        }
        self.step(j, 0.85, "Mengganti versi", "Switching version");
        self.app_config_patch(Some(ed), json!({"runtime": inst.dir_name})).await?;
        let running = self.config().edition == ed && self.sup.is_active();
        if running {
            self.step(j, 0.9, "Memastikan aplikasi berjalan", "Verifying the app starts");
            let ok = self.wait_stable().await;
            if !ok {
                let err = self.sup.snapshot().last_error.unwrap_or_default();
                log::error!("runtime upgrade to {kind} {} failed ({err}); rolling back", inst.version);
                let _ = self.app_config_patch(Some(ed), json!({"runtime": prev_cfg_runtime})).await;
                let _ = self.sup.restart(ed).await;
                let p = prev.clone().unwrap_or_default();
                return Err(ApiErr::new(
                    422,
                    "health_check_failed",
                    format!("Health-check gagal; dipulihkan ke {p} (rollback)."),
                    format!("Health check failed; rolled back to {p}."),
                )
                .with(json!({"rolledBack": true, "previous": prev})));
            }
        }
        Ok(json!({"upgraded": true, "from": prev, "to": inst.version, "dirName": inst.dir_name, "edition": ed.as_str()}))
    }

    /// Running and still running 3 seconds later.
    async fn wait_stable(&self) -> bool {
        let st = self.sup.wait_settled(Duration::from_secs(45)).await;
        if st != AppState::Running {
            return false;
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
        self.sup.snapshot().state == AppState::Running
    }

    // ------------------------------------------------------------------ backup

    pub async fn backup_create(&self) -> Result<backup::BackupInfo, ApiErr> {
        let (paths, root) = (self.paths.clone(), self.data_root());
        std::fs::create_dir_all(&root).ok();
        let r = tokio::task::spawn_blocking(move || backup::create(&paths, &root, "data")).await.map_err(|e| ApiErr::new(500, "internal", e.to_string(), e.to_string()))??;
        log::info!("backup created: {}", r.name);
        Ok(r)
    }

    pub fn backup_create_job(self: &Arc<Self>) -> JobHandle {
        let job = self.jobs.create("backup-create");
        let (me, j) = (self.clone(), job.clone());
        tokio::spawn(async move {
            me.step(&j, 0.1, "Menghitung berkas", "Counting files");
            me.step(&j, 0.3, "Mengompres data", "Compressing data");
            match me.backup_create().await {
                Ok(b) => j.done(json!({"backup": b})),
                Err(e) => j.fail(&e.code, me.msg_of(&e), None),
            }
            me.ui_cmd(UiCmd::Refresh);
        });
        job
    }

    fn msg_of<'a>(&self, e: &'a ApiErr) -> &'a str {
        e.msg(&self.lang())
    }

    pub async fn backup_restore(&self, name: &str) -> Result<backup::RestoreResult, ApiErr> {
        // refuse a bad name/archive before stopping anything
        let zip = backup::resolve(&self.paths, name)?;
        backup::validate_zip(&zip)?;
        let was_active = self.sup.is_active();
        let ed = self.config().edition;
        if was_active {
            self.sup.stop().await;
        }
        let (paths, root, n) = (self.paths.clone(), self.data_root(), name.to_string());
        let res = tokio::task::spawn_blocking(move || backup::restore(&paths, &root, &n)).await.map_err(|e| ApiErr::new(500, "internal", e.to_string(), e.to_string()));
        if was_active {
            if let Err(e) = self.sup.start(ed).await {
                log::error!("could not restart after restore: {}", e.en);
            }
        }
        let r = res??;
        log::info!("backup restored: {name} (safety copy: {:?})", r.pre_restore);
        Ok(r)
    }

    /// Restore as a job. Unknown names are rejected up front (404/422); a damaged archive fails the job with `backup_corrupt`.
    pub fn backup_restore_job(self: &Arc<Self>, name: &str) -> Result<JobHandle, ApiErr> {
        backup::resolve(&self.paths, name)?;
        let job = self.jobs.create("backup-restore");
        let (me, j, name) = (self.clone(), job.clone(), name.to_string());
        tokio::spawn(async move {
            me.step(&j, 0.1, "Memeriksa arsip", "Checking the archive");
            me.step(&j, 0.4, "Memulihkan data", "Restoring data");
            match me.backup_restore(&name).await {
                Ok(r) => j.done(json!({"result": r})),
                Err(e) => j.fail(&e.code, me.msg_of(&e), None),
            }
            me.ui_cmd(UiCmd::Refresh);
        });
        Ok(job)
    }

    // ------------------------------------------------------------------ updates

    /// Release notes as bullet strings.
    fn notes_list(body: &str) -> Vec<String> {
        body.lines()
            .map(|l| l.trim().trim_start_matches(['-', '*', '+']).trim().trim_start_matches('#').trim().to_string())
            .filter(|l| !l.is_empty())
            .take(12)
            .collect()
    }

    pub async fn update_check(&self) -> Result<Value, ApiErr> {
        let cfg = self.config();
        let current = update::current_version(&self.paths);
        let rel = update::fetch_latest(&self.http, &cfg.update.app_repo, &cfg.update.app_channel).await.map_err(|e| {
            let status = match e.code {
                "not_configured" => 422,
                "not_found" => 404,
                _ => 502,
            };
            ApiErr::new(status, &format!("update_{}", e.code), e.message.clone(), e.message)
        })?;
        let v = match rel {
            Some(r) => {
                let newer = util::cmp_version(&r.version, &current).is_gt();
                json!({
                    "current": current, "latest": r.version, "available": newer, "date": r.published_at, "url": r.url,
                    "notes": if newer { Self::notes_list(&r.notes) } else { vec![] }, "channel": cfg.update.app_channel,
                    "installable": r.asset_url.is_some() && r.sums_url.is_some(), "asset": r.asset_name, "prerelease": r.prerelease,
                })
            }
            None => json!({"current": current, "latest": current, "available": false, "date": null, "url": null, "notes": [], "channel": cfg.update.app_channel, "installable": false}),
        };
        *self.update_cache.lock().unwrap() = Some(v.clone());
        Ok(v)
    }

    pub fn update_apply(self: &Arc<Self>) -> Result<JobHandle, ApiErr> {
        if self.jobs.is_running("app-update") {
            return Err(ApiErr::new(409, "busy", "Pembaruan sedang berjalan", "An update is already running"));
        }
        let job = self.jobs.create("app-update");
        let (me, j) = (self.clone(), job.clone());
        tokio::spawn(async move {
            match me.do_update(&j).await {
                Ok(v) => j.done(v),
                Err(e) => {
                    let m = if me.lang() == "en" { e.en.clone() } else { e.id.clone() };
                    log::error!("application update failed: {}", e.en);
                    j.fail(&e.code, &m, e.extra.clone());
                }
            }
            me.ui_cmd(UiCmd::Refresh);
        });
        Ok(job)
    }

    async fn do_update(&self, j: &JobHandle) -> Result<Value, ApiErr> {
        let uerr = |e: update::UpdateError| ApiErr::new(502, &format!("update_{}", e.code), e.message.clone(), e.message);
        self.step(j, 0.02, "Memeriksa pembaruan", "Checking for updates");
        // Fetch the release directly (the cached check carries no asset URLs).
        let cfg = self.config();
        let current = update::current_version(&self.paths);
        let rel = update::fetch_latest(&self.http, &cfg.update.app_repo, &cfg.update.app_channel).await.map_err(uerr)?;
        let Some(rel) = rel.filter(|r| util::cmp_version(&r.version, &current).is_gt()) else {
            return Ok(json!({"updated": false, "reason": "up_to_date", "current": current}));
        };
        let (Some(url), Some(sums_url), Some(name)) = (rel.asset_url.clone(), rel.sums_url.clone(), rel.asset_name.clone()) else {
            return Err(ApiErr::new(422, "update_no_package", "Rilis tidak memuat paket atau SHA256SUMS.txt", "The release has no package or SHA256SUMS.txt"));
        };
        let (version, from) = (rel.version.clone(), current.clone());
        if !util::safe_file_name(&name) {
            return Err(ApiErr::new(422, "update_bad_asset", "Nama berkas rilis tidak valid", "Invalid release asset name"));
        }
        // 1. checksum list
        self.step(j, 0.05, "Mengambil SHA256SUMS.txt", "Fetching SHA256SUMS.txt");
        let net = |e: reqwest::Error| ApiErr::new(502, "update_network", e.to_string(), e.to_string());
        let sums_text = self.http.get(&sums_url).send().await.and_then(|r| r.error_for_status()).map_err(net)?.text().await.map_err(net)?;
        let expected = update::parse_sums(&sums_text).get(&name).cloned().ok_or_else(|| ApiErr::new(422, "update_no_checksum", format!("Tidak ada checksum untuk {name}"), format!("No checksum listed for {name}")))?;
        // 2. download
        let dl = self.paths.tmp().join("update-dl");
        let _ = std::fs::remove_dir_all(&dl);
        let zip = dl.join(&name);
        let jj = j.clone();
        let label = self.tr("Mengunduh pembaruan", "Downloading the update");
        let got = update::download(&self.http, &url, &zip, &move |d, t| {
            let f = t.map(|t| d as f64 / t.max(1) as f64).unwrap_or(0.0);
            jj.set(0.08 + f * 0.5, &format!("{label} ({:.1} MB)", d as f64 / 1048576.0), "");
        })
        .await
        .map_err(uerr)?;
        self.step(j, 0.6, "Verifikasi SHA-256", "Verifying SHA-256");
        if !util::ct_eq(got.as_bytes(), expected.as_bytes()) {
            let _ = std::fs::remove_dir_all(&dl);
            return Err(ApiErr::new(422, "sha256_mismatch", "Checksum unduhan tidak cocok; pembaruan dibatalkan.", "Download checksum mismatch; update aborted."));
        }
        // 3. backup, stop, swap
        self.step(j, 0.65, "Backup otomatis", "Automatic backup");
        let (paths, root) = (self.paths.clone(), self.data_root());
        if root.is_dir() {
            let _ = tokio::task::spawn_blocking(move || backup::create(&paths, &root, "pre-update")).await;
        }
        let was_active = self.sup.is_active();
        let ed = self.config().edition;
        self.step(j, 0.72, "Menghentikan aplikasi", "Stopping the app");
        self.sup.stop().await;
        self.step(j, 0.78, "Memasang", "Installing");
        let (paths, zip2, v2) = (self.paths.clone(), zip.clone(), version.clone());
        let swapped = tokio::task::spawn_blocking(move || update::apply_package(&paths, &zip2, &v2)).await.map_err(|e| ApiErr::new(500, "internal", e.to_string(), e.to_string()))?;
        let swapped = match swapped {
            Ok(s) => s,
            Err(e) => {
                if was_active {
                    let _ = self.sup.start(ed).await;
                }
                return Err(uerr(e));
            }
        };
        // 4. restart + verify, rollback on failure
        if was_active {
            self.step(j, 0.88, "Restart", "Restarting");
            let started = self.sup.start(ed).await;
            let ok = started.is_ok() && self.wait_stable().await;
            if !ok {
                log::error!("updated app did not start; rolling back to {from}");
                self.sup.stop().await;
                swapped.rollback();
                let _ = self.sup.start(ed).await;
                let _ = std::fs::remove_dir_all(&dl);
                return Err(ApiErr::new(422, "update_failed", format!("Aplikasi baru gagal berjalan; dikembalikan ke versi {from}."), format!("The updated app failed to start; rolled back to {from}.")).with(json!({"rolledBack": true})));
            }
        }
        let _ = std::fs::remove_dir_all(&dl);
        log::info!("application updated {from} -> {version}");
        if let Some(c) = self.update_cache.lock().unwrap().as_mut() {
            c["available"] = json!(false);
            c["current"] = json!(version);
        }
        Ok(json!({"updated": true, "from": from, "to": version, "rolledBack": false}))
    }

    // ------------------------------------------------------------------ diagnostics / open

    pub async fn diagnostics(self: &Arc<Self>) -> Value {
        let me = self.clone();
        let checks = tokio::task::spawn_blocking(move || {
            let cfg = me.config();
            let snap = me.active_snapshot();
            let running = if matches!(snap.state, AppState::Running | AppState::Starting) { snap.port.map(|p| (cfg.edition, p)) } else { None };
            diagnostics::run(&me.paths, &cfg, &me.rt, running)
        })
        .await
        .unwrap_or_default();
        let fails = checks.iter().filter(|c| c.status == "fail").count();
        let warns = checks.iter().filter(|c| c.status == "warn").count();
        json!({"checks": checks, "summary": {"fail": fails, "warn": warns, "pass": checks.len() - fails - warns}})
    }

    /// Safe automatic fixes. Currently: enable PHP extensions that are shipped but switched off in php.ini.
    pub async fn diagnostics_fix(self: &Arc<Self>, id: &str) -> Result<Value, ApiErr> {
        let not_fixable = || ApiErr::new(422, "not_fixable", "Masalah ini tidak bisa diperbaiki otomatis.", "This issue cannot be fixed automatically.");
        if id != "php-ext" {
            return Err(not_fixable());
        }
        let rt = self.launcher.resolve_runtime(Edition::Php).map_err(|_| not_fixable())?.0;
        let enabled = tokio::task::spawn_blocking(move || diagnostics::fix_php_extensions(&rt)).await.map_err(|e| ApiErr::new(500, "internal", e.to_string(), e.to_string()))?;
        match enabled {
            Ok(list) => {
                log::info!("php.ini: enabled extensions {}", list.join(", "));
                if self.config().edition == Edition::Php {
                    self.restart_if_running(Edition::Php).await;
                }
                Ok(json!({"ok": true, "enabled": list}))
            }
            Err(e) if e.code == "not_fixable" => Err(not_fixable()),
            Err(e) => Err(ApiErr::new(500, e.code, e.message.clone(), e.message)),
        }
    }

    pub fn open(&self, what: &str) -> Result<(), ApiErr> {
        let cfg = self.config();
        match what {
            "app:php" | "app:next" | "app" => {
                let ed = if what == "app" { cfg.edition } else { Edition::parse(&what[4..]).unwrap_or(cfg.edition) };
                if ed != cfg.edition {
                    return Err(ApiErr::new(409, "not_active", format!("Edisi {} tidak aktif", ed.label()), format!("The {} edition is not active", ed.label())));
                }
                util::shell_open(&cfg.app_url(ed));
            }
            "folder:data" => {
                let p = self.data_root();
                let _ = std::fs::create_dir_all(&p);
                util::shell_open(&p.to_string_lossy());
            }
            "folder:logs" => util::shell_open(&self.paths.logs().to_string_lossy()),
            "folder:backups" => util::shell_open(&self.paths.backups().to_string_lossy()),
            "folder:app" | "folder:root" => util::shell_open(&self.paths.base.to_string_lossy()),
            "panel" => util::shell_open(&format!("http://127.0.0.1:{}/", cfg.control_port)),
            _ => return Err(ApiErr::new(422, "bad_request", "Nilai 'what' tidak dikenal", "Unknown 'what' value")),
        }
        Ok(())
    }
}

fn os_label() -> String {
    let key = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let name = crate::winproc::reg_get_string("HKLM", key, "ProductName").unwrap_or_else(|| "Windows".into());
    let build = crate::winproc::reg_get_string("HKLM", key, "CurrentBuildNumber").unwrap_or_default();
    let name = if build.parse::<u32>().unwrap_or(0) >= 22000 { name.replace("Windows 10", "Windows 11") } else { name };
    if build.is_empty() {
        name
    } else {
        format!("{name} (build {build})")
    }
}
