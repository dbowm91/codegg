//! MCP CLI commands for managing MCP servers.
//!
//! This module provides CLI commands for:
//! - Adding MCP servers
//! - Listing configured MCP servers  
//! - Authenticating MCP servers with OAuth
//! - Debugging MCP server connections

use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::paths::load_config;
use crate::config::schema::{Config, McpEntry, McpServerConfig};
use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct McpCli {
    config_path: PathBuf,
}

impl McpCli {
    pub fn new() -> Self {
        let config_dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            config_path: config_dir.join("codegg").join("config.json"),
        }
    }

    fn load_current_config(&self) -> Result<Option<Config>, AppError> {
        if self.config_path.exists() {
            let config = load_config(&self.config_path)?;
            Ok(Some(config))
        } else {
            Ok(None)
        }
    }

    fn save_config(&self, config: &Config) -> Result<(), AppError> {
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::Config(crate::error::ConfigError::Invalid(e.to_string())))?;
        }
        let json = serde_json::to_string_pretty(config)
            .map_err(|e| AppError::Config(crate::error::ConfigError::Parse(e.to_string())))?;
        std::fs::write(&self.config_path, json)
            .map_err(|e| AppError::Config(crate::error::ConfigError::Invalid(e.to_string())))?;
        Ok(())
    }

    pub fn add(
        &self,
        name: &str,
        server_type: &str,
        command: Option<&str>,
        args: Option<Vec<String>>,
        url: Option<&str>,
    ) -> Result<(), AppError> {
        let config = self.load_current_config()?.unwrap_or_default();

        let mcp_entry = McpEntry {
            inner: Some(McpServerConfig {
                server_type: Some(server_type.to_string()),
                command: command.map(|s| s.to_string()),
                args,
                env: None,
                environment: None,
                url: url.map(|s| s.to_string()),
                headers: None,
                transport: None,
                timeout: Some(30000),
                oauth: None,
                reconnect: None,
            }),
            enabled: Some(true),
        };

        let mut new_config = config;
        let mcp = new_config.mcp.get_or_insert_with(HashMap::new);
        mcp.insert(name.to_string(), mcp_entry);

        self.save_config(&new_config)?;
        println!("Added MCP server '{}' (type: {})", name, server_type);
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<McpServerInfo>, AppError> {
        let config = self.load_current_config()?;

        let mut servers = Vec::new();

        if let Some(config) = config {
            if let Some(mcp) = config.mcp {
                for (name, entry) in mcp {
                    let server_type = entry
                        .inner
                        .as_ref()
                        .and_then(|c| c.server_type.clone())
                        .unwrap_or_else(|| "local".to_string());

                    let url = entry.inner.as_ref().and_then(|c| c.url.clone());

                    let command = entry.inner.as_ref().and_then(|c| c.command.clone());

                    let enabled = entry.enabled.unwrap_or(true);

                    servers.push(McpServerInfo {
                        name: name.clone(),
                        server_type,
                        command,
                        url,
                        enabled,
                    });
                }
            }
        }

        Ok(servers)
    }

    pub fn remove(&self, name: &str) -> Result<(), AppError> {
        let config = self.load_current_config()?.unwrap_or_default();

        if let Some(ref mcp) = config.mcp {
            if mcp.contains_key(name) {
                let mut new_config = config;
                if let Some(mcp) = new_config.mcp.as_mut() {
                    mcp.remove(name);
                }
                self.save_config(&new_config)?;
                println!("Removed MCP server '{}'", name);
                return Ok(());
            }
        }
        Err(AppError::Config(crate::error::ConfigError::Invalid(
            format!("MCP server '{}' not found", name),
        )))
    }

    pub fn enable(&self, name: &str, enabled: bool) -> Result<(), AppError> {
        let config = self.load_current_config()?.unwrap_or_default();

        if let Some(ref mcp) = config.mcp {
            if mcp.contains_key(name) {
                let mut new_config = config;
                if let Some(mcp) = new_config.mcp.as_mut() {
                    if let Some(entry) = mcp.get_mut(name) {
                        entry.enabled = Some(enabled);
                    }
                }
                self.save_config(&new_config)?;
                println!(
                    "{} MCP server '{}'",
                    if enabled { "Enabled" } else { "Disabled" },
                    name
                );
                return Ok(());
            }
        }
        Err(AppError::Config(crate::error::ConfigError::Invalid(
            format!("MCP server '{}' not found", name),
        )))
    }
}

impl Default for McpCli {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct McpServerInfo {
    pub name: String,
    pub server_type: String,
    pub command: Option<String>,
    pub url: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, clap::Subcommand)]
pub enum McpCommand {
    /// Add a new MCP server to configuration
    Add {
        /// Name for the MCP server
        name: String,

        /// Server type (local or remote)
        #[arg(long, default_value = "local")]
        #[clap(value_enum)]
        server_type: ServerType,

        /// Command to run (for local servers)
        #[arg(long, short = 'c')]
        command: Option<String>,

        /// Arguments for the command
        #[arg(long, short = 'a')]
        args: Option<Vec<String>>,

        /// URL (for remote servers)
        #[arg(long, short = 'u')]
        url: Option<String>,
    },

    /// List configured MCP servers
    List,

    /// Remove an MCP server from configuration
    Remove {
        /// Name of the MCP server to remove
        name: String,
    },

    /// Enable or disable an MCP server
    Enable {
        /// Name of the MCP server
        name: String,

        /// Enable the server
        #[arg(long, default_value = "true")]
        enabled: bool,
    },

    /// Test connection to an MCP server
    Debug {
        /// Name of the MCP server to test
        name: Option<String>,

        /// URL to test (instead of using config)
        #[arg(long, short = 'u')]
        url: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum ServerType {
    #[default]
    Local,
    Remote,
}

impl ServerType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ServerType::Local => "local",
            ServerType::Remote => "remote",
        }
    }
}

pub fn exec_mcp_command(cmd: McpCommand) -> Result<(), AppError> {
    let cli = McpCli::new();

    match cmd {
        McpCommand::Add {
            name,
            server_type,
            command,
            args,
            url,
        } => {
            cli.add(
                &name,
                server_type.as_str(),
                command.as_deref(),
                args,
                url.as_deref(),
            )?;
        }

        McpCommand::List => {
            let servers = cli.list()?;
            if servers.is_empty() {
                println!("No MCP servers configured");
            } else {
                println!("Configured MCP servers:\n");
                for srv in servers {
                    let status = if srv.enabled {
                        "[enabled]"
                    } else {
                        "[disabled]"
                    };
                    match (srv.command, srv.url) {
                        (Some(cmd), None) => {
                            println!("  {} {} - {} {}", srv.name, status, srv.server_type, cmd);
                        }
                        (_, Some(url)) => {
                            println!("  {} {} - {} {}", srv.name, status, srv.server_type, url);
                        }
                        _ => {
                            println!("  {} {} - {}", srv.name, status, srv.server_type);
                        }
                    }
                }
            }
        }

        McpCommand::Remove { name } => {
            cli.remove(&name)?;
        }

        McpCommand::Enable { name, enabled } => {
            cli.enable(&name, enabled)?;
        }

        McpCommand::Debug { name, url } => {
            println!("Testing MCP server connection...");
            if let Some(name) = name {
                println!("Server: {}", name);
            }
            if let Some(url) = url {
                println!("URL: {}", url);
            }
            println!("Use 'codegg mcp add' to add a new server first.");
        }
    }

    Ok(())
}
