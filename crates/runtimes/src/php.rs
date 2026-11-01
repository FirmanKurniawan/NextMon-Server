//! PHP release metadata parsing (releases.json + archives/ index scraping). Pure functions.
use crate::types::{AvailableRuntime, Options, Variant};
use crate::version::Version;
use crate::{Error, Result};
use serde_json::Value;

/// Build toolchain required by a PHP series on Windows: vs16 for 8.0-8.3, vs17 for 8.4+.
pub fn expected_build(v: &Version) -> &'static str {
    if (v.major, v.minor) >= (8, 4) {
        "vs17"
    } else {
        "vs16"
    }
}

fn parse_size_mb(s: &str) -> Option<f64> {
    let t = s.trim();
    let (num, mult) = if let Some(x) = t.strip_suffix("MB") {
        (x, 1.0)
    } else if let Some(x) = t.strip_suffix("KB") {
        (x, 1.0 / 1024.0)
    } else if let Some(x) = t.strip_suffix("GB") {
        (x, 1024.0)
    } else {
        (t, 1.0)
    };
    num.trim().parse::<f64>().ok().map(|n| n * mult)
}

/// Does series `v` pass `Options` (min version + optional allow-list)?
pub fn series_allowed(v: &Version, o: &Options) -> bool {
    let min = Version::parse(&o.min_php).unwrap_or(Version::new(8, 1, 0));
    if (v.major, v.minor) < (min.major, min.minor) {
        return false;
    }
    o.php_series.is_empty() || o.php_series.iter().any(|s| Version::parse(s).map(|p| p.same_minor(v)).unwrap_or(false))
}

/// Parse releases.json (latest patch per series). `base` is the URL directory the `path` values live in.
/// Returns only x64 zips for the requested `variant`, series filtered by `Options`.
pub fn parse_releases(json: &str, base: &str, variant: Variant, o: &Options) -> Result<Vec<AvailableRuntime>> {
    let root: Value = serde_json::from_str(json)?;
    let obj = root.as_object().ok_or_else(|| Error::Parse("releases.json is not an object".into()))?;
    let base = base.trim_end_matches('/');
    let mut out = vec![];
    for (_minor, rel) in obj {
        let full = match rel.get("version").and_then(|v| v.as_str()) {
            Some(v) => v,
            None => continue,
        };
        let ver = match Version::parse(full) {
            Some(v) => v,
            None => continue,
        };
        if !series_allowed(&ver, o) {
            continue;
        }
        // Keys look like "nts-vs17-x64" / "ts-vs16-x64"; pick the one for our variant (any vs/vc generation).
        let prefix = format!("{}-", variant.as_str());
        let Some(rel_obj) = rel.as_object() else { continue };
        let key = rel_obj
            .keys()
            .filter(|k| k.starts_with(&prefix) && k.ends_with("-x64"))
            .max() // vs17 > vs16 if both ever exist
            .cloned();
        let Some(key) = key else { continue };
        let build = key.split('-').nth(1).unwrap_or("").to_string();
        let Some(zip) = rel_obj[&key].get("zip") else { continue };
        let Some(path) = zip.get("path").and_then(|p| p.as_str()) else { continue };
        out.push(AvailableRuntime {
            version: ver.short(),
            full_version: ver.to_string(),
            channel: "stable".into(),
            lts_name: None,
            date: rel_obj[&key].get("mtime").and_then(|m| m.as_str()).map(|m| m.chars().take(10).collect()),
            url: format!("{base}/{path}"),
            sha256: zip.get("sha256").and_then(|s| s.as_str()).map(|s| s.to_ascii_lowercase()),
            size_mb: zip.get("size").and_then(|s| s.as_str()).and_then(parse_size_mb),
            is_newer: false,
            tsnts: variant.as_str().into(),
            build,
            unverified: false,
        });
    }
    out.sort_by(|a, b| Version::parse(&b.full_version).cmp(&Version::parse(&a.full_version)));
    Ok(out)
}

/// Parse a PHP Windows archive file name -> (version, is_nts, build).
/// "php-8.4.2-nts-Win32-vs17-x64.zip" (NTS), "php-8.4.2-Win32-vs17-x64.zip" (TS), "php-8.4.27-1-nts-Win32-vs17-x64.zip".
pub fn parse_archive_name(name: &str) -> Option<(Version, bool, String)> {
    let stem = name.strip_prefix("php-")?.strip_suffix("-x64.zip")?;
    if stem.starts_with("debug-pack") || stem.starts_with("devel-pack") || stem.starts_with("test-pack") {
        return None;
    }
    let toks: Vec<&str> = stem.split('-').collect();
    // [ver, (rev)?, (nts)?, Win32, vsNN]
    let n = toks.len();
    if n < 3 || toks[n - 2] != "Win32" {
        return None;
    }
    let build = toks[n - 1].to_string();
    if !(build.starts_with("vs") || build.starts_with("vc")) {
        return None;
    }
    let mut mid = &toks[..n - 2];
    let nts = mid.last() == Some(&"nts");
    if nts {
        mid = &mid[..mid.len() - 1];
    }
    let ver_s = mid.join("-");
    Some((Version::parse(&ver_s)?, nts, build))
}

/// Scrape the archives/ directory listing for a version (matching patch; highest revision wins).
/// There is no published checksum in archives, so the result is `unverified`.
pub fn find_in_archives(html: &str, base: &str, want: &Version, variant: Variant) -> Option<AvailableRuntime> {
    let base = base.trim_end_matches('/');
    let mut best: Option<(Version, String, String)> = None;
    for part in html.split("href=\"").skip(1) {
        let Some(name) = part.split('"').next() else { continue };
        let Some((v, nts, build)) = parse_archive_name(name) else { continue };
        if (variant == Variant::Nts) != nts || !(v.major == want.major && v.minor == want.minor && v.patch == want.patch) {
            continue;
        }
        if want.rev != 0 && v.rev != want.rev {
            continue;
        }
        if best.as_ref().map(|(b, _, _)| v > *b).unwrap_or(true) {
            best = Some((v, name.to_string(), build));
        }
    }
    let (v, name, build) = best?;
    Some(AvailableRuntime {
        version: v.short(),
        full_version: v.to_string(),
        channel: "archive".into(),
        lts_name: None,
        date: None,
        url: format!("{base}/{name}"),
        sha256: None,
        size_mb: None,
        is_newer: false,
        tsnts: variant.as_str().into(),
        build,
        unverified: true,
    })
}

/// Directory name for an installed PHP: "8.4.27" (NTS) / "8.4.27-ts".
pub fn dir_name(version: &str, variant: Variant) -> String {
    match variant {
        Variant::Nts => version.to_string(),
        Variant::Ts => format!("{version}-ts"),
    }
}

/// Inverse of `dir_name`: (version, variant).
pub fn parse_dir_name(dir: &str) -> Option<(String, Variant)> {
    let (v, var) = match dir.strip_suffix("-ts") {
        Some(v) => (v, Variant::Ts),
        None => (dir, Variant::Nts),
    };
    Version::parse(v).map(|p| (p.short(), var))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIX: &str = include_str!("../tests/fixtures/php_releases.json");

    #[test]
    fn parses_fixture_nts_default() {
        let o = Options::default();
        let l = parse_releases(FIX, "https://windows.php.net/downloads/releases", Variant::Nts, &o).unwrap();
        let minors: Vec<_> = l.iter().map(|r| Version::parse(&r.version).unwrap().minor_key()).collect();
        assert_eq!(minors, vec!["8.5", "8.4", "8.3", "8.2", "8.1"], "7.4/8.0 must be filtered (min 8.1), newest first");
        let r84 = l.iter().find(|r| r.version.starts_with("8.4.")).unwrap();
        assert_eq!(r84.tsnts, "nts");
        assert_eq!(r84.build, "vs17");
        assert!(r84.url.ends_with("-nts-Win32-vs17-x64.zip"), "{}", r84.url);
        assert_eq!(r84.sha256.as_deref().map(str::len), Some(64));
        assert!(r84.size_mb.unwrap() > 20.0);
        assert!(r84.full_version.starts_with(&r84.version));
        let r83 = l.iter().find(|r| r.version.starts_with("8.3.")).unwrap();
        assert_eq!(r83.build, "vs16");
    }

    #[test]
    fn ts_variant_and_series_allowlist() {
        let o = Options { php_series: vec!["8.4".into()], ..Default::default() };
        let l = parse_releases(FIX, "https://x/", Variant::Ts, &o).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].tsnts, "ts");
        assert!(l[0].url.contains("-Win32-vs17-x64.zip") && !l[0].url.contains("-nts-"));
    }

    #[test]
    fn build_toolchain_rule() {
        assert_eq!(expected_build(&Version::new(8, 3, 1)), "vs16");
        assert_eq!(expected_build(&Version::new(8, 4, 0)), "vs17");
        assert_eq!(expected_build(&Version::new(8, 5, 0)), "vs17");
    }

    #[test]
    fn archive_names() {
        let (v, nts, b) = parse_archive_name("php-8.4.2-nts-Win32-vs17-x64.zip").unwrap();
        assert_eq!((v, nts, b.as_str()), (Version::new(8, 4, 2), true, "vs17"));
        let (v, nts, _) = parse_archive_name("php-8.4.2-Win32-vs17-x64.zip").unwrap();
        assert_eq!((v.patch, nts), (2, false));
        let (v, nts, _) = parse_archive_name("php-8.4.27-1-nts-Win32-vs17-x64.zip").unwrap();
        assert_eq!((v.rev, nts), (1, true));
        assert!(parse_archive_name("php-debug-pack-8.4.2-Win32-vs17-x64.zip").is_none());
        assert!(parse_archive_name("php-8.4.2-Win32-vs17-x86.zip").is_none());
    }

    #[test]
    fn archive_lookup_picks_variant() {
        let html = r#"<a href="php-8.4.2-Win32-vs17-x64.zip">x</a><a href="php-8.4.2-nts-Win32-vs17-x64.zip">x</a>
                      <a href="php-8.4.3-nts-Win32-vs17-x64.zip">x</a><a href="php-debug-pack-8.4.2-nts-Win32-vs17-x64.zip">x</a>"#;
        let r = find_in_archives(html, "https://h/archives/", &Version::new(8, 4, 2), Variant::Nts).unwrap();
        assert_eq!(r.url, "https://h/archives/php-8.4.2-nts-Win32-vs17-x64.zip");
        assert!(r.unverified && r.sha256.is_none());
        let ts = find_in_archives(html, "https://h/archives", &Version::new(8, 4, 2), Variant::Ts).unwrap();
        assert!(ts.url.ends_with("php-8.4.2-Win32-vs17-x64.zip"));
        assert!(find_in_archives(html, "https://h", &Version::new(8, 4, 9), Variant::Nts).is_none());
    }

    #[test]
    fn dir_names_roundtrip() {
        assert_eq!(dir_name("8.4.27", Variant::Ts), "8.4.27-ts");
        assert_eq!(parse_dir_name("8.4.27-ts"), Some(("8.4.27".into(), Variant::Ts)));
        assert_eq!(parse_dir_name("8.4.27"), Some(("8.4.27".into(), Variant::Nts)));
        assert_eq!(parse_dir_name(".tmp-x"), None);
    }
}
