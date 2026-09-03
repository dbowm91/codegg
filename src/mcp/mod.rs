#![allow(clippy::type_complexity)]

//! Model Context Protocol (MCP) implementation.
//!
//! This module provides MCP client functionality for connecting to external MCP servers.
//! MCP is a protocol for extending AI assistants with custom tools and resources.

pub mod auth;
pub mod cli;
pub mod ide_server;
pub mod local;
pub mod remote;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::McpError;
use crate::provider::ToolDefinition;
use auth::OAuthManager;
use local::LocalClient;
use remote::McpConnectionManager;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPrompt {
    pub name: String,
    pub description: Option<String>,
    pub arguments: Option<Vec<PromptArgument>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptArgument {
    pub name: String,
    pub description: Option<String>,
    pub required: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResourceContent {
    pub uri: String,
    pub mime_type: Option<String>,
    pub text: Option<String>,
    pub blob: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub server: String,
}

/// The lossless portion of an MCP `tools/call` response needed by
/// CodeGG-managed integrations. `text` remains the compatibility projection
/// used by existing callers; `structured` retains `content[type=json]` or
/// the protocol-level `structuredContent` value when a server provides it.
#[derive(Debug, Clone, Default)]
pub struct McpToolCallResult {
    pub text: String,
    pub structured: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default)]
pub enum McpServerStatus {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

pub struct McpServer {
    pub name: String,
    pub origin: McpServerOrigin,
    pub status: McpServerStatus,
    pub tools: Vec<McpTool>,
    pub server_version: Option<String>,
    pub client: McpClientType,
}

/// Ownership metadata for an MCP server. Plugin names are internal and
/// namespaced; configured servers remain authoritative and cannot be replaced
/// by a plugin declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpServerOrigin {
    Configured,
    Plugin {
        plugin_id: String,
        plugin_version: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpPluginReconcileReport {
    pub connected: Vec<String>,
    pub removed: Vec<String>,
    pub collisions: Vec<String>,
    pub failed: Vec<String>,
}

#[derive(Clone)]
pub enum McpClientType {
    Local(Arc<RwLock<LocalClient>>),
    Remote(Arc<RwLock<McpConnectionManager>>),
    /// In-process mock used by integration tests. Production code never
    /// constructs this variant; the `register_mock_server` helper
    /// (test-only) is the single entry point. The variant is left
    /// un-compiled-out so the helper can be reached from integration
    /// test binaries that don't share the library's `cfg(test)`
    /// configuration.
    Mock(
        Arc<
            std::sync::Mutex<
                Box<dyn Fn(&str, serde_json::Value) -> Result<String, McpError> + Send + Sync>,
            >,
        >,
    ),
}

pub struct McpService {
    servers: HashMap<String, McpServer>,
    oauth: OAuthManager,
}

/// Per-server policy for how raw MCP tools should be exposed to the
/// model.
///
/// `McpService::list_filtered_tools` and friends consult this when
/// building the model-facing tool catalog. The default
/// (raw hidden, native wrapper preferred) is the eggsearch pattern
/// from `src/search_backend/`; it is extended here so additional
/// Codegg-managed backends can opt in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpExposurePolicy {
    /// When `true`, raw `mcp__<server>__<tool>` definitions are
    /// included in the model-facing tool catalog.
    pub show_raw: bool,
    /// Servers whose raw tools should be hidden from the model even
    /// when `show_raw` is `true`. Used for Codegg-managed backends
    /// that have a native wrapper (`websearch`, `webfetch`).
    pub hidden_servers: Vec<String>,
}

impl McpExposurePolicy {
    /// Default policy: hide raw tools, with no hidden-server
    /// overrides. The model sees only the stable native wrappers.
    pub fn hide_all() -> Self {
        Self {
            show_raw: false,
            hidden_servers: Vec::new(),
        }
    }

    /// Whether a given fully-qualified MCP tool name (e.g.
    /// `mcp__eggsearch__web_search`) should be visible to the model
    /// under this policy.
    pub fn is_visible(&self, tool_name: &str) -> bool {
        if !self.show_raw {
            return false;
        }
        // Check if the tool belongs to a server explicitly hidden.
        if let Some(server) = parse_mcp_tool_server(tool_name) {
            if self.hidden_servers.iter().any(|s| s == &server) {
                return false;
            }
        }
        true
    }
}

/// Parse `mcp__<server>__<tool>` and return the server name.
///
/// Returns `None` if the input doesn't follow the MCP naming
/// convention.
pub fn parse_mcp_tool_server(tool_name: &str) -> Option<String> {
    let rest = tool_name.strip_prefix("mcp__")?;
    let (server, _tool) = rest.split_once("__")?;
    if server.is_empty() {
        return None;
    }
    Some(server.to_string())
}

impl McpService {
    pub fn new() -> Self {
        Self {
            servers: HashMap::new(),
            oauth: OAuthManager::new(),
        }
    }

    /// Create a workspace/turn-scoped view containing only configured
    /// servers. Client handles remain shared, while plugin-owned servers are
    /// intentionally excluded so one workspace's activation cannot leak into
    /// another workspace's model tool surface.
    pub fn clone_configured_servers(&self) -> Self {
        let servers = self
            .servers
            .iter()
            .filter(|(_, server)| matches!(server.origin, McpServerOrigin::Configured))
            .map(|(name, server)| {
                (
                    name.clone(),
                    McpServer {
                        name: server.name.clone(),
                        origin: McpServerOrigin::Configured,
                        status: server.status.clone(),
                        tools: server.tools.clone(),
                        server_version: server.server_version.clone(),
                        client: server.client.clone(),
                    },
                )
            })
            .collect();
        Self {
            servers,
            oauth: OAuthManager::new(),
        }
    }

    pub async fn connect_stdio(
        &mut self,
        name: &str,
        command: &str,
        args: &[String],
        env: HashMap<String, String>,
        timeout: u64,
    ) -> Result<(), McpError> {
        let key = name.to_string();
        if self.servers.contains_key(&key) {
            return Err(McpError::Server(format!("server {name} already exists")));
        }

        let mut client = LocalClient::new(command, args.to_vec(), env, timeout);
        client.initialize().await?;
        let server_version = client.server_version().map(str::to_owned);
        let tools = client.discover_tools().await?;
        let mcp_tools = tools
            .into_iter()
            .map(|t| McpTool {
                name: t.name.clone(),
                description: t.description.clone(),
                input_schema: t.parameters.clone(),
                server: name.to_string(),
            })
            .collect();

        let server = McpServer {
            name: name.to_string(),
            origin: McpServerOrigin::Configured,
            status: McpServerStatus::Connected,
            tools: mcp_tools,
            server_version,
            client: McpClientType::Local(Arc::new(RwLock::new(client))),
        };
        self.servers.insert(key, server);
        Ok(())
    }

    pub async fn connect_http(
        &mut self,
        name: &str,
        url: &str,
        headers: HashMap<String, String>,
        timeout: u64,
    ) -> Result<(), McpError> {
        if self.servers.contains_key(name) {
            return Err(McpError::Server(format!(
                "server {name} with URL {url} already exists"
            )));
        }

        let mut manager = McpConnectionManager::new(url, headers, timeout)?;

        if let Some(token) = self.oauth.get_token_for_server(url) {
            manager.set_oauth_token(token).await;
        }

        manager.connect().await?;
        let server_version = manager.server_version().map(str::to_owned);
        let tools = manager.discover_tools().await?;
        let mcp_tools = tools
            .into_iter()
            .map(|t| McpTool {
                name: t.name.clone(),
                description: t.description.clone(),
                input_schema: t.parameters.clone(),
                server: name.to_string(),
            })
            .collect();

        let server = McpServer {
            name: name.to_string(),
            origin: McpServerOrigin::Configured,
            status: McpServerStatus::Connected,
            tools: mcp_tools,
            server_version,
            client: McpClientType::Remote(Arc::new(RwLock::new(manager))),
        };
        self.servers.insert(name.to_string(), server);
        Ok(())
    }

    pub async fn disconnect(&mut self, name: &str) -> Result<(), McpError> {
        let server = self
            .servers
            .get_mut(name)
            .ok_or_else(|| McpError::Server(format!("server {name} not found")))?;

        match &server.client {
            McpClientType::Local(client) => {
                client.write().await.shutdown().await?;
            }
            McpClientType::Remote(client) => {
                client.write().await.disconnect().await?;
            }
            McpClientType::Mock(_) => {
                // Nothing to do; mock has no real connection.
            }
        }
        server.status = McpServerStatus::Disconnected;
        server.tools.clear();
        Ok(())
    }

    /// Reconcile only plugin-owned servers. Configured and other plugin
    /// origins are never removed or overwritten by this operation.
    pub async fn reconcile_plugin_servers(
        &mut self,
        desired: &[crate::plugin::ResolvedPluginMcpServer],
    ) -> McpPluginReconcileReport {
        let mut report = McpPluginReconcileReport::default();
        let desired_names: std::collections::HashSet<&str> = desired
            .iter()
            .map(|server| server.canonical_name.as_str())
            .collect();

        let stale: Vec<String> = self
            .servers
            .iter()
            .filter_map(|(name, server)| match &server.origin {
                McpServerOrigin::Plugin { .. } if !desired_names.contains(name.as_str()) => {
                    Some(name.clone())
                }
                _ => None,
            })
            .collect();
        for name in stale {
            match self.disconnect(&name).await {
                Ok(()) => {
                    self.servers.remove(&name);
                    report.removed.push(name);
                }
                Err(error) => report.failed.push(format!(
                    "failed to remove stale plugin MCP server '{name}': {error}"
                )),
            }
        }

        for server in desired {
            if let Some(existing) = self.servers.get(&server.canonical_name) {
                match &existing.origin {
                    McpServerOrigin::Configured => report.collisions.push(format!(
                        "plugin '{}' cannot replace configured MCP server '{}'",
                        server.plugin_id, server.canonical_name
                    )),
                    McpServerOrigin::Plugin { plugin_id, .. } if plugin_id != &server.plugin_id => {
                        report.collisions.push(format!(
                            "plugin '{}' cannot replace MCP server '{}' owned by '{}'",
                            server.plugin_id, server.canonical_name, plugin_id
                        ))
                    }
                    McpServerOrigin::Plugin { .. } => {}
                }
                continue;
            }

            let declaration = &server.declaration;
            let Some(server_type) = declaration.canonical_server_type() else {
                report.failed.push(format!(
                    "MCP server '{}' from plugin '{}' has unsupported type '{}'",
                    server.canonical_name, server.plugin_id, declaration.server_type
                ));
                continue;
            };
            let result = self
                .connect_from_config(
                    &server.canonical_name,
                    server_type,
                    declaration.command.as_deref(),
                    Some(&declaration.args),
                    Some(declaration.env.clone()),
                    declaration.url.as_deref(),
                    Some(declaration.headers.clone()),
                    declaration.timeout.unwrap_or(60_000),
                )
                .await;
            match result {
                Ok(()) => {
                    if let Some(registered) = self.servers.get_mut(&server.canonical_name) {
                        registered.origin = McpServerOrigin::Plugin {
                            plugin_id: server.plugin_id.clone(),
                            plugin_version: server.plugin_version.clone(),
                        };
                    }
                    report.connected.push(server.canonical_name.clone());
                }
                Err(error) => report.failed.push(format!(
                    "MCP server '{}' from plugin '{}': {error}",
                    server.canonical_name, server.plugin_id
                )),
            }
        }
        report
    }

    pub async fn call_tool(
        &self,
        server: &str,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<String, McpError> {
        Ok(self
            .call_tool_structured(server, tool, arguments)
            .await?
            .text)
    }

    /// Call an MCP tool while retaining structured content when the
    /// transport provides it. The legacy [`Self::call_tool`] API continues
    /// to return only the text projection.
    pub async fn call_tool_structured(
        &self,
        server: &str,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<McpToolCallResult, McpError> {
        let srv = self
            .servers
            .get(server)
            .ok_or_else(|| McpError::Server(format!("server {server} not found")))?;

        match &srv.client {
            McpClientType::Local(client) => {
                client
                    .write()
                    .await
                    .call_tool_structured(tool, arguments)
                    .await
            }
            McpClientType::Remote(client) => {
                client
                    .write()
                    .await
                    .call_tool_structured(tool, arguments)
                    .await
            }
            McpClientType::Mock(handler) => {
                let text = (handler
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()))(
                    tool, arguments
                )?;
                Ok(McpToolCallResult {
                    structured: serde_json::from_str(&text).ok(),
                    text,
                })
            }
        }
    }

    pub fn list_tools(&self) -> Vec<ToolDefinition> {
        self.list_filtered_tools(&McpExposurePolicy {
            show_raw: true,
            hidden_servers: Vec::new(),
        })
    }

    pub fn server_version(&self, server: &str) -> Option<String> {
        self.servers
            .get(server)
            .and_then(|s| s.server_version.clone())
    }

    pub fn server_origin(&self, server: &str) -> Option<McpServerOrigin> {
        self.servers.get(server).map(|server| server.origin.clone())
    }

    /// List MCP tool definitions, filtered through the given
    /// exposure policy.
    ///
    /// When `policy.show_raw` is `false`, no `mcp__*__*` tool
    /// definitions are returned. When `true`, definitions for
    /// servers in `policy.hidden_servers` are filtered out (so a
    /// native wrapper owns that surface and the model never sees
    /// the raw duplicate).
    pub fn list_filtered_tools(&self, policy: &McpExposurePolicy) -> Vec<ToolDefinition> {
        if !policy.show_raw {
            return Vec::new();
        }
        self.servers
            .values()
            .filter(|s| !policy.hidden_servers.iter().any(|h| h == &s.name))
            .flat_map(|s| {
                s.tools.iter().map(|t| ToolDefinition {
                    name: format!("mcp__{}__{}", s.name, t.name),
                    description: t.description.clone(),
                    parameters: t.input_schema.clone(),
                    defer_loading: None,
                })
            })
            .collect()
    }

    pub fn server_tools(&self) -> HashMap<&str, &Vec<McpTool>> {
        self.servers
            .iter()
            .map(|(name, server)| (name.as_str(), &server.tools))
            .collect()
    }

    pub fn server_status(&self) -> Vec<(&str, &McpServerStatus)> {
        self.servers
            .iter()
            .map(|(name, server)| (name.as_str(), &server.status))
            .collect()
    }

    pub async fn handle_tool_list_changed(
        &mut self,
        server: &str,
    ) -> Result<Vec<ToolDefinition>, McpError> {
        let mcp_tools = {
            let server_ref = self
                .servers
                .get_mut(server)
                .ok_or_else(|| McpError::Server(format!("server {server} not found")))?;

            let tools = match &server_ref.client {
                McpClientType::Local(client) => client.write().await.discover_tools().await?,
                McpClientType::Remote(client) => client.write().await.discover_tools().await?,
                McpClientType::Mock(_) => {
                    return Err(McpError::Server(
                        "handle_tool_list_changed is not supported on mock servers".into(),
                    ));
                }
            };

            tools
                .into_iter()
                .map(|t| McpTool {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    input_schema: t.parameters.clone(),
                    server: server.to_string(),
                })
                .collect::<Vec<_>>()
        };

        let srv = self
            .servers
            .get_mut(server)
            .ok_or_else(|| McpError::Server(format!("server {server} not found")))?;
        srv.tools = mcp_tools;

        Ok(srv
            .tools
            .iter()
            .map(|t| ToolDefinition {
                name: format!("mcp__{}__{}", srv.name, t.name),
                description: t.description.clone(),
                parameters: t.input_schema.clone(),
                defer_loading: None,
            })
            .collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn connect_from_config(
        &mut self,
        name: &str,
        server_type: &str,
        command: Option<&str>,
        args: Option<&[String]>,
        env: Option<HashMap<String, String>>,
        url: Option<&str>,
        headers: Option<HashMap<String, String>>,
        timeout: u64,
    ) -> Result<(), McpError> {
        match server_type {
            "local" => {
                let cmd = command
                    .ok_or_else(|| McpError::Server("local server requires command".into()))?;
                let args = args.unwrap_or(&[]);
                let env = env.unwrap_or_default();
                self.connect_stdio(name, cmd, args, env, timeout).await
            }
            "remote" => {
                let u = url.ok_or_else(|| McpError::Server("remote server requires url".into()))?;
                let h = headers.unwrap_or_default();
                self.connect_http(name, u, h, timeout).await
            }
            _ => Err(McpError::Server(format!(
                "unknown server type: {server_type}"
            ))),
        }
    }

    pub async fn shutdown_all(&mut self) {
        let names: Vec<String> = self.servers.keys().cloned().collect();
        for name in names {
            let _ = self.disconnect(&name).await;
        }
    }

    /// Test-only helper: register a server with pre-built tools and a
    /// mock call handler. Returns the registered server name.
    ///
    /// Production code never calls this method; it exists so that
    /// integration tests can wire up a fake MCP server without
    /// spawning a real subprocess.
    pub fn register_mock_server(
        &mut self,
        name: &str,
        tools: Vec<McpTool>,
        handler: Box<dyn Fn(&str, serde_json::Value) -> Result<String, McpError> + Send + Sync>,
    ) {
        self.register_mock_server_with_origin(name, McpServerOrigin::Configured, tools, handler);
    }

    /// Test-only helper for exercising origin-aware reconciliation without
    /// spawning a subprocess. Production plugin MCP connections use the
    /// normal stdio/http methods above.
    pub fn register_mock_plugin_server(
        &mut self,
        name: &str,
        plugin_id: &str,
        plugin_version: &str,
        tools: Vec<McpTool>,
        handler: Box<dyn Fn(&str, serde_json::Value) -> Result<String, McpError> + Send + Sync>,
    ) {
        self.register_mock_server_with_origin(
            name,
            McpServerOrigin::Plugin {
                plugin_id: plugin_id.to_string(),
                plugin_version: plugin_version.to_string(),
            },
            tools,
            handler,
        );
    }

    fn register_mock_server_with_origin(
        &mut self,
        name: &str,
        origin: McpServerOrigin,
        tools: Vec<McpTool>,
        handler: Box<dyn Fn(&str, serde_json::Value) -> Result<String, McpError> + Send + Sync>,
    ) {
        let server = McpServer {
            name: name.to_string(),
            origin,
            status: McpServerStatus::Connected,
            tools,
            server_version: None,
            client: McpClientType::Mock(Arc::new(std::sync::Mutex::new(handler))),
        };
        self.servers.insert(name.to_string(), server);
    }

    pub fn oauth_manager(&self) -> &OAuthManager {
        &self.oauth
    }

    pub fn oauth_manager_mut(&mut self) -> &mut OAuthManager {
        &mut self.oauth
    }

    pub async fn list_prompts(&self, server: &str) -> Result<Vec<McpPrompt>, McpError> {
        let srv = self
            .servers
            .get(server)
            .ok_or_else(|| McpError::Server(format!("server {server} not found")))?;

        match &srv.client {
            McpClientType::Local(client) => client.write().await.list_prompts().await,
            McpClientType::Remote(client) => client.write().await.list_prompts().await,
            McpClientType::Mock(_) => Ok(Vec::new()),
        }
    }

    pub async fn get_prompt(
        &self,
        server: &str,
        name: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<String, McpError> {
        let srv = self
            .servers
            .get(server)
            .ok_or_else(|| McpError::Server(format!("server {server} not found")))?;

        match &srv.client {
            McpClientType::Local(client) => client.write().await.get_prompt(name, arguments).await,
            McpClientType::Remote(client) => client.write().await.get_prompt(name, arguments).await,
            McpClientType::Mock(_) => Err(McpError::Server(
                "get_prompt is not supported on mock servers".into(),
            )),
        }
    }

    pub async fn list_resources(&self, server: &str) -> Result<Vec<McpResource>, McpError> {
        let srv = self
            .servers
            .get(server)
            .ok_or_else(|| McpError::Server(format!("server {server} not found")))?;

        match &srv.client {
            McpClientType::Local(client) => client.write().await.list_resources().await,
            McpClientType::Remote(client) => client.write().await.list_resources().await,
            McpClientType::Mock(_) => Ok(Vec::new()),
        }
    }

    pub async fn read_resource(
        &self,
        server: &str,
        uri: &str,
    ) -> Result<McpResourceContent, McpError> {
        let srv = self
            .servers
            .get(server)
            .ok_or_else(|| McpError::Server(format!("server {server} not found")))?;

        match &srv.client {
            McpClientType::Local(client) => client.write().await.read_resource(uri).await,
            McpClientType::Remote(client) => client.write().await.read_resource(uri).await,
            McpClientType::Mock(_) => Err(McpError::Server(
                "read_resource is not supported on mock servers".into(),
            )),
        }
    }
}

impl Default for McpService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::McpError;

    fn mock_handler(_tool: &str, _args: serde_json::Value) -> Result<String, McpError> {
        Ok("ok".to_string())
    }

    fn build_service() -> McpService {
        let mut svc = McpService::new();
        svc.register_mock_server(
            "eggsearch",
            vec![
                McpTool {
                    name: "web_search".to_string(),
                    description: "raw web search".to_string(),
                    input_schema: serde_json::json!({}),
                    server: "eggsearch".to_string(),
                },
                McpTool {
                    name: "web_fetch".to_string(),
                    description: "raw web fetch".to_string(),
                    input_schema: serde_json::json!({}),
                    server: "eggsearch".to_string(),
                },
            ],
            Box::new(mock_handler),
        );
        svc.register_mock_server(
            "github",
            vec![McpTool {
                name: "list_issues".to_string(),
                description: "list issues".to_string(),
                input_schema: serde_json::json!({}),
                server: "github".to_string(),
            }],
            Box::new(mock_handler),
        );
        svc
    }

    #[test]
    fn list_tools_shows_everything_by_default() {
        let svc = build_service();
        let tools = svc.list_tools();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"mcp__eggsearch__web_search"));
        assert!(names.contains(&"mcp__eggsearch__web_fetch"));
        assert!(names.contains(&"mcp__github__list_issues"));
    }

    #[test]
    fn list_filtered_tools_hides_raw_by_default() {
        let svc = build_service();
        let policy = McpExposurePolicy::hide_all();
        let tools = svc.list_filtered_tools(&policy);
        assert!(tools.is_empty(), "hide_all should hide all raw tools");
    }

    #[test]
    fn list_filtered_tools_hides_managed_servers() {
        let svc = build_service();
        let policy = McpExposurePolicy {
            show_raw: true,
            hidden_servers: vec!["eggsearch".to_string()],
        };
        let tools = svc.list_filtered_tools(&policy);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(!names.contains(&"mcp__eggsearch__web_search"));
        assert!(!names.contains(&"mcp__eggsearch__web_fetch"));
        assert!(names.contains(&"mcp__github__list_issues"));
    }

    #[test]
    fn list_filtered_tools_shows_all_when_unrestricted() {
        let svc = build_service();
        let policy = McpExposurePolicy {
            show_raw: true,
            hidden_servers: Vec::new(),
        };
        let tools = svc.list_filtered_tools(&policy);
        assert_eq!(tools.len(), 3);
    }

    #[test]
    fn parse_mcp_tool_server_extracts_server() {
        assert_eq!(
            parse_mcp_tool_server("mcp__eggsearch__web_search"),
            Some("eggsearch".to_string())
        );
        assert_eq!(
            parse_mcp_tool_server("mcp__github__list_issues"),
            Some("github".to_string())
        );
        assert_eq!(parse_mcp_tool_server("websearch"), None);
        assert_eq!(parse_mcp_tool_server("mcp____tool"), None);
    }

    #[test]
    fn exposure_policy_is_visible() {
        let policy = McpExposurePolicy::hide_all();
        assert!(!policy.is_visible("mcp__eggsearch__web_search"));

        let policy = McpExposurePolicy {
            show_raw: true,
            hidden_servers: vec!["eggsearch".to_string()],
        };
        assert!(!policy.is_visible("mcp__eggsearch__web_search"));
        assert!(policy.is_visible("mcp__github__list_issues"));

        let policy = McpExposurePolicy {
            show_raw: true,
            hidden_servers: Vec::new(),
        };
        assert!(policy.is_visible("mcp__eggsearch__web_search"));
    }
}
