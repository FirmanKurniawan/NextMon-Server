//! Semantic-ish version parsing/comparison. Handles PHP "8.4.27-1" (build revision) and Node "v24.12.0".
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    /// Packaging revision (PHP "8.4.27-1"); 0 when absent.
    pub rev: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpgradeKind {
    /// Same or older.
    None,
    /// Same major.minor, higher patch/revision (safe upgrade).
    Patch,
    /// Same major, higher minor.
    Minor,
    /// Higher major.
    Major,
}

impl Version {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Version { major, minor, patch, rev: 0 }
    }

    /// Parse "8.4", "8.4.27", "8.4.27-1", "v24.12.0", "24.12.0-rc1" (non-numeric suffix ignored).
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim().trim_start_matches(|c| c == 'v' || c == 'V');
        let (core, rest) = match s.find('-') {
            Some(i) => (&s[..i], Some(&s[i + 1..])),
            None => (s, None),
        };
        let mut it = core.split('.');
        let major: u32 = it.next()?.parse().ok()?;
        let minor: u32 = match it.next() {
            Some(x) => x.parse().ok()?,
            None => 0,
        };
        let patch: u32 = match it.next() {
            Some(x) => x.parse().ok()?,
            None => 0,
        };
        if it.next().is_some() {
            return None;
        }
        let rev = rest.and_then(|r| r.parse::<u32>().ok()).unwrap_or(0);
        Some(Version { major, minor, patch, rev })
    }

    /// "8.4" style key.
    pub fn minor_key(&self) -> String {
        format!("{}.{}", self.major, self.minor)
    }

    /// "8.4.27" (no revision).
    pub fn short(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }

    pub fn same_minor(&self, other: &Version) -> bool {
        self.major == other.major && self.minor == other.minor
    }

    /// Classify what upgrading `self` -> `candidate` would be.
    pub fn upgrade_kind(&self, candidate: &Version) -> UpgradeKind {
        if candidate <= self {
            UpgradeKind::None
        } else if candidate.major > self.major {
            UpgradeKind::Major
        } else if candidate.minor > self.minor {
            UpgradeKind::Minor
        } else {
            UpgradeKind::Patch
        }
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Version {
    fn cmp(&self, o: &Self) -> Ordering {
        (self.major, self.minor, self.patch, self.rev).cmp(&(o.major, o.minor, o.patch, o.rev))
    }
}
impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.rev > 0 {
            write!(f, "{}.{}.{}-{}", self.major, self.minor, self.patch, self.rev)
        } else {
            write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
        }
    }
}

/// Compare two version strings; unparsable strings sort lowest.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    match (Version::parse(a), Version::parse(b)) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => a.cmp(b),
    }
}

/// True when `candidate` is strictly newer than `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    compare_versions(candidate, current) == Ordering::Greater
}

/// Kind of upgrade from `current` to `candidate` (None when not newer / unparsable).
pub fn upgrade_kind(current: &str, candidate: &str) -> UpgradeKind {
    match (Version::parse(current), Version::parse(candidate)) {
        (Some(a), Some(b)) => a.upgrade_kind(&b),
        _ => UpgradeKind::None,
    }
}

/// Highest version among `candidates` in the same major.minor as `current` (a "patch upgrade").
pub fn latest_patch_for<'a>(current: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let cur = Version::parse(current)?;
    candidates
        .into_iter()
        .filter_map(|c| Version::parse(c).map(|v| (v, c)))
        .filter(|(v, _)| v.same_minor(&cur))
        .max_by_key(|(v, _)| *v)
        .map(|(_, c)| c.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_forms() {
        assert_eq!(Version::parse("v24.12.0"), Some(Version::new(24, 12, 0)));
        assert_eq!(Version::parse("8.4.27-1").unwrap().rev, 1);
        assert_eq!(Version::parse("8.4").unwrap(), Version::new(8, 4, 0));
        assert_eq!(Version::parse("27.0.0-alpha"), Some(Version::new(27, 0, 0)));
        assert!(Version::parse("abc").is_none());
        assert!(Version::parse("1.2.3.4").is_none());
    }
    #[test]
    fn ordering_is_numeric() {
        assert!(is_newer("8.4.10", "8.4.9"));
        assert!(is_newer("8.4.27-1", "8.4.27"));
        assert!(!is_newer("8.4.27", "8.4.27"));
        assert!(is_newer("v20.10.0", "20.9.0"));
        assert_eq!(compare_versions("8.10.0", "8.9.0"), Ordering::Greater);
    }
    #[test]
    fn upgrade_kinds() {
        assert_eq!(upgrade_kind("8.4.1", "8.4.27"), UpgradeKind::Patch);
        assert_eq!(upgrade_kind("8.4.27", "8.5.1"), UpgradeKind::Minor);
        assert_eq!(upgrade_kind("22.1.0", "24.0.0"), UpgradeKind::Major);
        assert_eq!(upgrade_kind("8.4.27", "8.4.27"), UpgradeKind::None);
        assert_eq!(upgrade_kind("8.5.1", "8.4.27"), UpgradeKind::None);
    }
    #[test]
    fn latest_patch() {
        let c = ["8.4.3", "8.4.27", "8.5.0", "8.3.30"];
        assert_eq!(latest_patch_for("8.4.1", c).as_deref(), Some("8.4.27"));
        assert_eq!(latest_patch_for("7.0.0", c), None);
    }
}
