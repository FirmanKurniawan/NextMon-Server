//! Safe zip extraction (zip-slip protection) and sha256 helpers. Blocking; call via spawn_blocking.
use crate::types::CancelToken;
use crate::{Error, Result};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

/// Hard cap on total uncompressed bytes (zip-bomb guard).
const MAX_TOTAL: u64 = 4 * 1024 * 1024 * 1024;

/// Validate an archive entry name and return a normalised relative path.
/// Rejects absolute paths, `..`, drive prefixes, ':' (drive / NTFS streams) and NULs.
pub fn safe_relative(name: &str) -> Result<PathBuf> {
    let bad = || Error::UnsafeArchivePath(name.to_string());
    if name.contains('\0') {
        return Err(bad());
    }
    let norm = name.replace('\\', "/");
    if norm.starts_with('/') {
        return Err(bad());
    }
    let mut out = PathBuf::new();
    for part in norm.split('/') {
        match part {
            "" | "." => continue,
            ".." => return Err(bad()),
            p => {
                if p.contains(':') {
                    return Err(bad());
                }
                out.push(p);
            }
        }
    }
    // Belt and braces: only Normal components allowed.
    if out.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(bad());
    }
    Ok(out)
}

/// Extract `zip_path` into `dest` (created if missing). When `strip_single_root` and every entry
/// shares one top-level folder, that folder is removed (Node: `node-v22.x-win-x64/`).
/// `progress(done_entries, total_entries)`. Returns number of files written.
pub fn extract_zip(
    zip_path: &Path,
    dest: &Path,
    strip_single_root: bool,
    cancel: &CancelToken,
    progress: &dyn Fn(u64, u64),
) -> Result<u64> {
    let file = File::open(zip_path)?;
    let mut ar = zip::ZipArchive::new(file)?;
    let total = ar.len();

    // Pass 1: validate every name up-front so nothing is written for a malicious archive.
    let mut names: Vec<PathBuf> = Vec::with_capacity(total);
    for i in 0..total {
        let e = ar.by_index_raw(i)?;
        names.push(safe_relative(&e.name()?)?);
    }
    let strip = strip_single_root && single_root(&names);

    fs::create_dir_all(dest)?;
    let mut written = 0u64;
    let mut bytes_total = 0u64;
    let mut buf = vec![0u8; 256 * 1024];
    for i in 0..total {
        cancel.check()?;
        let mut entry = ar.by_index(i)?;
        let mut rel = names[i].clone();
        if strip {
            rel = rel.components().skip(1).collect();
        }
        if rel.as_os_str().is_empty() {
            continue;
        }
        if let Some(mode) = entry.unix_mode() {
            if mode & 0o170000 == 0o120000 {
                return Err(Error::UnsafeArchivePath(format!("{} (symlink)", entry.name()?)));
            }
        }
        let out = dest.join(&rel);
        // Lexical containment re-check.
        if !out.starts_with(dest) {
            return Err(Error::UnsafeArchivePath(entry.name()?.to_string()));
        }
        if entry.is_dir() {
            fs::create_dir_all(&out)?;
        } else {
            if let Some(p) = out.parent() {
                fs::create_dir_all(p)?;
            }
            let mut f = File::create(&out)?;
            loop {
                let n = entry.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                bytes_total += n as u64;
                if bytes_total > MAX_TOTAL {
                    return Err(Error::Archive("archive expands beyond size limit".into()));
                }
                f.write_all(&buf[..n])?;
            }
            written += 1;
        }
        if i % 25 == 0 || i + 1 == total {
            progress(i as u64 + 1, total as u64);
        }
    }
    Ok(written)
}

fn single_root(names: &[PathBuf]) -> bool {
    let mut root: Option<std::ffi::OsString> = None;
    let mut nested = false;
    for n in names {
        let mut c = n.components();
        let first = match c.next() {
            Some(Component::Normal(s)) => s.to_os_string(),
            _ => continue,
        };
        if c.next().is_some() {
            nested = true;
        }
        match &root {
            None => root = Some(first),
            Some(r) if *r == first => {}
            Some(_) => return false,
        }
    }
    root.is_some() && nested
}

/// Streaming sha256 of a file, lowercase hex.
pub fn sha256_file(path: &Path, cancel: &CancelToken) -> Result<String> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        cancel.check()?;
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// Recursive directory size in bytes (best effort).
pub fn dir_size(path: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(d) = stack.pop() {
        if let Ok(rd) = fs::read_dir(&d) {
            for e in rd.flatten() {
                match e.metadata() {
                    Ok(m) if m.is_dir() => stack.push(e.path()),
                    Ok(m) => total += m.len(),
                    Err(_) => {}
                }
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;

    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let f = File::create(path).unwrap();
        let mut w = zip::ZipWriter::new(f);
        for (n, d) in entries {
            w.start_file(*n, SimpleFileOptions::default()).unwrap();
            w.write_all(d).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn rejects_zip_slip_names() {
        for bad in ["../evil.txt", "a/../../evil", "/abs.txt", "C:/win.txt", "a\\..\\..\\x", "x:y", "\\\\srv\\share"] {
            assert!(safe_relative(bad).is_err(), "{bad} should be rejected");
        }
        assert_eq!(safe_relative("a\\b/c.txt").unwrap(), PathBuf::from("a").join("b").join("c.txt"));
    }

    #[test]
    fn extract_blocks_slip_and_writes_nothing() {
        let t = tempfile::tempdir().unwrap();
        let z = t.path().join("bad.zip");
        make_zip(&z, &[("ok.txt", b"1"), ("../evil.txt", b"x")]);
        let dest = t.path().join("out");
        let r = extract_zip(&z, &dest, false, &CancelToken::new(), &|_, _| {});
        assert!(matches!(r, Err(Error::UnsafeArchivePath(_))));
        assert!(!t.path().join("evil.txt").exists());
        assert!(!dest.join("ok.txt").exists(), "nothing may be written for a malicious archive");
    }

    #[test]
    fn strips_single_root_folder() {
        let t = tempfile::tempdir().unwrap();
        let z = t.path().join("n.zip");
        make_zip(&z, &[("node-v1-win-x64/node.exe", b"MZ"), ("node-v1-win-x64/sub/x.js", b"1")]);
        let dest = t.path().join("out");
        extract_zip(&z, &dest, true, &CancelToken::new(), &|_, _| {}).unwrap();
        assert!(dest.join("node.exe").is_file());
        assert!(dest.join("sub").join("x.js").is_file());
    }

    #[test]
    fn flat_zip_not_stripped() {
        let t = tempfile::tempdir().unwrap();
        let z = t.path().join("p.zip");
        make_zip(&z, &[("php.exe", b"MZ"), ("ext/php_curl.dll", b"1")]);
        let dest = t.path().join("out");
        extract_zip(&z, &dest, true, &CancelToken::new(), &|_, _| {}).unwrap();
        assert!(dest.join("php.exe").is_file());
        assert!(dest.join("ext").join("php_curl.dll").is_file());
    }

    #[test]
    fn cancel_stops_extraction() {
        let t = tempfile::tempdir().unwrap();
        let z = t.path().join("c.zip");
        make_zip(&z, &[("a", b"1"), ("b", b"2")]);
        let c = CancelToken::new();
        c.cancel();
        let r = extract_zip(&z, &t.path().join("o"), false, &c, &|_, _| {});
        assert!(matches!(r, Err(Error::Cancelled)));
    }

    #[test]
    fn sha256_known_vector() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("abc");
        fs::write(&p, b"abc").unwrap();
        assert_eq!(
            sha256_file(&p, &CancelToken::new()).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
