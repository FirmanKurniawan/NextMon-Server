//! Minimal blocking HTTP/1.1 client for talking to the local control API (CLI and single-instance forwarding).
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

pub struct Client {
    pub port: u16,
    pub token: String,
}

pub struct Reply {
    pub status: u16,
    pub body: String,
}

impl Reply {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or(Value::Null)
    }
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
    /// Error message from `{error:{message:{id,en}}}` (or a plain string), else the raw body.
    pub fn error_message(&self, english: bool) -> String {
        let j = self.json();
        let m = &j["error"]["message"];
        let pick = if m.is_object() { m[if english { "en" } else { "id" }].as_str().or_else(|| m["en"].as_str()) } else { m.as_str() };
        pick.map(String::from).unwrap_or_else(|| self.body.chars().take(300).collect())
    }
}

impl Client {
    pub fn new(port: u16, token: &str) -> Client {
        Client { port, token: token.to_string() }
    }

    pub fn request(&self, method: &str, path: &str, body: Option<&Value>, timeout: Duration) -> Result<Reply, String> {
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));
        let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(800)).map_err(|e| e.to_string())?;
        s.set_read_timeout(Some(timeout)).ok();
        s.set_write_timeout(Some(Duration::from_secs(5))).ok();
        let payload = body.map(|b| b.to_string()).unwrap_or_default();
        let req = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nX-Token: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
            self.port,
            self.token,
            payload.len()
        );
        s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&raw).to_string();
        let (head, body) = text.split_once("\r\n\r\n").ok_or("malformed response")?;
        let status = head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).ok_or("malformed status line")?;
        let chunked = head.to_ascii_lowercase().contains("transfer-encoding: chunked");
        let body = if chunked { dechunk(body) } else { body.to_string() };
        Ok(Reply { status, body })
    }

    pub fn get(&self, path: &str) -> Result<Reply, String> {
        self.request("GET", path, None, Duration::from_secs(30))
    }

    pub fn post(&self, path: &str, body: &Value) -> Result<Reply, String> {
        self.request("POST", path, Some(body), Duration::from_secs(120))
    }

    pub fn alive(&self) -> bool {
        self.request("GET", "/api/version", None, Duration::from_secs(2)).map(|r| r.ok()).unwrap_or(false)
    }
}

fn dechunk(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some((len_line, after)) = rest.split_once("\r\n") {
        let n = usize::from_str_radix(len_line.trim().split(';').next().unwrap_or("0"), 16).unwrap_or(0);
        if n == 0 || after.len() < n {
            break;
        }
        out.push_str(&after[..n]);
        rest = after[n..].trim_start_matches("\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn dechunk_works() {
        assert_eq!(super::dechunk("5\r\nhello\r\n3\r\n wo\r\n0\r\n\r\n"), "hello wo");
    }
}
