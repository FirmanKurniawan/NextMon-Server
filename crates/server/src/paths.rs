//! Portable folder layout: everything lives next to the exe (or NEXTMON_HOME for tests/dev).
use crate::config::Edition;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Paths {
    pub base: PathBuf,
}

impl Paths {
    pub fn discover() -> Paths {
        if let Ok(h) = std::env::var("NEXTMON_HOME") {
            if !h.trim().is_empty() {
                return Paths { base: PathBuf::from(h) };
            }
        }
        Paths { base: crate::util::exe_dir() }
    }
    pub fn new(base: impl Into<PathBuf>) -> Paths {
        Paths { base: base.into() }
    }
    pub fn config_file(&self) -> PathBuf {
        self.base.join("config.json")
    }
    pub fn app_root(&self) -> PathBuf {
        self.base.join("app")
    }
    pub fn app_dir(&self, e: Edition) -> PathBuf {
        self.app_root().join(e.as_str())
    }
    pub fn runtimes(&self) -> PathBuf {
        self.base.join("runtimes")
    }
    pub fn logs(&self) -> PathBuf {
        self.base.join("logs")
    }
    pub fn backups(&self) -> PathBuf {
        self.base.join("backups")
    }
    pub fn run_dir(&self) -> PathBuf {
        self.base.join("run")
    }
    pub fn ui_dir(&self) -> PathBuf {
        self.base.join("ui")
    }
    pub fn pid_file(&self, e: Edition) -> PathBuf {
        self.run_dir().join(format!("{}.pid", e.as_str()))
    }
    pub fn tmp(&self) -> PathBuf {
        self.base.join(".tmp")
    }
    /// Root of all user data (backed up as a whole). Per-edition data lives in `<root>/php` and `<root>/next`.
    pub fn data_root(&self, data_dir: &str) -> PathBuf {
        let p = Path::new(data_dir);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.base.join(p)
        }
    }
    pub fn ensure_dirs(&self) {
        for d in [self.logs(), self.backups(), self.run_dir(), self.runtimes().join("php"), self.runtimes().join("node")] {
            let _ = std::fs::create_dir_all(d);
        }
    }
}
