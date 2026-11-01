//! "Doctor": environment checks shown in the Diagnostics page and by `nextmon-server doctor`, plus the safe auto-fixes.
use crate::config::{Config, Edition};
use crate::paths::Paths;
use crate::runtime_api::{InstalledRt, RuntimeApi};
use crate::winproc;
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;

pub const REQUIRED_PHP_EXT: [&str; 9] = ["openssl", "mbstring", "sockets", "curl", "gd", "pdo_sqlite", "sqlite3", "zip", "fileinfo"];
const VC_REDIST_URL: &str = "https://aka.ms/vs/17/release/vc_redist.x64.exe";
const WEBVIEW2_URL: &str = "https://developer.microsoft.com/microsoft-edge/webview2/";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub id: String,
    /// pass | warn | fail
    pub status: &'static str,
    pub title: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint_url: Option<String>,
    pub fixable: bool,
}

pub fn vc_runtime_present() -> bool {
    let sys = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    Path::new(&sys).join("System32").join("vcruntime140.dll").is_file()
}

pub fn webview2_version() -> Option<String> {
    const GUID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    for (root, sub) in [
        ("HKLM", format!("SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{GUID}")),
        ("HKLM", format!("SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\{GUID}")),
        ("HKCU", format!("Software\\Microsoft\\EdgeUpdate\\Clients\\{GUID}")),
    ] {
        if let Some(v) = winproc::reg_get_string(root, &sub, "pv") {
            if !v.is_empty() && v != "0.0.0.0" {
                return Some(v);
            }
        }
    }
    None
}

fn run_capture(exe: &Path, args: &[&str]) -> Option<String> {
    let mut c = std::process::Command::new(exe);
    c.args(args);
    crate::util::hide_std(&mut c);
    let out = c.output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Lower-case module names from `php -m`.
pub fn php_modules(php_exe: &Path, ini: Option<&Path>) -> Option<Vec<String>> {
    let mut args: Vec<String> = vec![];
    if let Some(i) = ini {
        args.push("-c".into());
        args.push(i.to_string_lossy().into_owned());
    }
    args.push("-m".into());
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let text = run_capture(php_exe, &refs)?;
    Some(text.lines().map(|l| l.trim().to_ascii_lowercase()).filter(|l| !l.is_empty() && !l.starts_with('[')).collect())
}

fn pick_runtime(rt: &Arc<dyn RuntimeApi>, cfg: &Config, ed: Edition) -> Option<InstalledRt> {
    let installed = rt.list_installed(ed.runtime_kind());
    let want = &cfg.app(ed).runtime;
    if want.is_empty() {
        installed.into_iter().next()
    } else {
        installed.into_iter().find(|i| &i.dir_name == want || &i.version == want)
    }
}

pub fn run(paths: &Paths, cfg: &Config, rt: &Arc<dyn RuntimeApi>, active_running: Option<(Edition, u16)>) -> Vec<Check> {
    let id = cfg.language == "id";
    let t = |idt: &str, en: &str| if id { idt.to_string() } else { en.to_string() };
    let mut out: Vec<Check> = vec![];
    let mut add = |cid: &str, status: &'static str, title: String, detail: String, hint: Option<String>, hint_url: Option<&str>, fixable: bool| {
        out.push(Check { id: cid.into(), status, title, detail, hint, hint_url: hint_url.map(String::from), fixable });
    };

    // VC++ runtime
    if vc_runtime_present() {
        add("vcredist", "pass", "Microsoft Visual C++ Runtime".into(), t("vcruntime140.dll ditemukan (x64)", "vcruntime140.dll found (x64)"), None, None, false);
    } else {
        add(
            "vcredist",
            "fail",
            t("Microsoft Visual C++ Runtime belum terpasang", "Microsoft Visual C++ Runtime is missing"),
            t("PHP dan Node.js butuh Visual C++ Redistributable 2015-2022 x64.", "PHP and Node.js need the Visual C++ 2015-2022 x64 Redistributable."),
            Some(t("Unduh dan pasang vc_redist.x64.exe dari Microsoft.", "Download and install vc_redist.x64.exe from Microsoft.")),
            Some(VC_REDIST_URL),
            false,
        );
    }

    // Port of the active edition (the other edition never runs at the same time, so it is only informational)
    let ed = cfg.edition;
    let port = cfg.app(ed).port;
    if active_running == Some((ed, port)) {
        add("ports", "pass", t("Port tidak bentrok", "No port conflict"), t(&format!("Port {port} dipakai oleh NextMON ({})", ed.label()), &format!("Port {port} is used by NextMON ({})", ed.label())), None, None, false);
    } else if crate::net::port_free(&cfg.bind, port) {
        add("ports", "pass", t("Port tidak bentrok", "No port conflict"), t(&format!("Port {port} bebas"), &format!("Port {port} is free")), None, None, false);
    } else {
        let who = winproc::port_owner(port).map(|p| format!("{} (PID {p})", winproc::process_name(p).unwrap_or_else(|| "?".into()))).unwrap_or_else(|| "?".into());
        add(
            "ports",
            "fail",
            t("Port bentrok", "Port conflict"),
            t(&format!("Port {port} dipakai {who}"), &format!("Port {port} is used by {who}")),
            Some(t("Ganti port di halaman Aplikasi.", "Change the port on the Applications page.")),
            None,
            false,
        );
    }

    // Runtime + application per edition
    for e in Edition::ALL {
        let kind = e.runtime_kind();
        let active = e == cfg.edition;
        let r = pick_runtime(rt, cfg, e);
        match &r {
            Some(r) => add(&format!("runtime-{kind}"), "pass", format!("Runtime {}", if kind == "php" { "PHP" } else { "Node.js" }), format!("{} {}", kind, r.version), None, None, false),
            None => add(
                &format!("runtime-{kind}"),
                if active { "fail" } else { "warn" },
                t(&format!("Runtime {kind} belum terpasang"), &format!("{kind} runtime not installed")),
                t(&format!("Edisi {} membutuhkannya", e.label()), &format!("The {} edition needs it", e.label())),
                Some(t("Pasang dari halaman Runtime.", "Install it on the Runtime page.")),
                None,
                false,
            ),
        }
        let marker = match e {
            Edition::Php => paths.app_dir(e).join("public").join("index.php"),
            Edition::Next => paths.app_dir(e).join("server.js"),
        };
        add(
            &format!("app-{}", e.as_str()),
            if marker.is_file() { "pass" } else if active { "fail" } else { "warn" },
            t(&format!("Aplikasi {}", e.label()), &format!("{} application", e.label())),
            if marker.is_file() { marker.to_string_lossy().into_owned() } else { t(&format!("tidak ditemukan: {}", marker.display()), &format!("not found: {}", marker.display())) },
            None,
            None,
            false,
        );

        if e == Edition::Php {
            if let Some(r) = &r {
                let ini = r.path.join("php.ini");
                match php_modules(&r.path.join("php.exe"), if ini.is_file() { Some(&ini) } else { None }) {
                    Some(mods) => {
                        let missing: Vec<&str> = REQUIRED_PHP_EXT.iter().copied().filter(|m| !mods.iter().any(|x| x == m)).collect();
                        let no_intl = !mods.iter().any(|x| x == "intl");
                        let fixable = fixable_extensions(&r.path, &missing, no_intl);
                        if !missing.is_empty() {
                            add(
                                "php-ext",
                                "fail",
                                t("Ekstensi PHP wajib", "Required PHP extensions"),
                                t(&format!("Ekstensi tidak aktif di PHP {}: {}", r.version, missing.join(", ")), &format!("Not enabled in PHP {}: {}", r.version, missing.join(", "))),
                                Some(t("Aktifkan extension=... di php.ini runtime (tombol Perbaiki bila tersedia).", "Enable extension=... in the runtime php.ini (use Fix when available).")),
                                None,
                                fixable,
                            );
                        } else if no_intl {
                            add(
                                "php-ext",
                                "warn",
                                t("Ekstensi PHP wajib", "Required PHP extensions"),
                                t(&format!("Ekstensi wajib lengkap; intl (opsional) tidak aktif di PHP {}", r.version), &format!("All required extensions present; optional intl is not enabled in PHP {}", r.version)),
                                Some(t("Aktifkan extension=intl di php.ini runtime.", "Enable extension=intl in the runtime php.ini.")),
                                None,
                                fixable,
                            );
                        } else {
                            add("php-ext", "pass", t("Ekstensi PHP wajib", "Required PHP extensions"), t(&format!("Semua ekstensi ada di PHP {}", r.version), &format!("All extensions present in PHP {}", r.version)), None, None, false);
                        }
                    }
                    None => add(
                        "php-ext",
                        "fail",
                        t("php.exe tidak bisa dijalankan", "php.exe cannot run"),
                        t("Periksa VC++ Runtime atau pasang ulang PHP.", "Check the VC++ Runtime or reinstall PHP."),
                        None,
                        Some(VC_REDIST_URL),
                        false,
                    ),
                }
            }
        } else if let Some(r) = &r {
            match run_capture(&r.path.join("node.exe"), &["-v"]) {
                Some(v) => {
                    let ver = v.trim().trim_start_matches('v').to_string();
                    let ok = crate::util::cmp_version(&ver, &cfg.min_node).is_ge();
                    add(
                        "node-min",
                        if ok { "pass" } else { "fail" },
                        t("Versi Node.js memenuhi syarat Next.js", "Node.js version meets the Next.js requirement"),
                        t(&format!("Node {ver} (minimal {})", cfg.min_node), &format!("Node {ver} (minimum {})", cfg.min_node)),
                        if ok { None } else { Some(t("Pasang atau pakai Node.js yang lebih baru di halaman Runtime.", "Install or select a newer Node.js on the Runtime page.")) },
                        None,
                        false,
                    );
                }
                None => add(
                    "node-min",
                    "fail",
                    t("node.exe tidak bisa dijalankan", "node.exe cannot run"),
                    t("Periksa VC++ Runtime atau pasang ulang Node.js.", "Check the VC++ Runtime or reinstall Node.js."),
                    None,
                    Some(VC_REDIST_URL),
                    false,
                ),
            }
        }
    }

    // Data dir writable
    let data = paths.data_root(&cfg.data_dir).join(cfg.edition.as_str());
    let probe = data.join(".write-test");
    let writable = std::fs::create_dir_all(&data).and_then(|_| std::fs::write(&probe, b"x")).is_ok();
    let _ = std::fs::remove_file(&probe);
    add(
        "data-write",
        if writable { "pass" } else { "fail" },
        t("Izin tulis folder data", "Data folder is writable"),
        if writable { t(&format!("{} dapat ditulis", data.display()), &format!("{} is writable", data.display())) } else { t(&format!("{} tidak bisa ditulis", data.display()), &format!("{} is not writable", data.display())) },
        if writable { None } else { Some(t("Pindahkan NextMON Server ke folder yang dapat ditulis (bukan Program Files).", "Move NextMON Server to a writable folder (not Program Files).")) },
        None,
        false,
    );

    // WebView2
    match webview2_version() {
        Some(v) => add("webview2", "pass", "WebView2 Runtime".into(), t(&format!("Terpasang ({v})"), &format!("Installed ({v})")), None, None, false),
        None => add(
            "webview2",
            "warn",
            "WebView2 Runtime".into(),
            t("Tidak ditemukan; panel dibuka di browser bawaan.", "Not found; the panel opens in your default browser."),
            Some(t("Pasang Microsoft Edge WebView2 agar panel tampil di jendela sendiri.", "Install Microsoft Edge WebView2 to get the panel in its own window.")),
            Some(WEBVIEW2_URL),
            false,
        ),
    }

    // LAN exposure
    if cfg.bind == "0.0.0.0" {
        add(
            "lan",
            "warn",
            t("Aplikasi terbuka untuk jaringan lokal", "App is reachable from the LAN"),
            t("bind 0.0.0.0: perangkat lain di jaringan dapat membuka aplikasi.", "bind 0.0.0.0: other devices on the network can open the app."),
            Some(t("Izinkan port di Windows Firewall hanya untuk jaringan privat; jangan buka ke internet.", "Allow the port in Windows Firewall for private networks only; never expose it to the internet.")),
            None,
            false,
        );
    }

    // Disk
    let free = crate::util::disk_free_gb(&paths.base);
    if free > 0.0 {
        add("disk", if free < 1.0 { "warn" } else { "pass" }, t("Ruang disk", "Disk space"), format!("{free} GB"), None, None, false);
    }
    out
}

/// Extensions we could switch on because their DLL exists in the runtime's ext/ folder.
fn fixable_extensions(php_dir: &Path, missing_required: &[&str], no_intl: bool) -> bool {
    let mut want: Vec<&str> = missing_required.to_vec();
    if no_intl {
        want.push("intl");
    }
    want.iter().any(|e| php_dir.join("ext").join(format!("php_{e}.dll")).is_file())
}

#[derive(Debug)]
pub struct FixError {
    pub code: &'static str,
    pub message: String,
}

/// Enable PHP extensions whose DLLs exist but are switched off in php.ini. Returns the list of extensions enabled.
pub fn fix_php_extensions(php_dir: &Path) -> Result<Vec<String>, FixError> {
    let ini_path = php_dir.join("php.ini");
    let text = std::fs::read_to_string(&ini_path).map_err(|e| FixError { code: "not_fixable", message: format!("php.ini not readable: {e}") })?;
    let mods = php_modules(&php_dir.join("php.exe"), Some(&ini_path)).unwrap_or_default();
    let mut wanted: Vec<&str> = REQUIRED_PHP_EXT.to_vec();
    wanted.push("intl");
    let mut enabled = vec![];
    let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    for ext in wanted {
        if mods.iter().any(|m| m == ext) || !php_dir.join("ext").join(format!("php_{ext}.dll")).is_file() {
            continue;
        }
        let forms = [format!("extension={ext}"), format!("extension=php_{ext}.dll")];
        let mut done = false;
        for l in lines.iter_mut() {
            let tl = l.trim().trim_start_matches(';').trim();
            if forms.iter().any(|f| tl.eq_ignore_ascii_case(f)) {
                *l = tl.to_string();
                done = true;
                break;
            }
        }
        if !done {
            lines.push(format!("extension={ext}"));
        }
        enabled.push(ext.to_string());
    }
    if enabled.is_empty() {
        return Err(FixError { code: "not_fixable", message: "nothing to enable automatically".into() });
    }
    let bak = php_dir.join("php.ini.bak");
    if !bak.exists() {
        let _ = std::fs::copy(&ini_path, &bak);
    }
    crate::util::atomic_write(&ini_path, (lines.join("\r\n") + "\r\n").as_bytes()).map_err(|e| FixError { code: "io", message: e.to_string() })?;
    Ok(enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webview_and_vc_checks_do_not_panic() {
        let _ = webview2_version();
        let _ = vc_runtime_present();
    }

    #[test]
    fn fixable_requires_dll() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("ext")).unwrap();
        assert!(!fixable_extensions(d.path(), &["gd"], true));
        std::fs::write(d.path().join("ext").join("php_intl.dll"), b"x").unwrap();
        assert!(fixable_extensions(d.path(), &[], true));
    }
}
