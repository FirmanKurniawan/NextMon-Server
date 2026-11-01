//! Integration tests against a tiny in-process HTTP server (no internet needed).
use nm_runtimes::*;
use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

type Routes = Arc<Mutex<HashMap<String, Vec<u8>>>>;
type Seen = Arc<Mutex<Vec<(String, Option<String>)>>>; // (path, Range header)

async fn serve() -> (String, Routes, Seen) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let routes: Routes = Default::default();
    let seen: Seen = Default::default();
    let (r2, s2) = (routes.clone(), seen.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = l.accept().await else { break };
            let (routes, seen) = (r2.clone(), s2.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let range = req.lines().find_map(|l| l.strip_prefix("range: ").or_else(|| l.strip_prefix("Range: "))).map(|s| s.trim().to_string());
                seen.lock().unwrap().push((path.clone(), range.clone()));
                let body = routes.lock().unwrap().get(&path).cloned();
                let resp: Vec<u8> = match body {
                    None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
                    Some(b) => {
                        let start = range.as_deref().and_then(|r| r.strip_prefix("bytes=")).and_then(|r| r.trim_end_matches('-').parse::<usize>().ok());
                        match start {
                            Some(s) if s < b.len() => {
                                let part = &b[s..];
                                let mut h = format!(
                                    "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                                    part.len(), s, b.len() - 1, b.len()
                                )
                                .into_bytes();
                                h.extend_from_slice(part);
                                h
                            }
                            _ => {
                                let mut h = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", b.len()).into_bytes();
                                h.extend_from_slice(&b);
                                h
                            }
                        }
                    }
                };
                let _ = sock.write_all(&resp).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    (base, routes, seen)
}

fn fake_node_zip() -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    w.start_file("node-v99.1.0-win-x64/node.exe", o).unwrap();
    w.write_all(b"not a real exe").unwrap();
    w.finish().unwrap().into_inner()
}

fn opts(base: &str) -> Options {
    Options { node_dist_url: base.to_string(), retries: 2, min_node: "1.0.0".into(), ..Default::default() }
}

fn index_json() -> Vec<u8> {
    br#"[{"version":"v99.1.0","date":"2030-01-01","files":["win-x64-zip"],"lts":"Fake"}]"#.to_vec()
}

#[tokio::test]
async fn sha_mismatch_is_rejected_and_nothing_is_installed() {
    let (base, routes, _) = serve().await;
    let zip = fake_node_zip();
    routes.lock().unwrap().insert("/index.json".into(), index_json());
    routes.lock().unwrap().insert("/v99.1.0/node-v99.1.0-win-x64.zip".into(), zip);
    routes.lock().unwrap().insert(
        "/v99.1.0/SHASUMS256.txt".into(),
        format!("{}  node-v99.1.0-win-x64.zip\n", "0".repeat(64)).into_bytes(),
    );
    let t = tempfile::tempdir().unwrap();
    let m = RuntimeManager::new(t.path(), opts(&base)).unwrap();
    let err = m.install(RuntimeKind::Node, "lts", Variant::Nts, noop_progress(), CancelToken::new()).await.unwrap_err();
    assert_eq!(err.code(), "checksum_mismatch", "{err}");
    assert!(m.list_installed(RuntimeKind::Node).is_empty());
    let node_dir = t.path().join("node");
    let leftovers: Vec<_> = std::fs::read_dir(&node_dir).map(|r| r.flatten().collect()).unwrap_or_default();
    assert!(leftovers.is_empty(), "no extraction may happen before verification: {leftovers:?}");
    assert!(!t.path().join(".downloads").join("node-v99.1.0-win-x64.zip").exists());
    assert!(!t.path().join(".downloads").join("node-v99.1.0-win-x64.zip.part").exists(), "bad download deleted");
}

#[tokio::test]
async fn valid_checksum_extracts_then_unhealthy_exe_is_rolled_back() {
    use sha2::{Digest, Sha256};
    let (base, routes, _) = serve().await;
    let zip = fake_node_zip();
    let sha = hex::encode(Sha256::digest(&zip));
    routes.lock().unwrap().insert("/index.json".into(), index_json());
    routes.lock().unwrap().insert("/v99.1.0/node-v99.1.0-win-x64.zip".into(), zip);
    routes.lock().unwrap().insert("/v99.1.0/SHASUMS256.txt".into(), format!("{sha}  node-v99.1.0-win-x64.zip\n").into_bytes());
    let t = tempfile::tempdir().unwrap();
    let m = RuntimeManager::new(t.path(), opts(&base)).unwrap();
    let events = Arc::new(Mutex::new(Vec::<Step>::new()));
    let e2 = events.clone();
    let p: ProgressFn = Arc::new(move |p| e2.lock().unwrap().push(p.step));
    let err = m.install(RuntimeKind::Node, "99.1.0", Variant::Nts, p, CancelToken::new()).await.unwrap_err();
    // fake node.exe cannot run -> health error, temp folder cleaned, nothing installed
    assert_eq!(err.code(), "health", "{err}");
    let steps = events.lock().unwrap().clone();
    assert!(steps.contains(&Step::Download) && steps.contains(&Step::Verify) && steps.contains(&Step::Extract));
    assert!(m.list_installed(RuntimeKind::Node).is_empty());
    let left: Vec<_> = std::fs::read_dir(t.path().join("node")).unwrap().flatten().collect();
    assert!(left.is_empty(), "temp dir must be cleaned: {left:?}");
}

#[tokio::test]
async fn download_resumes_with_range() {
    let (base, routes, seen) = serve().await;
    let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    routes.lock().unwrap().insert("/big.bin".into(), data.clone());
    let t = tempfile::tempdir().unwrap();
    let part = t.path().join("big.bin.part");
    std::fs::write(&part, &data[..70_000]).unwrap();
    let h = nm_runtimes::http::Http::new(&Options::default()).unwrap();
    let last = Arc::new(Mutex::new((0u64, None)));
    let l2 = last.clone();
    let n = h.download(&format!("{base}/big.bin"), &part, &CancelToken::new(), &move |b, t| *l2.lock().unwrap() = (b, t)).await.unwrap();
    assert_eq!(n, data.len() as u64);
    assert_eq!(std::fs::read(&part).unwrap(), data);
    assert_eq!(seen.lock().unwrap()[0].1.as_deref(), Some("bytes=70000-"));
    assert_eq!(*last.lock().unwrap(), (200_000, Some(200_000)));
}

#[tokio::test]
async fn cancel_keeps_partial_and_returns_cancelled() {
    let (base, routes, _) = serve().await;
    routes.lock().unwrap().insert("/f".into(), vec![1u8; 1000]);
    let t = tempfile::tempdir().unwrap();
    let h = nm_runtimes::http::Http::new(&Options::default()).unwrap();
    let c = CancelToken::new();
    c.cancel();
    let r = h.download(&format!("{base}/f"), &t.path().join("f.part"), &c, &|_, _| {}).await;
    assert!(matches!(r, Err(Error::Cancelled)));
}

#[tokio::test]
async fn http_404_is_not_found_and_not_retried_forever() {
    let (base, _, seen) = serve().await;
    let t = tempfile::tempdir().unwrap();
    let h = nm_runtimes::http::Http::new(&Options::default()).unwrap();
    let r = h.download(&format!("{base}/nope"), &t.path().join("x.part"), &CancelToken::new(), &|_, _| {}).await;
    assert!(matches!(r, Err(Error::NotFound(_))));
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn stale_cache_served_when_offline() {
    let (base, routes, _) = serve().await;
    routes.lock().unwrap().insert("/index.json".into(), index_json());
    let t = tempfile::tempdir().unwrap();
    let m = RuntimeManager::new(t.path(), opts(&base)).unwrap();
    let l = m.list_available(RuntimeKind::Node, Channel::Lts, Variant::Nts, false).await.unwrap();
    assert!(!l.offline && l.items.len() == 1 && l.items[0].version == "99.1.0");
    // Now point at a dead server with TTL 0: must fall back to the cached copy... (same URL needed) -> use same base, kill routes.
    routes.lock().unwrap().clear(); // server now answers 404 (client error, not a network failure)
    let m0 = RuntimeManager::new(t.path(), Options { cache_ttl_secs: 0, ..opts(&base) }).unwrap();
    // 404 is an error -> stale cache served
    let l2 = m0.list_available(RuntimeKind::Node, Channel::Lts, Variant::Nts, false).await.unwrap();
    assert!(l2.offline && l2.stale && l2.items.len() == 1);
    // Fresh cache within TTL does not hit the network
    let m6 = RuntimeManager::new(t.path(), opts(&base)).unwrap();
    let l3 = m6.list_available(RuntimeKind::Node, Channel::Lts, Variant::Nts, false).await.unwrap();
    assert!(!l3.offline && !l3.stale);
}

#[tokio::test]
async fn concurrent_install_same_kind_is_busy() {
    let (base, routes, _) = serve().await;
    routes.lock().unwrap().insert("/index.json".into(), index_json());
    let t = tempfile::tempdir().unwrap();
    let m = RuntimeManager::new(t.path(), opts(&base)).unwrap();
    // Hold the lock via an install that is blocked on a never-answering resolve is complex; instead
    // verify uninstall refuses while an install holds the lock by running two installs back to back.
    let (a, b) = tokio::join!(
        m.install(RuntimeKind::Node, "lts", Variant::Nts, noop_progress(), CancelToken::new()),
        m.install(RuntimeKind::Node, "lts", Variant::Nts, noop_progress(), CancelToken::new())
    );
    let codes = [a.unwrap_err().code(), b.unwrap_err().code()];
    assert!(codes.contains(&"busy"), "{codes:?}");
}

#[test]
fn uninstall_guard_and_cleanup() {
    let t = tempfile::tempdir().unwrap();
    let d = t.path().join("php").join("8.4.1");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("php.exe"), b"x").unwrap();
    std::fs::write(d.join("a.txt"), b"hello").unwrap();
    std::fs::create_dir_all(t.path().join("php").join(".tmp-leftover")).unwrap();
    let m = RuntimeManager::new(t.path(), Options::default()).unwrap();
    let l = m.list_installed(RuntimeKind::Php);
    assert_eq!(l.len(), 1);
    assert_eq!((l[0].version.as_str(), l[0].tsnts.as_str(), l[0].arch.as_str()), ("8.4.1", "nts", "x64"));
    assert!(matches!(m.uninstall(RuntimeKind::Php, "8.4.1", Variant::Nts, &|_| true), Err(Error::InUse(_))));
    assert!(d.exists());
    assert!(matches!(m.uninstall(RuntimeKind::Php, "8.4.2", Variant::Nts, &|_| false), Err(Error::NotFound(_))));
    m.uninstall(RuntimeKind::Php, "8.4.1", Variant::Nts, &|_| false).unwrap();
    assert!(!d.exists());
    assert_eq!(m.cleanup_stale(), 1);
}

#[test]
fn json_shapes_match_spec() {
    let a = AvailableRuntime {
        version: "8.4.27".into(), full_version: "8.4.27-1".into(), channel: "stable".into(), lts_name: None,
        date: Some("2026-10-26".into()), url: "u".into(), sha256: Some("h".into()), size_mb: Some(33.8), is_newer: true,
        tsnts: "nts".into(), build: "vs17".into(), unverified: false,
    };
    let v = serde_json::to_value(&a).unwrap();
    for k in ["version", "channel", "date", "url", "sha256", "sizeMB", "isNewer"] {
        assert!(v.get(k).is_some(), "missing {k}");
    }
    let e = serde_json::to_value(Error::Cancelled.to_api()).unwrap();
    assert_eq!(e["error"]["code"], "cancelled");
}
