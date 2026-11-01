//! Public, serde-serialisable types (JSON shapes match `/api/runtimes/{php|node}` in SERVER-SPEC).
use crate::version::UpgradeKind;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeKind {
    Php,
    Node,
}

impl RuntimeKind {
    pub fn dir_name(&self) -> &'static str {
        match self {
            RuntimeKind::Php => "php",
            RuntimeKind::Node => "node",
        }
    }
    pub fn exe_name(&self) -> &'static str {
        match self {
            RuntimeKind::Php => "php.exe",
            RuntimeKind::Node => "node.exe",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "php" => Some(RuntimeKind::Php),
            "node" | "nodejs" => Some(RuntimeKind::Node),
            _ => None,
        }
    }
}

/// PHP thread-safety variant (ignored for Node).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Variant {
    #[default]
    Nts,
    Ts,
}

impl Variant {
    pub fn as_str(&self) -> &'static str {
        match self {
            Variant::Nts => "nts",
            Variant::Ts => "ts",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "nts" | "" => Some(Variant::Nts),
            "ts" => Some(Variant::Ts),
            _ => None,
        }
    }
}

/// Release channel. PHP only has `Stable`; Node has `Lts` / `Current`. `All` lists everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Stable,
    Lts,
    Current,
    All,
}

impl Channel {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "stable" | "" => Some(Channel::Stable),
            "lts" => Some(Channel::Lts),
            "current" | "latest" => Some(Channel::Current),
            "all" => Some(Channel::All),
            _ => None,
        }
    }
}

/// One downloadable runtime (an entry of `available` in the API).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableRuntime {
    /// Short version without packaging revision, e.g. "8.4.27" / "24.12.0". Used as install dir name.
    pub version: String,
    /// Full upstream version, e.g. "8.4.27-1" (same as `version` for Node).
    pub full_version: String,
    /// "stable" (PHP) | "lts" | "current"
    pub channel: String,
    /// LTS codename for Node (e.g. "Krypton").
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub lts_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub date: Option<String>,
    pub url: String,
    /// Known up-front for PHP releases.json; for Node it is read from SHASUMS256.txt at install time.
    pub sha256: Option<String>,
    #[serde(rename = "sizeMB")]
    pub size_mb: Option<f64>,
    pub is_newer: bool,
    /// "nts" | "ts" (PHP) ; "" for Node
    pub tsnts: String,
    /// PHP build toolchain, e.g. "vs17" / "vs16" (empty for Node).
    #[serde(default)]
    pub build: String,
    /// True when no sha256 is published (PHP archives); install needs `Options::allow_unverified`.
    #[serde(default)]
    pub unverified: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledRuntime {
    pub kind: RuntimeKind,
    /// Directory-name version without revision ("8.4.27").
    pub version: String,
    /// Upstream full version from install metadata ("8.4.27-1"); falls back to `version`.
    pub full_version: String,
    pub path: String,
    /// Filled by the caller (server knows the config).
    #[serde(default)]
    pub active: bool,
    #[serde(rename = "sizeMB")]
    pub size_mb: f64,
    /// "nts" | "ts" (PHP); "" for Node.
    pub tsnts: String,
    pub arch: String,
    /// Filled by the caller: app names using this runtime.
    #[serde(default)]
    pub used_by: Vec<String>,
}

impl InstalledRuntime {
    pub fn exe_path(&self) -> std::path::PathBuf {
        std::path::Path::new(&self.path).join(self.kind.exe_name())
    }
}

/// Response of `/api/runtimes/{php|node}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub installed: Vec<InstalledRuntime>,
    pub available: Vec<AvailableRuntime>,
    pub latest: Option<AvailableRuntime>,
    pub update_available: bool,
    /// Patch / Minor / Major relative to the newest installed version of the same series.
    pub update_kind: UpgradeKind,
    /// Online list could not be fetched; `available` comes from cache (or is empty).
    pub offline: bool,
    /// Cache older than TTL was served.
    pub stale: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Step {
    Resolve,
    Download,
    Verify,
    Extract,
    Configure,
    HealthCheck,
    Finalize,
    Done,
}

/// Progress event delivered to `ProgressFn`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub step: Step,
    pub bytes: u64,
    pub total: Option<u64>,
    /// Overall 0..=1 across all steps.
    pub progress: f64,
    pub message: String,
}

pub type ProgressFn = Arc<dyn Fn(Progress) + Send + Sync>;

pub fn noop_progress() -> ProgressFn {
    Arc::new(|_| {})
}

/// Cooperative cancellation. Clone freely; `cancel()` from any thread.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    pub(crate) fn check(&self) -> crate::Result<()> {
        if self.is_cancelled() {
            Err(crate::Error::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    /// e.g. "vc_runtime_missing", "missing_extension", "spawn_failed", "bad_exit", "startup_warning"
    pub code: String,
    pub severity: Severity,
    pub message: String,
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hint_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthReport {
    pub ok: bool,
    pub kind: RuntimeKind,
    pub path: String,
    pub version: Option<String>,
    /// Node only.
    pub npm_version: Option<String>,
    pub exit_code: Option<i64>,
    /// PHP: lowercase module names from `php -m`.
    pub loaded_extensions: Vec<String>,
    /// Required extensions that are absent.
    pub missing_extensions: Vec<String>,
    /// Optional extensions (intl, opcache) that are absent.
    pub optional_missing: Vec<String>,
    pub vc_runtime_missing: bool,
    pub vc_redist_url: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Result of a completed install.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallOutcome {
    pub runtime: InstalledRuntime,
    pub health: HealthReport,
    /// sha256 was checked against a published checksum.
    pub verified: bool,
}

/// Result of `check_update`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub installed: Option<String>,
    pub latest: Option<AvailableRuntime>,
    pub update_available: bool,
    pub update_kind: UpgradeKind,
}

/// Settings written into php.ini.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct IniOptions {
    pub timezone: String,
    pub memory_limit: String,
    pub upload_max_filesize: String,
    pub post_max_size: String,
    pub max_execution_time: u32,
    pub realpath_cache_size: String,
    /// Load opcache (zend_extension) when php_opcache.dll exists; `opcache.enable_cli=0` always.
    pub opcache: bool,
    /// Extensions to enable (only those whose DLL exists in ext/ are enabled).
    pub extensions: Vec<String>,
    /// Absolute path of cacert.pem (sets curl.cainfo and openssl.cafile).
    pub cacert: Option<String>,
    /// Arbitrary extra directives (applied last, override everything).
    pub extra: BTreeMap<String, String>,
}

impl Default for IniOptions {
    fn default() -> Self {
        IniOptions {
            timezone: "Asia/Jakarta".into(),
            memory_limit: "512M".into(),
            upload_max_filesize: "64M".into(),
            post_max_size: "64M".into(),
            max_execution_time: 60,
            realpath_cache_size: "4096K".into(),
            opcache: true,
            extensions: crate::ini::DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect(),
            cacert: None,
            extra: BTreeMap::new(),
        }
    }
}

/// Manager configuration. All URLs are overridable (tests, mirrors).
#[derive(Debug, Clone)]
pub struct Options {
    /// Minimum PHP series offered/installable via listing, "8.1".
    pub min_php: String,
    /// Optional allow-list of PHP series ("8.3","8.4"); empty = all >= min_php.
    pub php_series: Vec<String>,
    /// Minimum Node version offered in listings (Next 15 needs >= 20.9).
    pub min_node: String,
    pub cache_ttl_secs: u64,
    pub user_agent: String,
    /// Explicit proxy URL; otherwise HTTPS_PROXY / HTTP_PROXY env vars are honoured.
    pub proxy: Option<String>,
    pub connect_timeout_secs: u64,
    pub request_timeout_secs: u64,
    /// Idle timeout between download chunks.
    pub stall_timeout_secs: u64,
    pub retries: u32,
    /// Allow installing archives without a published sha256 (PHP archives/ old patch versions).
    pub allow_unverified: bool,
    /// Extensions that must be present (`php -m`). `intl` and opcache are optional.
    pub php_required_extensions: Vec<String>,
    pub php_ini: IniOptions,
    pub php_releases_url: String,
    pub php_archives_url: String,
    pub node_dist_url: String,
    pub cacert_url: String,
    /// Keep verified downloaded zips in runtimes/.downloads after install.
    pub keep_downloads: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            min_php: "8.1".into(),
            php_series: vec![],
            min_node: "20.9.0".into(),
            cache_ttl_secs: 6 * 3600,
            user_agent: "NextMON-Server".into(),
            proxy: None,
            connect_timeout_secs: 15,
            request_timeout_secs: 45,
            stall_timeout_secs: 30,
            retries: 3,
            allow_unverified: false,
            php_required_extensions: ["openssl", "mbstring", "sockets", "curl", "gd", "pdo_sqlite", "sqlite3", "zip", "fileinfo"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            php_ini: IniOptions::default(),
            php_releases_url: "https://windows.php.net/downloads/releases/releases.json".into(),
            php_archives_url: "https://windows.php.net/downloads/releases/archives/".into(),
            node_dist_url: "https://nodejs.org/dist".into(),
            cacert_url: "https://curl.se/ca/cacert.pem".into(),
            keep_downloads: false,
        }
    }
}
