//! Small shared helpers: time, hashing, tokens, atomic file writes, shell open, system info.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug)]
pub struct LocalTime {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub hour: u16,
    pub min: u16,
    pub sec: u16,
}

#[cfg(windows)]
pub fn now_local() -> LocalTime {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    let mut st = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut st) };
    LocalTime { year: st.wYear, month: st.wMonth, day: st.wDay, hour: st.wHour, min: st.wMinute, sec: st.wSecond }
}

#[cfg(not(windows))]
pub fn now_local() -> LocalTime {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let days = (secs / 86400) as i64;
    let (y, m, d) = civil_from_days(days);
    let rem = secs % 86400;
    LocalTime { year: y as u16, month: m as u16, day: d as u16, hour: (rem / 3600) as u16, min: (rem % 3600 / 60) as u16, sec: (rem % 60) as u16 }
}

#[allow(dead_code)]
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

impl LocalTime {
    pub fn date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
    pub fn time(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.min, self.sec)
    }
    pub fn stamp(&self) -> String {
        format!("{:04}{:02}{:02}-{:02}{:02}{:02}", self.year, self.month, self.day, self.hour, self.min, self.sec)
    }
    pub fn iso(&self) -> String {
        format!("{}T{}", self.date(), self.time())
    }
}

pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

pub fn random_token() -> String {
    let mut b = [0u8; 24];
    if getrandom::getrandom(&mut b).is_err() {
        // extremely unlikely; fall back to time-derived bytes so we never run with an empty token
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(1);
        for (i, x) in b.iter_mut().enumerate() {
            *x = ((t >> ((i % 16) * 8)) as u8) ^ (i as u8).wrapping_mul(31);
        }
    }
    hex(&b)
}

/// Constant-time byte comparison (length leak only).
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut d = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        d |= x ^ y;
    }
    d == 0
}

/// Write via temp file + rename so readers never see a half-written file.
pub fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Merge `patch` into `base` (objects recursively, everything else replaced).
pub fn json_merge(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                match b.get_mut(k) {
                    Some(bv) if bv.is_object() && v.is_object() && !is_map_key(k) => json_merge(bv, v),
                    _ => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (b, p) => *b = p.clone(),
    }
}

/// Map-like config keys are replaced wholesale instead of merged (so users can delete entries).
fn is_map_key(k: &str) -> bool {
    k == "env" || k == "extraIni"
}

/// Only names we generate or accept: letters, digits, dot, dash, underscore.
pub fn safe_file_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 128
        && !n.starts_with('.')
        && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
        && !n.contains("..")
}

pub fn dir_size(p: &Path) -> u64 {
    let mut total = 0;
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            if let Ok(md) = e.metadata() {
                if md.is_dir() {
                    total += dir_size(&e.path());
                } else {
                    total += md.len();
                }
            }
        }
    }
    total
}

pub fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf())).unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(windows)]
pub fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// Open a URL or folder with the shell (default browser / Explorer).
#[cfg(windows)]
pub fn shell_open(target: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    let op = wide(std::ffi::OsStr::new("open"));
    let t = wide(std::ffi::OsStr::new(target));
    unsafe {
        ShellExecuteW(std::ptr::null_mut(), op.as_ptr(), t.as_ptr(), std::ptr::null(), std::ptr::null(), 1);
    }
}

#[cfg(not(windows))]
pub fn shell_open(target: &str) {
    let _ = std::process::Command::new("xdg-open").arg(target).spawn();
}

/// Apply CREATE_NO_WINDOW to a std command.
pub fn hide_std(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

pub fn ram_mb() -> u64 {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut m: MEMORYSTATUSEX = std::mem::zeroed();
        m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut m) != 0 {
            return m.ullTotalPhys / 1024 / 1024;
        }
    }
    0
}

pub fn disk_free_gb(path: &Path) -> f64 {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let w = wide(path.as_os_str());
        let mut free = 0u64;
        let (mut total, mut tf) = (0u64, 0u64);
        if GetDiskFreeSpaceExW(w.as_ptr(), &mut free, &mut total, &mut tf) != 0 {
            return ((free as f64 / 1024.0 / 1024.0 / 1024.0) * 10.0).round() / 10.0;
        }
    }
    let _ = path;
    0.0
}

/// Compare dotted numeric versions ("8.4.27" vs "8.4.9"); non-numeric parts compare as 0.
pub fn cmp_version(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> Vec<u64> {
        s.trim_start_matches('v').split(|c: char| c == '.' || c == '-').map(|p| p.parse::<u64>().unwrap_or(0)).collect()
    };
    let (pa, pb) = (parse(a), parse(b));
    for i in 0..pa.len().max(pb.len()) {
        let (x, y) = (pa.get(i).copied().unwrap_or(0), pb.get(i).copied().unwrap_or(0));
        if x != y {
            return x.cmp(&y);
        }
    }
    std::cmp::Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions() {
        assert_eq!(cmp_version("8.4.27", "8.4.9"), std::cmp::Ordering::Greater);
        assert_eq!(cmp_version("v1.2", "1.2.0"), std::cmp::Ordering::Equal);
    }
    #[test]
    fn ct() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
    }
    #[test]
    fn names() {
        assert!(safe_file_name("data-20240101-000000.zip"));
        assert!(!safe_file_name("../x.zip"));
        assert!(!safe_file_name("a/b.zip"));
        assert!(!safe_file_name("a\\b.zip"));
        assert!(!safe_file_name(".hidden"));
    }
    #[test]
    fn merge() {
        let mut a = serde_json::json!({"a":{"x":1,"env":{"k":"v"}},"b":2});
        json_merge(&mut a, &serde_json::json!({"a":{"y":2,"env":{"z":"1"}}}));
        assert_eq!(a["a"]["x"], 1);
        assert_eq!(a["a"]["y"], 2);
        assert!(a["a"]["env"].get("k").is_none());
    }
}
