//! Supervisor integration tests with real child processes (ping.exe / cmd.exe as dummy apps).
#![cfg(windows)]

use nextmon_server::config::{shared, Config, Edition, SharedConfig};
use nextmon_server::launcher::{LaunchError, LaunchSpec, Launcher};
use nextmon_server::paths::Paths;
use nextmon_server::supervisor::{AppState, Supervisor};
use nextmon_server::{logger, winproc};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn sys32(exe: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into())).join("System32").join(exe)
}

fn long_running(tag: &str) -> LaunchSpec {
    LaunchSpec {
        program: sys32("ping.exe"),
        args: vec!["-n".into(), "300".into(), "127.0.0.1".into()],
        env: vec![("NM_TAG".into(), tag.into())],
        cwd: std::env::temp_dir(),
        ready_port: None,
        bind: "127.0.0.1".into(),
        runtime_label: format!("dummy {tag}"),
    }
}

fn exits_immediately() -> LaunchSpec {
    LaunchSpec { program: sys32("cmd.exe"), args: vec!["/c".into(), "exit".into(), "3".into()], ..long_running("crash") }
}

type SpecFn = Box<dyn Fn(Edition, usize) -> Result<LaunchSpec, LaunchError> + Send + Sync>;

struct TestLauncher {
    calls: AtomicUsize,
    f: SpecFn,
}

impl Launcher for TestLauncher {
    fn spec(&self, ed: Edition) -> Result<LaunchSpec, LaunchError> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        (self.f)(ed, n)
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    paths: Paths,
    cfg: SharedConfig,
    sup: Supervisor,
}

fn fixture(f: SpecFn, tweak: impl FnOnce(&mut Config)) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::new(dir.path());
    paths.ensure_dirs();
    let mut c = Config::default();
    c.fixup();
    tweak(&mut c);
    let cfg = shared(c);
    let sup = Supervisor::new(paths.clone(), cfg.clone(), Arc::new(TestLauncher { calls: AtomicUsize::new(0), f }));
    Fixture { _dir: dir, paths, cfg, sup }
}

async fn until(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        if cond() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    cond()
}

#[tokio::test]
async fn start_capture_logs_and_stop() {
    let fx = fixture(Box::new(|_, _| Ok(long_running("a"))), |_| {});
    assert_eq!(fx.sup.start(Edition::Php).await.unwrap(), nextmon_server::supervisor::StartOutcome::Started);
    assert_eq!(fx.sup.wait_settled(Duration::from_secs(5)).await, AppState::Running);
    let snap = fx.sup.snapshot();
    let pid = snap.pid.unwrap();
    assert!(winproc::is_alive(pid));
    assert_eq!(snap.edition, Some(Edition::Php));
    assert!(fx.paths.pid_file(Edition::Php).is_file(), "pid file written");
    // starting again is a no-op
    assert_eq!(fx.sup.start(Edition::Php).await.unwrap(), nextmon_server::supervisor::StartOutcome::AlreadyRunning);
    assert_eq!(fx.sup.snapshot().pid, Some(pid));
    // ping prints "Pinging ..." on stdout; it must land in logs/php-<date>.log
    let log = logger::current_log_file(&fx.paths, "php").unwrap();
    assert!(until(Duration::from_secs(5), || std::fs::read_to_string(&log).map(|s| s.contains("Pinging") && s.contains("[out]")).unwrap_or(false)).await, "stdout captured");
    fx.sup.stop().await;
    assert_eq!(fx.sup.snapshot().state, AppState::Stopped);
    assert!(until(Duration::from_secs(3), || !winproc::is_alive(pid)).await, "process terminated");
    assert!(!fx.paths.pid_file(Edition::Php).exists(), "pid file removed");
    assert_eq!(fx.sup.live_processes(), 0);
}

#[tokio::test]
async fn crash_loop_backs_off_then_gives_up() {
    let fx = fixture(Box::new(|_, _| Ok(exits_immediately())), |c| c.max_restarts = 2);
    fx.sup.start(Edition::Php).await.unwrap();
    let t0 = Instant::now();
    assert!(until(Duration::from_secs(15), || fx.sup.snapshot().state == AppState::Failed).await, "state: {:?}", fx.sup.snapshot().state);
    let s = fx.sup.snapshot();
    assert_eq!(s.restarts, 2);
    assert!(s.last_error.unwrap().contains("gave up"));
    // backoff 1s + 2s between attempts: must not have burned through them instantly
    assert!(t0.elapsed() >= Duration::from_millis(2500), "elapsed {:?}", t0.elapsed());
    assert_eq!(fx.sup.live_processes(), 0);
    let log = std::fs::read_to_string(logger::current_log_file(&fx.paths, "php").unwrap()).unwrap();
    assert!(log.contains("restart 1/2") && log.contains("restart 2/2"), "{log}");
}

#[tokio::test]
async fn restart_on_crash_recovers() {
    // the first two launches die, the third one stays up (restarts counter = 2 once it is up)
    let fx = fixture(Box::new(|_, n| Ok(if n <= 1 { exits_immediately() } else { long_running("ok") })), |_| {});
    fx.sup.start(Edition::Php).await.unwrap();
    assert!(until(Duration::from_secs(10), || fx.sup.snapshot().state == AppState::Running && fx.sup.snapshot().restarts >= 2).await, "{:?}", fx.sup.snapshot());
    assert!(winproc::is_alive(fx.sup.snapshot().pid.unwrap()));
    fx.sup.stop().await;
}

#[tokio::test]
async fn restart_on_crash_can_be_disabled() {
    let fx = fixture(Box::new(|_, _| Ok(exits_immediately())), |c| c.restart_on_crash = false);
    fx.sup.start(Edition::Php).await.unwrap();
    assert!(until(Duration::from_secs(5), || fx.sup.snapshot().state == AppState::Failed).await);
    assert_eq!(fx.sup.snapshot().restarts, 0);
}

#[tokio::test]
async fn stop_during_backoff_cancels_restart() {
    let fx = fixture(Box::new(|_, _| Ok(exits_immediately())), |c| c.max_restarts = 10);
    fx.sup.start(Edition::Php).await.unwrap();
    assert!(until(Duration::from_secs(5), || fx.sup.snapshot().state == AppState::Backoff).await);
    fx.sup.stop().await;
    assert_eq!(fx.sup.snapshot().state, AppState::Stopped);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(fx.sup.snapshot().state, AppState::Stopped, "no restart after stop");
    assert_eq!(fx.sup.live_processes(), 0);
}

#[tokio::test]
async fn never_two_editions_at_once() {
    let fx = fixture(Box::new(|ed, _| Ok(long_running(ed.as_str()))), |_| {});
    let sup = fx.sup.clone();
    // sample every pid we ever see
    let seen: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(vec![]));
    let (s2, seen2) = (sup.clone(), seen.clone());
    let sampler = tokio::spawn(async move {
        loop {
            if let Some(p) = s2.snapshot().pid {
                let mut v = seen2.lock().unwrap();
                if !v.contains(&p) {
                    v.push(p);
                }
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    // hammer it from several tasks with conflicting requests
    let mut hs = vec![];
    for i in 0..6 {
        let s = sup.clone();
        hs.push(tokio::spawn(async move {
            for k in 0..6 {
                match (i + k) % 4 {
                    0 => {
                        let _ = s.start(Edition::Php).await;
                    }
                    1 => {
                        let _ = s.start(Edition::Next).await;
                    }
                    2 => {
                        let _ = s.restart(if k % 2 == 0 { Edition::Php } else { Edition::Next }).await;
                    }
                    _ => {
                        let _ = s.switch(Edition::Next, true, &|| Ok(())).await;
                    }
                }
            }
        }));
    }
    for h in hs {
        h.await.unwrap();
    }
    assert!(sup.max_concurrent_processes() <= 1, "max concurrent was {}", sup.max_concurrent_processes());
    // at most one of all pids ever seen may be alive right now
    let alive_now = seen.lock().unwrap().iter().filter(|p| winproc::is_alive(**p)).count();
    assert!(alive_now <= 1, "{alive_now} processes alive");
    sup.stop().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    sampler.abort();
    let alive_after = seen.lock().unwrap().iter().filter(|p| winproc::is_alive(**p)).count();
    assert_eq!(alive_after, 0, "everything dead after stop");
    assert!(seen.lock().unwrap().len() > 2, "test actually switched processes");
}

#[tokio::test]
async fn switch_stops_old_edition_first_and_persists() {
    let fx = fixture(Box::new(|ed, _| Ok(long_running(ed.as_str()))), |_| {});
    fx.sup.start(Edition::Php).await.unwrap();
    let old = fx.sup.snapshot().pid.unwrap();
    let cfg = fx.cfg.clone();
    fx.sup
        .switch(Edition::Next, true, &move || {
            cfg.write().unwrap().edition = Edition::Next;
            Ok(())
        })
        .await
        .unwrap();
    assert!(until(Duration::from_secs(3), || !winproc::is_alive(old)).await, "old edition process is gone");
    let s = fx.sup.snapshot();
    assert_eq!(s.edition, Some(Edition::Next));
    assert_ne!(s.pid, Some(old));
    assert_eq!(fx.cfg.read().unwrap().edition, Edition::Next);
    fx.sup.stop().await;
}

#[tokio::test]
async fn failed_validation_does_not_stop_running_edition() {
    let fx = fixture(
        Box::new(|ed, _| if ed == Edition::Next { Err(LaunchError::new("runtime_missing", "no node", "tidak ada node")) } else { Ok(long_running("php")) }),
        |_| {},
    );
    fx.sup.start(Edition::Php).await.unwrap();
    let pid = fx.sup.snapshot().pid.unwrap();
    fx.sup.wait_settled(Duration::from_secs(5)).await;
    let r = fx.sup.switch(Edition::Next, true, &|| Ok(())).await;
    assert!(r.is_err());
    assert!(winproc::is_alive(pid), "php keeps running when the switch is refused");
    assert_eq!(fx.sup.snapshot().state, AppState::Running);
    fx.sup.stop().await;
}

#[tokio::test]
async fn busy_port_is_refused_with_clear_error() {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let fx = fixture(Box::new(move |_, _| Ok(LaunchSpec { ready_port: Some(port), ..long_running("p") })), |_| {});
    let e = fx.sup.start(Edition::Php).await.unwrap_err();
    assert_eq!(e.code, "port_in_use");
    assert_eq!(fx.sup.live_processes(), 0);
    assert_eq!(fx.sup.snapshot().state, AppState::Failed);
}

#[tokio::test]
async fn orphan_from_previous_run_is_killed() {
    let fx = fixture(Box::new(|_, _| Ok(long_running("x"))), |_| {});
    let exe = sys32("ping.exe");
    let mut c = std::process::Command::new(&exe);
    c.args(["-n", "300", "127.0.0.1"]).stdout(std::process::Stdio::null());
    nextmon_server::util::hide_std(&mut c);
    let mut child = c.spawn().unwrap();
    let pid = child.id();
    std::fs::write(fx.paths.pid_file(Edition::Php), format!("{{\"pid\":{pid},\"exe\":{:?}}}", exe.to_string_lossy())).unwrap();
    // a pid file that points at an unrelated live process must NOT kill it
    let me = std::process::id();
    std::fs::write(fx.paths.pid_file(Edition::Next), format!("{{\"pid\":{me},\"exe\":\"C:\\\\nowhere\\\\php.exe\"}}")).unwrap();
    assert_eq!(Supervisor::cleanup_orphans(&fx.paths), 1);
    assert!(until(Duration::from_secs(3), || !winproc::is_alive(pid)).await, "orphan killed");
    assert!(winproc::is_alive(me));
    assert!(!fx.paths.pid_file(Edition::Php).exists() && !fx.paths.pid_file(Edition::Next).exists());
    let _ = child.wait();
}

#[test]
fn job_object_kills_children_when_closed() {
    use std::os::windows::io::AsRawHandle;
    let mut c = std::process::Command::new(sys32("ping.exe"));
    c.args(["-n", "300", "127.0.0.1"]).stdout(std::process::Stdio::null());
    nextmon_server::util::hide_std(&mut c);
    let mut child = c.spawn().unwrap();
    let pid = child.id();
    let job = winproc::JobObject::new_kill_on_close().expect("job");
    assert!(job.assign_raw(child.as_raw_handle() as *mut _));
    assert!(winproc::is_alive(pid));
    drop(job); // what happens when our exe exits
    let end = Instant::now() + Duration::from_secs(3);
    while winproc::is_alive(pid) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!winproc::is_alive(pid), "child must die with the job handle");
    let _ = child.wait();
}
