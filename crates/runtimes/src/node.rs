//! Node.js release metadata parsing (dist/index.json, SHASUMS256.txt). Pure functions.
use crate::types::{AvailableRuntime, Channel, Options};
use crate::version::Version;
use crate::{Error, Result};
use serde_json::Value;
use std::collections::BTreeMap;

/// Parse index.json into releases that ship a win-x64 zip, newest first.
/// Pre-releases (nightly/alpha, version contains '-') are dropped. No min-version filter here.
pub fn parse_index(json: &str, dist_base: &str) -> Result<Vec<AvailableRuntime>> {
    let root: Value = serde_json::from_str(json)?;
    let arr = root.as_array().ok_or_else(|| Error::Parse("index.json is not an array".into()))?;
    let base = dist_base.trim_end_matches('/');
    let mut out = vec![];
    for e in arr {
        let Some(vs) = e.get("version").and_then(|v| v.as_str()) else { continue };
        if vs.contains('-') {
            continue;
        }
        let Some(ver) = Version::parse(vs) else { continue };
        let has_zip = e
            .get("files")
            .and_then(|f| f.as_array())
            .map(|f| f.iter().any(|x| x.as_str() == Some("win-x64-zip")))
            .unwrap_or(false);
        if !has_zip {
            continue;
        }
        let lts_name = e.get("lts").and_then(|l| l.as_str()).map(|s| s.to_string());
        let s = ver.short();
        out.push(AvailableRuntime {
            version: s.clone(),
            full_version: s.clone(),
            channel: if lts_name.is_some() { "lts".into() } else { "current".into() },
            lts_name,
            date: e.get("date").and_then(|d| d.as_str()).map(|s| s.to_string()),
            url: zip_url(base, &s),
            sha256: None,
            size_mb: None,
            is_newer: false,
            tsnts: String::new(),
            build: String::new(),
            unverified: false,
        });
    }
    out.sort_by(|a, b| Version::parse(&b.version).cmp(&Version::parse(&a.version)));
    Ok(out)
}

pub fn zip_url(dist_base: &str, ver: &str) -> String {
    let b = dist_base.trim_end_matches('/');
    format!("{b}/v{ver}/node-v{ver}-win-x64.zip")
}

pub fn shasums_url(dist_base: &str, ver: &str) -> String {
    format!("{}/v{ver}/SHASUMS256.txt", dist_base.trim_end_matches('/'))
}

/// Find the sha256 for `file_name` in a SHASUMS256.txt body ("<hex>  <name>").
pub fn find_sha(shasums: &str, file_name: &str) -> Option<String> {
    shasums.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let (h, n) = (it.next()?, it.next()?);
        (n.trim_start_matches('*') == file_name && h.len() == 64).then(|| h.to_ascii_lowercase())
    })
}

/// Apply channel + minimum-version filter to a parsed index.
/// - `Lts`: newest release of each LTS line (major) >= min.
/// - `Current`: the single newest release.
/// - `Stable`/`All`: newest of every major >= min (LTS or not).
pub fn select_channel(all: &[AvailableRuntime], ch: Channel, o: &Options) -> Vec<AvailableRuntime> {
    let min = Version::parse(&o.min_node).unwrap_or(Version::new(20, 9, 0));
    let ok = |r: &&AvailableRuntime| Version::parse(&r.version).map(|v| v >= min).unwrap_or(false);
    match ch {
        Channel::Current => all.iter().filter(ok).take(1).cloned().collect(),
        Channel::Lts | Channel::Stable | Channel::All => {
            let mut per_major: BTreeMap<u32, AvailableRuntime> = BTreeMap::new();
            for r in all.iter().filter(ok) {
                if ch == Channel::Lts && r.lts_name.is_none() {
                    continue;
                }
                let m = Version::parse(&r.version).unwrap().major;
                per_major.entry(m).or_insert_with(|| r.clone()); // `all` is newest-first
            }
            let mut v: Vec<_> = per_major.into_values().collect();
            v.sort_by(|a, b| Version::parse(&b.version).cmp(&Version::parse(&a.version)));
            v
        }
    }
}

/// Resolve a user spec ("lts", "latest"/"current", "v22", "22.11", "22.11.0") against the index.
pub fn resolve_spec<'a>(all: &'a [AvailableRuntime], spec: &str, o: &Options) -> Option<&'a AvailableRuntime> {
    let s = spec.trim().to_ascii_lowercase();
    let min = Version::parse(&o.min_node).unwrap_or(Version::new(20, 9, 0));
    match s.as_str() {
        "lts" | "stable" | "" => all.iter().find(|r| r.lts_name.is_some() && Version::parse(&r.version).map(|v| v >= min).unwrap_or(false)),
        "latest" | "current" => all.iter().find(|r| Version::parse(&r.version).map(|v| v >= min).unwrap_or(false)),
        _ => {
            let parts = s.trim_start_matches('v').split('.').count();
            let want = Version::parse(&s)?;
            all.iter().find(|r| {
                let v = Version::parse(&r.version).unwrap();
                match parts {
                    1 => v.major == want.major,
                    2 => v.major == want.major && v.minor == want.minor,
                    _ => v.major == want.major && v.minor == want.minor && v.patch == want.patch,
                }
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIX: &str = include_str!("../tests/fixtures/node_index.json");

    #[test]
    fn parses_index_fixture() {
        let l = parse_index(FIX, "https://nodejs.org/dist").unwrap();
        assert!(!l.is_empty());
        assert!(l.iter().all(|r| !r.version.contains('-')), "alpha/nightly dropped");
        assert!(l.windows(2).all(|w| Version::parse(&w[0].version) >= Version::parse(&w[1].version)), "newest first");
        let r = &l[0];
        assert_eq!(r.url, format!("https://nodejs.org/dist/v{0}/node-v{0}-win-x64.zip", r.version));
    }

    #[test]
    fn channels() {
        let l = parse_index(FIX, "https://nodejs.org/dist").unwrap();
        let o = Options::default();
        let cur = select_channel(&l, Channel::Current, &o);
        assert_eq!(cur.len(), 1);
        assert_eq!(cur[0].version, l[0].version);
        let lts = select_channel(&l, Channel::Lts, &o);
        assert!(!lts.is_empty());
        assert!(lts.iter().all(|r| r.lts_name.is_some() && r.channel == "lts"));
        let majors: Vec<_> = lts.iter().map(|r| Version::parse(&r.version).unwrap().major).collect();
        let mut uniq = majors.clone();
        uniq.dedup();
        assert_eq!(majors, uniq, "one entry per major");
        // min filter
        let o2 = Options { min_node: "99.0.0".into(), ..Default::default() };
        assert!(select_channel(&l, Channel::Lts, &o2).is_empty());
    }

    #[test]
    fn spec_resolution() {
        let l = parse_index(FIX, "https://nodejs.org/dist").unwrap();
        let o = Options::default();
        assert!(resolve_spec(&l, "lts", &o).unwrap().lts_name.is_some());
        assert_eq!(resolve_spec(&l, "latest", &o).unwrap().version, l[0].version);
        let m = Version::parse(&l[0].version).unwrap().major.to_string();
        assert!(resolve_spec(&l, &format!("v{m}"), &o).is_some());
        assert_eq!(resolve_spec(&l, &l[0].version, &o).unwrap().version, l[0].version);
        assert!(resolve_spec(&l, "1.2.3", &o).is_none());
    }

    #[test]
    fn shasums_lookup() {
        let t = format!("{}  node-v22.1.0-win-x64.7z\n{}  node-v22.1.0-win-x64.zip\n", "a".repeat(64), "B".repeat(64));
        assert_eq!(find_sha(&t, "node-v22.1.0-win-x64.zip"), Some("b".repeat(64)));
        assert_eq!(find_sha(&t, "nope.zip"), None);
    }
}
