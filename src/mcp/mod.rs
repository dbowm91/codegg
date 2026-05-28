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
    pub status: McpServerStatus,
    pub tools: Vec<McpTool>,
    pub client: McpClientType,
}

#[derive(Clone)]
pub enum McpClientType {
    Local(Arc<RwLock<LocalClient>>),
    Remote(Arc<RwLock<McpConnectionManager>>),
}

pub struct McpService {
    servers: HashMap<String, McpServer>,
    oauth: OAuthManager,
}

impl McpService {
    pub fn new() -> Self {
        Self {
            servers: HashMap::new(),
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
            status: McpServerStatus::Connected,
            tools: mcp_tools,
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
            status: McpServerStatus::Connected,
            tools: mcp_tools,
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
        }
        server.status = McpServerStatus::Disconnected;
        server.tools.clear();
        Ok(())
    }

    pub async fn call_tool(
        &self,
        server: &str,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<String, McpError> {
        let srv = self
            .servers
            .get(server)
            .ok_or_else(|| McpError::Server(format!("server {server} not found")))?;

        match &srv.client {
            McpClientType::Local(client) => client.write().await.call_tool(tool, arguments).await,
            McpClientType::Remote(client) => client.write().await.call_tool(tool, arguments).await,
        }
    }

    pub fn list_tools(&self) -> Vec<ToolDefinition> {
        self.servers
            .values()
            .flat_map(|s| {
                s.tools.iter().map(|t| ToolDefinition {
                    name: format!("mcp__{}__{}", s.name, t.name),
                    description: t.description.clone(),
                    parameters: t.input_schema.clone(),
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
        }
    }
}

impl Default for McpService {
    fn default() -> Self {
        Self::new()
    }
}
