use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use codegg::agent;
use codegg::auth::AuthCli;
use codegg::config::paths;
use codegg::config::schema::Config;
use codegg::core::CoreClient;
use codegg::error::{AppError, ConfigError, StorageError};
use codegg::exec::{ExecInput, ExecMode};
use codegg::mcp;
use codegg::memory::MemoryStore;
use codegg::protocol::core::{CoreRequest, CoreResponse, RequestEnvelope};
use codegg::provider::{self, ProviderRegistry};
use codegg::session::{MessageStore, Session, SessionStore};
use codegg::skills::SkillIndex;
use codegg::storage;
use codegg::tui;
use codegg::upgrade;
use serde::{Deserialize, Serialize};
use std::env;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing_subscriber::util::SubscriberInitExt;

#[derive(Parser, Clone, Debug)]
#[command(
    name = "codegg",
    version,
    about = "A lightweight, pure-Rust implementation of Codegg",
    after_help = r#"EXAMPLES:
    # Start a new session
    codegg

    # Resume last session
    codegg -c

    # Use specific model
    codegg -m claude-sonnet-4-20250514

    # Run a single prompt
    codegg --run "Hello, write a hello world program"

    # List available models
    codegg models

    # List models from specific provider
    codegg models -p anthropic

    # Attach to remote server
    codegg attach http://localhost:3000 --token YOUR_TOKEN

    # Start server
    codegg server --host 0.0.0.0 --port 8080"#
)]
struct Cli {
    /// Resume last session
    #[arg(long, short = 'c')]
    continue_session: bool,

    /// Open specific session
    #[arg(long, short = 's')]
    session: Option<String>,

    /// Override model
    #[arg(long, short = 'm')]
    model: Option<String>,

    /// Override agent
    #[arg(long, short = 'a')]
    agent: Option<String>,

    /// Ephemeral session (no persistence)
    #[arg(long)]
    no_session: bool,

    /// Fork a session
    #[arg(long)]
    fork: Option<String>,

    /// Run a single prompt and exit
    #[arg(long, short = 'p')]
    run: Option<String>,

    /// Output format for non-interactive mode (text, json)
    #[arg(long, short = 'f', default_value = "text")]
    output_format: OutputFormat,

    /// Hide status messages in non-interactive mode
    #[arg(long, short = 'q')]
    quiet: bool,

    /// Set current working directory
    #[arg(long = "cwd", value_name = "DIRECTORY")]
    cwd: Option<PathBuf>,

    /// Enable verbose output (-v for warning, -vv for info, -vvv for debug)
    #[arg(long, short = 'v', action = clap::ArgAction::Count)]
    verbose: u8,

    /// Core transport mode for TUI: inproc or stdio
    #[arg(long = "core-transport", value_enum)]
    core_transport: Option<CoreTransport>,

    /// Core transport endpoint (required for socket mode), e.g. unix:///tmp/codegg-core.sock
    #[arg(long = "core-endpoint")]
    core_endpoint: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Clone, Debug)]
enum Commands {
    /// List available providers
    Providers,
    /// List available models
    Models {
        /// Filter by provider
        #[arg(long, short = 'p')]
        provider: Option<String>,
    },
    /// List sessions
    Sessions {
        /// Show archived sessions
        #[arg(long)]
        archived: bool,
    },
    /// View a specific session
    Session {
        /// Session ID
        id: String,
    },
    /// Export a session to JSON
    Export {
        /// Session ID
        id: String,
        /// Output file path
        #[arg(long, short = 'o')]
        output: Option<String>,
    },
    /// Import a session from JSON
    Import {
        /// Input file path
        file: String,
    },
    /// Upgrade codegg to latest version
    Upgrade,
    /// Start the HTTP server
    #[cfg(feature = "server")]
    Server {
        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port to bind to
        #[arg(long, short = 'p')]
        port: Option<u16>,
    },
    /// Attach to a remote codegg server
    #[cfg(feature = "server")]
    Attach {
        /// Server URL (e.g. http://localhost:3000)
        url: String,
        /// Authentication token
        #[arg(long)]
        token: Option<String>,
    },
    /// Validate configuration files
    Validate {
        /// Path to config file (default: auto-detect)
        #[arg(long)]
        config: Option<String>,
    },
    /// Execute a prompt in non-interactive mode (CI/CD)
    Exec {
        /// JSON input with prompt, model, and agent
        #[arg(long)]
        json: Option<String>,

        /// Input file (reads from stdin if not specified)
        #[arg(long)]
        file: Option<String>,

        /// Output JSON format
        #[arg(long, short = 'j')]
        json_output: bool,

        /// Suppress log messages
        #[arg(long, short = 'q')]
        quiet: bool,

        /// Resume existing session
        #[arg(long, short = 's')]
        session: Option<String>,
    },
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for (bash, zsh, fish, powershell)
        #[arg(value_enum)]
        shell: clap_complete::Shell,
        /// Output directory (default: current directory)
        #[arg(long, short = 'o')]
        output: Option<String>,
    },
    /// Manage MCP servers (add, list, remove, enable, debug)
    Mcp {
        #[command(subcommand)]
        command: mcp::cli::McpCommand,
    },
    /// Run a deep research query
    Research {
        /// The research question
        question: String,
        /// Research mode
        #[arg(long, default_value = "narrow-answer")]
        mode: String,
        /// Target audience
        #[arg(long, default_value = "human")]
        audience: String,
        /// Research depth
        #[arg(long, default_value = "medium")]
        depth: String,
        /// Output profiles (can be specified multiple times)
        #[arg(long = "output")]
        outputs: Vec<String>,
        /// Sources to include (format: local, file:path, url:https://...)
        #[arg(long = "source")]
        sources: Vec<String>,
        /// Allow network fetching
        #[arg(long)]
        allow_network: bool,
    },
    /// Manage the core daemon
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
    /// Attach to a running daemon via local Unix socket
    AttachDaemon {
        /// Socket endpoint path
        #[arg(long)]
        endpoint: Option<String>,
        /// Session ID to attach to
        #[arg(long)]
        session: Option<String>,
        /// Create a new session
        #[arg(long)]
        new: bool,
    },
    #[command(hide = true, name = "core-stdio")]
    CoreStdio,
    /// Run diagnostics for search backend, MCP, providers, and storage.
    Doctor {
        /// Restrict diagnostics to a single subsystem.
        #[arg(long, value_enum)]
        subsystem: Option<DoctorSubsystem>,
    },
    /// Manage the user-level credential store (status, set-key, logout).
    Auth {
        #[command(subcommand)]
        command: AuthSubcommand,
    },
}

#[derive(Subcommand, Clone, Debug)]
enum AuthSubcommand {
    /// List stored credentials (no plaintext, no ciphertext, no fingerprint).
    Status,
    /// Store an API key for a provider. Reads the key from stdin.
    ///
    /// Example:
    ///   printf '%s' "$OPENAI_API_KEY" | codegg auth set-key openai
    ///   printf '%s' "$OPENAI_API_KEY" | codegg auth set-key openai --account work
    SetKey {
        /// Provider id (e.g. openai, anthropic, xai). Must contain only
        /// `[A-Za-z0-9_-]`.
        provider: String,
        /// Optional account id for multi-account stores. Must contain
        /// only `[A-Za-z0-9_-]`.
        #[arg(long)]
        account: Option<String>,
    },
    /// Remove stored credentials for a provider.
    ///
    /// Pass `--account '*'` to remove every account for the provider.
    Logout {
        /// Provider id (e.g. openai, anthropic, xai). Must contain only
        /// `[A-Za-z0-9_-]`.
        provider: String,
        /// Optional account id. Use `'*'` to remove all accounts. Must
        /// otherwise contain only `[A-Za-z0-9_-]`.
        #[arg(long)]
        account: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum DoctorSubsystem {
    /// All subsystems.
    All,
    /// Web search/fetch backend (eggsearch).
    Search,
    /// Configured MCP servers.
    Mcp,
    /// Language Server Protocol subsystem.
    Lsp,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
enum CoreTransport {
    Inproc,
    Stdio,
    Socket,
}

#[derive(Subcommand, Clone, Debug)]
enum DaemonCommand {
    /// Start the core daemon
    Start {
        /// Socket endpoint path
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Stop the running daemon
    Stop {
        /// Socket endpoint path
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Show daemon status
    Status {
        /// Socket endpoint path
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Show daemon logs
    Logs {
        /// Log file path (default: auto-detect)
        #[arg(long)]
        file: Option<String>,
        /// Number of lines to show
        #[arg(long, default_value = "50")]
        lines: usize,
    },
}

fn default_socket_path() -> String {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/Library/Application Support/codegg/core.sock", home)
    }
    #[cfg(target_os = "linux")]
    {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/codegg/core.sock", runtime_dir)
    }
    #[cfg(not(target_os = "macos"))]
    #[cfg(not(target_os = "linux"))]
    {
        "/tmp/codegg-core.sock".to_string()
    }
}

fn default_log_path() -> std::path::PathBuf {
    std::path::PathBuf::from("codegg_debug.log")
}

fn resolve_endpoint(endpoint: Option<String>) -> String {
    endpoint
        .or_else(|| std::env::var("CODEGG_CORE_ENDPOINT").ok())
        .unwrap_or_else(default_socket_path)
}

impl OutputFormat {
    pub fn format(&self, content: &str) -> String {
        match self {
            OutputFormat::Text => content.to_string(),
            OutputFormat::Json => {
                let escaped = content
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "\\n")
                    .replace('\r', "\\r")
                    .replace('\t', "\\t");
                format!(r#"{{"response": "{}"}}"#, escaped)
            }
        }
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), AppError> {
    let cli = Cli::parse();

    let log_level = match cli.verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };

    let env_log_level = std::env::var("RUST_LOG").ok();

    let use_debug_file = env_log_level.is_some() || cli.verbose >= 2;

    if use_debug_file {
        if let Ok(log_file) = std::fs::File::create("codegg_debug.log") {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
                )
                .with_writer(std::sync::Mutex::new(log_file))
                .init();
        } else {
            // Fallback to no-op subscriber if we can't create the file, to avoid stdout/stderr flood in TUI
            tracing_subscriber::registry().init();
        }
    } else if cli.command.is_none() {
        // Silent by default in TUI mode if no verbose flag
        tracing_subscriber::registry().init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
            )
            .init();
    }

    if let Some(command) = &cli.command {
        match command {
            Commands::Providers => cmd_providers().await?,
            Commands::Models { provider } => cmd_models(provider.clone()).await?,
            Commands::Sessions { archived } => cmd_sessions(*archived).await?,
            Commands::Session { id } => cmd_session_view(id).await?,
            Commands::Export { id, output } => cmd_export(id, output.as_deref()).await?,
            Commands::Import { file } => cmd_import(file).await?,
            Commands::Upgrade => cmd_upgrade().await?,
            #[cfg(feature = "server")]
            Commands::Server { host, port } => {
                let config = Config::load().unwrap_or_default();
                let port = port
                    .or_else(|| config.server.as_ref().and_then(|s| s.port))
                    .unwrap_or(3000);
                cmd_server(host, port).await?;
            }
            #[cfg(feature = "server")]
            Commands::Attach { url, token } => {
                cmd_attach(url, token.as_deref()).await?;
            }
            Commands::Doctor { subsystem } => {
                cmd_doctor(subsystem.unwrap_or(DoctorSubsystem::All)).await?;
            }
            Commands::Auth { command } => {
                cmd_auth(command.clone())?;
            }
            Commands::Validate { config } => {
                cmd_validate(config.as_deref()).await?;
            }
            Commands::Exec {
                json,
                file,
                json_output,
                quiet,
                session,
            } => {
                cmd_exec(
                    json.as_deref(),
                    file.as_deref(),
                    *json_output,
                    *quiet,
                    session.as_deref(),
                )
                .await?;
            }
            Commands::Completions { shell, output } => {
                cmd_completions(*shell, output.as_deref())?;
            }
            Commands::Mcp { command } => {
                mcp::cli::exec_mcp_command(command.clone())?;
            }
            Commands::Research {
                question,
                mode,
                audience,
                depth,
                outputs,
                sources,
                allow_network,
            } => {
                cmd_research(
                    question,
                    mode,
                    audience,
                    depth,
                    outputs,
                    sources,
                    *allow_network,
                )
                .await?;
            }
            Commands::Daemon { command } => match command {
                DaemonCommand::Start { endpoint } => {
                    run_daemon(endpoint.clone()).await;
                }
                DaemonCommand::Stop { endpoint } => {
                    let ep = resolve_endpoint(endpoint.clone());
                    let pid_file = std::path::Path::new(&ep).with_extension("pid");
                    match tokio::fs::read_to_string(&pid_file).await {
                        Ok(pid_str) => {
                            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                                // Check if process is alive before sending signal
                                let kill_result = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
                                if kill_result == 0 {
                                    println!("Sent SIGTERM to daemon (PID {})", pid);
                                    let _ = tokio::fs::remove_file(&pid_file).await;
                                } else {
                                    eprintln!(
                                        "Daemon process {} not found (may have already exited)",
                                        pid
                                    );
                                    let _ = tokio::fs::remove_file(&pid_file).await;
                                }
                            } else {
                                eprintln!("Invalid PID in {}", pid_file.display());
                            }
                        }
                        Err(_) => {
                            eprintln!("No daemon PID file found at {}", pid_file.display());
                            eprintln!("Is the daemon running?");
                        }
                    }
                }
                DaemonCommand::Status { endpoint } => {
                    let ep = resolve_endpoint(endpoint.clone());
                    let pid_file = std::path::Path::new(&ep).with_extension("pid");
                    match codegg::core::transport::SocketCoreClient::connect(&format!(
                        "unix://{}",
                        ep
                    ))
                    .await
                    {
                        Ok(client) => {
                            let req = codegg::core::new_request(
                                "status-1".into(),
                                CoreRequest::SnapshotDaemon,
                            );
                            match client.request(req).await {
                                Ok(CoreResponse::SnapshotDaemon {
                                    daemon_id,
                                    uptime_secs,
                                    active_sessions,
                                    connected_clients,
                                    ..
                                }) => {
                                    println!("Daemon is running");
                                    println!("  Daemon ID: {}", daemon_id);
                                    println!("  Uptime: {}s", uptime_secs);
                                    println!("  Active sessions: {}", active_sessions.len());
                                    println!("  Connected clients: {}", connected_clients.len());
                                    for s in &active_sessions {
                                        println!(
                                            "    Session: {} ({})",
                                            &s.session_id[..8.min(s.session_id.len())],
                                            s.status
                                        );
                                    }
                                }
                                Ok(other) => {
                                    println!(
                                        "Daemon is running (unexpected response: {:?})",
                                        other
                                    );
                                }
                                Err(e) => {
                                    eprintln!("Daemon connected but failed to respond: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Daemon is not running or unreachable: {}", e);
                            if let Ok(pid) = tokio::fs::read_to_string(&pid_file).await {
                                eprintln!("Stale PID file found: {}", pid.trim());
                            }
                        }
                    }
                }
                DaemonCommand::Logs { file, lines } => {
                    let log_path = file
                        .clone()
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(default_log_path);
                    match tokio::fs::read_to_string(&log_path).await {
                        Ok(content) => {
                            let all_lines: Vec<&str> = content.lines().collect();
                            let start = all_lines.len().saturating_sub(*lines);
                            for line in &all_lines[start..] {
                                println!("{}", line);
                            }
                        }
                        Err(e) => {
                            eprintln!("Could not read log file {}: {}", log_path.display(), e);
                            eprintln!("Daemon logs are written to stderr or a debug log file.");
                            eprintln!(
                                "Start the daemon with RUST_LOG=info or -vv to enable logging."
                            );
                            std::process::exit(1);
                        }
                    }
                }
            },
            Commands::AttachDaemon {
                endpoint,
                session,
                new,
            } => {
                let ep = resolve_endpoint(endpoint.clone());
                let mut cli_copy = cli.clone();
                cli_copy.core_transport = Some(CoreTransport::Socket);
                cli_copy.core_endpoint = Some(format!("unix://{}", ep));
                if *new {
                    cli_copy.continue_session = false;
                    cli_copy.session = None;
                } else if let Some(sid) = session {
                    cli_copy.session = Some(sid.clone());
                }
                launch_tui(&cli_copy).await?;
            }
            Commands::CoreStdio => {
                run_core_stdio().await?;
            }
        }
        return Ok(());
    }

    if let Some(prompt) = &cli.run {
        run_single_shot(prompt, &cli).await?;
        return Ok(());
    }

    launch_tui(&cli).await?;

    Ok(())
}

async fn cmd_providers() -> Result<(), AppError> {
    // Load config so the same config-aware path used by `cmd_models` and
    // the TUI (`register_builtin_with_config`) takes effect here. This
    // ensures `codegg providers` and `codegg models` agree on which
    // providers are visible, including those sourced from the user
    // credential store.
    let config = Config::load().unwrap_or_default();
    let mut registry = ProviderRegistry::new();
    provider::register_builtin_with_config(&mut registry, &config);

    if registry.list().is_empty() {
        println!(
            "No providers configured. Set API keys, configure provider auth, or store a key with `codegg auth set-key <provider>`."
        );
        return Ok(());
    }

    println!("Available providers:\n");
    for p in registry.list() {
        println!("  {} - {}", p.id(), p.name());
    }

    Ok(())
}

async fn cmd_models(provider_filter: Option<String>) -> Result<(), AppError> {
    let config = Config::load().unwrap_or_default();
    let mut registry = ProviderRegistry::new();
    provider::register_builtin_with_config(&mut registry, &config);

    let providers = registry.list();
    let providers = match &provider_filter {
        Some(f) => providers.into_iter().filter(|p| p.id() == f).collect(),
        None => providers,
    };

    if providers.is_empty() {
        if let Some(f) = &provider_filter {
            println!("Provider not found: {}", f);
            return Ok(());
        }
        println!(
            "No providers configured. Set API keys, configure provider auth, or store a key with `codegg auth set-key <provider>`."
        );
        return Ok(());
    }

    for p in providers {
        println!("{}:", p.name());
        match p.models().await {
            Ok(models) => {
                for m in models {
                    println!("  {} ({})", m.id, m.name);
                }
            }
            Err(e) => {
                println!("  Error: {}", e);
            }
        }
    }

    Ok(())
}

async fn cmd_sessions(archived: bool) -> Result<(), AppError> {
    let project_dir = env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let pool = storage::init(&project_dir).await?;
    let store = SessionStore::new(pool);

    let sessions = if archived {
        store.list_all(&project_dir, None).await?
    } else {
        store.list(&project_dir, 50).await?
    };

    if sessions.is_empty() {
        println!("No sessions found.");
        return Ok(());
    }

    for s in sessions {
        let status = if s.time_archived.is_some() {
            "archived"
        } else if s.parent_id.is_some() {
            "fork"
        } else {
            "active"
        };
        println!(
            "{}  {}  {}  {}",
            &s.id[..8],
            s.title,
            status,
            chrono::DateTime::from_timestamp_millis(s.time_updated)
                .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default()
        );
    }

    Ok(())
}

async fn cmd_session_view(id: &str) -> Result<(), AppError> {
    let project_dir = env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let pool = storage::init(&project_dir).await?;
    let store = SessionStore::new(pool);

    let session = store
        .get(id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound(format!("session {}", id))))?;

    println!("Session: {}", session.title);
    println!("ID: {}", session.id);
    println!("Project: {}", session.project_id);
    println!("Directory: {}", session.directory);
    println!(
        "Status: {}",
        if session.time_archived.is_some() {
            "archived"
        } else {
            "active"
        }
    );
    if let Some(parent) = &session.parent_id {
        println!("Parent: {}", parent);
    }
    if let Some(url) = &session.share_url {
        println!("Share URL: {}", url);
    }
    println!(
        "Created: {}",
        chrono::DateTime::from_timestamp_millis(session.time_created)
            .map(|d| d.to_rfc2822())
            .unwrap_or_default()
    );
    println!(
        "Updated: {}",
        chrono::DateTime::from_timestamp_millis(session.time_updated)
            .map(|d| d.to_rfc2822())
            .unwrap_or_default()
    );

    let children = store.children(id).await?;
    if !children.is_empty() {
        println!("\nForks ({}):", children.len());
        for c in children {
            println!("  {} - {}", &c.id[..8], c.title);
        }
    }

    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionExport {
    session: Session,
    messages: Vec<serde_json::Value>,
    version: String,
}

async fn cmd_export(id: &str, output: Option<&str>) -> Result<(), AppError> {
    let project_dir = env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let pool = storage::init(&project_dir).await?;
    let session_store = SessionStore::new(pool.clone());
    let message_store = codegg::session::MessageStore::new(pool);

    let session = session_store
        .get(id)
        .await?
        .ok_or_else(|| AppError::Storage(StorageError::NotFound(format!("session {}", id))))?;

    let messages = message_store
        .list(id)
        .await?
        .into_iter()
        .map(|m| serde_json::to_value(m).unwrap_or(serde_json::Value::Null))
        .collect();

    let export = SessionExport {
        session,
        messages,
        version: "1".to_string(),
    };

    let json = serde_json::to_string_pretty(&export).map_err(AppError::Json)?;

    let out_path = output
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("session-{}.json", id)));

    tokio::fs::write(&out_path, json).await?;
    println!("Exported session to {}", out_path.display());

    Ok(())
}

async fn cmd_import(file: &str) -> Result<(), AppError> {
    let content = tokio::fs::read_to_string(file).await?;
    let export: SessionExport = serde_json::from_str(&content).map_err(AppError::Json)?;

    let project_dir = env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let pool = storage::init(&project_dir).await?;
    let session_store = SessionStore::new(pool.clone());
    let message_store = codegg::session::MessageStore::new(pool);

    let input = codegg::session::CreateSession {
        project_id: export.session.project_id,
        directory: export.session.directory,
        title: Some(export.session.title),
        parent_id: export.session.parent_id,
        workspace_id: export.session.workspace_id,
        agent: None,
        model: None,
        tags: None,
    };

    let created = session_store.create(input).await?;

    for msg_data in export.messages {
        message_store.create(&created.id, msg_data).await?;
    }

    println!("Imported session as {}", created.id);

    Ok(())
}

async fn cmd_upgrade() -> Result<(), AppError> {
    println!("Checking for updates...");

    let info = upgrade::check_for_updates().await?;

    if !info.needs_update {
        println!("Already on latest version ({})", info.current);
        return Ok(());
    }

    println!(
        "New version available: {} (current: {})",
        info.latest.as_deref().unwrap_or("unknown"),
        info.current
    );
    println!("Run the following to upgrade:");
    println!("  curl -fsSL https://codegg.ai/install.sh");

    Ok(())
}

async fn cmd_validate(config_path: Option<&str>) -> Result<(), AppError> {
    match config_path {
        Some(path) => {
            let content = std::fs::read_to_string(path)
                .map_err(|e| AppError::Config(ConfigError::NotFound(format!("{}: {}", path, e))))?;
            let interpolated = paths::interpolate_env_vars(&content);
            let config = paths::parse_config(&interpolated, std::path::Path::new(path))?;
            match config.validate() {
                Ok(()) => {
                    println!("Configuration is valid: {}", path);
                    Ok(())
                }
                Err(errors) => {
                    eprintln!("Configuration validation failed for {}:", path);
                    for error in &errors {
                        eprintln!("  - {}", error);
                    }
                    Err(AppError::Config(ConfigError::Parse(errors.join("; "))))
                }
            }
        }
        None => {
            let config = Config::load()?;
            match config.validate() {
                Ok(()) => {
                    println!("Configuration is valid.");
                    Ok(())
                }
                Err(errors) => {
                    eprintln!("Configuration validation failed:");
                    for error in &errors {
                        eprintln!("  - {}", error);
                    }
                    Err(AppError::Config(ConfigError::Parse(errors.join("; "))))
                }
            }
        }
    }
}

async fn cmd_doctor(subsystem: DoctorSubsystem) -> Result<(), AppError> {
    use codegg::search_backend::bootstrap;

    let config = Config::load()?;
    println!("== Codegg doctor ==");
    println!("config: loaded");

    if matches!(subsystem, DoctorSubsystem::Mcp) {
        println!("\n== MCP ==");
        list_mcp_servers(&config);
        return Ok(());
    }

    if matches!(subsystem, DoctorSubsystem::Lsp) {
        println!("\n== LSP ==");
        list_lsp_diagnostics(&config);
        return Ok(());
    }

    if matches!(subsystem, DoctorSubsystem::All | DoctorSubsystem::Search) {
        println!("\n== Search backend ==");
        let (_svc, report) = bootstrap::bootstrap_search_backend(&config).await;
        for line in report.summary_lines() {
            println!("{line}");
        }
    }

    if matches!(subsystem, DoctorSubsystem::All | DoctorSubsystem::Mcp) {
        println!("\n== MCP ==");
        list_mcp_servers(&config);
    }

    if matches!(subsystem, DoctorSubsystem::All | DoctorSubsystem::Lsp) {
        println!("\n== LSP ==");
        list_lsp_diagnostics(&config);
    }

    Ok(())
}

fn cmd_auth(command: AuthSubcommand) -> Result<(), AppError> {
    let cli = AuthCli::new();
    match command {
        AuthSubcommand::Status => cli.status(),
        AuthSubcommand::SetKey { provider, account } => {
            let key = codegg::auth::cli::read_key_from_stdin()?;
            if key.is_empty() {
                return Err(AppError::Config(ConfigError::Invalid(
                    "no key provided on stdin".to_string(),
                )));
            }
            cli.set_key(&provider, account.as_deref(), &key)
        }
        AuthSubcommand::Logout { provider, account } => cli.logout(&provider, account.as_deref()),
    }
}

fn list_mcp_servers(config: &Config) {
    let Some(mcp) = config.mcp.as_ref() else {
        println!("No MCP servers configured");
        return;
    };
    if mcp.is_empty() {
        println!("No MCP servers configured");
        return;
    }
    for (name, entry) in mcp {
        let enabled = entry.enabled.unwrap_or(true);
        let server_type = entry
            .inner
            .as_ref()
            .and_then(|c| c.server_type.clone())
            .unwrap_or_else(|| "local".to_string());
        let cmd = entry
            .inner
            .as_ref()
            .and_then(|c| c.command.clone())
            .unwrap_or_default();
        let url = entry
            .inner
            .as_ref()
            .and_then(|c| c.url.clone())
            .unwrap_or_default();
        let detail = if !url.is_empty() { url } else { cmd };
        println!(
            "  - {} [{}] enabled={} {}",
            name, server_type, enabled, detail
        );
    }
}

fn list_lsp_diagnostics(config: &Config) {
    use codegg::tool::RegistryBackendStatusKind;
    use codegg::tool::ToolRegistry;

    let registry = ToolRegistry::with_config(config);

    // --- registry/backend state ---
    let lsp_status = registry
        .backend_report(None)
        .into_iter()
        .find(|r| r.domain == "lsp");

    match &lsp_status {
        Some(status) => {
            let kind_label = match status.status {
                RegistryBackendStatusKind::Active => "active",
                RegistryBackendStatusKind::Disabled => "disabled",
                RegistryBackendStatusKind::ConfiguredButUnavailable => "unavailable",
                RegistryBackendStatusKind::FallbackToNative => "fallback-native",
            };
            println!(
                "registry: {} (backend={}, kind={})",
                status.tool, status.backend, kind_label
            );
        }
        None => {
            println!("registry: lsp not registered");
        }
    }

    // --- agent exposure gate ---
    let expose_lsp_tool = config
        .experimental
        .as_ref()
        .and_then(|e| e.lsp_tool)
        .unwrap_or(false);
    println!("agent exposure gate: experimental.lsp_tool = {expose_lsp_tool}");

    // --- model tool: visible only when registry-visible and gate allows ---
    let lsp_in_definitions = registry.definitions().iter().any(|d| d.name == "lsp");
    if lsp_in_definitions && expose_lsp_tool {
        println!("model tool: exposed (native lsp)");
    } else if lsp_in_definitions && !expose_lsp_tool {
        println!("model tool: hidden (registry-visible but agent exposure gate is false)");
    } else {
        println!("model tool: hidden (lsp not in registry definitions)");
    }
}

fn cmd_completions(shell: Shell, output_dir: Option<&str>) -> Result<(), AppError> {
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();

    match output_dir {
        Some(dir) => {
            let dir_path = std::path::Path::new(dir);
            if !dir_path.is_dir() {
                return Err(AppError::Other(anyhow::anyhow!(
                    "Output directory does not exist: {}",
                    dir
                )));
            }
            let file_name = match shell {
                Shell::Bash => format!("{}.bash", name),
                Shell::Zsh => format!("_{}", name),
                Shell::Fish => format!("{}.fish", name),
                Shell::PowerShell => format!("{}.ps1", name),
                Shell::Elvish => format!("{}.elv", name),
                _ => format!("{}.sh", name),
            };
            let file_path = dir_path.join(&file_name);
            let mut file = std::fs::File::create(&file_path).map_err(|e| {
                AppError::Other(anyhow::anyhow!(
                    "Failed to create file: {}: {}",
                    file_path.display(),
                    e
                ))
            })?;
            generate(shell, &mut cmd, name, &mut file);
            println!(
                "Generated {} completions in: {}",
                shell,
                file_path.display()
            );
        }
        None => {
            let mut stdout = std::io::stdout();
            generate(shell, &mut cmd, name, &mut stdout);
        }
    }
    Ok(())
}

async fn cmd_exec(
    json_input: Option<&str>,
    file_input: Option<&str>,
    json_output: bool,
    quiet: bool,
    session: Option<&str>,
) -> Result<(), AppError> {
    let input_json = if let Some(path) = file_input {
        tokio::fs::read_to_string(path).await?
    } else if let Some(json_str) = json_input {
        json_str.to_string()
    } else {
        let mut buffer = String::new();
        std::io::stdin().read_to_string(&mut buffer)?;
        buffer
    };

    let input: ExecInput = serde_json::from_str(&input_json)
        .map_err(|e| AppError::Other(anyhow::anyhow!("Failed to parse exec input JSON: {}", e)))?;

    let exec_mode = ExecMode::new(quiet, json_output, session.map(String::from));
    let output = exec_mode.run(input).await?;
    exec_mode.print_output(&output);
    std::process::exit(ExecMode::exit_code(&output));
}

async fn run_single_shot(prompt: &str, cli: &Cli) -> Result<(), AppError> {
    let config = Config::load().unwrap_or_default();
    let mut registry = ProviderRegistry::new();
    provider::register_builtin_with_config(&mut registry, &config);

    let default_model = config.model.clone().unwrap_or_default();
    let model = cli.model.as_ref().unwrap_or(&default_model);
    let (provider_id, model_name) = parse_model(model);

    let provider = registry
        .get(&provider_id)
        .ok_or_else(|| AppError::Other(anyhow::anyhow!("Provider not found: {}", provider_id)))?
        .clone_box();

    let agents = agent::resolve_agents(&config)?;
    let target_agent = cli
        .agent
        .as_deref()
        .unwrap_or(config.default_agent.as_deref().unwrap_or("build"));
    let selected_agent = agents
        .iter()
        .find(|a| a.name == target_agent)
        .cloned()
        .ok_or_else(|| AppError::Other(anyhow::anyhow!("Agent not found: {}", target_agent)))?;

    let permission_checker =
        codegg::permission::PermissionChecker::new(Some(&config), None).with_active_mode(&config);
    // Bootstraps the search backend (eggsearch by default) before the agent
    // loop starts. Idempotent if already bootstrapped.
    let (mcp_service, _report) =
        codegg::search_backend::bootstrap::bootstrap_search_backend(&config).await;
    let tool_registry = codegg::tool::ToolRegistry::with_config(&config);
    let mut agent_loop = codegg::agent::r#loop::AgentLoop::new(
        agents,
        provider,
        permission_checker,
        tool_registry,
        config.clone(),
        mcp_service,
        None,
        std::sync::Arc::new(codegg::context::InMemoryArtifactStore::new()),
    );
    let session_id = uuid::Uuid::new_v4().to_string();
    agent_loop.set_session_id(&session_id);
    agent_loop.set_agent(&selected_agent.name)?;

    let request = provider::ChatRequest {
        messages: vec![provider::Message::User {
            content: vec![provider::ContentPart::Text {
                text: prompt.to_string().into(),
            }],
        }],
        model: model_name.to_string(),
        tools: None,
        system: Some(codegg::agent::prompt::load_agent_prompt(
            &selected_agent,
            &config,
            &model_name,
        )),
        temperature: selected_agent.temperature,
        top_p: selected_agent.top_p,
        max_tokens: None,
        response_format: None,
        thinking_budget: selected_agent.thinking_budget,
        reasoning_effort: selected_agent.reasoning_effort,
    };

    let events = agent_loop.run(request).await?;
    let mut processor = codegg::agent::processor::EventProcessor::new();
    let mut final_usage = None;
    for event in events {
        if let provider::ChatEvent::Finish { usage, .. } = &event {
            final_usage = Some((usage.input_tokens, usage.output_tokens));
        }
        processor.process(event);
    }
    print!("{}", processor.text());
    println!();
    if let Some((input, output)) = final_usage {
        eprintln!("\nTokens: {} input, {} output", input, output);
    }

    Ok(())
}

async fn launch_tui(cli: &Cli) -> Result<(), AppError> {
    let project_dir = env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let core_transport = cli
        .core_transport
        .map(|m| match m {
            CoreTransport::Inproc => "inproc".to_string(),
            CoreTransport::Stdio => "stdio".to_string(),
            CoreTransport::Socket => "socket".to_string(),
        })
        .unwrap_or_else(|| {
            std::env::var("CODEGG_CORE_TRANSPORT")
                .unwrap_or_else(|_| "inproc".to_string())
                .to_lowercase()
        });
    let is_socket_mode = core_transport == "socket";

    let config = Config::load().unwrap_or_default();
    let notification_mgr = crate::tui::components::notification::NotificationManager::new(
        config.notifications.clone().unwrap_or_default(),
    );

    // Boot the search backend (eggsearch by default) early so the
    // McpService is available to any in-process agent loop. This call
    // is idempotent: the daemon's TurnSubmit handler will reuse the
    // same Arc<RwLock<McpService>> rather than spawning a second
    // eggsearch process.
    if !is_socket_mode {
        let (_mcp_service, _report) =
            codegg::search_backend::bootstrap::bootstrap_search_backend(&config).await;
    }

    // Only initialize heavy local resources for inproc/stdio modes.
    // In socket mode the daemon owns the DB, providers, scheduler, etc.
    let (pool, session_store, message_store, memory_store, user_prefs, model_ids, agents) =
        if is_socket_mode {
            (
                None,
                None,
                None,
                None,
                None,
                Vec::new(),
                agent::resolve_agents(&config)?,
            )
        } else {
            let pool = storage::init(&project_dir).await?;
            let session_store = Arc::new(SessionStore::new(pool.clone()));
            let message_store = Arc::new(MessageStore::new(pool.clone()));
            let user_prefs = storage::UserPreferences::new(pool.clone());
            let memory_store = Arc::new(MemoryStore::new().unwrap_or_else(|e| {
                tracing::warn!(
                    "Failed to initialize memory store: {}, continuing without persistent memory",
                    e
                );
                MemoryStore::default()
            }));

            let mut registry = ProviderRegistry::new();
            provider::register_builtin_with_config(&mut registry, &config);

            let discovery = provider::discovery::ModelDiscoveryService::new(PathBuf::new())
                .with_pool(pool.clone());
            discovery.initialize().await;
            let model_ids = if discovery.needs_refresh().await {
                let models = discovery.refresh(&registry).await;
                models
                    .iter()
                    .map(|m| format!("{}/{}", m.provider, m.id))
                    .collect()
            } else {
                discovery.get_model_ids().await
            };

            let agents = agent::resolve_agents(&config)?;
            (
                Some(pool),
                Some(session_store),
                Some(message_store),
                Some(memory_store),
                Some(user_prefs),
                model_ids,
                agents,
            )
        };

    let mut app = tui::App::new(project_dir.clone());
    if let Some(ref ss) = session_store {
        app.set_session_store(Arc::clone(ss));
    }
    if let Some(ref ms) = message_store {
        app.set_message_store(Arc::clone(ms));
    }
    if let Some(ref up) = user_prefs {
        app.set_preferences(up.clone());
    }
    if let Some(ref mm) = memory_store {
        app.set_memory_store(mm.clone());
    }
    app.set_models(model_ids.clone());
    app.agent_state.agents = agents.clone();
    app.notification_manager = Some(notification_mgr);

    if is_socket_mode {
        // Pull theme from config only (no SQLite-backed preferences in socket mode)
        // apply_persisted_preferences is skipped since there's no pool.
    } else {
        // Pull the user's saved theme and last-used model out of SQLite and
        // apply them on top of the config-file defaults. Called once at
        // startup; live changes go through the dedicated persist_* helpers.
        app.apply_persisted_preferences();
    }

    // Build subagent pool and scheduler only for local modes.
    if !is_socket_mode {
        let mut subagent_registry = ProviderRegistry::new();
        provider::register_builtin_with_config(&mut subagent_registry, &config);

        let subagent_pool = crate::agent::worker::SubAgentPool::new(
            &config,
            agents,
            subagent_registry,
            session_store
                .as_ref()
                .expect("session_store must exist in non-socket mode")
                .clone(),
            pool.clone(),
        )
        .await;
        app.subagent_pool = Some(Arc::new(subagent_pool));
        let scheduler = Arc::new(
            crate::agent::task::BackgroundScheduler::new()
                .with_pool(pool.clone().expect("pool must exist in non-socket mode")),
        );
        if let Some(ref sub_pool) = app.subagent_pool {
            scheduler.spawn_loop(Arc::clone(sub_pool), std::time::Duration::from_secs(10));
        }
        app.bg_scheduler = Some(scheduler);
    }

    // Create the shared LSP service early so it can be passed to both the
    // TUI (for security review) and the daemon (for agent prompt context).
    let lsp_service: Option<Arc<codegg::lsp::service::LspService>> = if !is_socket_mode {
        Some(codegg::lsp::LspService::new_arc(
            codegg::lsp::config_lsp_to_egglsp(config.lsp.clone().unwrap_or_default()),
        ))
    } else {
        None
    };

    let core_client: Arc<dyn CoreClient> = if core_transport == "stdio" {
        let exe = std::env::current_exe()
            .map_err(|e| AppError::Other(anyhow::anyhow!("cannot resolve current exe: {}", e)))?;
        let args = vec!["core-stdio".to_string()];
        match codegg::core::transport::StdioCoreClient::spawn(
            exe.to_string_lossy().as_ref(),
            &args,
            Some(std::path::Path::new(&project_dir)),
        )
        .await
        {
            Ok(client) => Arc::new(client),
            Err(e) => {
                tracing::warn!(
                    "failed to initialize stdio core transport ({}), falling back to inproc",
                    e
                );
                Arc::new(codegg::core::InprocCoreClient::new(
                    app.subagent_pool.clone(),
                    memory_store.as_ref().cloned(),
                    app.bg_scheduler.clone(),
                    pool.clone(),
                    config.clone(),
                    lsp_service.clone(),
                ))
            }
        }
    } else if is_socket_mode {
        let endpoint = cli
            .core_endpoint
            .clone()
            .or_else(|| std::env::var("CODEGG_CORE_ENDPOINT").ok())
            .unwrap_or_else(|| format!("unix://{}", default_socket_path()));
        match codegg::core::transport::SocketCoreClient::connect(&endpoint).await {
            Ok(client) => {
                // Mark the app as RemoteCore now that we have a live connection.
                app.ui_state.mode = codegg::tui::app::state::AppMode::RemoteCore { endpoint };
                Arc::new(client)
            }
            Err(e) => {
                if cli.core_transport.is_some() {
                    // Check if auto_start is enabled in daemon config
                    let auto_start = config
                        .daemon
                        .as_ref()
                        .and_then(|d| d.auto_start)
                        .unwrap_or(false);
                    if auto_start {
                        tracing::info!("Auto-starting daemon...");
                        let _ = tokio::process::Command::new(
                            std::env::current_exe().unwrap_or_default(),
                        )
                        .args(["daemon", "start"])
                        .spawn();
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        match codegg::core::transport::SocketCoreClient::connect(&endpoint).await {
                            Ok(client) => {
                                app.ui_state.mode =
                                    codegg::tui::app::state::AppMode::RemoteCore { endpoint };
                                Arc::new(client)
                            }
                            Err(e) => {
                                eprintln!("Daemon auto-start failed: {}", e);
                                eprintln!("Start the daemon with: codegg daemon start");
                                std::process::exit(1);
                            }
                        }
                    } else {
                        eprintln!("Failed to connect to core daemon at {}: {}", endpoint, e);
                        eprintln!("Start the daemon with: codegg daemon start");
                        std::process::exit(1);
                    }
                } else {
                    tracing::warn!(
                        "failed to initialize socket core transport ({}), falling back to inproc",
                        e
                    );
                    Arc::new(codegg::core::InprocCoreClient::new(
                        app.subagent_pool.clone(),
                        memory_store.as_ref().cloned(),
                        app.bg_scheduler.clone(),
                        pool.clone(),
                        config.clone(),
                        lsp_service.clone(),
                    ))
                }
            }
        }
    } else {
        Arc::new(codegg::core::InprocCoreClient::new(
            app.subagent_pool.clone(),
            memory_store.as_ref().cloned(),
            app.bg_scheduler.clone(),
            pool.clone(),
            config.clone(),
            lsp_service.clone(),
        ))
    };
    app.set_core_client(core_client);

    // Create a shared LspTool for security-review enrichment and other
    // LSP-backed TUI operations.  Only in local (non-socket) mode —
    // socket mode has no LspTool on the client side.
    if !is_socket_mode {
        if let Some(svc) = lsp_service {
            app.lsp_tool = Some(std::sync::Arc::new(
                codegg::tool::lsp::LspTool::new(svc)
                    .with_allowed_root(std::path::PathBuf::from(&project_dir)),
            ));
        }
    }

    if is_socket_mode {
        let n = app.init_remote_core().await;
        tracing::debug!("RemoteCore init: loaded {} models", n);
    }

    if let Some(agent_name) = &cli.agent {
        if let Some(idx) = app
            .agent_state
            .agents
            .iter()
            .position(|a| a.name == *agent_name)
        {
            app.agent_state.current_agent = idx;
        }
    }

    if let Some(model) = &cli.model {
        app.agent_state.current_model = model.clone();
    }

    let mut skills = SkillIndex::new();
    skills.load(&project_dir).await?;

    if let Some(skill_prompt) = cli.session.as_ref() {
        if let Some(skill_body) = skills.activate(skill_prompt.trim_start_matches("skill:")) {
            app.prompt_state.prompt.set_text(skill_body);
        }
    }

    if cli.no_session {
        tui::run_event_loop(&mut app).await?;
        return Ok(());
    }

    // Session loading. Inproc mode has a local `SessionStore` and
    // uses it directly; socket/RemoteCore mode has no local store
    // and must drive session loading through the `CoreClient` via
    // `App::load_initial_session_via_core`.
    if let Some(ss) = &session_store {
        if let Some(fork_id) = &cli.fork {
            match ss.fork(fork_id).await {
                Ok(forked) => {
                    app.set_session(forked);
                }
                Err(e) => {
                    eprintln!("Failed to fork session: {}", e);
                }
            }
        } else if let Some(session_id) = &cli.session {
            match ss.get(session_id).await {
                Ok(Some(sess)) => {
                    app.set_session(sess);
                }
                Ok(None) => {
                    eprintln!("Session not found: {}", session_id);
                }
                Err(e) => {
                    eprintln!("Failed to load session: {}", e);
                }
            }
        } else if cli.continue_session {
            match ss.list(&project_dir, 1).await {
                Ok(sessions) if !sessions.is_empty() => {
                    app.set_session(sessions[0].clone());
                }
                _ => {}
            }
        }
    } else {
        use codegg::tui::app::InitialSessionRequest;
        let request = if let Some(fork_id) = &cli.fork {
            InitialSessionRequest::Fork {
                session_id: fork_id.clone(),
            }
        } else if let Some(session_id) = &cli.session {
            InitialSessionRequest::Attach {
                session_id: session_id.clone(),
            }
        } else if cli.continue_session {
            InitialSessionRequest::Continue {
                project_dir: project_dir.clone(),
            }
        } else if cli.no_session {
            InitialSessionRequest::None
        } else {
            InitialSessionRequest::New {
                directory: project_dir.clone(),
                title: None,
            }
        };
        app.load_initial_session_via_core(request).await;
    }

    tui::run_event_loop(&mut app).await
}

async fn cmd_research(
    question: &str,
    mode: &str,
    audience: &str,
    depth: &str,
    outputs: &[String],
    source_specs: &[String],
    allow_network: bool,
) -> Result<(), AppError> {
    use codegg::research::coordinator::ResearchCoordinator;
    use codegg::research::types::*;

    let project_root = std::env::current_dir().map_err(AppError::Io)?;

    let research_mode = match mode {
        "landscape" => ResearchMode::Landscape,
        "architecture-decision" => ResearchMode::ArchitectureDecision,
        "library-evaluation" => ResearchMode::LibraryEvaluation,
        "api-investigation" => ResearchMode::ApiInvestigation,
        "debugging-investigation" => ResearchMode::DebuggingInvestigation,
        "security-review" => ResearchMode::SecurityReview,
        "spec-digest" => ResearchMode::SpecDigest,
        "narrow-answer" => ResearchMode::NarrowAnswer,
        _ => {
            eprintln!("Unknown mode: {mode}. Use: landscape, architecture-decision, library-evaluation, api-investigation, debugging-investigation, security-review, spec-digest, narrow-answer");
            std::process::exit(1);
        }
    };

    let research_audience = match audience {
        "human" => ResearchAudience::Human,
        "agent-planner" => ResearchAudience::AgentPlanner,
        "agent-coder" => ResearchAudience::AgentCoder,
        "agent-reviewer" => ResearchAudience::AgentReviewer,
        "agent-debugger" => ResearchAudience::AgentDebugger,
        _ => {
            eprintln!("Unknown audience: {audience}. Use: human, agent-planner, agent-coder, agent-reviewer, agent-debugger");
            std::process::exit(1);
        }
    };

    let research_depth = match depth {
        "low" => ResearchDepth::Low,
        "medium" => ResearchDepth::Medium,
        "high" => ResearchDepth::High,
        _ => {
            eprintln!("Unknown depth: {depth}. Use: low, medium, high");
            std::process::exit(1);
        }
    };

    let output_profiles: Vec<ResearchOutputProfile> = if outputs.is_empty() {
        vec![ResearchOutputProfile::HumanFullReport]
    } else {
        outputs
            .iter()
            .map(|o| match o.as_str() {
                "human-full" => ResearchOutputProfile::HumanFullReport,
                "human-brief" => ResearchOutputProfile::HumanBrief,
                "agent-answer" => ResearchOutputProfile::AgentAnswer,
                "agent-handoff" => ResearchOutputProfile::AgentHandoff,
                "evidence-bundle" => ResearchOutputProfile::EvidenceBundle,
                other => {
                    eprintln!("Unknown output profile: {other}. Use: human-full, human-brief, agent-answer, agent-handoff, evidence-bundle");
                    std::process::exit(1);
                }
            })
            .collect()
    };

    let sources: Vec<ResearchSourceSpec> = source_specs
        .iter()
        .map(|s| {
            if s == "local" {
                ResearchSourceSpec {
                    spec_type: SourceSpecType::Local,
                    value: String::new(),
                }
            } else if let Some(path) = s.strip_prefix("file:") {
                ResearchSourceSpec {
                    spec_type: SourceSpecType::File,
                    value: path.to_string(),
                }
            } else if let Some(url) = s.strip_prefix("url:") {
                ResearchSourceSpec {
                    spec_type: SourceSpecType::Url,
                    value: url.to_string(),
                }
            } else if let Some(text) = s.strip_prefix("text:") {
                ResearchSourceSpec {
                    spec_type: SourceSpecType::Text,
                    value: text.to_string(),
                }
            } else {
                // Default to file
                ResearchSourceSpec {
                    spec_type: SourceSpecType::File,
                    value: s.clone(),
                }
            }
        })
        .collect();

    let max_sources = match research_depth {
        ResearchDepth::Low => 8,
        ResearchDepth::Medium => 30,
        ResearchDepth::High => 80,
    };

    let request = ResearchRequest {
        id: uuid::Uuid::new_v4().to_string(),
        question: question.to_string(),
        mode: research_mode,
        audience: research_audience,
        depth: research_depth,
        output_profiles,
        constraints: vec![],
        sources,
        existing_context_refs: vec![],
        budget: ResearchBudget {
            max_sources,
            max_chunks_per_source: 20,
            max_evidence_spans: 200,
            max_model_calls: 0,
            max_output_tokens: None,
            allow_network,
        },
        created_at: chrono::Utc::now(),
    };

    let artifact_root = project_root.join(".codegg").join("research");
    let coordinator = ResearchCoordinator::new(project_root, artifact_root);

    eprintln!("Starting research run {}...", &request.id[..8]);
    eprintln!("Question: {}", request.question);
    eprintln!(
        "Mode: {:?}, Depth: {:?}, Audience: {:?}",
        request.mode, request.depth, request.audience
    );

    match coordinator.run(request).await {
        Ok(result) => {
            eprintln!("\nResearch completed successfully!");
            eprintln!("Status: {:?}", result.status);
            eprintln!(
                "Sources: {}, Evidence: {}, Claims: {}, Contradictions: {}",
                result.counts.sources,
                result.counts.evidence_spans,
                result.counts.claims,
                result.counts.contradictions,
            );
            eprintln!("\nArtifacts:");
            for output in &result.outputs {
                eprintln!("  {:?}: {}", output.profile, output.path.display());
            }
            eprintln!("\nRun directory: {}", result.artifact_dir.display());
        }
        Err(e) => {
            eprintln!("Research failed: {e}");
            std::process::exit(1);
        }
    }

    Ok(())
}

async fn run_daemon(endpoint: Option<String>) {
    let config = Config::load().unwrap_or_default();
    let project_dir = match config
        .daemon
        .as_ref()
        .and_then(|d| d.project_scope.as_deref())
    {
        Some("user") => dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .to_string_lossy()
            .to_string(),
        _ => env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
    };

    let pool = match storage::init(&project_dir).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Failed to initialize storage: {}", e);
            std::process::exit(1);
        }
    };
    let session_store = Arc::new(SessionStore::new(pool.clone()));
    let memory_store = Arc::new(MemoryStore::new().unwrap_or_else(|e| {
        tracing::warn!(
            "Failed to initialize memory store: {}, continuing without persistent memory",
            e
        );
        MemoryStore::default()
    }));
    let agents = match agent::resolve_agents(&config) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Failed to resolve agents: {}", e);
            std::process::exit(1);
        }
    };

    let mut subagent_registry = ProviderRegistry::new();
    provider::register_builtin_with_config(&mut subagent_registry, &config);
    let subagent_pool = crate::agent::worker::SubAgentPool::new(
        &config,
        agents,
        subagent_registry,
        session_store,
        Some(pool.clone()),
    )
    .await;
    let subagent_pool = Arc::new(subagent_pool);
    let scheduler =
        Arc::new(crate::agent::task::BackgroundScheduler::new().with_pool(pool.clone()));
    scheduler.spawn_loop(
        Arc::clone(&subagent_pool),
        std::time::Duration::from_secs(10),
    );

    let daemon = Arc::new(codegg::core::daemon::CoreDaemon::new(
        Some(pool),
        Some(subagent_pool),
        Some(memory_store),
        Some(scheduler),
    ));

    daemon.start_event_bridge();
    daemon.recover_state().await;

    let ep = endpoint
        .or_else(|| std::env::var("CODEGG_CORE_ENDPOINT").ok())
        .unwrap_or_else(default_socket_path);

    if let Some(parent) = std::path::Path::new(&ep).parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    let _ = tokio::fs::remove_file(&ep).await;

    // Write PID file for stop/status commands
    let pid_file = std::path::Path::new(&ep).with_extension("pid");
    tokio::fs::write(&pid_file, std::process::id().to_string())
        .await
        .ok();

    tracing::info!("Starting core daemon on {}", ep);
    println!("Core daemon listening on {}", ep);

    // Spawn cleanup task for SIGTERM/SIGINT
    let pid_file_cleanup = pid_file.clone();
    let ep_cleanup = ep.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = tokio::fs::remove_file(&pid_file_cleanup).await;
        let _ = tokio::fs::remove_file(&ep_cleanup).await;
        std::process::exit(0);
    });

    if let Err(e) = codegg::core::transport::daemon_socket::run_core_socket(daemon, &ep).await {
        eprintln!("Daemon error: {}", e);
        let _ = tokio::fs::remove_file(&pid_file).await;
        std::process::exit(1);
    }
    // Clean up PID file on normal exit
    let _ = tokio::fs::remove_file(&pid_file).await;
}

async fn run_core_stdio() -> Result<(), AppError> {
    let project_dir = env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let pool = storage::init(&project_dir).await?;
    let session_store = Arc::new(SessionStore::new(pool.clone()));
    let memory_store = Arc::new(MemoryStore::new().unwrap_or_else(|_| MemoryStore::default()));
    let config = Config::load().unwrap_or_default();
    let agents = agent::resolve_agents(&config)?;

    let mut subagent_registry = ProviderRegistry::new();
    provider::register_builtin_with_config(&mut subagent_registry, &config);
    let subagent_pool = crate::agent::worker::SubAgentPool::new(
        &config,
        agents,
        subagent_registry,
        session_store,
        Some(pool.clone()),
    )
    .await;
    let subagent_pool = Arc::new(subagent_pool);
    let scheduler =
        Arc::new(crate::agent::task::BackgroundScheduler::new().with_pool(pool.clone()));
    scheduler.spawn_loop(
        Arc::clone(&subagent_pool),
        std::time::Duration::from_secs(10),
    );

    let core = codegg::core::InprocCoreClient::new(
        Some(subagent_pool),
        Some(memory_store),
        Some(scheduler),
        Some(pool),
        config,
        None,
    );

    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await.map_err(AppError::Io)? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<RequestEnvelope<CoreRequest>>(trimmed) {
            Ok(req) => core.request(req).await.unwrap_or(CoreResponse::Error {
                code: "request_failed".to_string(),
                message: "core request execution failed".to_string(),
            }),
            Err(e) => CoreResponse::Error {
                code: "invalid_request".to_string(),
                message: e.to_string(),
            },
        };
        let out = serde_json::to_string(&response).map_err(AppError::Json)?;
        stdout
            .write_all(out.as_bytes())
            .await
            .map_err(AppError::Io)?;
        stdout.write_all(b"\n").await.map_err(AppError::Io)?;
        stdout.flush().await.map_err(AppError::Io)?;
    }
    Ok(())
}

fn parse_model(model: &str) -> (String, String) {
    if let Some(pos) = model.find('/') {
        (model[..pos].to_string(), model[pos + 1..].to_string())
    } else {
        ("openai".to_string(), model.to_string())
    }
}

#[cfg(feature = "server")]
async fn cmd_server(host: &str, port: u16) -> Result<(), AppError> {
    let project_dir = std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let pool = match storage::init(&project_dir).await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("Failed to initialize storage for daemon: {}", e);
            return codegg::server::run_server(host, port, None)
                .await
                .map_err(|e| AppError::Other(anyhow::anyhow!("Server error: {}", e)));
        }
    };
    let session_store = Arc::new(SessionStore::new(pool.clone()));
    let memory_store = Arc::new(MemoryStore::new().unwrap_or_else(|e| {
        tracing::warn!("Failed to initialize memory store: {}", e);
        MemoryStore::default()
    }));
    let config = Config::load().unwrap_or_default();
    let agents = match agent::resolve_agents(&config) {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!("Failed to resolve agents: {}, continuing without daemon", e);
            return codegg::server::run_server(host, port, None)
                .await
                .map_err(|e| AppError::Other(anyhow::anyhow!("Server error: {}", e)));
        }
    };

    let mut subagent_registry = ProviderRegistry::new();
    provider::register_builtin_with_config(&mut subagent_registry, &config);
    let subagent_pool = crate::agent::worker::SubAgentPool::new(
        &config,
        agents,
        subagent_registry,
        session_store,
        Some(pool.clone()),
    )
    .await;
    let subagent_pool = Arc::new(subagent_pool);
    let scheduler =
        Arc::new(crate::agent::task::BackgroundScheduler::new().with_pool(pool.clone()));
    scheduler.spawn_loop(
        Arc::clone(&subagent_pool),
        std::time::Duration::from_secs(10),
    );

    let daemon = Arc::new(codegg::core::daemon::CoreDaemon::new(
        Some(pool),
        Some(subagent_pool),
        Some(memory_store),
        Some(scheduler),
    ));

    daemon.start_event_bridge();
    daemon.recover_state().await;

    codegg::server::run_server(host, port, Some(daemon))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("Server error: {}", e)))
}

#[cfg(feature = "server")]
async fn cmd_attach(url: &str, token: Option<&str>) -> Result<(), AppError> {
    codegg::client::run_attach(url, token)
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("Client error: {}", e)))
}
