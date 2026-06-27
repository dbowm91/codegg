use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tracing::{info, warn};

use socket2::{Domain, Protocol, Socket, Type};

const MDNS_PORT: u16 = 5353;
const MDNS_MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const DNS_HEADER_LEN: usize = 12;
const DNS_POINTER_MASK: usize = 0xC0;
const DNS_POINTER_TAG: usize = 0xC0;
const DNS_LABEL_MASK: usize = 0x3F;
const DNS_SRV_RECORD: u16 = 0x0021;
const DNS_CLASS_IN: u16 = 0x0001;
const DNS_QUESTION_TRAILER_LEN: usize = 4;
const DNS_RESOURCE_FIXED_LEN: usize = 10;

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

        let (query_name, question_name_end) = parse_dns_name(data, DNS_HEADER_LEN)?;
        let question_end = question_name_end.checked_add(DNS_QUESTION_TRAILER_LEN)?;
        if question_end > data.len() {
            return None;
        }

        let service_name = service_name.trim_end_matches('.');
        if !query_name.contains(service_name) && !query_name.contains("_opencode") {
            return None;
        }

        let target = encode_dns_name(hostname)?;
        let rdlen = 6usize.checked_add(target.len())?;
        let rdlen = u16::try_from(rdlen).ok()?;

        let mut response = Vec::new();
        response.extend_from_slice(&id);
        response.extend_from_slice(&flags.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&data[DNS_HEADER_LEN..question_end]);

        response.extend_from_slice(&0xC00Cu16.to_be_bytes());
        response.extend_from_slice(&DNS_SRV_RECORD.to_be_bytes());
        response.extend_from_slice(&DNS_CLASS_IN.to_be_bytes());
        response.extend_from_slice(&120u32.to_be_bytes());
        response.extend_from_slice(&rdlen.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&port.to_be_bytes());
        response.extend_from_slice(&target);

        Some(response)
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

    query.extend_from_slice(&encode_dns_name(name).unwrap_or_else(|| vec![0]));

    query.extend_from_slice(&0x000cu16.to_be_bytes());
    query.extend_from_slice(&DNS_CLASS_IN.to_be_bytes());

    query
}

fn parse_mdns_response(data: &[u8]) -> Option<String> {
    if data.len() < DNS_HEADER_LEN {
        return None;
    }

    let answer_count = u16::from_be_bytes([data[6], data[7]]);
    if answer_count == 0 {
        return None;
    }

    let mut pos = DNS_HEADER_LEN;
    for _ in 0..u16::from_be_bytes([data[4], data[5]]) {
        let (_, next) = parse_dns_name(data, pos)?;
        pos = next.checked_add(DNS_QUESTION_TRAILER_LEN)?;
        if pos > data.len() {
            return None;
        }
    }

    for _ in 0..answer_count {
        let (_, record_start) = parse_dns_name(data, pos)?;
        if record_start + DNS_RESOURCE_FIXED_LEN > data.len() {
            return None;
        }

        let rtype = u16::from_be_bytes([data[record_start], data[record_start + 1]]);
        let rdlen = u16::from_be_bytes([data[record_start + 8], data[record_start + 9]]) as usize;
        let rdata_start = record_start + DNS_RESOURCE_FIXED_LEN;
        let rdata_end = rdata_start.checked_add(rdlen)?;
        if rdata_end > data.len() {
            return None;
        }

        if rtype == DNS_SRV_RECORD && rdlen >= 7 {
            let port = u16::from_be_bytes([data[rdata_start + 4], data[rdata_start + 5]]);
            if let Some((host, _)) = parse_dns_name(data, rdata_start + 6) {
                return Some(format!("{host}:{port}"));
            }
        }

        pos = rdata_end;
    }

    None
}

fn parse_dns_name(data: &[u8], offset: usize) -> Option<(String, usize)> {
    let mut labels = Vec::new();
    let mut pos = offset;
    let mut next_pos = None;
    let mut visited = std::collections::HashSet::new();

    loop {
        if pos >= data.len() || !visited.insert(pos) {
            return None;
        }

        let len = data[pos] as usize;
        if len == 0 {
            let end = next_pos.unwrap_or(pos + 1);
            return Some((labels.join("."), end));
        }

        if len & DNS_POINTER_MASK == DNS_POINTER_TAG {
            if pos + 1 >= data.len() {
                return None;
            }
            let ptr = ((len & DNS_LABEL_MASK) << 8) | data[pos + 1] as usize;
            if ptr >= data.len() {
                return None;
            }
            next_pos.get_or_insert(pos + 2);
            pos = ptr;
            continue;
        }

        if len & DNS_POINTER_MASK != 0 || len > 63 {
            return None;
        }

        let label_start = pos + 1;
        let label_end = label_start.checked_add(len)?;
        if label_end > data.len() {
            return None;
        }
        labels.push(String::from_utf8_lossy(&data[label_start..label_end]).to_string());
        pos = label_end;
    }
}

fn encode_dns_name(name: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for label in name.trim_end_matches('.').split('.') {
        if label.is_empty() || label.len() > 63 {
            return None;
        }
        out.push(u8::try_from(label.len()).ok()?);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    if out.len() > 255 {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_name_simple() {
        let data = b"\x09_opencode\x04_tcp\x05local\x00";
        let (name, next) = parse_dns_name(data, 0).unwrap();
        assert_eq!(name, "_opencode._tcp.local");
        assert_eq!(next, data.len());
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
        let (host, next) = parse_dns_name(data, 0).unwrap();
        assert_eq!(host, "codegg.local");
        assert_eq!(next, data.len());
    }

    #[test]
    fn test_parse_compressed_srv_response() {
        let mut response = Vec::new();
        response.extend_from_slice(&[0x00, 0x00]);
        response.extend_from_slice(&0x8400u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&1u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&encode_dns_name("_opencode._tcp.local.").unwrap());
        response.extend_from_slice(&0x000cu16.to_be_bytes());
        response.extend_from_slice(&DNS_CLASS_IN.to_be_bytes());

        let target = encode_dns_name("codegg.local.").unwrap();
        let rdlen = u16::try_from(6 + target.len()).unwrap();
        response.extend_from_slice(&0xC00Cu16.to_be_bytes());
        response.extend_from_slice(&DNS_SRV_RECORD.to_be_bytes());
        response.extend_from_slice(&DNS_CLASS_IN.to_be_bytes());
        response.extend_from_slice(&120u32.to_be_bytes());
        response.extend_from_slice(&rdlen.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&0u16.to_be_bytes());
        response.extend_from_slice(&3000u16.to_be_bytes());
        response.extend_from_slice(&target);

        assert_eq!(
            parse_mdns_response(&response).as_deref(),
            Some("codegg.local:3000")
        );
    }

    #[test]
    fn test_handle_query_builds_parseable_srv_response() {
        let query = build_query("_opencode._tcp.local.");
        let response =
            MdnsService::handle_query(&query, "_opencode._tcp.local.", "codegg.local.", 3000)
                .unwrap();

        assert_eq!(
            parse_mdns_response(&response).as_deref(),
            Some("codegg.local:3000")
        );
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
