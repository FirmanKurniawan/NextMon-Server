//! Process supervisor. Runs at most ONE edition process at a time (invariant enforced by `op_lock`).
//! Spawns hidden, captures stdout/stderr to logs/, restarts on crash with backoff, kills the whole process tree
//! through a Windows job object (also when this exe dies), keeps a PID file for orphan cleanup.
use crate::config::{Edition, SharedConfig};
use crate::launcher::{LaunchError, LaunchSpec, Launcher};
use crate::logger::AppLog;
use crate::paths::Paths;
use crate::winproc;
use serde::Serialize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{watch, Mutex as AsyncMutex};
use tokio::task::JoinHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AppState {
    Stopped,
    Starting,
    Running,
    Stopping,
    /// Crashed; waiting before the next automatic restart.
    #[serde(rename = "crashed")]
    Backoff,
    /// Gave up (max restarts exceeded or could not be launched).
    Failed,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub edition: Option<Edition>,
    pub state: AppState,
    pub pid: Option<u32>,
    pub port: Option<u16>,
    pub uptime_secs: u64,
    pub restarts: u32,
    pub last_error: Option<String>,
    pub retry_in_secs: Option<u64>,
    pub runtime: String,
}

#[derive(Default)]
struct Status {
    edition: Option<Edition>,
    state: Option<AppState>,
    pid: Option<u32>,
    port: Option<u16>,
    started: Option<Instant>,
    restarts: u32,
    last_error: Option<String>,
    retry_until: Option<Instant>,
    runtime: String,
}

struct MonitorHandle {
    stop_tx: watch::Sender<bool>,
    task: JoinHandle<()>,
    edition: Edition,
}

struct Inner {
    paths: Paths,
    cfg: SharedConfig,
    launcher: Arc<dyn Launcher>,
    /// Serialises start/stop/switch so two editions can never be spawned concurrently.
    op_lock: AsyncMutex<Option<MonitorHandle>>,
    st: Mutex<Status>,
    logs: [Arc<AppLog>; 2],
    live: AtomicUsize,
    max_live: AtomicUsize,
}

#[derive(Clone)]
pub struct Supervisor {
    inner: Arc<Inner>,
}

struct Running {
    child: tokio::process::Child,
    pid: u32,
    job: Option<winproc::JobObject>,
    readers: Vec<JoinHandle<()>>,
    spec: LaunchSpec,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StartOutcome {
    Started,
    AlreadyRunning,
}

fn idx(e: Edition) -> usize {
    match e {
        Edition::Php => 0,
        Edition::Next => 1,
    }
}

impl Supervisor {
    pub fn new(paths: Paths, cfg: SharedConfig, launcher: Arc<dyn Launcher>) -> Supervisor {
        let logs = [Arc::new(AppLog::new(paths.logs(), "php")), Arc::new(AppLog::new(paths.logs(), "next"))];
        Supervisor {
            inner: Arc::new(Inner {
                paths,
                cfg,
                launcher,
                op_lock: AsyncMutex::new(None),
                st: Mutex::new(Status::default()),
                logs,
                live: AtomicUsize::new(0),
                max_live: AtomicUsize::new(0),
            }),
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let s = self.inner.st.lock().unwrap();
        Snapshot {
            edition: s.edition,
            state: s.state.unwrap_or(AppState::Stopped),
            pid: s.pid,
            port: s.port,
            uptime_secs: match (s.state, s.started) {
                (Some(AppState::Running | AppState::Starting), Some(t)) => t.elapsed().as_secs(),
                _ => 0,
            },
            restarts: s.restarts,
            last_error: s.last_error.clone(),
            retry_in_secs: s.retry_until.map(|t| t.saturating_duration_since(Instant::now()).as_secs()),
            runtime: s.runtime.clone(),
        }
    }

    /// Highest number of simultaneously live child processes ever observed (must stay <= 1).
    pub fn max_concurrent_processes(&self) -> usize {
        self.inner.max_live.load(Ordering::SeqCst)
    }

    pub fn live_processes(&self) -> usize {
        self.inner.live.load(Ordering::SeqCst)
    }

    /// True while the active edition is starting/running/backing off.
    pub fn is_active(&self) -> bool {
        matches!(self.snapshot().state, AppState::Starting | AppState::Running | AppState::Backoff | AppState::Stopping)
    }

    pub async fn start(&self, ed: Edition) -> Result<StartOutcome, LaunchError> {
        let mut g = self.inner.op_lock.lock().await;
        self.start_locked(&mut g, ed).await
    }

    pub async fn stop(&self) {
        let mut g = self.inner.op_lock.lock().await;
        self.stop_locked(&mut g).await;
    }

    pub async fn restart(&self, ed: Edition) -> Result<(), LaunchError> {
        let mut g = self.inner.op_lock.lock().await;
        self.inner.launcher.spec(ed)?; // validate before killing the running instance
        self.stop_locked(&mut g).await;
        self.start_locked(&mut g, ed).await.map(|_| ())
    }

    /// Switch the active edition: validate the new one, stop the old one, `persist` the new setting, optionally start.
    pub async fn switch(&self, ed: Edition, start: bool, persist: &(dyn Fn() -> Result<(), String> + Sync)) -> Result<(), SwitchError> {
        let mut g = self.inner.op_lock.lock().await;
        if start {
            self.inner.launcher.spec(ed).map_err(SwitchError::Launch)?;
        }
        self.stop_locked(&mut g).await;
        persist().map_err(SwitchError::Persist)?;
        if start {
            self.start_locked(&mut g, ed).await.map_err(SwitchError::Launch)?;
        }
        Ok(())
    }

    /// Wait until the app is Running (or has failed). Returns the final state.
    pub async fn wait_settled(&self, timeout: Duration) -> AppState {
        let end = Instant::now() + timeout;
        loop {
            let s = self.snapshot().state;
            if matches!(s, AppState::Running | AppState::Failed | AppState::Stopped) || Instant::now() >= end {
                return s;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn start_locked(&self, g: &mut Option<MonitorHandle>, ed: Edition) -> Result<StartOutcome, LaunchError> {
        if let Some(m) = g.as_ref() {
            let st = self.snapshot().state;
            let alive = matches!(st, AppState::Starting | AppState::Running | AppState::Backoff);
            if m.edition == ed && alive && !m.task.is_finished() {
                return Ok(StartOutcome::AlreadyRunning);
            }
        }
        // Different edition (or dead monitor): make sure the old one is fully gone first.
        self.stop_locked(g).await;

        let spec = self.inner.launcher.spec(ed)?;
        let running = self.inner.spawn_child(ed, spec).map_err(|e| {
            self.inner.set_failed(ed, &e.en);
            e
        })?;
        {
            let mut s = self.inner.st.lock().unwrap();
            s.edition = Some(ed);
            s.restarts = 0;
            s.last_error = None;
        }
        let (stop_tx, stop_rx) = watch::channel(false);
        let inner = self.inner.clone();
        let task = tokio::spawn(async move { inner.monitor(ed, stop_rx, running).await });
        *g = Some(MonitorHandle { stop_tx, task, edition: ed });
        Ok(StartOutcome::Started)
    }

    async fn stop_locked(&self, g: &mut Option<MonitorHandle>) {
        let Some(m) = g.take() else {
            let mut s = self.inner.st.lock().unwrap();
            if matches!(s.state, Some(AppState::Failed)) {
                s.state = Some(AppState::Stopped);
            }
            return;
        };
        {
            let mut s = self.inner.st.lock().unwrap();
            s.state = Some(AppState::Stopping);
        }
        let _ = m.stop_tx.send(true);
        let mut task = m.task;
        if tokio::time::timeout(Duration::from_secs(20), &mut task).await.is_err() {
            task.abort();
            let pid = self.inner.st.lock().unwrap().pid;
            if let Some(p) = pid {
                winproc::kill_pid(p);
            }
        }
        let mut s = self.inner.st.lock().unwrap();
        s.state = Some(AppState::Stopped);
        s.pid = None;
        s.started = None;
        s.retry_until = None;
    }

    /// Kill leftovers from a previous run whose PID file still points at a live process of ours.
    pub fn cleanup_orphans(paths: &Paths) -> usize {
        let mut n = 0;
        for ed in Edition::ALL {
            let f = paths.pid_file(ed);
            let Ok(text) = std::fs::read_to_string(&f) else { continue };
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                let pid = v["pid"].as_u64().unwrap_or(0) as u32;
                let exe = v["exe"].as_str().unwrap_or("").to_ascii_lowercase();
                if pid != 0 && winproc::is_alive(pid) {
                    let cur = winproc::image_path(pid).unwrap_or_default().to_ascii_lowercase();
                    if !exe.is_empty() && cur == exe {
                        log::warn!("killing orphaned {} process pid {pid} from a previous run", ed.as_str());
                        if winproc::kill_pid(pid) {
                            n += 1;
                        }
                    }
                }
            }
            let _ = std::fs::remove_file(f);
        }
        n
    }
}

#[derive(Debug)]
pub enum SwitchError {
    Launch(LaunchError),
    Persist(String),
}

impl Inner {
    fn set_failed(&self, ed: Edition, msg: &str) {
        let mut s = self.st.lock().unwrap();
        s.edition = Some(ed);
        s.state = Some(AppState::Failed);
        s.pid = None;
        s.last_error = Some(msg.to_string());
    }

    fn spawn_child(&self, ed: Edition, spec: LaunchSpec) -> Result<Running, LaunchError> {
        let log = self.logs[idx(ed)].clone();
        // Port must be free (also catches a stale process we do not own).
        if let Some(port) = spec.ready_port {
            if !crate::net::port_free(&spec.bind, port) {
                let owner = winproc::port_owner(port)
                    .map(|p| format!(" ({} pid {p})", winproc::process_name(p).unwrap_or_else(|| "?".into())))
                    .unwrap_or_default();
                return Err(LaunchError::new(
                    "port_in_use",
                    format!("Port {port} is already in use{owner}"),
                    format!("Port {port} sudah dipakai{owner}"),
                ));
            }
        }
        if !spec.program.is_file() {
            return Err(LaunchError::new(
                "exe_missing",
                format!("Executable not found: {}", spec.program.display()),
                format!("Program tidak ditemukan: {}", spec.program.display()),
            ));
        }
        let mut cmd = tokio::process::Command::new(&spec.program);
        cmd.args(&spec.args)
            .current_dir(&spec.cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(false);
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let mut child = cmd.spawn().map_err(|e| {
            LaunchError::new("spawn_failed", format!("Cannot start {}: {e}", spec.program.display()), format!("Gagal menjalankan {}: {e}", spec.program.display()))
        })?;
        let pid = child.id().unwrap_or(0);
        let job = winproc::JobObject::new_kill_on_close();
        #[cfg(windows)]
        if let (Some(j), Some(h)) = (job.as_ref(), child.raw_handle()) {
            if !j.assign_raw(h as *mut std::ffi::c_void) {
                log::warn!("could not assign pid {pid} to job object");
            }
        }
        let mut readers = vec![];
        if let Some(out) = child.stdout.take() {
            let l = log.clone();
            readers.push(tokio::spawn(pump(out, l, "out")));
        }
        if let Some(err) = child.stderr.take() {
            let l = log.clone();
            readers.push(tokio::spawn(pump(err, l, "err")));
        }
        let now = self.live.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_live.fetch_max(now, Ordering::SeqCst);

        let _ = std::fs::create_dir_all(self.paths.run_dir());
        let pidjson = serde_json::json!({"pid": pid, "exe": spec.program.to_string_lossy(), "edition": ed.as_str(), "started": crate::util::now_local().iso()});
        let _ = std::fs::write(self.paths.pid_file(ed), pidjson.to_string());
        log.write("sup", &format!("started pid {pid}: {} {}", spec.program.display(), spec.args.join(" ")));
        log::info!("{} started (pid {pid}, {})", ed.as_str(), spec.runtime_label);
        {
            let mut s = self.st.lock().unwrap();
            s.edition = Some(ed);
            s.state = Some(AppState::Starting);
            s.pid = Some(pid);
            s.port = spec.ready_port;
            s.started = Some(Instant::now());
            s.retry_until = None;
            s.runtime = spec.runtime_label.clone();
        }
        Ok(Running { child, pid, job, readers, spec })
    }

    async fn reap(&self, ed: Edition, run: &mut Running) {
        if let Some(j) = &run.job {
            j.terminate();
        }
        let _ = tokio::time::timeout(Duration::from_secs(5), run.child.wait()).await;
        for r in run.readers.drain(..) {
            let _ = tokio::time::timeout(Duration::from_secs(2), r).await;
        }
        self.live.fetch_sub(1, Ordering::SeqCst);
        let _ = std::fs::remove_file(self.paths.pid_file(ed));
    }

    async fn monitor(self: Arc<Self>, ed: Edition, mut stop_rx: watch::Receiver<bool>, first: Running) {
        let log = self.logs[idx(ed)].clone();
        let mut restarts: u32 = 0;
        let mut current: Option<Running> = Some(first);
        loop {
            let started_at = Instant::now();
            let mut give_up_reason: Option<String> = None;
            if let Some(mut run) = current.take() {
                let pid = run.pid;
                let ready = tokio::spawn(readiness(self.clone(), ed, pid, run.spec.ready_port));
                let stopped = tokio::select! {
                    st = run.child.wait() => {
                        let msg = match st {
                            Ok(s) => format!("process {pid} exited ({s})"),
                            Err(e) => format!("wait failed: {e}"),
                        };
                        log.write("sup", &msg);
                        log::warn!("{} {msg}", ed.as_str());
                        false
                    }
                    _ = stop_rx.changed() => true,
                };
                ready.abort();
                if stopped {
                    log.write("sup", &format!("stopping pid {pid}"));
                    let _ = run.child.start_kill();
                    self.reap(ed, &mut run).await;
                    let mut s = self.st.lock().unwrap();
                    s.state = Some(AppState::Stopped);
                    s.pid = None;
                    return;
                }
                self.reap(ed, &mut run).await;
                {
                    let mut s = self.st.lock().unwrap();
                    s.pid = None;
                    s.last_error = Some("process exited unexpectedly".into());
                }
                if started_at.elapsed() > Duration::from_secs(60) {
                    restarts = 0; // ran long enough to be considered healthy
                }
            }
            // Decide whether to retry.
            let (auto, max) = {
                let c = self.cfg.read().unwrap();
                (c.restart_on_crash, c.max_restarts)
            };
            if !auto {
                give_up_reason = Some("restartOnCrash is off".into());
            } else if restarts >= max {
                give_up_reason = Some(format!("gave up after {max} restarts"));
            }
            if let Some(why) = give_up_reason {
                log.write("sup", &why);
                log::error!("{} {why}", ed.as_str());
                let mut s = self.st.lock().unwrap();
                s.state = Some(AppState::Failed);
                s.pid = None;
                s.last_error = Some(format!("{why}; see logs"));
                s.retry_until = None;
                return;
            }
            restarts += 1;
            let delay = Duration::from_secs((1u64 << (restarts - 1).min(5)).min(30));
            {
                let mut s = self.st.lock().unwrap();
                s.state = Some(AppState::Backoff);
                s.restarts = restarts;
                s.retry_until = Some(Instant::now() + delay);
            }
            log.write("sup", &format!("restart {restarts}/{max} in {}s", delay.as_secs()));
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = stop_rx.changed() => {
                    let mut s = self.st.lock().unwrap();
                    s.state = Some(AppState::Stopped);
                    s.retry_until = None;
                    return;
                }
            }
            if *stop_rx.borrow() {
                let mut s = self.st.lock().unwrap();
                s.state = Some(AppState::Stopped);
                return;
            }
            match self.launcher.spec(ed).and_then(|spec| self.spawn_child(ed, spec)) {
                Ok(r) => current = Some(r),
                Err(e) => {
                    log.write("sup", &format!("restart failed: {}", e.en));
                    log::warn!("{} restart failed: {}", ed.as_str(), e.en);
                    self.st.lock().unwrap().last_error = Some(e.en);
                    // loop again: counts as another failed attempt
                }
            }
        }
    }
}

/// Marks the app Running once its TCP port accepts connections (or after 30 s regardless, with a warning).
async fn readiness(inner: Arc<Inner>, ed: Edition, pid: u32, port: Option<u16>) {
    let set_running = |warn: Option<&str>| {
        let mut s = inner.st.lock().unwrap();
        if s.pid == Some(pid) && matches!(s.state, Some(AppState::Starting)) {
            s.state = Some(AppState::Running);
            if let Some(w) = warn {
                s.last_error = Some(w.to_string());
            }
        }
    };
    let Some(port) = port else {
        set_running(None);
        return;
    };
    let end = Instant::now() + Duration::from_secs(30);
    while Instant::now() < end {
        if crate::net::can_connect(port).await {
            set_running(None);
            inner.logs[idx(ed)].write("sup", &format!("ready on port {port}"));
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    inner.logs[idx(ed)].write("sup", &format!("port {port} did not accept connections within 30s"));
    set_running(Some("port did not open within 30s"));
}

async fn pump<R: tokio::io::AsyncRead + Unpin>(r: R, log: Arc<AppLog>, stream: &'static str) {
    let mut lines = BufReader::new(r).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(l)) => log.write(stream, &l),
            _ => break,
        }
    }
}
