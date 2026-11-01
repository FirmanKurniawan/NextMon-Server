//! php.ini generation from php.ini-production (pure string transform, unit-testable).
use crate::types::IniOptions;
use std::collections::HashSet;

/// Extensions NextMON wants enabled (only if their DLL exists).
pub const DEFAULT_EXTENSIONS: &[&str] =
    &["openssl", "mbstring", "sockets", "curl", "gd", "pdo_sqlite", "sqlite3", "zip", "fileinfo", "intl"];

struct Ini {
    lines: Vec<String>,
}

fn strip_semis(l: &str) -> &str {
    l.trim_start().trim_start_matches(';').trim_start()
}

impl Ini {
    /// Match `key = ...` either active or commented. Prefers the first active line.
    fn find(&self, key: &str) -> Option<usize> {
        let is_key = |l: &str, commented: bool| {
            let t = l.trim_start();
            let is_c = t.starts_with(';');
            if is_c != commented {
                return false;
            }
            let body = strip_semis(l);
            match body.split_once('=') {
                Some((k, _)) => k.trim().eq_ignore_ascii_case(key),
                None => false,
            }
        };
        self.lines
            .iter()
            .position(|l| is_key(l, false))
            .or_else(|| self.lines.iter().position(|l| is_key(l, true)))
    }

    fn set(&mut self, key: &str, value: &str) {
        let line = format!("{key} = {value}");
        match self.find(key) {
            Some(i) => self.lines[i] = line,
            None => self.lines.push(line),
        }
    }

    /// Enable `extension=name` / `zend_extension=name` (uncomment or append). Idempotent.
    fn enable_ext(&mut self, directive: &str, name: &str) {
        let matches = |l: &str, commented: bool| {
            let t = l.trim_start();
            if t.starts_with(';') != commented {
                return false;
            }
            let body = strip_semis(l);
            match body.split_once('=') {
                Some((k, v)) => {
                    k.trim().eq_ignore_ascii_case(directive) && {
                        let v = v.trim().trim_matches('"');
                        let v = v.strip_prefix("php_").unwrap_or(v);
                        let v = v.strip_suffix(".dll").unwrap_or(v);
                        v.eq_ignore_ascii_case(name)
                    }
                }
                None => false,
            }
        };
        if self.lines.iter().any(|l| matches(l, false)) {
            return;
        }
        if let Some(i) = self.lines.iter().position(|l| matches(l, true)) {
            self.lines[i] = format!("{directive}={name}");
        } else {
            self.lines.push(format!("{directive}={name}"));
        }
    }
}

/// Transform `php.ini-production` text into NextMON's php.ini.
/// `available_dlls` = lowercase file names found in `ext/` (e.g. "php_curl.dll").
pub fn transform_php_ini(src: &str, o: &IniOptions, available_dlls: &HashSet<String>) -> String {
    let eol = if src.contains("\r\n") { "\r\n" } else { "\n" };
    let mut ini = Ini { lines: src.lines().map(|l| l.to_string()).collect() };

    ini.set("extension_dir", "\"ext\"");
    ini.set("date.timezone", &o.timezone);
    ini.set("memory_limit", &o.memory_limit);
    ini.set("upload_max_filesize", &o.upload_max_filesize);
    ini.set("post_max_size", &o.post_max_size);
    ini.set("max_execution_time", &o.max_execution_time.to_string());
    ini.set("file_uploads", "On");
    ini.set("realpath_cache_size", &o.realpath_cache_size);
    ini.set("realpath_cache_ttl", "120");

    for e in &o.extensions {
        let e = e.to_ascii_lowercase();
        if available_dlls.contains(&format!("php_{e}.dll")) {
            ini.enable_ext("extension", &e);
        }
    }
    if o.opcache && available_dlls.contains("php_opcache.dll") {
        ini.enable_ext("zend_extension", "opcache");
        ini.set("opcache.enable", "1");
        ini.set("opcache.enable_cli", "0");
        ini.set("opcache.memory_consumption", "128");
        ini.set("opcache.interned_strings_buffer", "16");
        ini.set("opcache.max_accelerated_files", "10000");
        ini.set("opcache.validate_timestamps", "1");
        ini.set("opcache.revalidate_freq", "2");
    } else {
        ini.set("opcache.enable_cli", "0");
    }
    if let Some(ca) = &o.cacert {
        let ca = format!("\"{}\"", ca.replace('/', "\\"));
        ini.set("curl.cainfo", &ca);
        ini.set("openssl.cafile", &ca);
    }
    for (k, v) in &o.extra {
        ini.set(k, v);
    }
    let mut s = ini.lines.join(eol);
    s.push_str(eol);
    s
}

/// Names of `php_*.dll` modules -> extension names (for tests/diagnostics).
pub fn ext_name_from_dll(file: &str) -> Option<String> {
    let f = file.to_ascii_lowercase();
    f.strip_prefix("php_")?.strip_suffix(".dll").map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "; comment about extension_dir = foo\r\n\
memory_limit = 128M\r\n\
upload_max_filesize = 2M\r\n\
;date.timezone =\r\n\
; On windows:\r\n\
;extension_dir = \"ext\"\r\n\
;extension=curl\r\n\
;extension=gd\r\n\
extension=mbstring\r\n\
;extension=openssl\r\n\
;zend_extension=opcache\r\n\
;realpath_cache_size = 4096k\r\n\
[opcache]\r\n\
;opcache.enable_cli=0\r\n";

    fn dlls(l: &[&str]) -> HashSet<String> {
        l.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn transforms_production_ini() {
        let o = IniOptions { cacert: Some("X:/rt/cacert.pem".into()), ..Default::default() };
        let out = transform_php_ini(SAMPLE, &o, &dlls(&["php_curl.dll", "php_openssl.dll", "php_mbstring.dll", "php_opcache.dll"]));
        assert!(out.contains("\r\nextension_dir = \"ext\"\r\n"));
        assert!(out.contains("\r\nextension=curl\r\n"));
        assert!(out.contains("\r\nextension=openssl\r\n"));
        assert!(out.contains("\r\nzend_extension=opcache\r\n"));
        assert!(out.contains("\r\nopcache.enable_cli = 0\r\n"));
        assert!(!out.contains(";opcache.enable_cli"));
        assert!(out.contains("\r\ndate.timezone = Asia/Jakarta\r\n"));
        assert!(out.contains("\r\nrealpath_cache_size = 4096K\r\n"));
        assert!(out.contains("\r\nmemory_limit = 512M\r\n"));
        assert!(out.contains("\r\nupload_max_filesize = 64M\r\n"));
        assert!(out.contains("curl.cainfo = \"X:\\rt\\cacert.pem\""));
        assert!(out.contains("openssl.cafile = \"X:\\rt\\cacert.pem\""));
        // gd DLL missing -> stays disabled; prose comment untouched
        assert!(out.contains(";extension=gd"));
        assert!(out.starts_with("; comment about extension_dir = foo\r\n"));
        // mbstring was already active: exactly one active line
        assert_eq!(out.matches("extension=mbstring").count(), 1);
    }

    #[test]
    fn idempotent_and_opcache_off() {
        let o = IniOptions { opcache: false, ..Default::default() };
        let d = dlls(&["php_curl.dll", "php_opcache.dll"]);
        let once = transform_php_ini(SAMPLE, &o, &d);
        let twice = transform_php_ini(&once, &o, &d);
        assert_eq!(once, twice);
        assert!(once.contains(";zend_extension=opcache"));
        assert!(once.contains("opcache.enable_cli = 0"));
    }

    #[test]
    fn extra_overrides_and_appends() {
        let mut o = IniOptions::default();
        o.extra.insert("memory_limit".into(), "1G".into());
        o.extra.insert("my.custom".into(), "1".into());
        let out = transform_php_ini("memory_limit = 128M\n", &o, &dlls(&[]));
        assert!(out.contains("memory_limit = 1G\n"));
        assert!(out.contains("my.custom = 1\n"));
        assert!(!out.contains('\r'));
    }
}
