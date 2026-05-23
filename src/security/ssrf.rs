use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};
use std::str::FromStr;

pub fn is_internal_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => {
            let octets = ipv4.octets();
            ipv4.is_loopback()
                || octets[0] == 10
                || (octets[0] == 172 && (octets[1] & 0xf0) == 16)
                || (octets[0] == 192 && octets[1] == 168)
                || (octets[0] == 169 && octets[1] == 254)
                || octets[0] == 0
                || (octets[0] == 100 && (octets[1] & 0xc0) == 64)
                || (octets[0] == 198 && (octets[1] & 0xfe) == 18)
                || (octets[0] & 0xf0) == 224
        }
        IpAddr::V6(ipv6) => {
            let segments = ipv6.segments();
            ipv6.is_loopback()
                || ipv6.is_unicast_link_local()
                || ipv6_segments_to_ipv4(ipv6)
                    .map(|v4| is_internal_ip(&IpAddr::V4(v4)))
                    .unwrap_or(false)
                || (segments[0] & 0xfe00) == 0xfc00
                || (segments[0] & 0xff00) == 0xff00
                || (segments[0] == 0
                    && segments[1] == 0
                    && segments[2] == 0
                    && segments[3] == 0
                    && segments[4] == 0
                    && segments[5] == 0
                    && segments[6] == 0
                    && segments[7] == 0)
        }
    }
}

pub fn ipv6_segments_to_ipv4(ipv6: &Ipv6Addr) -> Option<Ipv4Addr> {
    let segments = ipv6.segments();
    if segments[0] == 0
        && segments[1] == 0
        && segments[2] == 0
        && segments[3] == 0
        && segments[4] == 0
    {
        if segments[5] == 0xffff {
            return Some(Ipv4Addr::new(
                (segments[6] >> 8) as u8,
                (segments[6] & 0xff) as u8,
                (segments[7] >> 8) as u8,
                (segments[7] & 0xff) as u8,
            ));
        }
        if segments[5] == 0 {
            return Some(Ipv4Addr::new(
                (segments[6] >> 8) as u8,
                (segments[6] & 0xff) as u8,
                (segments[7] >> 8) as u8,
                (segments[7] & 0xff) as u8,
            ));
        }
    }
    None
}

pub fn validate_host_ip(host: &str, port: u16) -> Result<Vec<IpAddr>, String> {
    let socket_addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|_| format!("cannot resolve host to address: {}", host))?
        .collect();

    let validated_ips: Vec<IpAddr> = socket_addrs.iter().map(|addr| addr.ip()).collect();

    for ip in &validated_ips {
        if is_internal_ip(ip) {
            return Err(format!(
                "access to internal addresses not allowed: {}",
                host
            ));
        }
    }

    if let Ok(ip) = IpAddr::from_str(host) {
        if is_internal_ip(&ip) {
            return Err(format!(
                "access to internal addresses not allowed: {}",
                host
            ));
        }
    }

    Ok(validated_ips)
}

pub fn revalidate_dns(host: &str, port: u16, validated_ips: &[IpAddr]) -> Result<(), String> {
    let current_addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|_| format!("cannot resolve host to address: {}", host))?
        .collect();

    let current_ips: Vec<IpAddr> = current_addrs.iter().map(|addr| addr.ip()).collect();

    for ip in &current_ips {
        if !validated_ips.contains(ip) {
            if let IpAddr::V6(ipv6) = ip {
                if let Some(v4) = ipv6_segments_to_ipv4(ipv6) {
                    if validated_ips.contains(&IpAddr::V4(v4)) {
                        continue;
                    }
                }
            }
            return Err(format!(
                "DNS rebinding attack detected: IP address changed for {}",
                host
            ));
        }
    }

    Ok(())
}

pub fn validate_url_host(url: &str) -> Result<String, String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("invalid URL: {}", e))?;

    match parsed.scheme() {
        "http" | "https" => {}
        _ => {
            return Err(format!("unsupported URL scheme: {}", parsed.scheme()));
        }
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| "URL must have a host".to_string())?
        .to_string();

    let port = parsed
        .port()
        .unwrap_or(if parsed.scheme() == "https" { 443 } else { 80 });

    validate_host_ip(&host, port)?;

    Ok(host.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_internal_ip_loopback() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(127, 255, 255, 255))));
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1))));
    }

    #[test]
    fn test_is_internal_ip_private_class_a() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(10, 255, 255, 255))));
    }

    #[test]
    fn test_is_internal_ip_private_class_b() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(172, 16, 0, 0))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(172, 31, 255, 255))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(172, 20, 0, 0))));
    }

    #[test]
    fn test_is_internal_ip_private_class_c() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(192, 168, 0, 0))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(192, 168, 255, 255))));
    }

    #[test]
    fn test_is_internal_ip_link_local() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(169, 254, 0, 0))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(169, 254, 255, 255))));
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::LOCALHOST)));
    }

    #[test]
    fn test_is_internal_ip_multicast() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(224, 0, 0, 0))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(239, 255, 255, 255))));
    }

    #[test]
    fn test_is_internal_ip_cgnat() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(100, 64, 0, 0))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(100, 127, 255, 255))));
    }

    #[test]
    fn test_is_internal_ip_ipv4_mapped_ipv6() {
        let ipv4_mapped = Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0xc0a8, 0x0001);
        assert!(is_internal_ip(&IpAddr::V6(ipv4_mapped)));
    }

    #[test]
    fn test_is_internal_ip_site_local() {
        let site_local = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 0);
        assert!(is_internal_ip(&IpAddr::V6(site_local)));
    }

    #[test]
    fn test_is_internal_ip_unicast_link_local() {
        let link_local = Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1);
        assert!(is_internal_ip(&IpAddr::V6(link_local)));
    }

    #[test]
    fn test_is_internal_ip_public() {
        assert!(!is_internal_ip(&IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
        assert!(!is_internal_ip(&IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
        assert!(!is_internal_ip(&IpAddr::V6(Ipv6Addr::new(0x2001, 0x4860, 0, 0x2000, 0, 0, 0, 0))));
    }

    #[test]
    fn test_ipv6_segments_to_ipv4() {
        let ipv4_mapped = Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0xc0a8, 0x0001);
        assert_eq!(ipv6_segments_to_ipv4(&ipv4_mapped), Some(Ipv4Addr::new(192, 168, 0, 1)));

        let ipv4_compatible = Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0xc0a8, 0x0001);
        assert_eq!(ipv6_segments_to_ipv4(&ipv4_compatible), Some(Ipv4Addr::new(192, 168, 0, 1)));

        let regular_ipv6 = Ipv6Addr::new(0x2001, 0x4860, 0, 0x2000, 0, 0, 0, 0);
        assert_eq!(ipv6_segments_to_ipv4(&regular_ipv6), None);
    }

    #[test]
    fn test_validate_url_host_https() {
        assert_eq!(validate_url_host("https://8.8.8.8").unwrap(), "8.8.8.8");
        assert_eq!(validate_url_host("https://1.1.1.1").unwrap(), "1.1.1.1");
    }

    #[test]
    fn test_validate_url_host_http() {
        assert_eq!(validate_url_host("http://8.8.8.8").unwrap(), "8.8.8.8");
    }

    #[test]
    fn test_validate_url_host_with_port() {
        assert_eq!(validate_url_host("https://8.8.8.8:8443").unwrap(), "8.8.8.8");
    }

    #[test]
    fn test_validate_url_host_unsupported_scheme() {
        assert!(validate_url_host("ftp://8.8.8.8").is_err());
        assert!(validate_url_host("file:///etc/passwd").is_err());
        assert!(validate_url_host("javascript:alert(1)").is_err());
    }

    #[test]
    fn test_validate_url_host_no_host() {
        assert!(validate_url_host("https://").is_err());
    }

    #[test]
    fn test_validate_url_host_internal_blocked() {
        assert!(validate_url_host("https://127.0.0.1").is_err());
        assert!(validate_url_host("https://192.168.1.1").is_err());
        assert!(validate_url_host("https://10.0.0.1").is_err());
        assert!(validate_url_host("https://172.16.0.1").is_err());
    }
}
