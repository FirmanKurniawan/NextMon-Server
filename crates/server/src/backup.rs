//! Backup / restore of the data folder as zip, with zip-slip protection and a pre-restore safety copy.
use crate::paths::Paths;
use crate::util;
use serde::Serialize;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const MANIFEST: &str = ".nextmon-backup.json";

#[derive(Debug)]
pub struct BackupError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for BackupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

fn be(code: &'static str, m: impl ToString) -> BackupError {
    BackupError { code, message: m.to_string() }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub name: String,
    pub size: u64,
    #[serde(rename = "sizeMB")]
    pub size_mb: f64,
    /// ISO-8601 UTC modification time.
    pub date: String,
    /// manual (user created) | auto (safety copies made before restore/update/runtime upgrade)
    pub kind: String,
}

fn info_for(p: &Path) -> Option<BackupInfo> {
    let name = p.file_name()?.to_string_lossy().to_string();
    let md = p.metadata().ok()?;
    let kind = if name.starts_with("pre-") { "auto" } else { "manual" }.to_string();
    let date = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| unix_to_iso(d.as_secs()))
        .unwrap_or_default();
    Some(BackupInfo { name, size: md.len(), size_mb: ((md.len() as f64 / 1048576.0) * 100.0).round() / 100.0, date, kind })
}

fn unix_to_iso(secs: u64) -> String {
    // UTC ISO-8601 from epoch seconds (no extra crates)
    let days = (secs / 86400) as i64;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let rem = secs % 86400;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

pub fn list(paths: &Paths) -> Vec<BackupInfo> {
    let mut v: Vec<BackupInfo> = std::fs::read_dir(paths.backups())
        .map(|rd| rd.flatten().filter(|e| e.path().extension().map(|x| x == "zip").unwrap_or(false)).filter_map(|e| info_for(&e.path())).collect())
        .unwrap_or_default();
    v.sort_by(|a, b| b.name.cmp(&a.name));
    v
}

/// Validate a user-supplied backup name and return its path (must exist).
pub fn resolve(paths: &Paths, name: &str) -> Result<PathBuf, BackupError> {
    if !util::safe_file_name(name) || !name.ends_with(".zip") {
        return Err(be("bad_name", "invalid backup name"));
    }
    let p = paths.backups().join(name);
    if !p.is_file() {
        return Err(be("not_found", "backup not found"));
    }
    Ok(p)
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, String)>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            let p = e.path();
            if ft.is_dir() {
                collect_files(root, &p, out);
            } else if let Ok(rel) = p.strip_prefix(root) {
                out.push((p.clone(), rel.to_string_lossy().replace('\\', "/")));
            }
        }
    }
}

/// Zip `data_root` into backups/<prefix>-<stamp>.zip (entries relative to the data root).
pub fn create(paths: &Paths, data_root: &Path, prefix: &str) -> Result<BackupInfo, BackupError> {
    std::fs::create_dir_all(paths.backups()).map_err(|e| be("io", e))?;
    let stamp = util::now_local().stamp();
    let mut name = format!("{prefix}-{stamp}.zip");
    let mut n = 2;
    while paths.backups().join(&name).exists() {
        name = format!("{prefix}-{stamp}-{n}.zip");
        n += 1;
    }
    let dest = paths.backups().join(&name);
    let tmp = paths.backups().join(format!("{name}.part"));
    let mut files = vec![];
    collect_files(data_root, data_root, &mut files);
    let result = (|| -> Result<usize, BackupError> {
        let f = File::create(&tmp).map_err(|e| be("io", e))?;
        let mut zw = zip::ZipWriter::new(f);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let manifest = serde_json::json!({"nextmonBackup": 1, "created": util::now_local().iso(), "files": files.len()});
        zw.start_file(MANIFEST, opts).map_err(|e| be("zip", e))?;
        zw.write_all(manifest.to_string().as_bytes()).map_err(|e| be("io", e))?;
        let mut skipped = 0;
        let mut buf = vec![0u8; 64 * 1024];
        for (abs, rel) in &files {
            let Ok(mut src) = File::open(abs) else {
                skipped += 1;
                continue;
            };
            zw.start_file(rel.as_str(), opts).map_err(|e| be("zip", e))?;
            loop {
                let n = src.read(&mut buf).map_err(|e| be("io", e))?;
                if n == 0 {
                    break;
                }
                zw.write_all(&buf[..n]).map_err(|e| be("io", e))?;
            }
        }
        zw.finish().map_err(|e| be("zip", e))?;
        Ok(skipped)
    })();
    match result {
        Ok(skipped) => {
            if skipped > 0 {
                log::warn!("backup {name}: {skipped} file(s) could not be read and were skipped");
            }
            std::fs::rename(&tmp, &dest).map_err(|e| be("io", e))?;
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    }
    if prefix.starts_with("pre-") {
        prune(paths, prefix, 10);
    }
    info_for(&dest).ok_or_else(|| be("io", "backup vanished"))
}

fn prune(paths: &Paths, prefix: &str, keep: usize) {
    let mut v: Vec<_> = list(paths).into_iter().filter(|b| b.name.starts_with(&format!("{prefix}-"))).collect();
    v.sort_by(|a, b| b.name.cmp(&a.name));
    for old in v.into_iter().skip(keep) {
        let _ = std::fs::remove_file(paths.backups().join(old.name));
    }
}

pub fn delete(paths: &Paths, name: &str) -> Result<(), BackupError> {
    let p = resolve(paths, name)?;
    std::fs::remove_file(p).map_err(|e| be("io", e))
}

/// Entry name as UTF-8; unreadable names become "" (and are rejected by `safe_entry_path`).
pub fn entry_name(e: &zip::read::ZipFile<'_, File>) -> String {
    e.name().map(|c| c.into_owned()).unwrap_or_default()
}

/// Map a zip entry name to a safe relative path, or None if it could escape the target folder.
pub fn safe_entry_path(name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.starts_with('/') || name.starts_with('\\') || name.contains(':') || name.contains('\0') {
        return None;
    }
    let mut out = PathBuf::new();
    for part in name.split(['/', '\\']) {
        match part {
            "" | "." => continue,
            ".." => return None,
            p => {
                // Windows reserved trailing dots/spaces would alias other names
                if p.ends_with('.') || p.ends_with(' ') {
                    return None;
                }
                out.push(p)
            }
        }
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Check every entry up front (all-or-nothing) so a malicious archive writes nothing.
pub fn validate_zip(zip_path: &Path) -> Result<usize, BackupError> {
    let f = File::open(zip_path).map_err(|e| be("io", e))?;
    let mut ar = zip::ZipArchive::new(f).map_err(|e| be("bad_archive", e))?;
    for i in 0..ar.len() {
        let e = ar.by_index(i).map_err(|e| be("bad_archive", e))?;
        if safe_entry_path(&entry_name(&e)).is_none() {
            return Err(be("unsafe_archive", format!("unsafe path in archive: {}", entry_name(&e))));
        }
        if let Some(mode) = e.unix_mode() {
            if mode & 0o170000 == 0o120000 {
                return Err(be("unsafe_archive", format!("symlink in archive: {}", entry_name(&e))));
            }
        }
    }
    Ok(ar.len())
}

/// Extract after `validate_zip`. `filter` receives the safe relative path and decides whether to extract it
/// (return a different path to remap, None to skip).
pub fn extract_zip(zip_path: &Path, dest: &Path, filter: &dyn Fn(&Path) -> Option<PathBuf>) -> Result<usize, BackupError> {
    validate_zip(zip_path)?;
    let f = File::open(zip_path).map_err(|e| be("io", e))?;
    let mut ar = zip::ZipArchive::new(f).map_err(|e| be("bad_archive", e))?;
    std::fs::create_dir_all(dest).map_err(|e| be("io", e))?;
    let mut n = 0;
    for i in 0..ar.len() {
        let mut e = ar.by_index(i).map_err(|e| be("bad_archive", e))?;
        let Some(rel) = safe_entry_path(&entry_name(&e)) else { continue };
        let Some(rel) = filter(&rel) else { continue };
        let out = dest.join(&rel);
        if !out.starts_with(dest) {
            return Err(be("unsafe_archive", "path escapes destination"));
        }
        if e.is_dir() || entry_name(&e).ends_with('/') {
            std::fs::create_dir_all(&out).map_err(|e| be("io", e))?;
            continue;
        }
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p).map_err(|e| be("io", e))?;
        }
        let mut w = File::create(&out).map_err(|e| be("io", e))?;
        std::io::copy(&mut e, &mut w).map_err(|e| be("io", e))?;
        n += 1;
    }
    Ok(n)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub restored: String,
    pub pre_restore: Option<String>,
    pub files: usize,
}

/// Replace `data_root` by the contents of backup `name`. The caller must have stopped the app (open files block the swap).
/// A copy of the current data is saved first as `pre-restore-*.zip`; on any failure the original folder is put back.
pub fn restore(paths: &Paths, data_root: &Path, name: &str) -> Result<RestoreResult, BackupError> {
    let zip = resolve(paths, name)?;
    validate_zip(&zip)?; // refuse bad archives before touching anything
    let pre = if data_root.is_dir() { Some(create(paths, data_root, "pre-restore")?.name) } else { None };

    let stamp = util::now_local().stamp();
    let staging = sibling(data_root, &format!("restoring-{stamp}"));
    let old = sibling(data_root, &format!("old-{stamp}"));
    let _ = std::fs::remove_dir_all(&staging);
    let files = match extract_zip(&zip, &staging, &|rel| if rel.as_os_str() == MANIFEST { None } else { Some(rel.to_path_buf()) }) {
        Ok(n) => n,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    let had_old = data_root.exists();
    if had_old {
        if let Err(e) = std::fs::rename(data_root, &old) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(be("in_use", format!("cannot move current data aside (is the app still running?): {e}")));
        }
    }
    if let Err(e) = std::fs::rename(&staging, data_root) {
        if had_old {
            let _ = std::fs::rename(&old, data_root); // roll back
        }
        let _ = std::fs::remove_dir_all(&staging);
        return Err(be("io", e));
    }
    if had_old {
        let _ = std::fs::remove_dir_all(&old);
    }
    Ok(RestoreResult { restored: name.to_string(), pre_restore: pre, files })
}

fn sibling(p: &Path, suffix: &str) -> PathBuf {
    let n = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "data".into());
    p.with_file_name(format!(".{n}.{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, Paths, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let p = Paths::new(d.path());
        p.ensure_dirs();
        let data = p.data_root("data");
        std::fs::create_dir_all(data.join("php/sub")).unwrap();
        std::fs::write(data.join("php/a.db"), b"alpha").unwrap();
        std::fs::write(data.join("php/sub/b.txt"), b"beta").unwrap();
        std::fs::create_dir_all(data.join("next")).unwrap();
        std::fs::write(data.join("next/c.json"), b"{}").unwrap();
        (d, p, data)
    }

    #[test]
    fn roundtrip_with_pre_restore() {
        let (_d, p, data) = fixture();
        let b = create(&p, &data, "data").unwrap();
        assert!(b.name.starts_with("data-") && b.size > 0);
        assert_eq!(list(&p).len(), 1);
        // mutate data after the backup
        std::fs::write(data.join("php/a.db"), b"CHANGED").unwrap();
        std::fs::write(data.join("php/new.txt"), b"x").unwrap();
        let r = restore(&p, &data, &b.name).unwrap();
        assert_eq!(r.files, 3);
        assert_eq!(std::fs::read(data.join("php/a.db")).unwrap(), b"alpha");
        assert_eq!(std::fs::read(data.join("php/sub/b.txt")).unwrap(), b"beta");
        assert!(!data.join("php/new.txt").exists(), "restore replaces, not merges");
        assert!(!data.join(MANIFEST).exists());
        // pre-restore backup keeps the changed state
        let pre = r.pre_restore.unwrap();
        assert!(pre.starts_with("pre-restore-"));
        restore(&p, &data, &pre).unwrap();
        assert_eq!(std::fs::read(data.join("php/a.db")).unwrap(), b"CHANGED");
        // no staging leftovers
        let stray = std::fs::read_dir(&p.base).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with(".data."));
        assert!(!stray);
    }

    #[test]
    fn zip_slip_is_rejected_and_nothing_is_touched() {
        let (_d, p, data) = fixture();
        let evil = p.backups().join("evil.zip");
        {
            let mut zw = zip::ZipWriter::new(File::create(&evil).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            zw.start_file("ok.txt", o).unwrap();
            zw.write_all(b"ok").unwrap();
            zw.start_file("../../escaped.txt", o).unwrap();
            zw.write_all(b"pwned").unwrap();
            zw.finish().unwrap();
        }
        let e = restore(&p, &data, "evil.zip").unwrap_err();
        assert_eq!(e.code, "unsafe_archive");
        assert_eq!(std::fs::read(data.join("php/a.db")).unwrap(), b"alpha");
        assert!(!p.base.parent().unwrap().join("escaped.txt").exists());
        assert!(!p.base.join("escaped.txt").exists());
    }

    #[test]
    fn entry_path_rules() {
        assert!(safe_entry_path("a/b.txt").is_some());
        assert!(safe_entry_path("a\\b.txt").is_some());
        assert!(safe_entry_path("../x").is_none());
        assert!(safe_entry_path("a/../../x").is_none());
        assert!(safe_entry_path("/etc/passwd").is_none());
        assert!(safe_entry_path("C:/Windows/x").is_none());
        assert!(safe_entry_path("a/b:stream").is_none());
        assert!(safe_entry_path("").is_none());
    }

    #[test]
    fn name_validation() {
        let (_d, p, _data) = fixture();
        assert_eq!(resolve(&p, "..\\config.json").unwrap_err().code, "bad_name");
        assert_eq!(resolve(&p, "x.zip").unwrap_err().code, "not_found");
        assert_eq!(resolve(&p, "a/b.zip").unwrap_err().code, "bad_name");
    }
}
