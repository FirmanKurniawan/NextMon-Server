//! Adapter between the server and the runtime manager. The server only depends on the `RuntimeApi` trait:
//! `NmRuntimes` delegates to the real `nm_runtimes` crate, `FsRuntimes` is an offline stub (folder scan only, used in tests).
use futures_util::future::BoxFuture;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct RtError {
    pub code: String,
    pub message: String,
    pub message_id: String,
}

impl RtError {
    pub fn new(code: &str, en: &str, id: &str) -> RtError {
        RtError { code: code.into(), message: en.into(), message_id: id.into() }
    }
}

#[derive(Debug, Clone)]
pub struct InstalledRt {
    pub kind: String,
    /// Short version ("8.4.27").
    pub version: String,
    /// Folder name under runtimes/<kind>/ - this is what `apps.*.runtime` stores ("8.4.27" or "8.4.27-ts").
    pub dir_name: String,
    pub path: PathBuf,
    pub size_mb: f64,
    pub tsnts: String,
    pub arch: String,
    pub full_version: String,
}

impl InstalledRt {
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "kind": self.kind, "version": self.version, "dirName": self.dir_name, "fullVersion": self.full_version,
            "path": self.path.to_string_lossy(), "sizeMB": self.size_mb, "tsnts": self.tsnts, "arch": self.arch,
        })
    }
}

/// Progress callback: (0..1, step/message).
pub type ProgressCb = Arc<dyn Fn(f64, String) + Send + Sync>;

pub trait RuntimeApi: Send + Sync {
    /// Newest first.
    fn list_installed(&self, kind: &str) -> Vec<InstalledRt>;
    /// JSON shaped like `/api/runtimes/{kind}` (installed/available/latest/updateAvailable/offline...).
    fn overview(&self, kind: &str, channel: &str, variant: &str, force: bool) -> BoxFuture<'static, Result<Value, RtError>>;
    /// Latest downloadable version for the channel as JSON (`{version, channel, ...}`), None when nothing newer/known.
    fn resolve_latest(&self, kind: &str, channel: &str) -> BoxFuture<'static, Result<Option<Value>, RtError>>;
    fn install(&self, kind: &str, spec: &str, variant: Option<String>, progress: ProgressCb) -> BoxFuture<'static, Result<InstalledRt, RtError>>;
    fn uninstall(&self, kind: &str, dir_name: &str, in_use: &dyn Fn(&InstalledRt) -> bool) -> Result<(), RtError>;
    /// `php -v`/`node -v` + required extensions; JSON report with at least `ok: bool`.
    fn health_check(&self, kind: &str, dir: &Path) -> BoxFuture<'static, Value>;
    fn cleanup_stale(&self) -> usize {
        0
    }
}

pub fn exe_name(kind: &str) -> &'static str {
    if kind == "node" {
        "node.exe"
    } else {
        "php.exe"
    }
}

// ---------------------------------------------------------------- real implementation

pub struct NmRuntimes {
    mgr: nm_runtimes::RuntimeManager,
}

impl NmRuntimes {
    pub fn new(root: PathBuf, min_node: &str) -> Result<NmRuntimes, RtError> {
        let opts = nm_runtimes::Options { min_node: min_node.to_string(), ..Default::default() };
        nm_runtimes::RuntimeManager::new(root, opts).map(|mgr| NmRuntimes { mgr }).map_err(conv_err)
    }
}

fn conv_err(e: nm_runtimes::Error) -> RtError {
    let b = e.to_api().error;
    RtError { code: b.code.to_string(), message: b.message, message_id: b.message_id }
}

fn rk(kind: &str) -> Result<nm_runtimes::RuntimeKind, RtError> {
    nm_runtimes::RuntimeKind::parse(kind).ok_or_else(|| RtError::new("bad_kind", "unknown runtime kind", "jenis runtime tidak dikenal"))
}

fn conv_installed(r: &nm_runtimes::InstalledRuntime) -> InstalledRt {
    let path = PathBuf::from(&r.path);
    InstalledRt {
        kind: r.kind.dir_name().to_string(),
        version: r.version.clone(),
        dir_name: path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| r.version.clone()),
        path,
        size_mb: r.size_mb,
        tsnts: r.tsnts.clone(),
        arch: r.arch.clone(),
        full_version: r.full_version.clone(),
    }
}

impl RuntimeApi for NmRuntimes {
    fn list_installed(&self, kind: &str) -> Vec<InstalledRt> {
        match rk(kind) {
            Ok(k) => self.mgr.list_installed(k).iter().map(conv_installed).collect(),
            Err(_) => vec![],
        }
    }

    fn overview(&self, kind: &str, channel: &str, variant: &str, force: bool) -> BoxFuture<'static, Result<Value, RtError>> {
        let mgr = self.mgr.clone();
        let (kind, channel, variant) = (kind.to_string(), channel.to_string(), variant.to_string());
        Box::pin(async move {
            let k = rk(&kind)?;
            let ch = nm_runtimes::Channel::parse(&channel).unwrap_or_default();
            let var = nm_runtimes::Variant::parse(&variant).unwrap_or_default();
            let ov = mgr.overview(k, ch, var, force).await.map_err(conv_err)?;
            serde_json::to_value(ov).map_err(|e| RtError::new("parse", &e.to_string(), &e.to_string()))
        })
    }

    fn resolve_latest(&self, kind: &str, channel: &str) -> BoxFuture<'static, Result<Option<Value>, RtError>> {
        let mgr = self.mgr.clone();
        let (kind, channel) = (kind.to_string(), channel.to_string());
        Box::pin(async move {
            let k = rk(&kind)?;
            let ch = nm_runtimes::Channel::parse(&channel).unwrap_or_default();
            let r = mgr.resolve_latest(k, ch, nm_runtimes::Variant::Nts).await.map_err(conv_err)?;
            Ok(r.and_then(|a| serde_json::to_value(a).ok()))
        })
    }

    fn install(&self, kind: &str, spec: &str, variant: Option<String>, progress: ProgressCb) -> BoxFuture<'static, Result<InstalledRt, RtError>> {
        let mgr = self.mgr.clone();
        let (kind, spec) = (kind.to_string(), spec.to_string());
        Box::pin(async move {
            let k = rk(&kind)?;
            let v = variant.as_deref().and_then(nm_runtimes::Variant::parse).unwrap_or_default();
            let cb: nm_runtimes::ProgressFn = Arc::new(move |p: nm_runtimes::Progress| progress(p.progress, p.message.clone()));
            let out = mgr.install(k, &spec, v, cb, nm_runtimes::CancelToken::new()).await.map_err(conv_err)?;
            Ok(conv_installed(&out.runtime))
        })
    }

    fn uninstall(&self, kind: &str, dir_name: &str, in_use: &dyn Fn(&InstalledRt) -> bool) -> Result<(), RtError> {
        let k = rk(kind)?;
        let inst = self
            .mgr
            .list_installed(k)
            .into_iter()
            .map(|r| (r.clone(), conv_installed(&r)))
            .find(|(_, c)| c.dir_name == dir_name)
            .ok_or_else(|| RtError::new("not_found", "runtime not installed", "runtime tidak terpasang"))?;
        let variant = nm_runtimes::Variant::parse(&inst.0.tsnts).unwrap_or_default();
        self.mgr
            .uninstall(k, &inst.0.version, variant, &|r| in_use(&conv_installed(r)))
            .map_err(conv_err)
    }

    fn health_check(&self, kind: &str, dir: &Path) -> BoxFuture<'static, Value> {
        let mgr = self.mgr.clone();
        let (kind, dir) = (kind.to_string(), dir.to_path_buf());
        Box::pin(async move {
            match rk(&kind) {
                Ok(k) => serde_json::to_value(mgr.health_check_path(k, &dir).await).unwrap_or(Value::Null),
                Err(e) => serde_json::json!({"ok": false, "error": e.message}),
            }
        })
    }

    fn cleanup_stale(&self) -> usize {
        self.mgr.cleanup_stale()
    }
}

// ---------------------------------------------------------------- offline stub (folder scan only)

pub struct FsRuntimes {
    pub root: PathBuf,
}

impl RuntimeApi for FsRuntimes {
    fn list_installed(&self, kind: &str) -> Vec<InstalledRt> {
        let mut v: Vec<InstalledRt> = vec![];
        if let Ok(rd) = std::fs::read_dir(self.root.join(kind)) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || !e.path().join(exe_name(kind)).is_file() {
                    continue;
                }
                let ts = name.ends_with("-ts");
                v.push(InstalledRt {
                    kind: kind.into(),
                    version: name.trim_end_matches("-ts").into(),
                    dir_name: name.clone(),
                    path: e.path(),
                    size_mb: crate::util::dir_size(&e.path()) as f64 / 1048576.0,
                    tsnts: if kind == "php" { (if ts { "ts" } else { "nts" }).into() } else { String::new() },
                    arch: "x64".into(),
                    full_version: name,
                });
            }
        }
        v.sort_by(|a, b| crate::util::cmp_version(&b.version, &a.version));
        v
    }
    fn overview(&self, kind: &str, _c: &str, _v: &str, _f: bool) -> BoxFuture<'static, Result<Value, RtError>> {
        let installed: Vec<Value> = self.list_installed(kind).iter().map(|i| i.to_json()).collect();
        Box::pin(async move {
            Ok(serde_json::json!({"installed": installed, "available": [], "latest": null, "updateAvailable": false, "offline": true, "stale": false}))
        })
    }
    fn resolve_latest(&self, _k: &str, _c: &str) -> BoxFuture<'static, Result<Option<Value>, RtError>> {
        Box::pin(async { Err(offline()) })
    }
    fn install(&self, _k: &str, _s: &str, _v: Option<String>, _p: ProgressCb) -> BoxFuture<'static, Result<InstalledRt, RtError>> {
        Box::pin(async { Err(offline()) })
    }
    fn uninstall(&self, kind: &str, dir_name: &str, in_use: &dyn Fn(&InstalledRt) -> bool) -> Result<(), RtError> {
        let inst = self
            .list_installed(kind)
            .into_iter()
            .find(|i| i.dir_name == dir_name)
            .ok_or_else(|| RtError::new("not_found", "runtime not installed", "runtime tidak terpasang"))?;
        if in_use(&inst) {
            return Err(RtError::new("in_use", "runtime is in use", "runtime sedang dipakai"));
        }
        std::fs::remove_dir_all(&inst.path).map_err(|e| RtError::new("io", &e.to_string(), &e.to_string()))
    }
    fn health_check(&self, kind: &str, dir: &Path) -> BoxFuture<'static, Value> {
        let exe = dir.join(exe_name(kind));
        let kind = kind.to_string();
        Box::pin(async move {
            let mut c = std::process::Command::new(&exe);
            c.arg(if kind == "node" { "-v" } else { "-v" });
            crate::util::hide_std(&mut c);
            match c.output() {
                Ok(o) if o.status.success() => serde_json::json!({"ok": true, "version": String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("")}),
                Ok(o) => serde_json::json!({"ok": false, "exitCode": o.status.code()}),
                Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
            }
        })
    }
}

fn offline() -> RtError {
    RtError::new("unavailable", "runtime manager is not available (offline stub)", "pengelola runtime tidak tersedia (stub offline)")
}
