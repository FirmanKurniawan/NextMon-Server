//! config.json: schema, defaults, validation, atomic save. One active edition (php|next) at a time.
use crate::paths::Paths;
use crate::util;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

pub type SharedConfig = Arc<RwLock<Config>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edition {
    Php,
    Next,
}

impl Edition {
    pub const ALL: [Edition; 2] = [Edition::Php, Edition::Next];
    pub fn as_str(&self) -> &'static str {
        match self {
            Edition::Php => "php",
            Edition::Next => "next",
        }
    }
    pub fn parse(s: &str) -> Option<Edition> {
        match s.to_ascii_lowercase().as_str() {
            "php" => Some(Edition::Php),
            "next" | "nextjs" | "next.js" => Some(Edition::Next),
            _ => None,
        }
    }
    /// Runtime kind that executes this edition.
    pub fn runtime_kind(&self) -> &'static str {
        match self {
            Edition::Php => "php",
            Edition::Next => "node",
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Edition::Php => "PHP",
            Edition::Next => "Next.js",
        }
    }
}

fn lenient_map<'de, D: Deserializer<'de>>(d: D) -> Result<BTreeMap<String, String>, D::Error> {
    let raw: BTreeMap<String, Value> = BTreeMap::deserialize(d)?;
    Ok(raw
        .into_iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, v)| {
            let s = match v {
                Value::String(s) => s,
                Value::Bool(b) => (if b { "1" } else { "0" }).to_string(),
                other => other.to_string(),
            };
            (k, s)
        })
        .collect())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AppCfg {
    pub port: u16,
    /// Installed runtime version to use; empty = highest installed.
    pub runtime: String,
    pub opcache: bool,
    #[serde(deserialize_with = "lenient_map")]
    pub extra_ini: BTreeMap<String, String>,
    #[serde(deserialize_with = "lenient_map")]
    pub env: BTreeMap<String, String>,
}

impl AppCfg {
    pub fn default_for(e: Edition) -> AppCfg {
        match e {
            Edition::Php => AppCfg {
                port: 8080,
                runtime: String::new(),
                opcache: true,
                extra_ini: BTreeMap::new(),
                env: BTreeMap::from([("NEXTMON_DEMO".to_string(), "0".to_string())]),
            },
            Edition::Next => AppCfg { port: 3000, runtime: String::new(), opcache: false, extra_ini: BTreeMap::new(), env: BTreeMap::new() },
        }
    }
}

impl Default for AppCfg {
    fn default() -> Self {
        AppCfg::default_for(Edition::Php)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Apps {
    pub php: AppCfg,
    pub next: AppCfg,
}

impl Default for Apps {
    fn default() -> Self {
        Apps { php: AppCfg::default_for(Edition::Php), next: AppCfg::default_for(Edition::Next) }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct UpdateCfg {
    pub check_on_start: bool,
    pub channel_php: String,
    pub channel_node: String,
    pub app_repo: String,
    pub app_channel: String,
}

impl Default for UpdateCfg {
    fn default() -> Self {
        UpdateCfg {
            check_on_start: true,
            channel_php: "stable".into(),
            channel_node: "lts".into(),
            app_repo: "FirmanKurniawan/NextMon".into(),
            app_channel: "stable".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    pub version: u32,
    pub language: String,
    pub theme: String,
    pub control_port: u16,
    pub control_token: String,
    pub edition: Edition,
    pub bind: String,
    pub auto_start_with_windows: bool,
    pub start_minimized: bool,
    pub auto_start_edition: bool,
    pub open_browser_on_start: bool,
    pub data_dir: String,
    pub log_retention_days: u32,
    pub restart_on_crash: bool,
    pub max_restarts: u32,
    /// First-run wizard finished (set by the panel).
    pub onboarded: bool,
    /// Minimum Node.js version Next.js edition needs.
    pub min_node: String,
    pub apps: Apps,
    pub update: UpdateCfg,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            version: 1,
            language: "id".into(),
            theme: "auto".into(),
            control_port: 47878,
            control_token: String::new(),
            edition: Edition::Php,
            bind: "127.0.0.1".into(),
            auto_start_with_windows: false,
            start_minimized: false,
            auto_start_edition: true,
            open_browser_on_start: true,
            data_dir: "data".into(),
            log_retention_days: 14,
            restart_on_crash: true,
            max_restarts: 5,
            onboarded: false,
            min_node: "20.9.0".into(),
            apps: Apps::default(),
            update: UpdateCfg::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfigError {
    pub field: String,
    pub message: String,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

fn err(field: &str, message: &str) -> ConfigError {
    ConfigError { field: field.into(), message: message.into() }
}

impl Config {
    pub fn app(&self, e: Edition) -> &AppCfg {
        match e {
            Edition::Php => &self.apps.php,
            Edition::Next => &self.apps.next,
        }
    }
    pub fn app_mut(&mut self, e: Edition) -> &mut AppCfg {
        match e {
            Edition::Php => &mut self.apps.php,
            Edition::Next => &mut self.apps.next,
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if !["id", "en"].contains(&self.language.as_str()) {
            return Err(err("language", "must be id or en"));
        }
        if !["auto", "light", "dark"].contains(&self.theme.as_str()) {
            return Err(err("theme", "must be auto, light or dark"));
        }
        if self.control_port < 1024 {
            return Err(err("controlPort", "must be 1024-65535"));
        }
        if self.control_token.len() < 16 || !self.control_token.chars().all(|c| c.is_ascii_graphic()) {
            return Err(err("controlToken", "must be at least 16 printable characters"));
        }
        if !["127.0.0.1", "0.0.0.0"].contains(&self.bind.as_str()) {
            return Err(err("bind", "must be 127.0.0.1 or 0.0.0.0"));
        }
        if self.data_dir.trim().is_empty() || self.data_dir.contains('\0') {
            return Err(err("dataDir", "must not be empty"));
        }
        if !(1..=3650).contains(&self.log_retention_days) {
            return Err(err("logRetentionDays", "must be 1-3650"));
        }
        if self.max_restarts > 100 {
            return Err(err("maxRestarts", "must be 0-100"));
        }
        if self.min_node.is_empty() || !self.min_node.chars().all(|c| c.is_ascii_digit() || c == '.') {
            return Err(err("minNode", "must look like 20.9.0"));
        }
        for e in Edition::ALL {
            let a = self.app(e);
            let f = |s: &str| format!("apps.{}.{}", e.as_str(), s);
            if a.port == 0 {
                return Err(err(&f("port"), "must be 1-65535"));
            }
            if a.port == self.control_port {
                return Err(err(&f("port"), "must differ from controlPort"));
            }
            if !a.runtime.is_empty() && !util::safe_file_name(&a.runtime) {
                return Err(err(&f("runtime"), "invalid version string"));
            }
            for (k, v) in &a.env {
                if k.is_empty() || k.contains('=') || k.contains('\0') || v.contains('\0') {
                    return Err(err(&f("env"), "invalid variable name or value"));
                }
            }
            for (k, v) in &a.extra_ini {
                if k.is_empty() || k.contains('=') || k.contains(char::is_whitespace) || v.contains('\0') || v.contains('\n') {
                    return Err(err(&f("extraIni"), "invalid ini key or value"));
                }
            }
        }
        // The two editions never run together, so sharing a port is allowed; we only reject the control port.
        let r = &self.update.app_repo;
        if r.contains(char::is_whitespace) {
            return Err(err("update.appRepo", "must be owner/repo"));
        }
        if !["stable", "beta"].contains(&self.update.app_channel.as_str()) {
            return Err(err("update.appChannel", "must be stable or beta"));
        }
        if !["stable", "security"].contains(&self.update.channel_php.as_str()) && !self.update.channel_php.is_empty() {
            return Err(err("update.channelPhp", "must be stable"));
        }
        if !["lts", "current"].contains(&self.update.channel_node.as_str()) {
            return Err(err("update.channelNode", "must be lts or current"));
        }
        Ok(())
    }

    /// Fill in values that must always be present (token).
    pub fn fixup(&mut self) {
        if self.control_token.len() < 16 {
            self.control_token = util::random_token();
        }
    }

    pub fn app_url(&self, e: Edition) -> String {
        format!("http://127.0.0.1:{}", self.app(e).port)
    }
}

pub fn shared(c: Config) -> SharedConfig {
    Arc::new(RwLock::new(c))
}

/// Load config.json, creating it with defaults on first run. A corrupt file is kept as config.json.bad-<stamp> and replaced.
pub fn load_or_create(paths: &Paths) -> (Config, Option<String>) {
    let file = paths.config_file();
    let mut note = None;
    let cfg = match std::fs::read_to_string(&file) {
        Ok(text) => match parse(&text) {
            Ok(c) => Some(c),
            Err(e) => {
                let bad = file.with_extension(format!("json.bad-{}", util::now_local().stamp()));
                let _ = std::fs::rename(&file, &bad);
                note = Some(format!("config.json invalid ({e}); moved to {} and recreated with defaults", bad.display()));
                None
            }
        },
        Err(_) => None,
    };
    let mut c = cfg.unwrap_or_default();
    let had_token = c.control_token.len() >= 16;
    c.fixup();
    if c.validate().is_err() {
        // A single bad field should not make the launcher unusable: fall back to defaults but keep the token.
        let msg = c.validate().err().map(|e| e.to_string()).unwrap_or_default();
        let tok = c.control_token.clone();
        let bad = file.with_extension(format!("json.bad-{}", util::now_local().stamp()));
        let _ = std::fs::copy(&file, &bad);
        c = Config::default();
        c.control_token = tok;
        note = Some(format!("config.json failed validation ({msg}); defaults restored, original saved as {}", bad.display()));
    }
    if !file.exists() || !had_token || note.is_some() {
        let _ = save(paths, &c);
    }
    (c, note)
}

pub fn parse(text: &str) -> Result<Config, String> {
    let text = text.trim_start_matches('\u{feff}');
    // Strip legacy keys silently by deserialising leniently (unknown fields are ignored).
    serde_json::from_str::<Config>(text).map_err(|e| e.to_string())
}

pub fn save(paths: &Paths, c: &Config) -> Result<(), String> {
    c.validate().map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(c).map_err(|e| e.to_string())?;
    util::atomic_write(&paths.config_file(), json.as_bytes()).map_err(|e| e.to_string())
}

/// Apply a JSON patch to a config and validate; returns the new config without saving.
pub fn apply_patch(cur: &Config, patch: &Value) -> Result<Config, ConfigError> {
    let mut v = serde_json::to_value(cur).map_err(|e| err("config", &e.to_string()))?;
    util::json_merge(&mut v, patch);
    let mut c: Config = serde_json::from_value(v).map_err(|e| err("config", &e.to_string()))?;
    c.fixup();
    c.validate()?;
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_paths() -> (tempfile::TempDir, Paths) {
        let d = tempfile::tempdir().unwrap();
        let p = Paths::new(d.path());
        (d, p)
    }

    #[test]
    fn first_run_creates_defaults_with_token() {
        let (_d, p) = tmp_paths();
        let (c, note) = load_or_create(&p);
        assert!(note.is_none());
        assert_eq!(c.control_port, 47878);
        assert_eq!(c.control_token.len(), 48);
        assert!(p.config_file().exists());
        let (c2, _) = load_or_create(&p);
        assert_eq!(c.control_token, c2.control_token, "token is stable across runs");
    }

    #[test]
    fn corrupt_file_is_backed_up_and_recreated() {
        let (_d, p) = tmp_paths();
        std::fs::write(p.config_file(), "{ not json").unwrap();
        let (c, note) = load_or_create(&p);
        assert!(note.is_some());
        assert_eq!(c.edition, Edition::Php);
        let bad = std::fs::read_dir(&p.base).unwrap().flatten().any(|e| e.file_name().to_string_lossy().contains(".bad-"));
        assert!(bad);
    }

    #[test]
    fn legacy_keys_ignored_and_partial_files_filled() {
        let c = parse(r#"{"edition":"next","apps":{"php":{"enabled":true,"port":8090,"extraIni":{"memory_limit":512}}},"autoStartApps":["php"]}"#).unwrap();
        assert_eq!(c.edition, Edition::Next);
        assert_eq!(c.apps.php.port, 8090);
        assert_eq!(c.apps.php.extra_ini["memory_limit"], "512");
        assert_eq!(c.apps.next.port, 3000);
        let out = serde_json::to_string(&c).unwrap();
        assert!(!out.contains("autoStartApps") && !out.contains("enabled"));
    }

    #[test]
    fn validation_rejects_bad_values() {
        let mut c = Config::default();
        c.fixup();
        assert!(c.validate().is_ok());
        let mut b = c.clone();
        b.apps.php.port = b.control_port;
        assert!(b.validate().is_err());
        let mut b = c.clone();
        b.bind = "192.168.1.5".into();
        assert!(b.validate().is_err());
        let mut b = c.clone();
        b.apps.php.runtime = "..\\evil".into();
        assert!(b.validate().is_err());
        let mut b = c.clone();
        b.apps.next.env.insert("A=B".into(), "x".into());
        assert!(b.validate().is_err());
        let mut b = c.clone();
        b.language = "fr".into();
        assert!(b.validate().is_err());
    }

    #[test]
    fn patch_merges_and_validates() {
        let mut c = Config::default();
        c.fixup();
        let n = apply_patch(&c, &serde_json::json!({"apps":{"php":{"port":9000}},"theme":"dark"})).unwrap();
        assert_eq!(n.apps.php.port, 9000);
        assert_eq!(n.apps.php.opcache, true);
        assert_eq!(n.theme, "dark");
        assert!(apply_patch(&c, &serde_json::json!({"apps":{"php":{"port":0}}})).is_err());
        assert!(apply_patch(&c, &serde_json::json!({"edition":"ruby"})).is_err());
    }

    #[test]
    fn atomic_save_roundtrip() {
        let (_d, p) = tmp_paths();
        let mut c = Config::default();
        c.fixup();
        c.apps.next.port = 3100;
        save(&p, &c).unwrap();
        let back = parse(&std::fs::read_to_string(p.config_file()).unwrap()).unwrap();
        assert_eq!(back, c);
        // no stray temp files left behind
        let stray = std::fs::read_dir(&p.base).unwrap().flatten().any(|e| e.file_name().to_string_lossy().contains(".tmp"));
        assert!(!stray);
    }
}
