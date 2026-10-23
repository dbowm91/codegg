mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    // The IP classifier under test is production's. This file used to
    // carry a *copy* of `codegg::security::ssrf::is_internal_ip`, and the
    // copy had drifted from it in three predicates (IPv4 multicast, the
    // IPv6 unique-local range, IPv6 multicast). Two green tests then
    // asserted opposite answers to the same question: this file asserted
    // that `fc01::` is not internal, while
    // `tests/security_permission/ssrf.inc` asserted against production
    // that it is. Only one can be true of the production predicate.
    use codegg::security::ssrf::is_internal_ip;

    // IPv4 multicast (`224.0.0.0/4`) is blocked by production. The
    // drifted local copy of the classifier had no such predicate.
    #[test]
    fn test_mcp_ipv4_multicast() {
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(224, 0, 0, 0))));
        assert!(is_internal_ip(&IpAddr::V4(Ipv4Addr::new(
            239, 255, 255, 255
        ))));
    }

    // The unique-local range is `fc00::/7`, so every address in
    // `fc00::`..`fdff::` is internal — not only the two exact values the
    // drifted local copy happened to recognise. `fc01::` is inside the
    // range, and production blocks it.
    #[test]
    fn test_mcp_ipv6_ula_beyond_exact_values() {
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xfc01, 0, 0, 0, 0, 0, 0, 0
        ))));
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xfdff, 0, 0, 0, 0, 0, 0, 0
        ))));
    }

    // IPv6 multicast (`ff00::/8`) is blocked by production. The drifted
    // local copy had no such predicate.
    #[test]
    fn test_mcp_ipv6_multicast() {
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xff00, 0, 0, 0, 0, 0, 0, 0
        ))));
        assert!(is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xff02, 0, 0, 0, 0, 0, 0, 1
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
        // `fc01::` is inside the `fc00::/7` unique-local range, so
        // production blocks it. This file used to assert that it was *not*
        // internal — a consequence of the drifted local copy of the
        // classifier, and contradicted by
        // `tests/security_permission/ssrf.inc`, which asserts the
        // opposite against production. The corrected assertion lives in
        // `test_mcp_ipv6_ula_beyond_exact_values`. What remains here is
        // the address genuinely outside the unique-local range.
        assert!(!is_internal_ip(&IpAddr::V6(Ipv6Addr::new(
            0xfe00, 0, 0, 0, 0, 0, 0, 0
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
