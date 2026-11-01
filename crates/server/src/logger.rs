//! Logging: `log` facade -> logs/server.log with size rotation; per-edition app logs (daily files); retention cleanup.
use crate::paths::Paths;
use crate::util;
use log::{Level, LevelFilter, Log, Metadata, Record};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

const MAX_BYTES: u64 = 2 * 1024 * 1024;
const KEEP: usize = 5;

struct Inner {
    path: PathBuf,
    file: Option<File>,
    size: u64,
}

pub struct ServerLogger {
    inner: Mutex<Inner>,
    to_stderr: bool,
}

fn open_append(p: &Path) -> Option<(File, u64)> {
    let f = OpenOptions::new().create(true).append(true).open(p).ok()?;
    let size = f.metadata().map(|m| m.len()).unwrap_or(0);
    Some((f, size))
}

impl Inner {
    fn rotate(&mut self) {
        self.file = None;
        // server.log -> server.1.log -> ... -> server.{KEEP-1}.log
        let dir = self.path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let _ = std::fs::remove_file(dir.join(format!("server.{}.log", KEEP - 1)));
        for i in (1..KEEP - 1).rev() {
            let _ = std::fs::rename(dir.join(format!("server.{i}.log")), dir.join(format!("server.{}.log", i + 1)));
        }
        let _ = std::fs::rename(&self.path, dir.join("server.1.log"));
        if let Some((f, s)) = open_append(&self.path) {
            self.file = Some(f);
            self.size = s;
        }
    }
}

impl Log for ServerLogger {
    fn enabled(&self, m: &Metadata) -> bool {
        // keep dependency chatter out of our log
        m.level() <= Level::Info || m.target().starts_with("nextmon")
    }
    fn log(&self, r: &Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let t = util::now_local();
        let line = format!("{} {:<5} {}\n", t.iso().replace('T', " "), r.level(), r.args());
        if self.to_stderr {
            let _ = std::io::stderr().write_all(line.as_bytes());
        }
        if let Ok(mut g) = self.inner.lock() {
            if g.file.is_none() {
                if let Some((f, s)) = open_append(&g.path) {
                    g.file = Some(f);
                    g.size = s;
                }
            }
            if g.size > MAX_BYTES {
                g.rotate();
            }
            if let Some(f) = g.file.as_mut() {
                if f.write_all(line.as_bytes()).is_ok() {
                    g.size += line.len() as u64;
                }
            }
        }
    }
    fn flush(&self) {}
}

static LOGGER: std::sync::OnceLock<ServerLogger> = std::sync::OnceLock::new();

pub fn init(paths: &Paths, to_stderr: bool) {
    let _ = std::fs::create_dir_all(paths.logs());
    let lg = LOGGER.get_or_init(|| ServerLogger {
        inner: Mutex::new(Inner { path: paths.logs().join("server.log"), file: None, size: 0 }),
        to_stderr,
    });
    let _ = log::set_logger(lg);
    log::set_max_level(LevelFilter::Info);
}

/// Per-edition application log (stdout/stderr of php/node), one file per day.
pub struct AppLog {
    dir: PathBuf,
    name: String,
    state: Mutex<(String, Option<File>)>,
}

impl AppLog {
    pub fn new(dir: PathBuf, name: &str) -> AppLog {
        let _ = std::fs::create_dir_all(&dir);
        AppLog { dir, name: name.to_string(), state: Mutex::new((String::new(), None)) }
    }
    pub fn write(&self, stream: &str, line: &str) {
        let t = util::now_local();
        let date = t.date();
        if let Ok(mut g) = self.state.lock() {
            if g.0 != date || g.1.is_none() {
                let p = self.dir.join(format!("{}-{}.log", self.name, date));
                g.1 = OpenOptions::new().create(true).append(true).open(p).ok();
                g.0 = date;
            }
            if let Some(f) = g.1.as_mut() {
                let _ = writeln!(f, "[{}] [{}] {}", t.time(), stream, line.trim_end_matches(['\r', '\n']));
            }
        }
    }
}

/// Newest log file for a name: `server` -> server.log, otherwise `<name>-YYYY-MM-DD.log` (latest date).
pub fn current_log_file(paths: &Paths, name: &str) -> Option<PathBuf> {
    if name == "server" {
        return Some(paths.logs().join("server.log"));
    }
    let prefix = format!("{name}-");
    let mut best: Option<String> = None;
    for e in std::fs::read_dir(paths.logs()).ok()?.flatten() {
        let n = e.file_name().to_string_lossy().to_string();
        if n.starts_with(&prefix) && n.ends_with(".log") && best.as_ref().map(|b| &n > b).unwrap_or(true) {
            best = Some(n);
        }
    }
    Some(paths.logs().join(best.unwrap_or_else(|| format!("{name}-{}.log", util::now_local().date()))))
}

/// Last `n` lines of a file (reads at most the last 2 MiB).
pub fn tail_lines(path: &Path, n: usize) -> Vec<String> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = File::open(path) else { return vec![] };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(2 * 1024 * 1024);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return vec![];
    }
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<&str> = text.lines().collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0); // probably cut mid-line
    }
    let skip = lines.len().saturating_sub(n);
    lines[skip..].iter().map(|s| s.to_string()).collect()
}

/// Delete log files older than `days` (by mtime). Returns number removed.
pub fn cleanup(paths: &Paths, days: u32) -> usize {
    let cutoff = SystemTime::now().checked_sub(Duration::from_secs(days as u64 * 86400));
    let Some(cutoff) = cutoff else { return 0 };
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(paths.logs()) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.ends_with(".log") || name == "server.log" {
                continue;
            }
            if let Ok(m) = e.metadata().and_then(|m| m.modified()) {
                if m < cutoff && std::fs::remove_file(e.path()).is_ok() {
                    n += 1;
                }
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tail_and_cleanup() {
        let d = tempfile::tempdir().unwrap();
        let p = Paths::new(d.path());
        p.ensure_dirs();
        let f = p.logs().join("php-2020-01-01.log");
        std::fs::write(&f, (1..=10).map(|i| format!("l{i}\n")).collect::<String>()).unwrap();
        assert_eq!(tail_lines(&f, 3), vec!["l8", "l9", "l10"]);
        assert_eq!(current_log_file(&p, "php").unwrap(), f);
        // make it old
        let old = SystemTime::now() - Duration::from_secs(40 * 86400);
        File::options().write(true).open(&f).unwrap().set_modified(old).unwrap();
        assert_eq!(cleanup(&p, 14), 1);
        assert!(!f.exists());
    }
    #[test]
    fn app_log_writes_daily_file() {
        let d = tempfile::tempdir().unwrap();
        let l = AppLog::new(d.path().to_path_buf(), "php");
        l.write("out", "hello\r\n");
        let name = format!("php-{}.log", util::now_local().date());
        let s = std::fs::read_to_string(d.path().join(name)).unwrap();
        assert!(s.contains("[out] hello") && !s.contains('\r'));
    }
}
