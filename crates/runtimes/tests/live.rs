//! Live tests (real internet, real downloads). Run with:
//!   NM_RT_TEST_ROOT=X:\nm-rt-test cargo test --test live -- --ignored --nocapture --test-threads=1
use nm_runtimes::*;
use std::sync::Arc;

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("NM_RT_TEST_ROOT").unwrap_or_else(|_| r"X:\nm-rt-test".into()))
}

fn progress() -> ProgressFn {
    let last = std::sync::Mutex::new(-1i64);
    Arc::new(move |p| {
        let pct = (p.progress * 100.0) as i64 / 10 * 10;
        let mut l = last.lock().unwrap();
        if pct != *l {
            *l = pct;
            println!("  [{:>3}%] {:?} {}", pct, p.step, p.message.chars().take(70).collect::<String>());
        }
    })
}

#[tokio::test]
#[ignore]
async fn live_list_available() {
    let m = RuntimeManager::new(root(), Options::default()).unwrap();
    let php = m.overview(RuntimeKind::Php, Channel::Stable, Variant::Nts, true).await.unwrap();
    println!("PHP offline={} :", php.offline);
    for a in &php.available {
        println!("  {} (full {}) {} {:?}MB sha={} {}", a.version, a.full_version, a.build, a.size_mb, a.sha256.is_some(), a.url);
    }
    assert!(php.available.iter().any(|a| a.version.starts_with("8.4.")));
    assert!(php.available.iter().all(|a| a.sha256.is_some()));
    let ts = m.list_available(RuntimeKind::Php, Channel::Stable, Variant::Ts, false).await.unwrap();
    assert!(ts.items.iter().all(|a| a.tsnts == "ts"));
    // second call must come from cache
    assert!(root().join(".cache").join("php-releases.json").is_file());

    for ch in [Channel::Lts, Channel::Current] {
        let n = m.list_available(RuntimeKind::Node, ch, Variant::Nts, false).await.unwrap();
        println!("Node {:?}:", ch);
        for a in &n.items {
            println!("  {} {:?} {} {}", a.version, a.lts_name, a.date.clone().unwrap_or_default(), a.url);
        }
        assert!(!n.items.is_empty());
    }
    // specific old patch via archives (unverified)
    let old = m.resolve(RuntimeKind::Php, "8.4.2", Variant::Nts).await.unwrap();
    println!("archive resolve 8.4.2 -> {} unverified={}", old.url, old.unverified);
    assert!(old.unverified);
}

#[tokio::test]
#[ignore]
async fn live_php_install_health_uninstall() {
    let m = RuntimeManager::new(root(), Options::default()).unwrap();
    let out = m.install(RuntimeKind::Php, "8.4", Variant::Nts, progress(), CancelToken::new()).await.unwrap();
    println!("installed {} at {} ({} MB) verified={}", out.runtime.version, out.runtime.path, out.runtime.size_mb, out.verified);
    println!("health: {}", serde_json::to_string_pretty(&out.health).unwrap());
    assert!(out.verified);
    assert!(out.health.ok, "missing: {:?}", out.health.missing_extensions);
    assert!(out.health.version.as_deref().unwrap().starts_with("8.4."));
    for e in ["openssl", "mbstring", "sockets", "curl", "gd", "pdo_sqlite", "sqlite3", "zip", "fileinfo"] {
        assert!(out.health.loaded_extensions.iter().any(|x| x == e), "{e} not loaded");
    }
    let ini = std::fs::read_to_string(std::path::Path::new(&out.runtime.path).join("php.ini")).unwrap();
    assert!(ini.contains("extension_dir = \"ext\"") && ini.contains("curl.cainfo"));
    assert!(root().join("cacert.pem").is_file());
    // already installed -> error
    let again = m.install(RuntimeKind::Php, "8.4", Variant::Nts, progress(), CancelToken::new()).await.unwrap_err();
    assert_eq!(again.code(), "already_installed");
    // re-run health via API
    let h = m.health_check(RuntimeKind::Php, &out.runtime.version, Variant::Nts).await.unwrap();
    assert!(h.ok);
    // TLS works from PHP with the bundled cacert
    let exe = out.runtime.exe_path();
    let r = std::process::Command::new(&exe)
        .args(["-r", "echo strlen(file_get_contents('https://curl.se/ca/cacert.pem', false, null, 0, 100)) > 0 ? 'TLS_OK' : 'TLS_FAIL';"])
        .output()
        .unwrap();
    println!("php https test: {}", String::from_utf8_lossy(&r.stdout));
    assert!(String::from_utf8_lossy(&r.stdout).contains("TLS_OK"), "{}", String::from_utf8_lossy(&r.stderr));
    m.uninstall(RuntimeKind::Php, &out.runtime.version, Variant::Nts, &|_| false).unwrap();
    assert!(m.list_installed(RuntimeKind::Php).is_empty());
}

#[tokio::test]
#[ignore]
async fn live_node_install_health_uninstall() {
    let m = RuntimeManager::new(root(), Options::default()).unwrap();
    let out = m.install(RuntimeKind::Node, "lts", Variant::Nts, progress(), CancelToken::new()).await.unwrap();
    println!("installed node {} at {} ({} MB)", out.runtime.version, out.runtime.path, out.runtime.size_mb);
    println!("health: node={:?} npm={:?}", out.health.version, out.health.npm_version);
    assert!(out.health.ok && out.verified);
    assert!(out.health.npm_version.is_some());
    assert!(std::path::Path::new(&out.runtime.path).join("node.exe").is_file(), "zip root folder must be flattened");
    m.uninstall(RuntimeKind::Node, &out.runtime.version, Variant::Nts, &|_| false).unwrap();
    assert!(m.list_installed(RuntimeKind::Node).is_empty());
}
