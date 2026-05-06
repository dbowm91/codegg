use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use codegg::agent;
use codegg::config::paths;
use codegg::config::schema::Config;
use codegg::error::{AppError, ConfigError};
use codegg::exec::{ExecInput, ExecMode};
use codegg::mcp;
use codegg::provider::{self, ProviderRegistry};
use codegg::session::{MessageStore, Session, SessionStore};
use codegg::skills::SkillIndex;
use codegg::storage;
use codegg::tui;
use serde::{Deserialize, Serialize};
use std::env;
use std::io::Read;
use tracing_subscriber::util::SubscriberInitExt;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser, Debug)]
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

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
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
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
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
    let mut registry = ProviderRegistry::new();
    provider::register_builtin(&mut registry);

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
        println!("No providers configured. Set API keys in your config.");
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
        .ok_or_else(|| AppError::Other(anyhow::anyhow!("Session not found: {}", id)))?;

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
    let message_store = codegg_rs::session::MessageStore::new(pool);

    let session = session_store
        .get(id)
        .await?
        .ok_or_else(|| AppError::Other(anyhow::anyhow!("Session not found: {}", id)))?;

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
    let message_store = codegg_rs::session::MessageStore::new(pool);

    let input = codegg_rs::session::CreateSession {
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

    let client = reqwest::Client::builder()
        .user_agent("codegg")
        .build()?;

    let resp = client
        .get("https://api.github.com/repos/anomalyco/codegg/releases/latest")
        .send()
        .await?;

    if !resp.status().is_success() {
        println!(
            "Could not check for updates. HTTP status: {}",
            resp.status()
        );
        return Ok(());
    }

    let release: serde_json::Value = resp.json().await?;
    let latest_version = release["tag_name"]
        .as_str()
        .unwrap_or("unknown")
        .trim_start_matches('v');

    let current_version = env!("CARGO_PKG_VERSION");

    if latest_version == current_version {
        println!("Already on latest version ({})", current_version);
        return Ok(());
    }

    println!(
        "New version available: {} (current: {})",
        latest_version, current_version
    );
    println!("Run the following to upgrade:");
    println!("  cargo install --git https://github.com/anomalyco/codegg --path codegg");

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
        .ok_or_else(|| AppError::Other(anyhow::anyhow!("Provider not found: {}", provider_id)))?;

    let request = provider::ChatRequest {
        messages: vec![provider::Message::User {
            content: vec![provider::ContentPart::Text {
                text: prompt.to_string().into(),
            }],
        }],
        model: model_name.to_string(),
        tools: None,
        system: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        response_format: None,
    };

    let mut stream = provider.stream(&request).await?;

    use futures::StreamExt;
    while let Some(event) = stream.next().await {
        match event? {
            provider::ChatEvent::TextDelta(text) => {
                print!("{}", text);
                use std::io::Write;
                std::io::stdout().flush()?;
            }
            provider::ChatEvent::Finish { usage, .. } => {
                eprintln!(
                    "\n\nTokens: {} input, {} output",
                    usage.input_tokens, usage.output_tokens
                );
            }
            _ => {}
        }
    }
    println!();

    Ok(())
}

async fn launch_tui(cli: &Cli) -> Result<(), AppError> {
    let project_dir = env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let pool = storage::init(&project_dir).await?;
    let session_store = Arc::new(SessionStore::new(pool.clone()));
    let message_store = Arc::new(MessageStore::new(pool.clone()));

    let config = Config::load().unwrap_or_default();
    let notification_mgr = crate::tui::components::notification::NotificationManager::new(
        config.notifications.clone().unwrap_or_default(),
    );
    let mut registry = ProviderRegistry::new();
    provider::register_builtin_with_config(&mut registry, &config);

    let discovery =
        provider::discovery::ModelDiscoveryService::new(PathBuf::new()).with_pool(pool.clone());
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

    let mut app = tui::App::new(project_dir.clone());
    app.set_session_store(Arc::clone(&session_store));
    app.set_message_store(Arc::clone(&message_store));
    app.set_models(model_ids.clone());
    app.agent_state.agents = agents.clone();
    app.notification_manager = Some(notification_mgr);

    let mut subagent_registry = ProviderRegistry::new();
    provider::register_builtin_with_config(&mut subagent_registry, &config);

    let subagent_pool = crate::agent::worker::SubAgentPool::new(
        &config,
        agents,
        subagent_registry,
        session_store.clone(),
        Some(pool.clone()),
    )
    .await;
    app.subagent_pool = Some(Arc::new(subagent_pool));
    let scheduler =
        Arc::new(crate::agent::task::BackgroundScheduler::new().with_pool(pool.clone()));
    if let Some(ref sub_pool) = app.subagent_pool {
        scheduler.spawn_loop(Arc::clone(sub_pool), std::time::Duration::from_secs(10));
    }
    app.bg_scheduler = Some(scheduler);

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

    if let Some(fork_id) = &cli.fork {
        match session_store.fork(fork_id).await {
            Ok(forked) => {
                app.set_session(forked);
            }
            Err(e) => {
                eprintln!("Failed to fork session: {}", e);
            }
        }
    } else if let Some(session_id) = &cli.session {
        match session_store.get(session_id).await {
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
        match session_store.list(&project_dir, 1).await {
            Ok(sessions) if !sessions.is_empty() => {
                app.set_session(sessions[0].clone());
            }
            _ => {}
        }
    }

    tui::run_event_loop(&mut app).await
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
    codegg_rs::server::run_server(host, port)
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("Server error: {}", e)))
}

#[cfg(feature = "server")]
async fn cmd_attach(url: &str, token: Option<&str>) -> Result<(), AppError> {
    codegg_rs::client::run_attach(url, token)
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("Client error: {}", e)))
}
