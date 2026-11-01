//! `nm_runtimes` — PHP / Node.js runtime manager for NextMON Server (Windows x64).
//!
//! Pure library: no UI, no HTTP server. All network calls go through `RuntimeManager`; long operations
//! take a `ProgressFn` and a `CancelToken`. Types serialise to the JSON shapes of `/api/runtimes/*`.
//!
//! Layout under `root` (the `runtimes/` folder next to the exe):
//! ```text
//! php/<ver>[-ts]/   node/<ver>/   .cache/*.json   .downloads/   cacert.pem
//! ```
pub mod archive;
pub mod error;
pub mod health;
pub mod http;
pub mod ini;
pub mod node;
pub mod php;
pub mod types;
pub mod version;

pub use error::{ApiError, ApiErrorBody, Error, Result};
pub use health::VC_REDIST_URL;
pub use types::*;
pub use version::{compare_versions, is_newer, latest_patch_for, upgrade_kind, UpgradeKind, Version};

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

const META_FILE: &str = ".nextmon.json";

/// Install metadata stored inside each runtime folder.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    full_version: String,
    installed_at: u64,
    url: String,
    sha256: Option<String>,
    verified: bool,
}

/// Result of listing the online catalogue.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub items: Vec<AvailableRuntime>,
    pub offline: bool,
    pub stale: bool,
}

struct Inner {
    root: PathBuf,
    opts: Options,
    http: http::Http,
    php_lock: Arc<Mutex<()>>,
    node_lock: Arc<Mutex<()>>,
}

/// Cheap to clone (Arc inside). Share one instance across the server.
#[derive(Clone)]
pub struct RuntimeManager {
    inner: Arc<Inner>,
}

fn nanos() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}

/// Removes a directory on drop unless disarmed (failure / cancel cleanup).
struct DirGuard(Option<PathBuf>);
impl DirGuard {
    fn disarm(&mut self) {
        self.0 = None;
    }
}
impl Drop for DirGuard {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            let _ = std::fs::remove_dir_all(p);
        }
    }
}

impl RuntimeManager {
    /// `root` is the `runtimes/` directory (created on demand).
    pub fn new(root: impl Into<PathBuf>, opts: Options) -> Result<Self> {
        let root = root.into();
        let http = http::Http::new(&opts)?;
        Ok(RuntimeManager {
            inner: Arc::new(Inner { root, opts, http, php_lock: Arc::new(Mutex::new(())), node_lock: Arc::new(Mutex::new(())) }),
        })
    }

    pub fn root(&self) -> &Path {
        &self.inner.root
    }
    pub fn options(&self) -> &Options {
        &self.inner.opts
    }
    pub fn kind_dir(&self, kind: RuntimeKind) -> PathBuf {
        self.inner.root.join(kind.dir_name())
    }
    fn cache_dir(&self) -> PathBuf {
        self.inner.root.join(".cache")
    }
    fn downloads_dir(&self) -> PathBuf {
        self.inner.root.join(".downloads")
    }
    pub fn cacert_path(&self) -> PathBuf {
        self.inner.root.join("cacert.pem")
    }
    fn lock_for(&self, kind: RuntimeKind) -> Arc<Mutex<()>> {
        match kind {
            RuntimeKind::Php => self.inner.php_lock.clone(),
            RuntimeKind::Node => self.inner.node_lock.clone(),
        }
    }

    // ------------------------------------------------------------------ installed

    /// Installed runtimes of `kind`, newest first. Sync (does a directory scan + size walk).
    pub fn list_installed(&self, kind: RuntimeKind) -> Vec<InstalledRuntime> {
        let mut out = vec![];
        let Ok(rd) = std::fs::read_dir(self.kind_dir(kind)) else { return out };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || !e.path().join(kind.exe_name()).is_file() {
                continue;
            }
            let (version, variant) = match kind {
                RuntimeKind::Php => match php::parse_dir_name(&name) {
                    Some(x) => x,
                    None => continue,
                },
                RuntimeKind::Node => match Version::parse(&name) {
                    Some(v) => (v.short(), Variant::Nts),
                    None => continue,
                },
            };
            let meta: Option<Meta> = std::fs::read_to_string(e.path().join(META_FILE)).ok().and_then(|s| serde_json::from_str(&s).ok());
            out.push(InstalledRuntime {
                kind,
                full_version: meta.map(|m| m.full_version).unwrap_or_else(|| version.clone()),
                version,
                path: e.path().to_string_lossy().into_owned(),
                active: false,
                size_mb: ((archive::dir_size(&e.path()) as f64 / 1_048_576.0) * 10.0).round() / 10.0,
                tsnts: if kind == RuntimeKind::Php { variant.as_str().into() } else { String::new() },
                arch: "x64".into(),
                used_by: vec![],
            });
        }
        out.sort_by(|a, b| compare_versions(&b.full_version, &a.full_version));
        out
    }

    /// Async wrapper of `list_installed` (runs on the blocking pool).
    pub async fn list_installed_async(&self, kind: RuntimeKind) -> Vec<InstalledRuntime> {
        let me = self.clone();
        tokio::task::spawn_blocking(move || me.list_installed(kind)).await.unwrap_or_default()
    }

    pub fn find_installed(&self, kind: RuntimeKind, version: &str, variant: Variant) -> Option<InstalledRuntime> {
        let want = Version::parse(version)?.short();
        self.list_installed(kind).into_iter().find(|r| r.version == want && (kind == RuntimeKind::Node || r.tsnts == variant.as_str()))
    }

    fn target_dir(&self, kind: RuntimeKind, version: &str, variant: Variant) -> PathBuf {
        let name = match kind {
            RuntimeKind::Php => php::dir_name(version, variant),
            RuntimeKind::Node => version.to_string(),
        };
        self.kind_dir(kind).join(name)
    }

    // ------------------------------------------------------------------ available

    /// Online catalogue for a channel. Cached 6 h in `.cache/`; stale cache is served when offline.
    /// PHP: latest patch of each supported series (variant picks NTS/TS). Node: see `node::select_channel`.
    pub async fn list_available(&self, kind: RuntimeKind, channel: Channel, variant: Variant, force: bool) -> Result<Listing> {
        let (mut items, offline, stale) = match kind {
            RuntimeKind::Php => {
                let c = self.fetch_php_releases(force).await?;
                let o = &self.inner.opts;
                let base = o.php_releases_url.rsplit_once('/').map(|x| x.0).unwrap_or("").to_string();
                (php::parse_releases(&c.body, &base, variant, o)?, c.offline, c.stale)
            }
            RuntimeKind::Node => {
                let (all, off, st) = self.fetch_node_index(force).await?;
                (node::select_channel(&all, channel, &self.inner.opts), off, st)
            }
        };
        self.mark_newer(kind, variant, &mut items);
        Ok(Listing { items, offline, stale })
    }

    async fn fetch_php_releases(&self, force: bool) -> Result<http::Cached> {
        let o = &self.inner.opts;
        self.inner.http.get_cached(&self.cache_dir(), "php-releases", &o.php_releases_url, o.cache_ttl_secs, force).await
    }

    async fn fetch_node_index(&self, force: bool) -> Result<(Vec<AvailableRuntime>, bool, bool)> {
        let o = &self.inner.opts;
        let url = format!("{}/index.json", o.node_dist_url.trim_end_matches('/'));
        let c = self.inner.http.get_cached(&self.cache_dir(), "node-index", &url, o.cache_ttl_secs, force).await?;
        Ok((node::parse_index(&c.body, &o.node_dist_url)?, c.offline, c.stale))
    }

    /// `isNewer`: newer than an installed runtime of the same series, or newer than everything installed.
    fn mark_newer(&self, kind: RuntimeKind, variant: Variant, items: &mut [AvailableRuntime]) {
        let inst: Vec<Version> = self
            .list_installed(kind)
            .iter()
            .filter(|r| kind == RuntimeKind::Node || r.tsnts == variant.as_str())
            .filter_map(|r| Version::parse(&r.full_version))
            .collect();
        let Some(max) = inst.iter().max().copied() else { return };
        for it in items.iter_mut() {
            if let Some(v) = Version::parse(&it.full_version) {
                it.is_newer = v > max || inst.iter().any(|i| {
                    let same = if kind == RuntimeKind::Php { i.same_minor(&v) } else { i.major == v.major };
                    same && v > *i
                });
            }
        }
    }

    /// Resolve a spec to a concrete downloadable release.
    /// PHP: "latest"/"stable", "8.4" (newest patch), "8.4.27" (falls back to the archives/ index, unverified).
    /// Node: "lts", "latest"/"current", "22", "22.11", "22.11.0" (optional leading "v").
    pub async fn resolve(&self, kind: RuntimeKind, spec: &str, variant: Variant) -> Result<AvailableRuntime> {
        let o = &self.inner.opts;
        let s = spec.trim().to_ascii_lowercase();
        match kind {
            RuntimeKind::Node => {
                let (all, _, _) = self.fetch_node_index(false).await?;
                node::resolve_spec(&all, &s, o).cloned().ok_or_else(|| Error::NotFound(format!("Node.js '{spec}'")))
            }
            RuntimeKind::Php => {
                let listing = self.list_available(kind, Channel::Stable, variant, false).await?;
                if matches!(s.as_str(), "" | "latest" | "stable") {
                    return listing.items.first().cloned().ok_or_else(|| Error::NotFound("no supported PHP release".into()));
                }
                let want = Version::parse(&s).ok_or_else(|| Error::Parse(format!("bad PHP version '{spec}'")))?;
                if !php::series_allowed(&want, o) {
                    return Err(Error::Unsupported(format!("PHP {}.{} is outside the supported series (min {})", want.major, want.minor, o.min_php)));
                }
                let parts = s.split(|c| c == '.').count();
                if parts <= 2 {
                    return listing
                        .items
                        .into_iter()
                        .find(|r| Version::parse(&r.version).map(|v| v.same_minor(&want)).unwrap_or(false))
                        .ok_or_else(|| Error::NotFound(format!("PHP {spec}")));
                }
                if let Some(r) = listing.items.iter().find(|r| {
                    Version::parse(&r.full_version).map(|v| v.short() == want.short() && (want.rev == 0 || v.rev == want.rev)).unwrap_or(false)
                }) {
                    return Ok(r.clone());
                }
                // Older patch: scrape archives/.
                let c = self.inner.http.get_cached(&self.cache_dir(), "php-archives", &o.php_archives_url, o.cache_ttl_secs, false).await?;
                php::find_in_archives(&c.body, &o.php_archives_url, &want, variant)
                    .ok_or_else(|| Error::NotFound(format!("PHP {spec} ({}) not in releases or archives", variant.as_str())))
            }
        }
    }

    /// Newest release of a channel.
    pub async fn resolve_latest(&self, kind: RuntimeKind, channel: Channel, variant: Variant) -> Result<Option<AvailableRuntime>> {
        Ok(self.list_available(kind, channel, variant, false).await?.items.into_iter().next())
    }

    /// Everything `GET /api/runtimes/{kind}` needs (installed + available + latest + update flags).
    pub async fn overview(&self, kind: RuntimeKind, channel: Channel, variant: Variant, force: bool) -> Result<Overview> {
        let installed = self.list_installed_async(kind).await;
        let (listing, offline) = match self.list_available(kind, channel, variant, force).await {
            Ok(l) => {
                let off = l.offline;
                (l, off)
            }
            Err(_) => (Listing { items: vec![], offline: true, stale: false }, true),
        };
        let (update_available, update_kind) = update_state(&installed, &listing.items, kind, variant);
        Ok(Overview {
            latest: listing.items.first().cloned(),
            update_available,
            update_kind,
            offline,
            stale: listing.stale,
            installed,
            available: listing.items,
        })
    }

    /// Update check relative to the newest installed runtime.
    pub async fn check_update(&self, kind: RuntimeKind, channel: Channel, variant: Variant) -> Result<UpdateInfo> {
        let installed = self.list_installed_async(kind).await;
        let listing = self.list_available(kind, channel, variant, false).await?;
        let (update_available, update_kind) = update_state(&installed, &listing.items, kind, variant);
        Ok(UpdateInfo {
            installed: installed.iter().find(|r| kind == RuntimeKind::Node || r.tsnts == variant.as_str()).map(|r| r.full_version.clone()),
            latest: listing.items.first().cloned(),
            update_available,
            update_kind,
        })
    }

    // ------------------------------------------------------------------ install

    /// Download, verify (sha256, BEFORE extracting), extract atomically, configure, health-check.
    /// Fails with `Busy` if another install/uninstall of the same kind is running.
    /// On failure or cancel the temp folder is removed; the partial download is kept for resume
    /// (a checksum-mismatching file is deleted).
    pub async fn install(
        &self,
        kind: RuntimeKind,
        spec: &str,
        variant: Variant,
        progress: ProgressFn,
        cancel: CancelToken,
    ) -> Result<InstallOutcome> {
        let lock = self.lock_for(kind);
        let _guard = lock.try_lock_owned().map_err(|_| Error::Busy(kind.dir_name().into()))?;
        let o = &self.inner.opts;
        let variant = if kind == RuntimeKind::Node { Variant::Nts } else { variant };
        let emit = |step: Step, p: f64, bytes: u64, total: Option<u64>, msg: &str| {
            progress(Progress { step, bytes, total, progress: p.clamp(0.0, 1.0), message: msg.to_string() })
        };

        emit(Step::Resolve, 0.0, 0, None, "Resolving version");
        cancel.check()?;
        let rel = self.resolve(kind, spec, variant).await?;
        let target = self.target_dir(kind, &rel.version, variant);
        if target.join(kind.exe_name()).is_file() {
            return Err(Error::AlreadyInstalled(format!("{} {}", kind.dir_name(), rel.version)));
        }
        let kind_dir = self.kind_dir(kind);
        tokio::fs::create_dir_all(&kind_dir).await?;

        // Expected checksum.
        let file_name = http::file_name_of_url(&rel.url);
        let expected: Option<String> = match kind {
            RuntimeKind::Php => rel.sha256.clone(),
            RuntimeKind::Node => {
                let t = self.inner.http.get_text(&node::shasums_url(&o.node_dist_url, &rel.version)).await?;
                Some(node::find_sha(&t, &file_name).ok_or_else(|| Error::NoChecksum(file_name.clone()))?)
            }
        };
        if expected.is_none() && !o.allow_unverified {
            return Err(Error::NoChecksum(file_name));
        }

        // Download (resumable) unless a verified archive from a previous run is present.
        let dl_dir = self.downloads_dir();
        let zip_path = dl_dir.join(&file_name);
        let part_path = dl_dir.join(format!("{file_name}.part"));
        tokio::fs::create_dir_all(&dl_dir).await?;
        let mut have_zip = false;
        if let (true, Some(exp)) = (zip_path.is_file(), &expected) {
            let (zp, c) = (zip_path.clone(), cancel.clone());
            let got = tokio::task::spawn_blocking(move || archive::sha256_file(&zp, &c)).await.map_err(|e| Error::Io(e.to_string()))??;
            if got.eq_ignore_ascii_case(exp) {
                have_zip = true;
            } else {
                let _ = tokio::fs::remove_file(&zip_path).await;
            }
        }
        if !have_zip {
            let prog = progress.clone();
            let name = format!("Downloading {}", rel.url);
            self.inner
                .http
                .download(&rel.url, &part_path, &cancel, &move |b, t| {
                    let frac = t.map(|t| if t > 0 { b as f64 / t as f64 } else { 0.0 }).unwrap_or(0.0);
                    prog(Progress { step: Step::Download, bytes: b, total: t, progress: 0.02 + 0.68 * frac, message: name.clone() });
                })
                .await?;

            // Verify BEFORE extracting.
            emit(Step::Verify, 0.70, 0, None, "Verifying sha256");
            if let Some(exp) = &expected {
                let (pp, c) = (part_path.clone(), cancel.clone());
                let got = tokio::task::spawn_blocking(move || archive::sha256_file(&pp, &c)).await.map_err(|e| Error::Io(e.to_string()))??;
                if !got.eq_ignore_ascii_case(exp) {
                    let _ = tokio::fs::remove_file(&part_path).await;
                    return Err(Error::ChecksumMismatch { expected: exp.clone(), actual: got });
                }
            }
            tokio::fs::rename(&part_path, &zip_path).await?;
        }
        cancel.check()?;

        // Extract into a temp dir next to the target, then rename (atomic on the same volume).
        let tmp = kind_dir.join(format!(".tmp-{}-{}", rel.version, nanos()));
        let mut guard = DirGuard(Some(tmp.clone()));
        emit(Step::Extract, 0.75, 0, None, "Extracting");
        {
            let (zp, tp, c, prog) = (zip_path.clone(), tmp.clone(), cancel.clone(), progress.clone());
            tokio::task::spawn_blocking(move || {
                archive::extract_zip(&zp, &tp, true, &c, &|d, t| {
                    let f = if t > 0 { d as f64 / t as f64 } else { 0.0 };
                    prog(Progress { step: Step::Extract, bytes: d, total: Some(t), progress: 0.75 + 0.17 * f, message: "Extracting".into() });
                })
            })
            .await
            .map_err(|e| Error::Io(e.to_string()))??;
        }
        if !tmp.join(kind.exe_name()).is_file() {
            return Err(Error::Archive(format!("{} not found in archive", kind.exe_name())));
        }

        if kind == RuntimeKind::Php {
            emit(Step::Configure, 0.92, 0, None, "Creating php.ini");
            if let Err(e) = self.ensure_cacert().await {
                // Non-fatal: PHP works without a CA bundle for non-TLS use; ini just omits cainfo.
                progress(Progress { step: Step::Configure, bytes: 0, total: None, progress: 0.93, message: format!("cacert.pem unavailable: {e}") });
            }
            let t2 = tmp.clone();
            let me = self.clone();
            tokio::task::spawn_blocking(move || me.configure_php_ini(&t2, None)).await.map_err(|e| Error::Io(e.to_string()))??;
        }
        cancel.check()?;

        emit(Step::HealthCheck, 0.95, 0, None, "Health check");
        let mut health = self.health_check_path(kind, &tmp).await;
        if !health.ok && !health.vc_runtime_missing && health.version.is_none() {
            // Could not even run: do not keep a broken install.
            let why = health.diagnostics.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("; ");
            return Err(Error::Health(why));
        }

        let meta = Meta {
            full_version: rel.full_version.clone(),
            installed_at: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
            url: rel.url.clone(),
            sha256: expected.clone(),
            verified: expected.is_some(),
        };
        tokio::fs::write(tmp.join(META_FILE), serde_json::to_vec_pretty(&meta)?).await?;

        emit(Step::Finalize, 0.98, 0, None, "Finalizing");
        let mut renamed = false;
        for i in 0..6 {
            match tokio::fs::rename(&tmp, &target).await {
                Ok(()) => {
                    renamed = true;
                    break;
                }
                Err(e) if i == 5 => return Err(e.into()),
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(250)).await, // AV scanners may hold files briefly
            }
        }
        if renamed {
            guard.disarm();
        }
        if !o.keep_downloads {
            let _ = tokio::fs::remove_file(&zip_path).await;
        }
        // Re-run the health check at the final location (paths in php.ini are relative / absolute-to-cacert only).
        let runtime = self
            .find_installed(kind, &rel.version, variant)
            .ok_or_else(|| Error::Io("installed runtime not found after rename".into()))?;
        health.path = runtime.path.clone();
        emit(Step::Done, 1.0, 0, None, "Done");
        Ok(InstallOutcome { runtime, health, verified: expected.is_some() })
    }

    // ------------------------------------------------------------------ uninstall

    /// Remove an installed runtime. `in_use` is asked first; returning true aborts with `InUse`.
    /// The folder is renamed away first (fails cleanly if files are locked), then deleted.
    pub fn uninstall(&self, kind: RuntimeKind, version: &str, variant: Variant, in_use: &dyn Fn(&InstalledRuntime) -> bool) -> Result<()> {
        let lock = self.lock_for(kind);
        let _g = lock.try_lock().map_err(|_| Error::Busy(kind.dir_name().into()))?;
        let rt = self
            .find_installed(kind, version, variant)
            .ok_or_else(|| Error::NotFound(format!("{} {version}", kind.dir_name())))?;
        if in_use(&rt) {
            return Err(Error::InUse(format!("{} {}", kind.dir_name(), rt.version)));
        }
        let src = PathBuf::from(&rt.path);
        let trash = self.kind_dir(kind).join(format!(".trash-{}-{}", rt.version, nanos()));
        std::fs::rename(&src, &trash).map_err(|e| Error::Io(format!("cannot remove {} (files in use?): {e}", src.display())))?;
        if std::fs::remove_dir_all(&trash).is_err() {
            // Rare partial failure: leave the trash folder; cleanup_stale() retries later.
        }
        Ok(())
    }

    /// Delete leftovers of interrupted installs/uninstalls (`.tmp-*`, `.trash-*`). Call at server start.
    pub fn cleanup_stale(&self) -> usize {
        let mut n = 0;
        for kind in [RuntimeKind::Php, RuntimeKind::Node] {
            if let Ok(rd) = std::fs::read_dir(self.kind_dir(kind)) {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if (name.starts_with(".tmp-") || name.starts_with(".trash-")) && std::fs::remove_dir_all(e.path()).is_ok() {
                        n += 1;
                    }
                }
            }
        }
        n
    }

    // ------------------------------------------------------------------ health / php.ini

    /// Health check of an installed runtime by version.
    pub async fn health_check(&self, kind: RuntimeKind, version: &str, variant: Variant) -> Result<HealthReport> {
        let rt = self.find_installed(kind, version, variant).ok_or_else(|| Error::NotFound(format!("{} {version}", kind.dir_name())))?;
        Ok(self.health_check_path(kind, Path::new(&rt.path)).await)
    }

    /// Health check of any folder containing php.exe / node.exe.
    pub async fn health_check_path(&self, kind: RuntimeKind, dir: &Path) -> HealthReport {
        match kind {
            RuntimeKind::Php => health::check_php(dir, &self.inner.opts.php_required_extensions, self.inner.opts.php_ini.opcache).await,
            RuntimeKind::Node => health::check_node(dir).await,
        }
    }

    /// (Re)generate `php.ini` in `dir` from `php.ini-production`. `overrides` replaces `Options::php_ini`
    /// (e.g. to apply the app's `opcache` / `extraIni` settings). Sync.
    pub fn configure_php_ini(&self, dir: &Path, overrides: Option<&IniOptions>) -> Result<()> {
        let mut o = overrides.cloned().unwrap_or_else(|| self.inner.opts.php_ini.clone());
        let ca = self.cacert_path();
        if o.cacert.is_none() && ca.is_file() {
            o.cacert = Some(ca.to_string_lossy().into_owned());
        }
        let src_path = ["php.ini-production", "php.ini-development"].iter().map(|n| dir.join(n)).find(|p| p.is_file());
        let src_path = src_path.ok_or_else(|| Error::NotFound("php.ini-production".into()))?;
        let src = std::fs::read_to_string(&src_path)?;
        let dlls: HashSet<String> = std::fs::read_dir(dir.join("ext"))
            .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_ascii_lowercase()).collect())
            .unwrap_or_default();
        let out = ini::transform_php_ini(&src, &o, &dlls);
        let tmp = dir.join("php.ini.tmp");
        std::fs::write(&tmp, out)?;
        std::fs::rename(&tmp, dir.join("php.ini"))?;
        Ok(())
    }

    /// Make sure `runtimes/cacert.pem` exists (downloaded once from curl.se; refreshed when older than 180 days).
    pub async fn ensure_cacert(&self) -> Result<PathBuf> {
        let p = self.cacert_path();
        let fresh = std::fs::metadata(&p)
            .ok()
            .filter(|m| m.len() > 1000)
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .map(|age| age.as_secs() < 180 * 86400)
            .unwrap_or(false);
        if fresh {
            return Ok(p);
        }
        match self.inner.http.get_text(&self.inner.opts.cacert_url).await {
            Ok(body) if body.contains("BEGIN CERTIFICATE") => {
                tokio::fs::create_dir_all(&self.inner.root).await?;
                let tmp = p.with_extension("pem.tmp");
                tokio::fs::write(&tmp, body).await?;
                tokio::fs::rename(&tmp, &p).await?;
                Ok(p)
            }
            Ok(_) => if p.is_file() { Ok(p) } else { Err(Error::Parse("cacert.pem has no certificates".into())) },
            Err(e) => if p.is_file() { Ok(p) } else { Err(e) },
        }
    }
}

fn update_state(installed: &[InstalledRuntime], avail: &[AvailableRuntime], kind: RuntimeKind, variant: Variant) -> (bool, UpgradeKind) {
    let newest = installed
        .iter()
        .filter(|r| kind == RuntimeKind::Node || r.tsnts == variant.as_str())
        .filter_map(|r| Version::parse(&r.full_version))
        .max();
    let Some(newest) = newest else { return (false, UpgradeKind::None) };
    let any = avail.iter().any(|a| a.is_newer);
    if !any {
        return (false, UpgradeKind::None);
    }
    let patch_available = avail.iter().filter_map(|a| Version::parse(&a.full_version)).any(|v| {
        let same = if kind == RuntimeKind::Php { v.same_minor(&newest) } else { v.major == newest.major };
        same && v > newest
    });
    if patch_available {
        return (true, UpgradeKind::Patch);
    }
    let top = avail.iter().filter_map(|a| Version::parse(&a.full_version)).max();
    (true, top.map(|t| newest.upgrade_kind(&t)).unwrap_or(UpgradeKind::None))
}
