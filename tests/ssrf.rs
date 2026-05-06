use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use codegg::security::ssrf::is_internal_ip;

#[cfg(test)]
mod tests {
    use super::*;
    use codegg::security::ssrf::{is_internal_ip, validate_host_ip, validate_url_host};

    #[test]
    fn test_ipv4_loopback() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            127, 255, 255, 255
        ))));
    }

    #[test]
    fn test_ipv4_zero() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))));
    }

    #[test]
    fn test_ipv4_10_range() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            10, 255, 255, 255
        ))));
    }

    #[test]
    fn test_ipv4_172_16_range() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(172, 16, 0, 0))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            172, 31, 255, 255
        ))));
    }

    #[test]
    fn test_ipv4_172_not_16_range() {
        assert!(!is_internal_ip(&IpAddr::V4(Ipv4Addr::new(172, 32, 0, 0))));
        assert!(!is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            172, 15, 255, 255
        ))));
    }

    #[test]
    fn test_ipv4_192_168_range() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(192, 168, 0, 0))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            192, 168, 255, 255
        ))));
    }

    #[test]
    fn test_ipv4_169_254_link_local() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(169, 254, 0, 0))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            169, 254, 255, 255
        ))));
    }

    #[test]
    fn test_ipv4_100_cgnat_range() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(100, 64, 0, 0))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            100, 127, 255, 255
        ))));
    }

    #[test]
    fn test_ipv4_100_not_cgnat_range() {
        assert!(!is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            100, 63, 255, 255
        ))));
        assert!(!is_internal_ip(&IpAddr::V4(Ipv4Addr::new(100, 128, 0, 0))));
    }

    #[test]
    fn test_ipv4_198_benchmark_range() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(198, 18, 0, 0))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            198, 19, 255, 255
        ))));
    }

    #[test]
    fn test_ipv4_198_not_benchmark_range() {
        assert!(!is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            198, 17, 255, 255
        ))));
        assert!(!is_internal_ip(&IpAddr::V4(Ipv4Addr::new(198, 20, 0, 0))));
    }

    #[test]
    fn test_ipv4_multicast() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(224, 0, 0, 0))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(224, 0, 0, 1))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            239, 255, 255, 255
        ))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(232, 0, 0, 0))));
    }

    #[test]
    fn test_ipv4_external() {
        assert!(!is_internal_ip(&IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
        assert!(!is_internal_ip(&IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
        assert!(!is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            142, 250, 185, 248
        ))));
    }

    #[test]
    fn test_ipv6_loopback() {
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0, 0, 0, 0, 0, 0, 0, 1
        ))));
    }

    #[test]
    fn test_ipv6_unspecified() {
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0, 0, 0, 0, 0, 0, 0, 0
        ))));
    }

    #[test]
    fn test_ipv6_ula_fc00() {
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xfc00, 0, 0, 0, 0, 0, 0, 0
        ))));
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xfc00, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff
        ))));
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xfc01, 0, 0, 0, 0, 0, 0, 0
        ))));
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xfcff, 0, 0, 0, 0, 0, 0, 0
        ))));
    }

    #[test]
    fn test_ipv6_ula_fd00() {
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xfd00, 0, 0, 0, 0, 0, 0, 0
        ))));
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xfd01, 0, 0, 0, 0, 0, 0, 0
        ))));
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xfdff, 0, 0, 0, 0, 0, 0, 0
        ))));
    }

    #[test]
    fn test_ipv6_ula_edge_cases() {
        assert!(!is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xfe00, 0, 0, 0, 0, 0, 0, 0
        ))));
        assert!(!is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xfeff, 0, 0, 0, 0, 0, 0, 0
        ))));
    }

    #[test]
    fn test_ipv6_multicast() {
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xff00, 0, 0, 0, 0, 0, 0, 0
        ))));
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xff02, 0, 0, 0, 0, 0, 0, 1
        ))));
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff
        ))));
        assert!(!is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xfe00, 0, 0, 0, 0, 0, 0, 0
        ))));
    }

    #[test]
    fn test_ipv6_external() {
        assert!(!is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0x2001, 0x4860, 0, 0, 0, 0, 0, 0
        ))));
        assert!(!is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0x2606, 0x4700, 0x470, 0x470, 0x470, 0x470, 0x470, 0x470
        ))));
    }

    #[test]
    fn test_ipv6_mapped_ipv4_loopback() {
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0, 0, 0, 0, 0, 0xffff, 0x7f00, 0x0001
        ))));
    }

    #[test]
    fn test_validate_url_host_https() {
        let result = validate_url_host("https://example.com/path");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "example.com");
    }

    #[test]
    fn test_validate_url_host_http() {
        let result = validate_url_host("http://example.com:8080/path");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "example.com");
    }

    #[test]
    fn test_validate_url_host_unsupported_scheme() {
        let result = validate_url_host("ftp://example.com");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unsupported URL scheme"));
    }

    #[test]
    fn test_validate_url_host_no_host() {
        let result = validate_url_host("https://");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_url_host_internal_blocked() {
        let result = validate_url_host("https://127.0.0.1");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("internal addresses not allowed"));
    }

    #[test]
    fn test_validate_url_host_internal_blocked_10() {
        let result = validate_url_host("http://10.0.0.1:8080");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("internal addresses not allowed"));
    }

    #[test]
    fn test_validate_host_ip_external() {
        let result = validate_host_ip("example.com", 443);
        assert!(result.is_ok());
        let ips = result.unwrap();
        assert!(!ips.is_empty());
        for ip in ips {
            assert!(!is_internal_ip(&ip));
        }
    }

    #[test]
    fn test_validate_host_ip_internal_blocked() {
        let result = validate_host_ip("127.0.0.1", 80);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("internal addresses not allowed"));
    }
}
