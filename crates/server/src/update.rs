//! Application updates from GitHub Releases: check, download + sha256 verify (SHA256SUMS.txt), atomic swap of app/ + ui/ + docs/ with rollback.
use crate::backup;
use crate::paths::Paths;
use crate::util;
use futures_util::StreamExt;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    pub version: String,
    pub tag: String,
    pub prerelease: bool,
    pub notes: String,
    pub url: String,
    pub published_at: String,
    pub asset_name: Option<String>,
    pub asset_url: Option<String>,
    pub asset_size: Option<u64>,
    pub sums_url: Option<String>,
}

#[derive(Debug)]
pub struct UpdateError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for UpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

fn ue(code: &'static str, m: impl ToString) -> UpdateError {
    UpdateError { code, message: m.to_string() }
}

/// Installed application version: app/VERSION (first line), else app/php/VERSION, else the launcher's own version.
pub fn current_version(paths: &Paths) -> String {
    for p in [paths.app_root().join("VERSION"), paths.app_root().join("php").join("VERSION")] {
        if let Ok(t) = std::fs::read_to_string(p) {
            let v = t.lines().next().unwrap_or("").trim().trim_start_matches('v').to_string();
            if !v.is_empty() {
                return v;
            }
        }
    }
    // No VERSION file shipped: releases track the launcher version.
    crate::core::VERSION.to_string()
}

pub fn valid_repo(repo: &str) -> bool {
    let mut it = repo.split('/');
    let (Some(o), Some(r), None) = (it.next(), it.next(), it.next()) else { return false };
    let ok = |s: &str| !s.is_empty() && s.len() <= 100 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    ok(o) && ok(r)
}

/// Pick the newest eligible release from the GitHub `releases` array.
pub fn pick_release(releases: &Value, channel: &str) -> Option<ReleaseInfo> {
    let mut best: Option<ReleaseInfo> = None;
    for r in releases.as_array()? {
        if r["draft"].as_bool().unwrap_or(false) {
            continue;
        }
        let pre = r["prerelease"].as_bool().unwrap_or(false);
        if pre && channel != "beta" {
            continue;
        }
        let tag = r["tag_name"].as_str().unwrap_or("").to_string();
        let version = tag.trim_start_matches('v').to_string();
        if version.is_empty() {
            continue;
        }
        let assets = r["assets"].as_array().cloned().unwrap_or_default();
        let find = |pred: &dyn Fn(&str) -> bool| assets.iter().find(|a| pred(a["name"].as_str().unwrap_or("")));
        let pkg = find(&|n| n.starts_with("NextMON-Server-") && n.ends_with("-win-x64.zip"));
        let sums = find(&|n| n.eq_ignore_ascii_case("SHA256SUMS.txt"));
        let info = ReleaseInfo {
            version,
            tag,
            prerelease: pre,
            notes: r["body"].as_str().unwrap_or("").chars().take(4000).collect(),
            url: r["html_url"].as_str().unwrap_or("").to_string(),
            published_at: r["published_at"].as_str().unwrap_or("").to_string(),
            asset_name: pkg.and_then(|a| a["name"].as_str()).map(String::from),
            asset_url: pkg.and_then(|a| a["browser_download_url"].as_str()).map(String::from),
            asset_size: pkg.and_then(|a| a["size"].as_u64()),
            sums_url: sums.and_then(|a| a["browser_download_url"].as_str()).map(String::from),
        };
        if best.as_ref().map(|b| util::cmp_version(&info.version, &b.version).is_gt()).unwrap_or(true) {
            best = Some(info);
        }
    }
    best
}

/// Parse `SHA256SUMS.txt` ("<hex>  <name>" or "<hex> *<name>").
pub fn parse_sums(text: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for l in text.lines() {
        let l = l.trim();
        let Some((h, n)) = l.split_once(char::is_whitespace) else { continue };
        let n = n.trim().trim_start_matches('*');
        if h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()) {
            m.insert(n.to_string(), h.to_ascii_lowercase());
        }
    }
    m
}

fn github_url_ok(u: &str) -> bool {
    u.starts_with("https://github.com/") || u.starts_with("https://api.github.com/") || u.starts_with("https://objects.githubusercontent.com/")
}

pub fn http_client() -> Result<reqwest::Client, UpdateError> {
    reqwest::Client::builder()
        .user_agent(concat!("NextMON-Server/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| ue("network", e))
}

pub async fn fetch_latest(client: &reqwest::Client, repo: &str, channel: &str) -> Result<Option<ReleaseInfo>, UpdateError> {
    if !valid_repo(repo) {
        return Err(ue("not_configured", "update.appRepo is not set (expected owner/repo)"));
    }
    let url = format!("https://api.github.com/repos/{repo}/releases?per_page=15");
    let resp = client.get(&url).header("Accept", "application/vnd.github+json").send().await.map_err(|e| ue("network", e))?;
    if resp.status().as_u16() == 404 {
        return Err(ue("not_found", "repository or releases not found"));
    }
    if !resp.status().is_success() {
        return Err(ue("network", format!("GitHub returned HTTP {}", resp.status())));
    }
    let v: Value = resp.json().await.map_err(|e| ue("parse", e))?;
    Ok(pick_release(&v, channel))
}

/// Download `url` to `dest`, returning the sha256 (hex). `progress(done, total)`.
pub async fn download(client: &reqwest::Client, url: &str, dest: &Path, progress: &(dyn Fn(u64, Option<u64>) + Sync)) -> Result<String, UpdateError> {
    if !github_url_ok(url) {
        return Err(ue("bad_url", format!("refusing to download from {url}")));
    }
    let resp = client.get(url).send().await.map_err(|e| ue("network", e))?;
    if !resp.status().is_success() {
        return Err(ue("network", format!("download failed: HTTP {}", resp.status())));
    }
    let total = resp.content_length();
    if let Some(p) = dest.parent() {
        std::fs::create_dir_all(p).map_err(|e| ue("io", e))?;
    }
    let mut f = std::fs::File::create(dest).map_err(|e| ue("io", e))?;
    let mut h = Sha256::new();
    let mut done = 0u64;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let c = chunk.map_err(|e| ue("network", e))?;
        h.update(&c);
        f.write_all(&c).map_err(|e| ue("io", e))?;
        done += c.len() as u64;
        progress(done, total);
    }
    Ok(util::hex(&h.finalize()))
}

pub struct Swapped {
    names: Vec<String>,
    base: PathBuf,
}

impl Swapped {
    /// Undo the swap: put the .bak folders back.
    pub fn rollback(&self) {
        for n in &self.names {
            let cur = self.base.join(n);
            let bak = self.base.join(format!("{n}.bak"));
            let _ = std::fs::remove_dir_all(&cur);
            if bak.exists() {
                let _ = std::fs::rename(&bak, &cur);
            }
        }
    }
}

const REPLACED: [&str; 3] = ["app", "ui", "docs"];

/// Extract a release package and swap `app/`, `ui/`, `docs/` into place (old copies kept as `<name>.bak`).
/// Never touches data/, config.json, runtimes/, backups/ or the exe. Returns a handle to roll back.
pub fn apply_package(paths: &Paths, zip_path: &Path, version: &str) -> Result<Swapped, UpdateError> {
    backup::validate_zip(zip_path).map_err(|e| ue("bad_archive", e.message))?;
    // detect a single wrapper folder ("NextMON Server/app/...")
    let wrapper = {
        let f = std::fs::File::open(zip_path).map_err(|e| ue("io", e))?;
        let mut ar = zip::ZipArchive::new(f).map_err(|e| ue("bad_archive", e))?;
        let mut first: Option<String> = None;
        let mut single = true;
        for i in 0..ar.len() {
            let e = ar.by_index(i).map_err(|e| ue("bad_archive", e))?;
            let Some(p) = backup::safe_entry_path(&backup::entry_name(&e)) else { continue };
            let top = p.components().next().map(|c| c.as_os_str().to_string_lossy().to_string()).unwrap_or_default();
            match &first {
                None => first = Some(top),
                Some(f) if *f != top => single = false,
                _ => {}
            }
        }
        match first {
            Some(f) if single && !REPLACED.contains(&f.as_str()) => Some(f),
            _ => None,
        }
    };
    let staging = paths.tmp().join(format!("update-{}", util::now_local().stamp()));
    let _ = std::fs::remove_dir_all(&staging);
    let strip = wrapper.clone();
    backup::extract_zip(zip_path, &staging, &move |rel| {
        let mut r = rel.to_path_buf();
        if let Some(w) = &strip {
            r = r.strip_prefix(w).ok()?.to_path_buf();
        }
        let top = r.components().next()?.as_os_str().to_string_lossy().to_string();
        if REPLACED.contains(&top.as_str()) {
            Some(r)
        } else {
            None
        }
    })
    .map_err(|e| ue("bad_archive", e.message))?;
    if !staging.join("app").is_dir() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(ue("bad_package", "the package does not contain an app/ folder"));
    }
    if !staging.join("app").join("VERSION").is_file() {
        let _ = std::fs::write(staging.join("app").join("VERSION"), version);
    }
    let mut done = Swapped { names: vec![], base: paths.base.clone() };
    for n in REPLACED {
        let new = staging.join(n);
        if !new.is_dir() {
            continue;
        }
        let cur = paths.base.join(n);
        let bak = paths.base.join(format!("{n}.bak"));
        let _ = std::fs::remove_dir_all(&bak);
        if cur.exists() {
            if let Err(e) = std::fs::rename(&cur, &bak) {
                done.rollback();
                let _ = std::fs::remove_dir_all(&staging);
                return Err(ue("in_use", format!("cannot replace {n}/ (files in use?): {e}")));
            }
        }
        if let Err(e) = std::fs::rename(&new, &cur) {
            if bak.exists() {
                let _ = std::fs::rename(&bak, &cur);
            }
            done.rollback();
            let _ = std::fs::remove_dir_all(&staging);
            return Err(ue("io", e));
        }
        done.names.push(n.to_string());
    }
    let _ = std::fs::remove_dir_all(&staging);
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_zip(path: &Path, files: &[(&str, &str)]) {
        let mut zw = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        for (n, c) in files {
            zw.start_file(*n, o).unwrap();
            zw.write_all(c.as_bytes()).unwrap();
        }
        zw.finish().unwrap();
    }

    #[test]
    fn release_selection() {
        let j = serde_json::json!([
          {"tag_name":"v1.3.0-rc1","prerelease":true,"assets":[]},
          {"tag_name":"v1.2.0","assets":[
             {"name":"NextMON-Server-v1.2.0-win-x64.zip","browser_download_url":"https://github.com/o/r/releases/download/v1.2.0/a.zip","size":10},
             {"name":"NextMON-Server-v1.2.0-win-x64-full.zip","browser_download_url":"https://github.com/o/r/x.zip"},
             {"name":"SHA256SUMS.txt","browser_download_url":"https://github.com/o/r/s.txt"}]},
          {"tag_name":"v1.1.0","assets":[]},
          {"tag_name":"v9.9.9","draft":true,"assets":[]}
        ]);
        let r = pick_release(&j, "stable").unwrap();
        assert_eq!(r.version, "1.2.0");
        assert_eq!(r.asset_name.as_deref(), Some("NextMON-Server-v1.2.0-win-x64.zip"));
        assert!(r.sums_url.is_some());
        assert_eq!(pick_release(&j, "beta").unwrap().version, "1.3.0-rc1");
    }

    #[test]
    fn sums_and_repo() {
        let h = "a".repeat(64);
        let m = parse_sums(&format!("{h}  NextMON-Server-v1-win-x64.zip\n{h} *other.zip\nbad line\n"));
        assert_eq!(m["NextMON-Server-v1-win-x64.zip"], h);
        assert_eq!(m["other.zip"], h);
        assert!(valid_repo("owner/repo") && !valid_repo("<owner>/<repo>") && !valid_repo("a/b/c") && !valid_repo("x"));
    }

    #[test]
    fn apply_preserves_data_and_config_then_rolls_back() {
        let d = tempfile::tempdir().unwrap();
        let p = Paths::new(d.path());
        std::fs::create_dir_all(p.base.join("app/php")).unwrap();
        std::fs::write(p.base.join("app/php/old.php"), "old").unwrap();
        std::fs::write(p.base.join("app/VERSION"), "1.0.0").unwrap();
        std::fs::create_dir_all(p.base.join("data/php")).unwrap();
        std::fs::write(p.base.join("data/php/keep.db"), "keep").unwrap();
        std::fs::write(p.base.join("config.json"), "{}").unwrap();
        let z = d.path().join("pkg.zip");
        make_zip(
            &z,
            &[
                ("NextMON Server/app/php/new.php", "new"),
                ("NextMON Server/app/VERSION", "1.1.0"),
                ("NextMON Server/ui/index.html", "<html>"),
                ("NextMON Server/data/php/EVIL.db", "x"),
                ("NextMON Server/config.json", "{\"evil\":1}"),
                ("NextMON Server/NextMON Server.exe", "MZ"),
            ],
        );
        let sw = apply_package(&p, &z, "1.1.0").unwrap();
        assert!(p.base.join("app/php/new.php").is_file() && !p.base.join("app/php/old.php").exists());
        assert_eq!(current_version(&p), "1.1.0");
        assert!(p.base.join("ui/index.html").is_file());
        assert_eq!(std::fs::read_to_string(p.base.join("data/php/keep.db")).unwrap(), "keep");
        assert!(!p.base.join("data/php/EVIL.db").exists());
        assert_eq!(std::fs::read_to_string(p.base.join("config.json")).unwrap(), "{}");
        assert!(!p.base.join("NextMON Server.exe").exists());
        sw.rollback();
        assert_eq!(current_version(&p), "1.0.0");
        assert!(p.base.join("app/php/old.php").is_file());
        assert!(!p.base.join("ui").exists());
    }

    #[test]
    fn bad_packages_are_refused_without_changes() {
        let d = tempfile::tempdir().unwrap();
        let p = Paths::new(d.path());
        std::fs::create_dir_all(p.base.join("app")).unwrap();
        std::fs::write(p.base.join("app/VERSION"), "1.0.0").unwrap();
        let z = d.path().join("slip.zip");
        make_zip(&z, &[("app/x", "1"), ("../../evil", "2")]);
        assert_eq!(apply_package(&p, &z, "2").err().unwrap().code, "bad_archive");
        let z2 = d.path().join("noapp.zip");
        make_zip(&z2, &[("ui/index.html", "x")]);
        assert_eq!(apply_package(&p, &z2, "2").err().unwrap().code, "bad_package");
        assert_eq!(current_version(&p), "1.0.0");
    }
}
