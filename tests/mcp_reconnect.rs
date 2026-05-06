#[cfg(test)]
mod tests {
    use codegg::mcp::{McpServerStatus, McpService};

    #[tokio::test]
    async fn test_mcp_service_connect_local_invalid_command() {
        let mut service = McpService::new();
        let result = service
            .connect_stdio(
                "test_server",
                "nonexistent_command_xyz",
                &[],
                std::collections::HashMap::new(),
                5000,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mcp_service_connect_remote_invalid_url() {
        let mut service = McpService::new();
        let result = service
            .connect_http(
                "test_server",
                "not a valid url",
                std::collections::HashMap::new(),
                5000,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mcp_service_disconnect_nonexistent() {
        let mut service = McpService::new();
        let result = service.disconnect("nonexistent_server").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mcp_service_call_tool_no_server() {
        let service = McpService::new();
        let result = service
            .call_tool("nonexistent", "tool", serde_json::json!({}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mcp_service_list_prompts_no_server() {
        let service = McpService::new();
        let result = service.list_prompts("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mcp_service_get_prompt_no_server() {
        let service = McpService::new();
        let result = service.get_prompt("nonexistent", "prompt", None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mcp_service_list_resources_no_server() {
        let service = McpService::new();
        let result = service.list_resources("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mcp_service_read_resource_no_server() {
        let service = McpService::new();
        let result = service.read_resource("nonexistent", "uri").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mcp_service_handle_tool_list_changed_no_server() {
        let mut service = McpService::new();
        let result = service.handle_tool_list_changed("nonexistent").await;
        assert!(result.is_err());
    }

    #[test]
    fn test_mcp_server_status_variants() {
        let status = McpServerStatus::Disconnected;
        assert!(matches!(status, McpServerStatus::Disconnected));

        let status = McpServerStatus::Connecting;
        assert!(matches!(status, McpServerStatus::Connecting));

        let status = McpServerStatus::Connected;
        assert!(matches!(status, McpServerStatus::Connected));

        let status = McpServerStatus::Error("test".to_string());
        assert!(matches!(status, McpServerStatus::Error(_)));
    }

    #[test]
    fn test_mcp_service_server_status_empty() {
        let service = McpService::new();
        let status = service.server_status();
        assert!(status.is_empty());
    }

    #[test]
    fn test_mcp_service_server_tools_empty() {
        let service = McpService::new();
        let tools = service.server_tools();
        assert!(tools.is_empty());
    }

    #[test]
    fn test_mcp_service_list_tools_empty() {
        let service = McpService::new();
        let tools = service.list_tools();
        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn test_mcp_service_shutdown_all_empty() {
        let mut service = McpService::new();
        service.shutdown_all().await;
        assert!(service.server_status().is_empty());
    }

    #[tokio::test]
    async fn test_mcp_service_connect_from_config_unknown_type() {
        let mut service = McpService::new();
        let result = service
            .connect_from_config("test", "unknown_type", None, None, None, None, None, 5000)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mcp_service_connect_from_config_local_missing_command() {
        let mut service = McpService::new();
        let result = service
            .connect_from_config("test", "local", None, None, None, None, None, 5000)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mcp_service_connect_from_config_remote_missing_url() {
        let mut service = McpService::new();
        let result = service
            .connect_from_config("test", "remote", None, None, None, None, None, 5000)
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_mcp_service_default() {
        let service = McpService::default();
        let tools = service.list_tools();
        assert!(tools.is_empty());
    }
}
