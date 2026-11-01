//! Health checks: run php/node, parse output, produce structured diagnostics.
use crate::types::*;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

pub const VC_REDIST_URL: &str = "https://aka.ms/vs/17/release/vc_redist.x64.exe";

const STATUS_DLL_NOT_FOUND: u32 = 0xC000_0135;
const STATUS_ENTRYPOINT_NOT_FOUND: u32 = 0xC000_0139;
const STATUS_INVALID_IMAGE: u32 = 0xC000_007B;

pub(crate) struct Captured {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub spawn_error: Option<String>,
    pub timed_out: bool,
}

/// Run `exe args` in `cwd` capturing output; no console window; hard timeout.
pub(crate) async fn run_capture(exe: &Path, args: &[&str], cwd: &Path, timeout: Duration) -> Captured {
    let mut cmd = tokio::process::Command::new(exe);
    cmd.args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    match cmd.spawn() {
        Err(e) => Captured { code: None, stdout: String::new(), stderr: String::new(), spawn_error: Some(e.to_string()), timed_out: false },
        Ok(child) => match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Ok(Ok(out)) => Captured {
                code: out.status.code(),
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
                spawn_error: None,
                timed_out: false,
            },
            Ok(Err(e)) => Captured { code: None, stdout: String::new(), stderr: String::new(), spawn_error: Some(e.to_string()), timed_out: false },
            // child dropped => kill_on_drop
            Err(_) => Captured { code: None, stdout: String::new(), stderr: String::new(), spawn_error: None, timed_out: true },
        },
    }
}

fn diag(code: &str, sev: Severity, en: impl Into<String>, id: impl Into<String>, url: Option<&str>) -> Diagnostic {
    Diagnostic { code: code.into(), severity: sev, message: en.into(), message_id: id.into(), hint_url: url.map(|s| s.to_string()) }
}

/// Inspect a failed run; pushes diagnostics and returns true when it looks like a missing/old VC++ runtime.
fn classify_failure(c: &Captured, what: &str, report: &mut HealthReport) -> bool {
    let combined = format!("{}\n{}", c.stdout, c.stderr).to_ascii_lowercase();
    let code = c.code.map(|x| x as u32);
    let vc = matches!(code, Some(STATUS_DLL_NOT_FOUND) | Some(STATUS_ENTRYPOINT_NOT_FOUND) | Some(STATUS_INVALID_IMAGE))
        || combined.contains("vcruntime140")
        || combined.contains("msvcp140");
    if let Some(e) = &c.spawn_error {
        report.diagnostics.push(diag(
            "spawn_failed",
            Severity::Error,
            format!("Cannot start {what}: {e}"),
            format!("Tidak bisa menjalankan {what}: {e}"),
            None,
        ));
    } else if c.timed_out {
        report.diagnostics.push(diag("timeout", Severity::Error, format!("{what} did not respond in time"), format!("{what} tidak merespons tepat waktu"), None));
    } else if vc {
        report.vc_runtime_missing = true;
        report.vc_redist_url = Some(VC_REDIST_URL.into());
        report.diagnostics.push(diag(
            "vc_runtime_missing",
            Severity::Error,
            format!("{what} cannot start: the Microsoft Visual C++ Redistributable (x64) is missing or outdated. Install it from {VC_REDIST_URL}"),
            format!("{what} tidak bisa dijalankan: Microsoft Visual C++ Redistributable (x64) belum terpasang atau terlalu lama. Pasang dari {VC_REDIST_URL}"),
            Some(VC_REDIST_URL),
        ));
    } else {
        report.diagnostics.push(diag(
            "bad_exit",
            Severity::Error,
            format!("{what} exited with code {:?}: {}", c.code, combined.trim().chars().take(300).collect::<String>()),
            format!("{what} berhenti dengan kode {:?}", c.code),
            None,
        ));
    }
    report.exit_code = c.code.map(|x| x as i64);
    vc
}

fn new_report(kind: RuntimeKind, dir: &Path) -> HealthReport {
    HealthReport {
        ok: false,
        kind,
        path: dir.to_string_lossy().into_owned(),
        version: None,
        npm_version: None,
        exit_code: None,
        loaded_extensions: vec![],
        missing_extensions: vec![],
        optional_missing: vec![],
        vc_runtime_missing: false,
        vc_redist_url: None,
        diagnostics: vec![],
    }
}

/// `php -v` + `php -m` against `dir` (folder containing php.exe). Run from a neutral cwd so that
/// relative `extension_dir` problems surface here rather than at app start.
pub async fn check_php(dir: &Path, required: &[String], want_opcache: bool) -> HealthReport {
    let mut r = new_report(RuntimeKind::Php, dir);
    let exe = dir.join("php.exe");
    let cwd = std::env::temp_dir();
    let ini = dir.join("php.ini");
    let ini_s = ini.to_string_lossy().into_owned();
    let mut base: Vec<&str> = vec![];
    if ini.is_file() {
        base.extend(["-c", ini_s.as_str()]);
    }
    if !exe.is_file() {
        r.diagnostics.push(diag("missing_exe", Severity::Error, "php.exe not found", "php.exe tidak ditemukan", None));
        return r;
    }
    let mut a = base.clone();
    a.push("-v");
    let v = run_capture(&exe, &a, &cwd, Duration::from_secs(30)).await;
    if v.spawn_error.is_some() || v.timed_out || v.code != Some(0) {
        classify_failure(&v, "php.exe", &mut r);
        return r;
    }
    r.exit_code = Some(0);
    r.version = parse_php_version(&v.stdout);
    for l in v.stderr.lines().chain(v.stdout.lines()).filter(|l| l.contains("Warning") || l.contains("Unable to load")) {
        r.diagnostics.push(diag("startup_warning", Severity::Warning, l.trim(), l.trim(), None));
    }

    let mut a = base;
    a.push("-m");
    let m = run_capture(&exe, &a, &cwd, Duration::from_secs(30)).await;
    if m.spawn_error.is_some() || m.timed_out || m.code != Some(0) {
        classify_failure(&m, "php -m", &mut r);
        return r;
    }
    r.loaded_extensions = parse_php_modules(&m.stdout);
    for l in m.stderr.lines().filter(|l| l.contains("Warning") || l.contains("Unable to load")) {
        r.diagnostics.push(diag("startup_warning", Severity::Warning, l.trim(), l.trim(), None));
    }
    for e in required {
        if !r.loaded_extensions.iter().any(|x| x.eq_ignore_ascii_case(e)) {
            r.missing_extensions.push(e.clone());
            r.diagnostics.push(diag(
                "missing_extension",
                Severity::Error,
                format!("Required PHP extension '{e}' is not loaded"),
                format!("Ekstensi PHP wajib '{e}' tidak termuat"),
                None,
            ));
        }
    }
    let optional: Vec<(&str, &str)> = {
        let mut o = vec![("intl", "intl")];
        if want_opcache {
            o.push(("opcache", "zend opcache"));
        }
        o
    };
    for (label, module) in optional {
        if !r.loaded_extensions.iter().any(|x| x == module) {
            r.optional_missing.push(label.into());
            r.diagnostics.push(diag(
                "optional_extension_missing",
                Severity::Info,
                format!("Optional extension '{label}' is not loaded"),
                format!("Ekstensi opsional '{label}' tidak termuat"),
                None,
            ));
        }
    }
    r.ok = r.missing_extensions.is_empty();
    r
}

pub(crate) fn parse_php_version(out: &str) -> Option<String> {
    // "PHP 8.4.27 (cli) (built: ...) (NTS Visual C++ 2022 x64)"
    let first = out.lines().find(|l| l.starts_with("PHP "))?;
    first.split_whitespace().nth(1).map(|s| s.to_string())
}

pub(crate) fn parse_php_modules(out: &str) -> Vec<String> {
    let mut v: Vec<String> = out
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('['))
        .map(|l| l.to_ascii_lowercase())
        .collect();
    v.sort();
    v.dedup();
    v
}

/// `node -v` + `npm -v` against `dir` (folder containing node.exe).
pub async fn check_node(dir: &Path) -> HealthReport {
    let mut r = new_report(RuntimeKind::Node, dir);
    let exe = dir.join("node.exe");
    if !exe.is_file() {
        r.diagnostics.push(diag("missing_exe", Severity::Error, "node.exe not found", "node.exe tidak ditemukan", None));
        return r;
    }
    let cwd = std::env::temp_dir();
    let v = run_capture(&exe, &["-v"], &cwd, Duration::from_secs(30)).await;
    if v.spawn_error.is_some() || v.timed_out || v.code != Some(0) {
        classify_failure(&v, "node.exe", &mut r);
        return r;
    }
    r.exit_code = Some(0);
    r.version = Some(v.stdout.trim().trim_start_matches('v').to_string());
    // Run npm's JS entry directly with our node (avoids cmd.exe / PATH lookups).
    let npm_cli = dir.join("node_modules").join("npm").join("bin").join("npm-cli.js");
    if npm_cli.is_file() {
        let cli = npm_cli.to_string_lossy().into_owned();
        let n = run_capture(&exe, &[cli.as_str(), "-v"], &cwd, Duration::from_secs(60)).await;
        if n.code == Some(0) {
            r.npm_version = Some(n.stdout.trim().to_string());
        } else {
            r.diagnostics.push(diag(
                "npm_failed",
                Severity::Error,
                format!("npm -v failed: {}", n.stderr.trim().chars().take(200).collect::<String>()),
                "npm -v gagal",
                None,
            ));
        }
    } else {
        r.diagnostics.push(diag("npm_missing", Severity::Error, "npm not found in this Node distribution", "npm tidak ada di distribusi Node ini", None));
    }
    r.ok = r.npm_version.is_some();
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_php_output() {
        assert_eq!(parse_php_version("PHP 8.4.27 (cli) (built: Oct 2026) (NTS Visual C++ 2022 x64)\nCopyright"), Some("8.4.27".into()));
        let m = parse_php_modules("[PHP Modules]\nCore\ncurl\nMbstring\n\n[Zend Modules]\nZend OPcache\n");
        assert!(m.contains(&"mbstring".to_string()));
        assert!(m.contains(&"zend opcache".to_string()));
        assert!(!m.iter().any(|x| x.starts_with('[')));
        assert_eq!(m.iter().filter(|x| *x == "zend opcache").count(), 1);
    }

    #[test]
    fn classifies_vc_missing() {
        let mut r = new_report(RuntimeKind::Php, Path::new("x"));
        let c = Captured { code: Some(0xC000_0135u32 as i32), stdout: String::new(), stderr: String::new(), spawn_error: None, timed_out: false };
        assert!(classify_failure(&c, "php.exe", &mut r));
        assert!(r.vc_runtime_missing);
        assert_eq!(r.vc_redist_url.as_deref(), Some("https://aka.ms/vs/17/release/vc_redist.x64.exe"));
        assert_eq!(r.diagnostics[0].code, "vc_runtime_missing");
        assert!(r.diagnostics[0].message_id.contains("Visual C++"));

        let mut r2 = new_report(RuntimeKind::Php, Path::new("x"));
        let c2 = Captured { code: Some(1), stdout: String::new(), stderr: "The program can't start because VCRUNTIME140.dll is missing".into(), spawn_error: None, timed_out: false };
        assert!(classify_failure(&c2, "php.exe", &mut r2));
        let mut r3 = new_report(RuntimeKind::Php, Path::new("x"));
        let c3 = Captured { code: Some(1), stdout: String::new(), stderr: "boom".into(), spawn_error: None, timed_out: false };
        assert!(!classify_failure(&c3, "php.exe", &mut r3));
    }
}
