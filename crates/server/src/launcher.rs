//! Builds the command line / environment for the active edition (PHP built-in server or Next.js standalone on Node).
use crate::config::{Edition, SharedConfig};
use crate::paths::Paths;
use crate::runtime_api::{exe_name, RuntimeApi};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
    /// TCP port that must become connectable for the app to count as "running" (None = running immediately).
    pub ready_port: Option<u16>,
    /// Address the app listens on (for the port-free check).
    pub bind: String,
    /// "php 8.4.27" - shown in status.
    pub runtime_label: String,
}

#[derive(Debug, Clone)]
pub struct LaunchError {
    pub code: String,
    pub en: String,
    pub id: String,
    /// For `runtime_missing`: which runtime kind could be installed ("php"/"node").
    pub runtime_kind: Option<String>,
}

impl LaunchError {
    pub fn new(code: &str, en: impl Into<String>, id: impl Into<String>) -> LaunchError {
        LaunchError { code: code.into(), en: en.into(), id: id.into(), runtime_kind: None }
    }
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.en)
    }
}

pub trait Launcher: Send + Sync {
    fn spec(&self, ed: Edition) -> Result<LaunchSpec, LaunchError>;
}

pub struct AppLauncher {
    pub paths: Paths,
    pub cfg: SharedConfig,
    pub rt: Arc<dyn RuntimeApi>,
}

fn abs(p: PathBuf) -> PathBuf {
    std::path::absolute(&p).unwrap_or(p)
}

impl AppLauncher {
    /// Resolve the runtime folder for an edition: configured dir name, else newest installed.
    pub fn resolve_runtime(&self, ed: Edition) -> Result<(PathBuf, String), LaunchError> {
        let kind = ed.runtime_kind();
        let want = self.cfg.read().unwrap().app(ed).runtime.clone();
        let installed = self.rt.list_installed(kind);
        let pick = if want.is_empty() {
            // prefer NTS builds for php; list is newest first
            installed.iter().find(|i| i.tsnts != "ts").or(installed.first())
        } else {
            installed.iter().find(|i| i.dir_name == want || i.version == want)
        };
        match pick {
            Some(i) => Ok((abs(i.path.clone()), i.version.clone())),
            None => {
                let mut e = LaunchError::new(
                    "runtime_missing",
                    if want.is_empty() { format!("No {kind} runtime is installed") } else { format!("{kind} runtime {want} is not installed") },
                    if want.is_empty() { format!("Runtime {kind} belum terpasang") } else { format!("Runtime {kind} {want} belum terpasang") },
                );
                e.runtime_kind = Some(kind.to_string());
                Err(e)
            }
        }
    }
}

impl Launcher for AppLauncher {
    fn spec(&self, ed: Edition) -> Result<LaunchSpec, LaunchError> {
        let cfg = self.cfg.read().unwrap().clone();
        let app = cfg.app(ed).clone();
        let (rt_dir, rt_version) = self.resolve_runtime(ed)?;
        let exe = rt_dir.join(exe_name(ed.runtime_kind()));
        let app_dir = abs(self.paths.app_dir(ed));
        let data = abs(self.paths.data_root(&cfg.data_dir)).join(ed.as_str());
        let _ = std::fs::create_dir_all(&data);

        let mut env: Vec<(String, String)> = vec![
            ("NEXTMON_DATA".into(), data.to_string_lossy().into_owned()),
            ("NEXTMON_BASE_URL".into(), format!("http://127.0.0.1:{}", app.port)),
            ("NEXTMON_EDITION".into(), ed.as_str().into()),
        ];
        let old_path = std::env::var("PATH").unwrap_or_default();
        env.push(("PATH".into(), format!("{};{}", rt_dir.display(), old_path)));

        let (args, cwd) = match ed {
            Edition::Php => {
                let public = app_dir.join("public");
                let index = public.join("index.php");
                if !index.is_file() {
                    return Err(LaunchError::new(
                        "app_missing",
                        format!("PHP app not found at {}", index.display()),
                        format!("Aplikasi PHP tidak ditemukan di {}", index.display()),
                    ));
                }
                let mut a: Vec<String> = vec![];
                let ini = rt_dir.join("php.ini");
                let ini_text = std::fs::read_to_string(&ini).unwrap_or_default();
                if ini.is_file() {
                    a.extend(["-c".into(), ini.to_string_lossy().into_owned()]);
                }
                if app.opcache {
                    let has_dll = rt_dir.join("ext").join("php_opcache.dll").is_file();
                    let loaded_by_ini = ini_text.lines().any(|l| {
                        let l = l.trim();
                        !l.starts_with(';') && l.starts_with("zend_extension") && l.contains("opcache")
                    });
                    if has_dll || loaded_by_ini {
                        if !loaded_by_ini {
                            a.extend(["-d".into(), "zend_extension=opcache".into()]);
                        }
                        for d in [
                            "opcache.enable=1",
                            "opcache.memory_consumption=128",
                            "opcache.interned_strings_buffer=16",
                            "opcache.max_accelerated_files=4000",
                            "opcache.validate_timestamps=1",
                            "opcache.revalidate_freq=2",
                            "opcache.jit=off",
                        ] {
                            a.extend(["-d".into(), d.into()]);
                        }
                    }
                }
                a.extend(["-d".into(), "realpath_cache_size=4096K".into(), "-d".into(), "realpath_cache_ttl=600".into()]);
                for (k, v) in &app.extra_ini {
                    a.extend(["-d".into(), format!("{k}={v}")]);
                }
                a.extend([
                    "-S".into(),
                    format!("{}:{}", cfg.bind, app.port),
                    "-t".into(),
                    public.to_string_lossy().into_owned(),
                    index.to_string_lossy().into_owned(),
                ]);
                (a, public)
            }
            Edition::Next => {
                let server = app_dir.join("server.js");
                if !server.is_file() {
                    return Err(LaunchError::new(
                        "app_missing",
                        format!("Next.js build not found at {}", server.display()),
                        format!("Build Next.js tidak ditemukan di {}", server.display()),
                    ));
                }
                if crate::util::cmp_version(&rt_version, &cfg.min_node) == std::cmp::Ordering::Less {
                    return Err(LaunchError::new(
                        "node_too_old",
                        format!("Node.js {rt_version} is older than the required {}", cfg.min_node),
                        format!("Node.js {rt_version} lebih lama dari yang dibutuhkan ({})", cfg.min_node),
                    ));
                }
                env.push(("PORT".into(), app.port.to_string()));
                env.push(("HOSTNAME".into(), cfg.bind.clone()));
                env.push(("NODE_ENV".into(), "production".into()));
                (vec![server.to_string_lossy().into_owned()], app_dir)
            }
        };
        for (k, v) in &app.env {
            env.retain(|(ek, _)| !ek.eq_ignore_ascii_case(k));
            env.push((k.clone(), v.clone()));
        }
        Ok(LaunchSpec {
            program: exe,
            args,
            env,
            cwd,
            ready_port: Some(app.port),
            bind: cfg.bind.clone(),
            runtime_label: format!("{} {}", ed.runtime_kind(), rt_version),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{shared, Config};
    use crate::runtime_api::FsRuntimes;

    fn setup() -> (tempfile::TempDir, AppLauncher) {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::new(d.path());
        let rtdir = paths.runtimes().join("php").join("8.4.27");
        std::fs::create_dir_all(rtdir.join("ext")).unwrap();
        std::fs::write(rtdir.join("php.exe"), b"x").unwrap();
        std::fs::write(rtdir.join("ext").join("php_opcache.dll"), b"x").unwrap();
        std::fs::write(rtdir.join("php.ini"), "extension_dir=ext\n").unwrap();
        let pub_ = paths.app_dir(Edition::Php).join("public");
        std::fs::create_dir_all(&pub_).unwrap();
        std::fs::write(pub_.join("index.php"), "<?php echo 1;").unwrap();
        let mut c = Config::default();
        c.fixup();
        let l = AppLauncher { paths: paths.clone(), cfg: shared(c), rt: Arc::new(FsRuntimes { root: paths.runtimes() }) };
        (d, l)
    }

    #[test]
    fn php_command_line() {
        let (_d, l) = setup();
        let s = l.spec(Edition::Php).unwrap();
        let line = s.args.join(" ");
        assert!(line.contains("-S 127.0.0.1:8080"), "{line}");
        assert!(line.contains("zend_extension=opcache"));
        assert!(line.contains("-c "));
        assert!(s.env.iter().any(|(k, v)| k == "NEXTMON_DATA" && v.ends_with("php")));
        assert!(s.env.iter().any(|(k, v)| k == "NEXTMON_DEMO" && v == "0"));
        assert_eq!(s.ready_port, Some(8080));
    }

    #[test]
    fn missing_runtime_and_app_are_reported() {
        let (_d, l) = setup();
        let e = l.spec(Edition::Next).unwrap_err();
        assert_eq!(e.code, "runtime_missing");
        assert_eq!(e.runtime_kind.as_deref(), Some("node"));
        l.cfg.write().unwrap().apps.php.runtime = "9.9.9".into();
        assert_eq!(l.spec(Edition::Php).unwrap_err().code, "runtime_missing");
    }
}
