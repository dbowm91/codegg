use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tracing::{info, warn};

use socket2::{Domain, Protocol, Socket, Type};

const MDNS_PORT: u16 = 5353;
const MDNS_MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);

pub struct MdnsService {
    running: Arc<AtomicBool>,
    socket: Arc<Mutex<Option<Arc<UdpSocket>>>>,
    service_name: String,
    port: u16,
    domain: String,
}

impl MdnsService {
    pub fn new(port: u16, domain: Option<String>) -> Self {
        let service_name = "_opencode._tcp.local.".to_string();
        let domain = domain.unwrap_or_else(|| "local.".to_string());
        Self {
            running: Arc::new(AtomicBool::new(false)),
            socket: Arc::new(Mutex::new(None)),
            service_name,
            port,
            domain,
        }
    }

    pub async fn start(&self) -> Result<(), String> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        let socket = match self.create_socket().await {
            Ok(s) => s,
            Err(e) => return Err(format!("failed to create mDNS socket: {e}")),
        };

        *self.socket.lock().await = Some(Arc::new(socket));
        self.running.store(true, Ordering::SeqCst);

        let running = self.running.clone();
        let socket = self.socket.clone();
        let service_name = self.service_name.clone();
        let hostname = format!("codegg.{}", self.domain);
        let port = self.port;

        tokio::spawn(async move {
            info!("mDNS service started: {} on port {}", service_name, port);

            while running.load(Ordering::SeqCst) {
                if let Some(ref sock) = *socket.lock().await {
                    let mut buf = [0u8; 4096];
                    match sock.recv_from(&mut buf).await {
                        Ok((len, addr)) => {
                            if len > 0 {
                                if let Some(response) =
                                    Self::handle_query(&buf[..len], &service_name, &hostname, port)
                                {
                                    if let Err(e) = sock.send_to(&response, addr).await {
                                        warn!("mDNS send error: {}", e);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            warn!("mDNS recv error: {}", e);
                            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        }
                    }
                } else {
                    break;
                }
            }

            info!("mDNS service stopped");
        });

        Ok(())
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    async fn create_socket(&self) -> Result<UdpSocket, String> {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
            .map_err(|e| format!("socket creation failed: {e}"))?;

        socket
            .set_reuse_address(true)
            .map_err(|e| format!("set reuse address failed: {e}"))?;

        let addr: std::net::SocketAddr = format!("0.0.0.0:{}", MDNS_PORT)
            .parse()
            .map_err(|e| format!("invalid address: {e}"))?;
        let sock_addr = socket2::SockAddr::from(addr);
        socket
            .bind(&sock_addr)
            .map_err(|e| format!("bind failed: {e}"))?;

        socket
            .join_multicast_v4(&MDNS_MULTICAST_ADDR, &Ipv4Addr::UNSPECIFIED)
            .map_err(|e| format!("multicast join failed: {e}"))?;

        let std_socket: std::net::UdpSocket = socket.into();
        let socket = UdpSocket::from_std(std_socket)
            .map_err(|e| format!("convert to tokio socket failed: {e}"))?;

        Ok(socket)
    }

    fn handle_query(data: &[u8], service_name: &str, hostname: &str, port: u16) -> Option<Vec<u8>> {
        if data.len() < 12 {
            return None;
        }

        let id = [data[0], data[1]];
        let flags: u16 = 0x8400;
        let questions = u16::from_be_bytes([data[4], data[5]]);

        if questions == 0 {
            return None;
        }

        let query_name = Self::parse_name(data, 12)?;
        if !query_name.contains(service_name) && !query_name.contains("_opencode") {
            return None;
        }

        let mut response = Vec::new();
        response.extend_from_slice(&id);
        response.extend_from_slice(&flags.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());

        let svc_name = "\x09_opencode\x04_tcp\x05local\x00".to_string();
        response.extend_from_slice(svc_name.as_bytes());
        response.extend_from_slice(&0x0021u16.to_be_bytes());
        response.extend_from_slice(&0x0001u16.to_be_bytes());
        response.extend_from_slice(&120u32.to_be_bytes());
        response.extend_from_slice(&10u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&port.to_be_bytes());
        if hostname.len() > 255 {
            return None;
        }
        response.extend_from_slice(hostname.as_bytes());

        Some(response)
    }

    fn parse_name(data: &[u8], offset: usize) -> Option<String> {
        let mut result = String::new();
        let mut pos = offset;
        let mut visited = std::collections::HashSet::new();

        loop {
            if pos >= data.len() {
                break;
            }
            if !visited.insert(pos) {
                break;
            }
            let len = data[pos] as usize;
            if len == 0 {
                break;
            }
            if len & 0xC0 == 0xC0 {
                if pos + 1 >= data.len() {
                    break;
                }
                let ptr = (len & 0x3F) << 8 | data[pos + 1] as usize;
                if ptr >= data.len() {
                    break;
                }
                if !result.is_empty() {
                    result.push('.');
                }
                if let Some(name) = Self::parse_name(data, ptr) {
                    result.push_str(&name);
                }
                break;
            }
            pos += 1;
            if pos + len > data.len() {
                return None;
            }
            if !result.is_empty() {
                result.push('.');
            }
            result.push_str(&String::from_utf8_lossy(&data[pos..pos + len]));
            pos += len;
        }

        Some(result)
    }
}

pub async fn discover_services(timeout_ms: u64) -> Vec<String> {
    let socket = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let query = build_query("_opencode._tcp.local.");
    let multicast = format!("{}:{}", MDNS_MULTICAST_ADDR, MDNS_PORT);

    if let Err(e) = socket.send_to(&query, &multicast).await {
        warn!("mDNS query send error: {}", e);
        return vec![];
    }

    let mut services = Vec::new();
    let mut buf = [0u8; 4096];
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }

        match tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await {
            Ok(Ok((len, _))) => {
                if let Some(info) = parse_mdns_response(&buf[..len]) {
                    services.push(info);
                }
            }
            _ => break,
        }
    }

    services
}

fn build_query(name: &str) -> Vec<u8> {
    let mut query = Vec::new();
    query.extend_from_slice(&[0x00, 0x00]);
    query.extend_from_slice(&0x0100u16.to_be_bytes());
    query.extend_from_slice(&1u16.to_be_bytes());
    query.extend_from_slice(&0u16.to_be_bytes());
    query.extend_from_slice(&0u16.to_be_bytes());
    query.extend_from_slice(&0u16.to_be_bytes());

    for label in name.split('.') {
        if label.is_empty() {
            query.push(0);
        } else {
            query.push(label.len() as u8);
            query.extend_from_slice(label.as_bytes());
        }
    }

    query.extend_from_slice(&0x000cu16.to_be_bytes());
    query.extend_from_slice(&0x0001u16.to_be_bytes());

    query
}

fn parse_mdns_response(data: &[u8]) -> Option<String> {
    if data.len() < 12 {
        return None;
    }

    let answer_count = u16::from_be_bytes([data[6], data[7]]);
    if answer_count == 0 {
        return None;
    }

    let mut pos = 12;
    for _ in 0..u16::from_be_bytes([data[4], data[5]]) {
        while pos < data.len() && data[pos] != 0 {
            pos += 1 + data[pos] as usize;
        }
        pos += 1;
    }

    for _ in 0..answer_count {
        if pos + 10 > data.len() {
            return None;
        }

        let rtype = u16::from_be_bytes([data[pos], data[pos + 1]]);
        let rdlen = u16::from_be_bytes([data[pos + 8], data[pos + 9]]);
        pos += 10;

        if rtype == 0x0021 && pos + 6 + rdlen as usize <= data.len() {
            let port = u16::from_be_bytes([data[pos + 4], data[pos + 5]]);
            let host_start = pos + 6;
            if let Some(host) = extract_host(&data[host_start..]) {
                return Some(format!("{host}:{port}"));
            }
        }

        pos += rdlen as usize;
    }

    None
}

fn extract_host(data: &[u8]) -> Option<String> {
    let mut result = String::new();
    let mut pos = 0;

    while pos < data.len() && data[pos] != 0 {
        let len = data[pos] as usize;
        if len == 0 || pos + 1 + len > data.len() {
            break;
        }
        if !result.is_empty() {
            result.push('.');
        }
        result.push_str(&String::from_utf8_lossy(&data[pos + 1..pos + 1 + len]));
        pos += 1 + len;
    }

    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_name_simple() {
        let data = b"\x09_opencode\x04_tcp\x05local\x00";
        let name = MdnsService::parse_name(data, 0).unwrap();
        assert_eq!(name, "_opencode._tcp.local");
    }

    #[test]
    fn test_build_query() {
        let query = build_query("_opencode._tcp.local.");
        assert!(!query.is_empty());
        assert_eq!(query[0], 0x00);
        assert_eq!(query[1], 0x00);
    }

    #[test]
    fn test_extract_host() {
        let data = b"\x06codegg\x05local\x00";
        let host = extract_host(data).unwrap();
        assert_eq!(host, "codegg.local");
    }

    #[test]
    fn test_mdns_service_creation() {
        let service = MdnsService::new(3000, None);
        assert!(!service.is_running());
        assert_eq!(service.port, 3000);
    }

    #[test]
    fn test_handle_query_too_short() {
        let result =
            MdnsService::handle_query(&[0, 1], "_opencode._tcp.local.", "codegg.local.", 3000);
        assert!(result.is_none());
    }

    #[test]
    fn test_handle_query_no_questions() {
        let data = vec![0u8; 12];
        let result =
            MdnsService::handle_query(&data, "_opencode._tcp.local.", "codegg.local.", 3000);
        assert!(result.is_none());
    }
}
