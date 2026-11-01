//! Port helpers.
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::time::Duration;

/// True if we can bind `bind:port` right now (Windows does not allow shared binds by default).
pub fn port_free(bind: &str, port: u16) -> bool {
    let ip: IpAddr = bind.parse().unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    if TcpListener::bind(SocketAddr::new(ip, port)).is_err() {
        return false;
    }
    // A listener on 0.0.0.0 would also conflict with 127.0.0.1; check loopback too when binding to all interfaces.
    if ip.is_unspecified() {
        return TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)).is_ok();
    }
    true
}

pub async fn can_connect(port: u16) -> bool {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    matches!(tokio::time::timeout(Duration::from_millis(400), tokio::net::TcpStream::connect(addr)).await, Ok(Ok(_)))
}

/// First LAN IPv4 address (for the "open from other devices" hint); best effort.
pub fn lan_ip() -> Option<String> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("8.8.8.8:80").ok()?; // no packet is sent for UDP connect
    let ip = s.local_addr().ok()?.ip();
    if ip.is_loopback() || ip.is_unspecified() {
        None
    } else {
        Some(ip.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_busy_port() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        assert!(!port_free("127.0.0.1", port));
        assert!(!port_free("0.0.0.0", port));
        drop(l);
        assert!(port_free("127.0.0.1", port));
    }
}
