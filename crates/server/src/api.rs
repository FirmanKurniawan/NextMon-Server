//! Control HTTP API + UI file server. Loopback only, token auth, Host/Origin checks, no CORS.
use crate::config::Edition;
use crate::core::{ApiErr, Core, UiCmd};
use crate::logger;
use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rust_embed::RustEmbed;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

#[derive(RustEmbed)]
#[folder = "../../ui/"]
struct Assets;

/// UI language ("en" or default Indonesian); kept in sync by `Core`.
pub static LANG_EN: AtomicBool = AtomicBool::new(false);

type R = Result<Json<Value>, ApiErr>;

impl IntoResponse for ApiErr {
    fn into_response(self) -> Response {
        // message is {id, en}: the panel picks the language itself
        let mut err = json!({
            "code": self.code,
            "message": { "id": self.id, "en": self.en },
        });
        if let (Some(Value::Object(x)), Some(m)) = (self.extra, err.as_object_mut()) {
            m.extend(x);
        }
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(json!({ "error": err }))).into_response()
    }
}

fn bad(code: &str, id: &str, en: &str) -> ApiErr {
    ApiErr::new(400, code, id, en)
}

fn body_json(b: &Bytes) -> Result<Value, ApiErr> {
    if b.iter().all(|c| c.is_ascii_whitespace()) {
        return Ok(json!({}));
    }
    serde_json::from_slice(b).map_err(|e| bad("bad_json", &format!("JSON tidak valid: {e}"), &format!("Invalid JSON: {e}")))
}

fn jstr<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(|x| x.as_str())
}

pub fn router(core: Arc<Core>) -> Router {
    let api = Router::new()
        .route("/status", get(status))
        .route("/version", get(version))
        .route("/edition", post(edition))
        .route("/app/{action}", post(app_action))
        .route("/app/config", post(app_config))
        .route("/logs/{name}", get(logs))
        .route("/logs/{name}/stream", get(logs_stream))
        .route("/config", get(config_get).post(config_post))
        .route("/config/token/regenerate", post(token_regenerate))
        .route("/ports/check", get(ports_check))
        .route("/runtimes/{kind}", get(runtimes))
        .route("/runtimes/{kind}/{action}", post(runtime_action))
        .route("/jobs/{id}", get(job_get))
        .route("/jobs/{id}/stream", get(job_stream))
        .route("/backup/create", post(backup_create))
        .route("/backup/list", get(backup_list))
        .route("/backup/restore", post(backup_restore))
        .route("/backup/delete", post(backup_delete))
        .route("/backup/download", get(backup_download))
        .route("/update/check", get(update_check))
        .route("/update/apply", post(update_apply))
        .route("/diagnostics", get(diagnostics))
        .route("/diagnostics/fix", post(diagnostics_fix))
        .route("/open", post(open))
        .route("/panel/open", post(panel_open))
        .route("/shutdown", post(shutdown));
    Router::new()
        .nest("/api", api)
        .fallback(static_file)
        .layer(middleware::from_fn_with_state(core.clone(), guard))
        .with_state(core)
}

// ------------------------------------------------------------------ guard: Host / Origin / token

fn allowed_hosts(port: u16) -> [String; 3] {
    [format!("127.0.0.1:{port}"), format!("localhost:{port}"), format!("[::1]:{port}")]
}

fn deny(status: u16, code: &str, msg: &str) -> Response {
    ApiErr::new(status, code, msg, msg).into_response()
}

async fn guard(State(core): State<Arc<Core>>, req: Request, next: Next) -> Response {
    let (port, token) = {
        let c = core.cfg.read().unwrap();
        (c.control_port, c.control_token.clone())
    };
    let hosts = allowed_hosts(port);
    // DNS-rebinding protection: the Host header must name loopback + our port.
    let host = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    if !hosts.iter().any(|h| *h == host) {
        return deny(403, "bad_host", "Host header not allowed");
    }
    // Cross-origin browsers send Origin; only our own origin is acceptable. (No CORS headers are ever sent.)
    if let Some(o) = req.headers().get(header::ORIGIN) {
        let o = o.to_str().unwrap_or("").to_ascii_lowercase();
        let ok = o.strip_prefix("http://").map(|h| hosts.iter().any(|x| x == h)).unwrap_or(false);
        if !ok {
            return deny(403, "bad_origin", "Cross-origin requests are not allowed");
        }
    }
    let path = req.uri().path().to_string();
    if path.starts_with("/api/") || path == "/api" {
        // Header only: the token never travels in URLs (the panel uses fetch for SSE and downloads too).
        let given = req.headers().get("x-token").and_then(|h| h.to_str().ok()).unwrap_or("").to_string();
        if token.is_empty() || !crate::util::ct_eq(given.as_bytes(), token.as_bytes()) {
            return deny(401, "unauthorized", "Missing or invalid X-Token");
        }
    } else if req.method() != Method::GET && req.method() != Method::HEAD {
        return deny(405, "method_not_allowed", "Method not allowed");
    }
    let is_api = path.starts_with("/api");
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    if is_api {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    resp
}

// ------------------------------------------------------------------ simple endpoints

async fn status(State(c): State<Arc<Core>>) -> R {
    Ok(Json(c.status_json()))
}

async fn version() -> R {
    Ok(Json(json!({"name": "NextMON Server", "version": crate::core::VERSION})))
}

async fn edition(State(c): State<Arc<Core>>, body: Bytes) -> R {
    let b = body_json(&body)?;
    let ed = jstr(&b, "edition").and_then(Edition::parse).ok_or_else(|| ApiErr::new(422, "invalid_edition", "Edisi harus php atau next.", "Edition must be php or next."))?;
    let start = b.get("start").and_then(|v| v.as_bool()).unwrap_or_else(|| c.sup.is_active());
    if ed == c.config().edition {
        c.launcher.resolve_runtime(ed)?;
        if start {
            c.app_start().await?;
        }
        return Ok(Json(json!({"ok": true, "edition": ed.as_str(), "changed": false})));
    }
    let job = c.switch_edition(ed, start)?;
    Ok(Json(json!({"ok": true, "jobId": job.id(), "edition": ed.as_str(), "start": start})))
}

async fn app_action(State(c): State<Arc<Core>>, Path(action): Path<String>) -> R {
    match action.as_str() {
        "start" => {
            let o = c.app_start().await?;
            c.ui_cmd(UiCmd::Refresh);
            Ok(Json(json!({"ok": true, "alreadyRunning": o == crate::supervisor::StartOutcome::AlreadyRunning, "app": c.status_json()["app"]})))
        }
        "stop" => {
            c.app_stop().await;
            c.ui_cmd(UiCmd::Refresh);
            Ok(Json(json!({"ok": true, "app": c.status_json()["app"]})))
        }
        "restart" => {
            c.app_restart().await?;
            c.ui_cmd(UiCmd::Refresh);
            Ok(Json(json!({"ok": true, "app": c.status_json()["app"]})))
        }
        _ => Err(ApiErr::new(404, "not_found", "Aksi tidak dikenal", "Unknown action")),
    }
}

async fn app_config(State(c): State<Arc<Core>>, body: Bytes) -> R {
    let mut b = body_json(&body)?;
    let ed = match b.get("edition") {
        Some(e) => Some(e.as_str().and_then(Edition::parse).ok_or_else(|| bad("bad_request", "edition tidak valid", "invalid edition"))?),
        None => None,
    };
    if let Some(o) = b.as_object_mut() {
        o.remove("edition");
    }
    Ok(Json(c.app_config_patch(ed, b).await?))
}

async fn config_get(State(c): State<Arc<Core>>) -> R {
    let mut v = serde_json::to_value(c.config()).unwrap_or(Value::Null);
    v["autostart"] = json!(crate::autostart::is_enabled());
    Ok(Json(v))
}

async fn token_regenerate(State(c): State<Arc<Core>>) -> R {
    Ok(Json(json!({"token": c.token_regenerate()?})))
}

async fn ports_check(State(c): State<Arc<Core>>, Query(q): Query<HashMap<String, String>>) -> R {
    let port = q.get("port").and_then(|p| p.parse::<u32>().ok()).filter(|p| (1..=65535).contains(p)).ok_or_else(|| ApiErr::new(422, "invalid_port", "Port tidak valid.", "Invalid port."))?;
    let ed = q.get("edition").and_then(|e| Edition::parse(e));
    Ok(Json(c.ports_check(port as u16, ed)))
}

async fn config_post(State(c): State<Arc<Core>>, body: Bytes) -> R {
    let b = body_json(&body)?;
    if !b.is_object() {
        return Err(ApiErr::new(422, "invalid_config", "Body harus berupa objek", "Body must be an object"));
    }
    let r = c.config_patch(b).await?;
    LANG_EN.store(c.lang() == "en", Ordering::Relaxed);
    Ok(Json(r))
}

// ------------------------------------------------------------------ logs

fn log_name(n: &str) -> Result<&str, ApiErr> {
    if ["server", "php", "next"].contains(&n) {
        Ok(n)
    } else {
        Err(ApiErr::new(404, "not_found", "Log tidak dikenal", "Unknown log"))
    }
}

async fn logs(State(c): State<Arc<Core>>, Path(name): Path<String>, Query(q): Query<HashMap<String, String>>) -> Result<Response, ApiErr> {
    let name = log_name(&name)?;
    if q.get("follow").map(|v| v == "1" || v == "true").unwrap_or(false) {
        return Ok(logs_stream(State(c), Path(name.to_string()), Query(q)).await?.into_response());
    }
    let file = logger::current_log_file(&c.paths, name).ok_or_else(|| ApiErr::new(404, "not_found", "Log tidak ada", "No log"))?;
    if q.get("download").map(|v| v == "1").unwrap_or(false) {
        let data = tokio::fs::read(&file).await.unwrap_or_default();
        let fname = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "log.txt".into());
        return Ok((
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8".to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{fname}\""))],
            data,
        )
            .into_response());
    }
    let n = q.get("lines").and_then(|v| v.parse::<usize>().ok()).unwrap_or(200).clamp(1, 5000);
    let lines = tokio::task::spawn_blocking({
        let f = file.clone();
        move || logger::tail_lines(&f, n)
    })
    .await
    .unwrap_or_default();
    if q.get("format").map(|v| v == "text").unwrap_or(false) {
        return Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], lines.join("\n")).into_response());
    }
    Ok(Json(json!({"name": name, "file": file.file_name().map(|n| n.to_string_lossy().to_string()), "lines": lines})).into_response())
}

async fn logs_stream(State(c): State<Arc<Core>>, Path(name): Path<String>, Query(q): Query<HashMap<String, String>>) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, ApiErr> {
    let name = log_name(&name)?.to_string();
    let _ = &q; // the stream carries only NEW lines (the panel loads the backlog from GET /api/logs/{name})
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(256);
    let paths = c.paths.clone();
    tokio::spawn(async move {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};
        let mut cur: Option<std::path::PathBuf> = None;
        let mut offset: u64 = 0;
        let mut partial = String::new();
        let mut first = true;
        loop {
            let file = logger::current_log_file(&paths, &name);
            if file != cur {
                cur = file.clone();
                offset = 0;
                partial.clear();
                if first {
                    // start at the end of the current file: only lines written from now on are sent
                    if let Some(f) = &file {
                        offset = tokio::fs::metadata(f).await.map(|m| m.len()).unwrap_or(0);
                    }
                    first = false;
                }
            }
            if let Some(f) = &cur {
                if let Ok(mut fh) = tokio::fs::File::open(f).await {
                    let len = fh.metadata().await.map(|m| m.len()).unwrap_or(0);
                    if len < offset {
                        offset = 0; // truncated / rotated
                    }
                    if len > offset && fh.seek(std::io::SeekFrom::Start(offset)).await.is_ok() {
                        let mut buf = Vec::new();
                        if fh.take(len - offset).read_to_end(&mut buf).await.is_ok() {
                            offset += buf.len() as u64;
                            partial.push_str(&String::from_utf8_lossy(&buf));
                            while let Some(i) = partial.find('\n') {
                                let line: String = partial.drain(..=i).collect();
                                if tx.send(Ok(Event::default().data(line.trim_end_matches(['\r', '\n'])))).await.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                }
            }
            if tx.is_closed() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
    });
    Ok(Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

// ------------------------------------------------------------------ runtimes + jobs

fn kind_ok(k: &str) -> Result<&str, ApiErr> {
    if k == "php" || k == "node" {
        Ok(k)
    } else {
        Err(ApiErr::new(404, "not_found", "Jenis runtime tidak dikenal", "Unknown runtime kind"))
    }
}

async fn runtimes(State(c): State<Arc<Core>>, Path(kind): Path<String>, Query(q): Query<HashMap<String, String>>) -> R {
    let kind = kind_ok(&kind)?;
    let force = q.get("refresh").map(|v| v == "1").unwrap_or(false);
    Ok(Json(c.runtime_overview(kind, force, q.get("variant").map(|s| s.as_str()), q.get("channel").map(|s| s.as_str())).await?))
}

async fn runtime_action(State(c): State<Arc<Core>>, Path((kind, action)): Path<(String, String)>, body: Bytes) -> Result<Response, ApiErr> {
    let kind = kind_ok(&kind)?.to_string();
    let b = body_json(&body)?;
    match action.as_str() {
        "install" => {
            let v = jstr(&b, "version").ok_or_else(|| bad("bad_request", "version wajib diisi", "version is required"))?;
            let job = c.runtime_install(&kind, v, jstr(&b, "variant").map(String::from))?;
            Ok(accepted(&job.id()))
        }
        "activate" => {
            let v = jstr(&b, "version").ok_or_else(|| bad("bad_request", "version wajib diisi", "version is required"))?;
            Ok(Json(c.runtime_activate(&kind, v, jstr(&b, "app")).await?).into_response())
        }
        "uninstall" => {
            let v = jstr(&b, "version").ok_or_else(|| bad("bad_request", "version wajib diisi", "version is required"))?.to_string();
            let c2 = c.clone();
            let k2 = kind.clone();
            tokio::task::spawn_blocking(move || c2.runtime_uninstall(&k2, &v)).await.map_err(|e| ApiErr::new(500, "internal", e.to_string(), e.to_string()))??;
            Ok(Json(json!({"ok": true})).into_response())
        }
        "upgrade" => {
            let job = c.runtime_upgrade(&kind, jstr(&b, "to").map(String::from), jstr(&b, "app").map(String::from))?;
            Ok(accepted(&job.id()))
        }
        _ => Err(ApiErr::new(404, "not_found", "Aksi tidak dikenal", "Unknown action")),
    }
}

/// 202 + {jobId}
fn accepted(id: &str) -> Response {
    (StatusCode::ACCEPTED, Json(json!({"jobId": id}))).into_response()
}

async fn job_get(State(c): State<Arc<Core>>, Path(id): Path<String>) -> R {
    c.jobs.get(&id).map(|j| Json(serde_json::to_value(j).unwrap_or(Value::Null))).ok_or_else(|| ApiErr::new(404, "not_found", "Job tidak ditemukan", "Job not found"))
}

async fn job_stream(State(c): State<Arc<Core>>, Path(id): Path<String>) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, ApiErr> {
    let mut rx = c.jobs.subscribe(&id).ok_or_else(|| ApiErr::new(404, "not_found", "Job tidak ditemukan", "Job not found"))?;
    let (tx, out) = mpsc::channel::<Result<Event, Infallible>>(16);
    tokio::spawn(async move {
        loop {
            let snap = rx.borrow_and_update().clone();
            let done = snap.state != "running";
            let ev = Event::default().data(serde_json::to_string(&snap).unwrap_or_default());
            if tx.send(Ok(ev)).await.is_err() || done {
                return;
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    });
    Ok(Sse::new(ReceiverStream::new(out)).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

// ------------------------------------------------------------------ backup

async fn backup_create(State(c): State<Arc<Core>>) -> Result<Response, ApiErr> {
    Ok(accepted(&c.backup_create_job().id()))
}

async fn backup_list(State(c): State<Arc<Core>>) -> R {
    Ok(Json(json!({"backups": crate::backup::list(&c.paths)})))
}

async fn backup_restore(State(c): State<Arc<Core>>, body: Bytes) -> Result<Response, ApiErr> {
    let b = body_json(&body)?;
    let name = jstr(&b, "name").ok_or_else(|| bad("bad_request", "name wajib diisi", "name is required"))?;
    Ok(accepted(&c.backup_restore_job(name)?.id()))
}

async fn backup_delete(State(c): State<Arc<Core>>, body: Bytes) -> R {
    let b = body_json(&body)?;
    let name = jstr(&b, "name").ok_or_else(|| bad("bad_request", "name wajib diisi", "name is required"))?;
    crate::backup::delete(&c.paths, name)?;
    Ok(Json(json!({"ok": true})))
}

async fn backup_download(State(c): State<Arc<Core>>, Query(q): Query<HashMap<String, String>>) -> Result<Response, ApiErr> {
    let name = q.get("name").cloned().unwrap_or_default();
    let path = crate::backup::resolve(&c.paths, &name)?;
    let file = tokio::fs::File::open(&path).await.map_err(|e| ApiErr::new(500, "io", e.to_string(), e.to_string()))?;
    let len = file.metadata().await.map(|m| m.len()).unwrap_or(0);
    let stream = futures_util::stream::unfold(file, |mut f| async move {
        use tokio::io::AsyncReadExt;
        let mut buf = vec![0u8; 64 * 1024];
        match f.read(&mut buf).await {
            Ok(0) | Err(_) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok::<_, std::io::Error>(Bytes::from(buf)), f))
            }
        }
    });
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/zip")
        .header(header::CONTENT_LENGTH, len)
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\""))
        .body(Body::from_stream(stream))
        .unwrap())
}

// ------------------------------------------------------------------ updates, diagnostics, open

async fn update_check(State(c): State<Arc<Core>>) -> R {
    Ok(Json(c.update_check().await?))
}

async fn update_apply(State(c): State<Arc<Core>>) -> Result<Response, ApiErr> {
    Ok(accepted(&c.update_apply()?.id()))
}

async fn diagnostics(State(c): State<Arc<Core>>) -> R {
    Ok(Json(c.diagnostics().await))
}

async fn diagnostics_fix(State(c): State<Arc<Core>>, body: Bytes) -> R {
    let b = body_json(&body)?;
    let id = jstr(&b, "id").ok_or_else(|| bad("bad_request", "id wajib diisi", "id is required"))?;
    Ok(Json(c.diagnostics_fix(id).await?))
}

async fn open(State(c): State<Arc<Core>>, body: Bytes) -> R {
    let b = body_json(&body)?;
    let what = jstr(&b, "what").ok_or_else(|| bad("bad_request", "what wajib diisi", "what is required"))?;
    c.open(what)?;
    Ok(Json(json!({"ok": true})))
}

async fn panel_open(State(c): State<Arc<Core>>) -> R {
    c.ui_cmd(UiCmd::ShowPanel);
    Ok(Json(json!({"ok": true})))
}

async fn shutdown(State(c): State<Arc<Core>>) -> R {
    let c2 = c.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        c2.ui_cmd(UiCmd::Quit);
        c2.shutdown.notify_waiters();
        c2.shutdown.notify_one();
    });
    Ok(Json(json!({"ok": true})))
}

// ------------------------------------------------------------------ static UI

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "webmanifest" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

const PLACEHOLDER: &str = "<!doctype html><html><head><meta charset=utf-8><title>NextMON Server</title></head><body style=\"font-family:system-ui;padding:2rem\"><h1>NextMON Server</h1><p>Panel UI belum terpasang (folder <code>ui/</code> kosong). The control API is running at <code>/api</code>.</p></body></html>";

/// Replace the `{{NM_TOKEN}}` placeholder; pages without it get a `nm-token` meta tag so any custom UI still works.
fn inject_token(html: &str, token: &str) -> String {
    if html.contains("{{NM_TOKEN}}") {
        return html.replace("{{NM_TOKEN}}", token);
    }
    let snippet = format!("<meta name=\"nm-token\" content=\"{token}\">");
    match html.to_ascii_lowercase().find("</head>") {
        Some(i) => format!("{}{}{}", &html[..i], snippet, &html[i..]),
        None => format!("{snippet}{html}"),
    }
}

async fn static_file(State(c): State<Arc<Core>>, req: Request) -> Response {
    if req.method() != Method::GET && req.method() != Method::HEAD {
        return deny(405, "method_not_allowed", "Method not allowed");
    }
    let raw = req.uri().path().trim_start_matches('/').to_string();
    let rel = if raw.is_empty() { "index.html".to_string() } else { raw };
    if rel.contains("..") || rel.contains('\\') || rel.contains(':') || rel.contains('\0') {
        return deny(400, "bad_path", "Bad path");
    }
    // 1. override from ./ui
    let override_path = c.paths.ui_dir().join(&rel);
    let data: Option<Vec<u8>> = match tokio::fs::read(&override_path).await {
        Ok(d) if override_path.starts_with(c.paths.ui_dir()) => Some(d),
        _ => Assets::get(&rel).map(|f| f.data.into_owned()),
    };
    let data = match data {
        Some(d) => d,
        None if rel == "index.html" => PLACEHOLDER.as_bytes().to_vec(),
        None => return deny(404, "not_found", "Not found"),
    };
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime(&rel)));
    let body: Vec<u8> = if rel.ends_with(".html") {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        let tok = c.cfg.read().unwrap().control_token.clone();
        inject_token(&String::from_utf8_lossy(&data), &tok).into_bytes()
    } else {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        data
    };
    (StatusCode::OK, headers, body).into_response()
}

// ------------------------------------------------------------------ server

pub async fn serve(core: Arc<Core>, listener: std::net::TcpListener) -> std::io::Result<()> {
    listener.set_nonblocking(true)?;
    let listener = tokio::net::TcpListener::from_std(listener)?;
    let app = router(core.clone());
    let c2 = core.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            c2.shutdown.notified().await;
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::paths::Paths;
    use crate::runtime_api::FsRuntimes;

    async fn start() -> (tempfile::TempDir, Arc<Core>, String, String) {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::new(d.path());
        paths.ensure_dirs();
        let mut cfg = Config::default();
        cfg.fixup();
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        cfg.control_port = l.local_addr().unwrap().port();
        let token = cfg.control_token.clone();
        let rt = Arc::new(FsRuntimes { root: paths.runtimes() });
        let core = Core::new(paths, cfg, rt);
        let port = core.config().control_port;
        let c2 = core.clone();
        tokio::spawn(async move {
            let _ = serve(c2, l).await;
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        (d, core, token, format!("127.0.0.1:{port}"))
    }

    async fn raw(addr: &str, req: &str) -> (u16, String) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
        s.write_all(req.as_bytes()).await.unwrap();
        let mut buf = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut buf)).await;
        let text = String::from_utf8_lossy(&buf).to_string();
        let code = text.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
        (code, text)
    }

    fn get(path: &str, host: &str, extra: &str) -> String {
        format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n{extra}\r\n")
    }

    #[tokio::test]
    async fn auth_host_and_origin_checks() {
        let (_d, _core, token, addr) = start().await;
        // no token -> 401
        let (c, _) = raw(&addr, &get("/api/status", &addr, "")).await;
        assert_eq!(c, 401);
        // wrong token -> 401
        let (c, _) = raw(&addr, &get("/api/status", &addr, "X-Token: nope\r\n")).await;
        assert_eq!(c, 401);
        // right token -> 200 with status JSON
        let (c, body) = raw(&addr, &get("/api/status", &addr, &format!("X-Token: {token}\r\n"))).await;
        assert_eq!(c, 200, "{body}");
        assert!(body.contains("\"edition\":\"php\""));
        // DNS rebinding: foreign Host even with a valid token -> 403
        let (c, _) = raw(&addr, &get("/api/status", "evil.example.com", &format!("X-Token: {token}\r\n"))).await;
        assert_eq!(c, 403);
        let (c, _) = raw(&addr, &get("/", "evil.example.com:80", "")).await;
        assert_eq!(c, 403);
        // localhost alias with right port is fine
        let port = addr.split(':').nth(1).unwrap();
        let (c, _) = raw(&addr, &get("/api/status", &format!("localhost:{port}"), &format!("X-Token: {token}\r\n"))).await;
        assert_eq!(c, 200);
        // foreign Origin -> 403, and no CORS header is ever emitted
        let (c, body) = raw(&addr, &get("/api/status", &addr, &format!("X-Token: {token}\r\nOrigin: http://evil.example.com\r\n"))).await;
        assert_eq!(c, 403);
        assert!(!body.to_ascii_lowercase().contains("access-control-allow"));
        // same-origin is fine
        let (c, body) = raw(&addr, &get("/api/status", &addr, &format!("X-Token: {token}\r\nOrigin: http://{addr}\r\n"))).await;
        assert_eq!(c, 200);
        assert!(!body.to_ascii_lowercase().contains("access-control-allow"));
        // the token is never accepted from the URL
        let (c, _) = raw(&addr, &get(&format!("/api/status?token={token}"), &addr, "")).await;
        assert_eq!(c, 401);
        let post = format!("POST /api/app/stop?token={token} HTTP/1.1\r\nHost: {addr}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let (c, _) = raw(&addr, &post).await;
        assert_eq!(c, 401, "mutations require the header");
        // preflight is not served
        let opt = format!("OPTIONS /api/status HTTP/1.1\r\nHost: {addr}\r\nOrigin: http://evil.example.com\r\nConnection: close\r\n\r\n");
        let (c, _) = raw(&addr, &opt).await;
        assert!(c == 403 || c == 401 || c == 405, "got {c}");
    }

    fn post(path: &str, addr: &str, token: &str, body: &str) -> String {
        format!("POST {path} HTTP/1.1\r\nHost: {addr}\r\nX-Token: {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
    }

    fn body_of(resp: &str) -> Value {
        serde_json::from_str(resp.split_once("\r\n\r\n").map(|x| x.1).unwrap_or("")).unwrap_or(Value::Null)
    }

    #[tokio::test]
    async fn ui_gets_token_injected_and_traversal_blocked() {
        let (d, _core, token, addr) = start().await;
        std::fs::create_dir_all(d.path().join("ui")).unwrap();
        std::fs::write(d.path().join("ui/index.html"), "<html><head><meta name=\"nm-token\" content=\"{{NM_TOKEN}}\"></head><body>hi</body></html>").unwrap();
        std::fs::write(d.path().join("ui/other.html"), "<html><head></head><body>x</body></html>").unwrap();
        let (c, body) = raw(&addr, &get("/", &addr, "")).await;
        assert_eq!(c, 200);
        assert!(body.contains(&format!("content=\"{token}\"")) && !body.contains("{{NM_TOKEN}}"), "{body}");
        // pages without the placeholder still get a nm-token meta tag
        let (_, body) = raw(&addr, &get("/other.html", &addr, "")).await;
        assert!(body.contains(&format!("name=\"nm-token\" content=\"{token}\"")), "{body}");
        let (c, _) = raw(&addr, &get("/..%2f..%2fconfig.json", &addr, "")).await;
        assert!(c == 400 || c == 404, "got {c}");
        let (c, body) = raw(&addr, &get("/config.json", &addr, "")).await;
        assert_eq!(c, 404, "config.json must not be served: {body}");
    }

    #[tokio::test]
    async fn config_post_validates_and_hides_token() {
        let (_d, core, token, addr) = start().await;
        // invalid value -> 422 with {error:{code, message:{id,en}}}
        let (c, text) = raw(&addr, &post("/api/config", &addr, &token, r#"{"bind":"10.1.2.3"}"#)).await;
        assert_eq!(c, 422, "{text}");
        let e = body_of(&text);
        assert_eq!(e["error"]["code"], "invalid_config");
        assert!(e["error"]["message"]["id"].is_string() && e["error"]["message"]["en"].is_string());
        // valid partial update, shallow merge of update.*, response has no token
        let (c, text) = raw(&addr, &post("/api/config", &addr, &token, r#"{"theme":"dark","update":{"checkOnStart":false},"onboarded":true}"#)).await;
        assert_eq!(c, 200, "{text}");
        let v = body_of(&text);
        assert!(v.get("controlToken").is_none());
        assert_eq!(v["update"]["checkOnStart"], false);
        assert_eq!(v["update"]["channelNode"], "lts", "other update.* keys survive");
        assert_eq!(core.config().theme, "dark");
        assert!(core.config().onboarded);
        let saved = std::fs::read_to_string(core.paths.config_file()).unwrap();
        assert!(saved.contains("\"theme\": \"dark\""));
        // keys that have their own endpoints are ignored here
        let (c, _) = raw(&addr, &post("/api/config", &addr, &token, r#"{"edition":"next","controlPort":1234,"apps":{"php":{"port":9}}}"#)).await;
        assert_eq!(c, 200);
        assert_eq!(core.config().edition, Edition::Php);
        assert_eq!(core.config().apps.php.port, 8080);
    }

    #[tokio::test]
    async fn status_shape_matches_the_panel_contract() {
        let (_d, _core, token, addr) = start().await;
        let (c, text) = raw(&addr, &get("/api/status", &addr, &format!("X-Token: {token}\r\n"))).await;
        assert_eq!(c, 200);
        let v = body_of(&text);
        assert_eq!(v["edition"], "php");
        assert_eq!(v["app"]["state"], "stopped");
        assert!(v["app"]["port"].is_number() && v["app"]["url"].is_string());
        assert!(v["editions"]["php"]["installed"].is_boolean() && v["editions"]["next"]["port"].is_number());
        assert!(v["config"].get("controlToken").is_none(), "token must not be exposed in status");
        assert_eq!(v["config"]["onboarded"], false);
        assert!(v["paths"]["data"].is_string() && v["paths"]["logs"].is_string() && v["paths"]["root"].is_string());
        assert!(v["about"]["license"].is_string());
        assert!(v["system"]["ramMB"].is_number());
        assert!(!text.contains(&token), "token must not appear anywhere in /api/status");
    }

    #[tokio::test]
    async fn edition_switch_is_refused_when_runtime_missing() {
        let (_d, core, token, addr) = start().await;
        let (c, text) = raw(&addr, &post("/api/edition", &addr, &token, r#"{"edition":"next","start":true}"#)).await;
        assert_eq!(c, 409, "{text}");
        assert_eq!(body_of(&text)["error"]["code"], "runtime_missing");
        assert_eq!(core.config().edition, Edition::Php, "nothing changed");
        let (c, text) = raw(&addr, &post("/api/edition", &addr, &token, r#"{"edition":"ruby"}"#)).await;
        assert_eq!(c, 422, "{text}");
    }

    #[tokio::test]
    async fn app_config_ports_and_runtime_errors() {
        let (_d, core, token, addr) = start().await;
        // port held by someone else -> 409 port_in_use
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let busy = l.local_addr().unwrap().port();
        let (c, text) = raw(&addr, &post("/api/app/config", &addr, &token, &format!(r#"{{"edition":"php","port":{busy}}}"#))).await;
        assert_eq!(c, 409, "{text}");
        assert_eq!(body_of(&text)["error"]["code"], "port_in_use");
        // unknown runtime -> 409 runtime_not_installed
        let (c, text) = raw(&addr, &post("/api/app/config", &addr, &token, r#"{"runtime":"1.2.3"}"#)).await;
        assert_eq!(c, 409, "{text}");
        assert_eq!(body_of(&text)["error"]["code"], "runtime_not_installed");
        // bad port
        let (c, text) = raw(&addr, &post("/api/app/config", &addr, &token, r#"{"port":70000}"#)).await;
        assert_eq!(c, 422, "{text}");
        // good edit of the inactive edition returns the flat app config
        drop(l);
        let (c, text) = raw(&addr, &post("/api/app/config", &addr, &token, &format!(r#"{{"edition":"next","port":{busy},"env":{{"A":"1"}}}}"#))).await;
        assert_eq!(c, 200, "{text}");
        let v = body_of(&text);
        assert_eq!(v["port"], busy);
        assert_eq!(v["env"]["A"], "1");
        assert_eq!(core.config().apps.next.port, busy);
    }

    #[tokio::test]
    async fn ports_check_and_token_regenerate() {
        let (_d, core, token, addr) = start().await;
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let busy = l.local_addr().unwrap().port();
        let (c, text) = raw(&addr, &get(&format!("/api/ports/check?port={busy}&edition=php"), &addr, &format!("X-Token: {token}\r\n"))).await;
        assert_eq!(c, 200);
        let v = body_of(&text);
        assert_eq!(v["free"], false);
        assert!(v["usedBy"].is_string());
        drop(l);
        let (_, text) = raw(&addr, &get(&format!("/api/ports/check?port={busy}"), &addr, &format!("X-Token: {token}\r\n"))).await;
        assert_eq!(body_of(&text)["free"], true);
        let (c, _) = raw(&addr, &get("/api/ports/check?port=0", &addr, &format!("X-Token: {token}\r\n"))).await;
        assert_eq!(c, 422);
        // regenerate: old token stops working, the new one is persisted and works
        let (c, text) = raw(&addr, &post("/api/config/token/regenerate", &addr, &token, "")).await;
        assert_eq!(c, 200, "{text}");
        let new = body_of(&text)["token"].as_str().unwrap().to_string();
        assert_ne!(new, token);
        assert_eq!(core.config().control_token, new);
        let (c, _) = raw(&addr, &get("/api/status", &addr, &format!("X-Token: {token}\r\n"))).await;
        assert_eq!(c, 401);
        let (c, _) = raw(&addr, &get("/api/status", &addr, &format!("X-Token: {new}\r\n"))).await;
        assert_eq!(c, 200);
        assert!(std::fs::read_to_string(core.paths.config_file()).unwrap().contains(&new));
    }

    #[tokio::test]
    async fn backup_jobs_and_listing() {
        let (_d, core, token, addr) = start().await;
        std::fs::create_dir_all(core.data_root().join("php")).unwrap();
        std::fs::write(core.data_root().join("php/a.db"), b"alpha").unwrap();
        let (c, text) = raw(&addr, &post("/api/backup/create", &addr, &token, "")).await;
        assert_eq!(c, 202, "{text}");
        let id = body_of(&text)["jobId"].as_str().unwrap().to_string();
        let mut state = String::new();
        for _ in 0..50 {
            let (_, t) = raw(&addr, &get(&format!("/api/jobs/{id}"), &addr, &format!("X-Token: {token}\r\n"))).await;
            state = body_of(&t)["state"].as_str().unwrap_or("").to_string();
            if state != "running" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert_eq!(state, "done");
        let (_, text) = raw(&addr, &get("/api/backup/list", &addr, &format!("X-Token: {token}\r\n"))).await;
        let list = body_of(&text);
        let b = &list["backups"][0];
        assert!(b["name"].as_str().unwrap().ends_with(".zip") && b["sizeMB"].is_number() && b["date"].is_string() && b["kind"] == "manual", "{list}");
        // restoring a name that does not exist is refused before any job starts
        let (c, _) = raw(&addr, &post("/api/backup/restore", &addr, &token, r#"{"name":"nope.zip"}"#)).await;
        assert_eq!(c, 404);
        let (c, _) = raw(&addr, &post("/api/backup/restore", &addr, &token, r#"{"name":"..\\x.zip"}"#)).await;
        assert_eq!(c, 422);
    }
}
