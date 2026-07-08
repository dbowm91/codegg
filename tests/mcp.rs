#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    fn ipv6_segments_to_ipv4(ipv6: &Ipv6Addr) -> Option<Ipv4Addr> {
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

    fn is_internal_ip(ip: &IpAddr) -> bool {
        match ip {
            IpAddr::V4(ipv4) => {
                let octets = ipv4.octets();
                ipv4.is_loopback()
                    || octets[0] == 10
                    || (octets[0] == 172 && (octets[1] & 0xf0) == 16)
                    || (octets[0] == 192 && octets[1] == 168)
                    || octets[0] == 169 && octets[1] == 254
                    || octets[0] == 0
                    || (octets[0] == 100 && (octets[1] & 0xc0) == 64)
                    || (octets[0] == 198 && (octets[1] & 0xfe) == 18)
            }
            IpAddr::V6(ipv6) => {
                let segments = ipv6.segments();
                ipv6.is_loopback()
                    || ipv6.is_unicast_link_local()
                    || ipv6_segments_to_ipv4(ipv6)
                        .map(|v4| is_internal_ip(&IpAddr::V4(v4)))
                        .unwrap_or(false)
                    || (segments[0] == 0xfc00 || segments[0] == 0xfd00)
                    || segments[0] == 0
                        && segments[1] == 0
                        && segments[2] == 0
                        && segments[3] == 0
                        && segments[4] == 0
                        && segments[5] == 0
                        && segments[6] == 0
                        && segments[7] == 0
            }
        }
    }

    #[test]
    fn test_mcp_ipv4_loopback() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            127, 255, 255, 255
        ))));
    }

    #[test]
    fn test_mcp_ipv4_zero() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))));
    }

    #[test]
    fn test_mcp_ipv4_10_range() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            10, 255, 255, 255
        ))));
    }

    #[test]
    fn test_mcp_ipv4_172_16_range() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(172, 16, 0, 0))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            172, 31, 255, 255
        ))));
    }

    #[test]
    fn test_mcp_ipv4_172_not_16_range() {
        assert!(!is_internal_ip(&IpAddr::V4(Ipv4Addr::new(172, 32, 0, 0))));
        assert!(!is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            172, 15, 255, 255
        ))));
    }

    #[test]
    fn test_mcp_ipv4_192_168_range() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(192, 168, 0, 0))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            192, 168, 255, 255
        ))));
    }

    #[test]
    fn test_mcp_ipv4_169_254_link_local() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(169, 254, 0, 0))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            169, 254, 255, 255
        ))));
    }

    #[test]
    fn test_mcp_ipv4_100_cgnat_range() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(100, 64, 0, 0))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            100, 127, 255, 255
        ))));
    }

    #[test]
    fn test_mcp_ipv4_100_not_cgnat_range() {
        assert!(!is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            100, 63, 255, 255
        ))));
        assert!(!is_internal_ip(&IpAddr::V4(Ipv4Addr::new(100, 128, 0, 0))));
    }

    #[test]
    fn test_mcp_ipv4_198_benchmark_range() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(198, 18, 0, 0))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            198, 19, 255, 255
        ))));
    }

    #[test]
    fn test_mcp_ipv4_198_not_benchmark_range() {
        assert!(!is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            198, 17, 255, 255
        ))));
        assert!(!is_internal_ip(&IpAddr::V4(Ipv4Addr::new(198, 20, 0, 0))));
    }

    #[test]
    fn test_mcp_ipv4_external() {
        assert!(!is_internal_ip(&IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
        assert!(!is_internal_ip(&IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
        assert!(!is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            142, 250, 185, 248
        ))));
    }

    #[test]
    fn test_mcp_ipv6_loopback() {
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0, 0, 0, 0, 0, 0, 0, 1
        ))));
    }

    #[test]
    fn test_mcp_ipv6_unspecified() {
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0, 0, 0, 0, 0, 0, 0, 0
        ))));
    }

    #[test]
    fn test_mcp_ipv6_ula_fc00() {
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xfc00, 0, 0, 0, 0, 0, 0, 0
        ))));
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xfc00, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff, 0xffff
        ))));
    }

    #[test]
    fn test_mcp_ipv6_ula_fd00() {
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xfd00, 0, 0, 0, 0, 0, 0, 0
        ))));
    }

    #[test]
    fn test_mcp_ipv6_not_ula() {
        assert!(!is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xfc01, 0, 0, 0, 0, 0, 0, 0
        ))));
        assert!(!is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xfe00, 0, 0, 0, 0, 0, 0, 0
        ))));
    }

    #[test]
    fn test_mcp_ipv6_external() {
        assert!(!is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0x2001, 0x4860, 0, 0, 0, 0, 0, 0
        ))));
        assert!(!is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0x2606, 0x4700, 0x470, 0x470, 0x470, 0x470, 0x470, 0x470
        ))));
    }

    #[test]
    fn test_mcp_ipv6_mapped_ipv4_loopback() {
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0, 0, 0, 0, 0, 0xffff, 0x7f00, 0x0001
        ))));
    }

    #[test]
    fn test_mcp_ipv6_link_local() {
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xfe80, 0, 0, 0, 0, 0, 0, 0
        ))));
    }

    #[test]
    fn test_mcp_service_tool_listing() {
        use codegg::mcp::McpService;

        let service = McpService::new();
        let tools = service.list_tools();

        assert!(tools.is_empty());
    }

    #[test]
    fn test_mcp_service_server_status() {
        use codegg::mcp::McpService;

        let service = McpService::new();
        let status = service.server_status();

        assert!(status.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_mcp_service_disconnect_nonexistent() {
        use codegg::mcp::McpService;

        let mut service = McpService::new();
        let result = service.disconnect("nonexistent").await;
        assert!(result.is_err());
    }

    #[test]
    fn test_mcp_service_list_tools_empty() {
        use codegg::mcp::McpService;

        let service = McpService::new();
        let tools = service.list_tools();
        assert!(tools.is_empty());
    }

    #[test]
    fn test_mcp_service_server_tools_empty() {
        use codegg::mcp::McpService;

        let service = McpService::new();
        let tools = service.server_tools();
        assert!(tools.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_mcp_service_shutdown_all() {
        use codegg::mcp::McpService;

        let mut service = McpService::new();
        service.shutdown_all().await;
        assert!(service.server_status().is_empty());
    }
}
