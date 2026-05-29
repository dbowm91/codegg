use crate::tui::app::Dialog;
use crate::util::fuzzy::fuzzy_score;
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandCategory {
    Session,
    Agent,
    System,
}

impl fmt::Display for CommandCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandCategory::Session => write!(f, "Session"),
            CommandCategory::Agent => write!(f, "Agent"),
            CommandCategory::System => write!(f, "System"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Command {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub category: CommandCategory,
    pub dialog: Option<Dialog>,
    pub template: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub subtask: Option<bool>,
    pub source: Option<String>,
}

impl Command {
    pub fn new(name: &str, category: CommandCategory, dialog: Option<Dialog>) -> Self {
        Self {
            name: name.to_string(),
            aliases: Vec::new(),
            description: String::new(),
            category,
            dialog,
            template: None,
            agent: None,
            model: None,
            subtask: None,
            source: None,
        }
    }

    pub fn with_aliases(mut self, aliases: &[&str]) -> Self {
        self.aliases = aliases.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn with_description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    pub fn with_template(mut self, template: &str) -> Self {
        self.template = Some(template.to_string());
        self
    }

    pub fn all_names(&self) -> Vec<&str> {
        let mut names = vec![self.name.as_str()];
        names.extend(self.aliases.iter().map(|s| s.as_str()));
        names
    }
}

pub struct CommandRegistry {
    commands: Vec<Command>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        let mut commands = vec![
            Command::new("/connect", CommandCategory::System, None)
                .with_description("Connect provider"),
            Command::new("/exit", CommandCategory::System, None)
                .with_aliases(&["quit", "q"])
                .with_description("Exit the app"),
            Command::new("/status", CommandCategory::System, None).with_description("View status"),
            Command::new("/themes", CommandCategory::System, None).with_description("Switch theme"),
            Command::new("/help", CommandCategory::System, None).with_description("Help"),
            Command::new("/sessions", CommandCategory::Session, None)
                .with_aliases(&["resume", "continue"])
                .with_description("Switch session"),
            Command::new("/new", CommandCategory::Session, None)
                .with_aliases(&["clear"])
                .with_description("New session"),
            Command::new("/share", CommandCategory::Session, None)
                .with_description("Share session"),
            Command::new("/unshare", CommandCategory::Session, None)
                .with_description("Unshare session"),
            Command::new("/rename", CommandCategory::Session, None)
                .with_description("Rename session"),
            Command::new("/compact", CommandCategory::Session, None)
                .with_aliases(&["summarize"])
                .with_description("Compact session"),
            Command::new("/timeline", CommandCategory::Session, None)
                .with_description("Jump to message"),
            Command::new("/fork", CommandCategory::Session, None)
                .with_description("Fork from message"),
            Command::new("/undo", CommandCategory::Session, None)
                .with_description("Undo previous message"),
            Command::new("/redo", CommandCategory::Session, None).with_description("Redo"),
            Command::new("/export", CommandCategory::Session, None)
                .with_description("Export session transcript"),
            Command::new("/import", CommandCategory::Session, None)
                .with_description("Import session"),
            Command::new("/timestamps", CommandCategory::Session, None)
                .with_aliases(&["toggle-timestamps"])
                .with_description("Toggle timestamps"),
            Command::new("/thinking", CommandCategory::Session, None)
                .with_aliases(&["toggle-thinking"])
                .with_description("Toggle thinking"),
            Command::new("/models", CommandCategory::System, Some(Dialog::Model))
                .with_description("Switch model"),
            Command::new("/models-refresh", CommandCategory::System, None)
                .with_aliases(&["refresh-models"])
                .with_description("Refresh model list"),
            Command::new("/variants", CommandCategory::System, None)
                .with_description("Switch model variant"),
            Command::new("/agents", CommandCategory::Agent, Some(Dialog::Agent))
                .with_description("Switch agent"),
            Command::new("/mcps", CommandCategory::System, Some(Dialog::Mcp))
                .with_description("Manage MCP servers"),
            Command::new("/workspaces", CommandCategory::System, None)
                .with_description("Manage workspaces"),
            Command::new("/tree", CommandCategory::System, None).with_description("Show file tree"),
            Command::new("/editor", CommandCategory::Agent, None).with_description("Open editor"),
            Command::new("/keybinds", CommandCategory::System, Some(Dialog::Keybind))
                .with_description("Customize keybindings"),
            Command::new("/context", CommandCategory::Session, None)
                .with_description("View context window usage"),
            Command::new("/cost", CommandCategory::Session, None)
                .with_description("View token usage and cost"),
            Command::new("/usage", CommandCategory::Session, None)
                .with_description("View rate limits and quota"),
            Command::new("/stats", CommandCategory::Session, Some(Dialog::Stats))
                .with_description("View session analytics and cost breakdown"),
            Command::new("/tui", CommandCategory::System, None)
                .with_aliases(&["fullscreen"])
                .with_description("Toggle fullscreen mode"),
            Command::new("/tts", CommandCategory::System, None)
                .with_aliases(&["voice"])
                .with_description("Toggle text-to-speech"),
            Command::new("/loop", CommandCategory::Agent, None)
                .with_description("Schedule periodic task (e.g. /loop 5m \"check status\")"),
            Command::new("/tasks", CommandCategory::Agent, None)
                .with_description("List background tasks"),
            Command::new("/task-del", CommandCategory::Agent, None)
                .with_description("Delete background task"),
            Command::new("/memory", CommandCategory::Session, None)
                .with_description("Memory dashboard"),
            Command::new("/memory-search", CommandCategory::Session, None)
                .with_description("Search memories (args: query)"),
            Command::new("/memory-list", CommandCategory::Session, None)
                .with_description("List memories (args: namespace)"),
            Command::new("/memory-remember", CommandCategory::Agent, None)
                .with_description("Remember something (args: text)"),
            Command::new("/memory-forget", CommandCategory::Agent, None)
                .with_description("Forget a memory (args: id)"),
            Command::new("/memory-consolidate", CommandCategory::Session, None)
                .with_description("Consolidate session into memories"),
            Command::new("/checkpoint", CommandCategory::Session, None)
                .with_description("Create a checkpoint of current session"),
            Command::new("/goal", CommandCategory::Session, None)
                .with_description("Manage active long-running goal (/goal set, show, pause, resume, clear, done, checkpoint, from-file)"),
            Command::new("/plan", CommandCategory::Session, None)
                .with_description("Manage task plan (/plan, /plan add, done, skip, block, clear)"),
            Command::new("/state", CommandCategory::Session, None)
                .with_description("Show work/session state view"),
            Command::new("/pr", CommandCategory::Agent, None)
                .with_description("GitHub pull requests")
                .with_template("Use GitHub MCP (mcp__github) to {args}"),
            Command::new("/issue", CommandCategory::Agent, None)
                .with_aliases(&["/bugs", "/features"])
                .with_description("GitHub issues")
                .with_template("Use GitHub MCP (mcp__github) to {args}"),
            Command::new("/review", CommandCategory::Session, Some(Dialog::Review))
                .with_description("Review changed files"),
            Command::new("/diff", CommandCategory::Session, None)
                .with_description("Show diff for a file (/diff <path>)"),
            Command::new("/tests", CommandCategory::Session, None)
                .with_description("Show test state (/tests, /tests last, /tests failed)"),
            Command::new("/revert", CommandCategory::Agent, None)
                .with_description("Revert a file change (/revert <path>)"),
            Command::new("/research", CommandCategory::Agent, None)
                .with_description("Run research on a question (/research <question> [--mode <mode>] [--depth <depth>])"),
            Command::new("/research-runs", CommandCategory::Session, None)
                .with_description("List recent research runs"),
            Command::new("/research-open", CommandCategory::Session, None)
                .with_description("Show artifacts for a research run (/research-open <run_id>)"),
        ];

        Self::append_dynamic_commands(&mut commands);
        Self { commands }
    }

    fn append_dynamic_commands(commands: &mut Vec<Command>) {
        let mut seen: HashMap<String, String> = HashMap::new();

        for cmd in commands.iter() {
            for name in cmd.all_names() {
                let normalized = Self::normalize_name(name);
                seen.insert(normalized, cmd.name.clone());
            }
        }

        let mut new_commands: Vec<Command> = Vec::new();

        let config = crate::config::schema::Config::load().unwrap_or_default();
        if let Some(config_commands) = config.commands.as_ref() {
            for cmd in crate::command::resolve_commands_from_config(config_commands) {
                let normalized = Self::normalize_name(&cmd.name);
                if !seen.contains_key(&normalized) {
                    seen.insert(normalized, cmd.name.clone());
                    new_commands.push(Command {
                        name: Self::to_slash_name(&cmd.name),
                        aliases: Vec::new(),
                        description: cmd.description.unwrap_or_default(),
                        category: CommandCategory::Agent,
                        dialog: None,
                        template: Some(cmd.template),
                        agent: cmd.agent,
                        model: cmd.model,
                        #[allow(deprecated)]
                        subtask: cmd.subtask,
                        source: Some(cmd.source),
                    });
                }
            }
        }

        let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let base = base.canonicalize().unwrap_or(base);
        let dynamic_commands = std::thread::scope(|s| {
            s.spawn(|| {
                crate::command::find_command_files_sync(&base)
                    .into_iter()
                    .filter_map(|r| r.ok())
                    .map(|cmd| {
                        let normalized = Self::normalize_name(&cmd.name);
                        (normalized, cmd)
                    })
                    .collect::<HashMap<_, _>>()
            })
            .join()
            .unwrap_or_default()
        });
        for (normalized, cmd) in dynamic_commands {
            if !seen.contains_key(&normalized) {
                seen.insert(normalized, cmd.name.clone());
                new_commands.push(Command {
                    name: Self::to_slash_name(&cmd.name),
                    aliases: Vec::new(),
                    description: cmd.description.unwrap_or_default(),
                    category: CommandCategory::Agent,
                    dialog: None,
                    template: Some(cmd.template),
                    agent: cmd.agent,
                    model: cmd.model,
                    #[allow(deprecated)]
                    subtask: cmd.subtask,
                    source: Some(cmd.source),
                });
            }
        }

        commands.append(&mut new_commands);
    }

    fn normalize_name(name: &str) -> String {
        name.trim().trim_start_matches('/').to_lowercase()
    }

    fn to_slash_name(name: &str) -> String {
        if name.starts_with('/') {
            name.to_string()
        } else {
            format!("/{}", name)
        }
    }

    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    pub fn find_by_name_or_alias(&self, name: &str) -> Option<&Command> {
        let needle = Self::normalize_name(name);
        self.commands.iter().find(|cmd| {
            Self::normalize_name(&cmd.name) == needle
                || cmd
                    .aliases
                    .iter()
                    .any(|alias| Self::normalize_name(alias) == needle)
        })
    }

    pub fn filter(&self, query: &str) -> Vec<(&Command, usize)> {
        let query = query.trim_start_matches('/');
        if query.is_empty() {
            return self.commands.iter().map(|c| (c, 0)).collect();
        }

        let mut scored: Vec<(&Command, usize)> = self
            .commands
            .iter()
            .filter_map(|cmd| {
                let mut best_score = 0usize;
                for name in cmd.all_names() {
                    let name_without_slash = name.trim_start_matches('/');
                    let score = fuzzy_score(query, name_without_slash);
                    if score > best_score {
                        best_score = score;
                    }
                    for alias in &cmd.aliases {
                        let score = fuzzy_score(query, alias);
                        if score > best_score {
                            best_score = score;
                        }
                    }
                }
                if best_score > 0 {
                    Some((cmd, best_score))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by(|a, b| b.1.cmp(&a.1));
        scored.truncate(10);
        scored
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub static COMMAND_REGISTRY: LazyLock<CommandRegistry> = LazyLock::new(CommandRegistry::new);
