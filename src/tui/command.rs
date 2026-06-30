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
        let mut commands = Self::built_in_commands();

        Self::append_dynamic_commands(&mut commands);
        Self { commands }
    }

    fn built_in_commands() -> Vec<Command> {
        vec![
            Command::new("/connect", CommandCategory::System, None)
                .with_description("Connect provider"),
            Command::new("/exit", CommandCategory::System, None)
                .with_aliases(&["quit", "q"])
                .with_description("Exit the app"),
            Command::new("/status", CommandCategory::System, None).with_description("View status"),
            Command::new("/themes", CommandCategory::System, None)
                .with_aliases(&["/theme"])
                .with_description("Switch theme (/theme, /theme list, /theme use <name>, /theme reload, /theme diagnostics)"),
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
                .with_description("Manage active long-running goal (/goal set, show, pause, resume, clear, done, checkpoint, from-file, budget [show|raise <axis> <n>])"),
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
            Command::new("/research-show", CommandCategory::Session, None)
                .with_description("Show research runs and details (/research-show report|handoff|claims <run_id>)"),
            Command::new("/search", CommandCategory::Session, None)
                .with_description("Search session transcript"),
            Command::new("/doctor", CommandCategory::System, None)
                .with_description("Run diagnostics (search backend, MCP, providers)"),
            Command::new("/lsp-status", CommandCategory::System, None)
                .with_description("Show LSP server status and diagnostics"),
            Command::new("/lsp-previews", CommandCategory::System, None)
                .with_aliases(&["/preview-list"])
                .with_description("List LSP preview artifacts"),
            Command::new("/lsp-preview", CommandCategory::System, None)
                .with_aliases(&["/preview-show"])
                .with_description("Show LSP preview detail (args: <id>)"),
            Command::new("/lsp-preview-clear", CommandCategory::System, None)
                .with_aliases(&["/preview-clear"])
                .with_description("Clear LSP preview(s) (args: <id> or --all)"),
            Command::new("/lsp-preview-refresh", CommandCategory::System, None)
                .with_aliases(&["/preview-refresh"])
                .with_description("Refresh LSP preview staleness (args: <id>)"),
            Command::new("/lsp-preview-apply", CommandCategory::System, None)
                .with_aliases(&["/preview-apply"])
                .with_description("Apply LSP preview patches to disk with hash revalidation (args: <id>)"),
            Command::new("/lsp-servers", CommandCategory::System, None)
                .with_aliases(&["/lsp-detail"])
                .with_description("List active LSP servers with status, root, generation"),
            Command::new("/lsp-capabilities", CommandCategory::System, None)
                .with_description("Show effective LSP capabilities (args: server-key)"),
            Command::new("/lsp-errors", CommandCategory::System, None)
                .with_description("Show LSP server errors and health (args: server-key)"),
            Command::new("/lsp-root", CommandCategory::System, None)
                .with_description("Diagnose LSP root for a file path (args: <path>)"),
            Command::new("/lsp-restart", CommandCategory::System, None)
                .with_description("Restart an LSP server (args: server-key)"),
            Command::new("/lsp-stop", CommandCategory::System, None)
                .with_description("Stop LSP servers (args: server-key or --all)"),
            Command::new("/lsp-cache-status", CommandCategory::System, None)
                .with_description("Show LSP semantic cache status and stats"),
            Command::new("/lsp-cache-clear", CommandCategory::System, None)
                .with_description("Clear LSP semantic cache (args: --all or <root-path>)"),
            Command::new("/lsp-doctor", CommandCategory::System, None)
                .with_description("Diagnose LSP status for a file path (args: <path>)"),
            Command::new("/lsp-context-diagnostics", CommandCategory::System, None)
                .with_description("Show LSP context diagnostics for a file path"),
            Command::new("/lsp-repair-local", CommandCategory::System, None)
                .with_description("Repair localized issue (args: <path[:line]>)"),
            Command::new("/lsp-repair-hunk", CommandCategory::System, None)
                .with_description("Repair code around diff hunks (args: <path> [hunk-id|range])"),
            Command::new("/lsp-review-file", CommandCategory::System, None)
                .with_description("Semantic review of a file (args: <path>)"),
            Command::new("/lsp-review-diff", CommandCategory::System, None)
                .with_description("Review changed files/hunks in current diff"),
            Command::new("/lsp-security-review", CommandCategory::System, None)
                .with_description("Enriched security review (args: [path|diff])"),
            Command::new("/lsp-impact", CommandCategory::System, None)
                .with_description("Impact analysis for a symbol (args: <path:line:col>)"),
            Command::new("/lsp-test-repair", CommandCategory::System, None)
                .with_description("Test failure repair (args: <test-file> [failure-text])"),
            Command::new("/lsp-interface", CommandCategory::System, None)
                .with_description("API boundary review (args: <path[:symbol]>)"),
            Command::new("/lsp-cross-repair", CommandCategory::System, None)
                .with_description("Cross-file repair context (args: <primary> [related...])"),
            Command::new("/lsp-call-neighbors", CommandCategory::System, None)
                .with_description("Call neighborhood (args: <path:line:col> [incoming|outgoing|both])"),
            Command::new("/tool-backends", CommandCategory::System, None)
                .with_aliases(&["/tools", "/backends"])
                .with_description("Show resolved backend for each model-facing tool (Native / MCP / Builtin / Disabled)"),
            Command::new("/security-review", CommandCategory::Agent, None)
                .with_description("Security review of changed files (/security-review [--changed] [--base <ref>] [--json] [--prompts-only] [--findings-only] [--no-content] [--no-filename] [--max-findings N] [--max-prompts N] [--enrich] [--panel])"),
            Command::new("/security-review-show", CommandCategory::Agent, Some(Dialog::SecurityReview))
                .with_description("Reopen the latest security review result panel (no rerun)"),
            Command::new("/security-review-cancel", CommandCategory::Agent, None)
                .with_description("Cancel an in-flight security review"),
            Command::new("/shell-list", CommandCategory::System, None)
                .with_description("List recent shell commands"),
            Command::new("/shell-show", CommandCategory::System, None)
                .with_description("Show detailed info for a shell command (args: <id|last>)"),
            Command::new("/shell-include", CommandCategory::System, None)
                .with_description("Include shell output in context (args: <id|last> [--tail N|--stdout|--stderr|--summary])"),
            Command::new("/shell-rerun", CommandCategory::System, None)
                .with_description("Re-run a shell command (args: <id|last>)"),
            Command::new("/shell-kill", CommandCategory::System, None)
                .with_description("Kill a running shell command (args: <id|last>)"),
            Command::new("/shell-ask", CommandCategory::System, None)
                .with_description("Ask about shell output (args: <id|last> <question>)"),
            Command::new("/tui-stats", CommandCategory::System, None)
                .with_description("Show TUI runtime diagnostics"),
        ]
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
                if let std::collections::hash_map::Entry::Vacant(e) = seen.entry(normalized) {
                    e.insert(cmd.name.clone());
                    new_commands.push(Self::from_dynamic_command(cmd));
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
            if let std::collections::hash_map::Entry::Vacant(e) = seen.entry(normalized) {
                e.insert(cmd.name.clone());
                new_commands.push(Self::from_dynamic_command(cmd));
            }
        }

        commands.append(&mut new_commands);
    }

    fn from_dynamic_command(cmd: crate::command::Command) -> Command {
        Command {
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
        }
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
                }
                if best_score > 0 {
                    Some((cmd, best_score))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by_key(|b| std::cmp::Reverse(b.1));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_command_count_matches_release_docs() {
        assert_eq!(CommandRegistry::built_in_commands().len(), 96);
    }

    #[test]
    fn filter_matches_aliases_once_through_all_names() {
        let registry = CommandRegistry {
            commands: CommandRegistry::built_in_commands(),
        };

        let results = registry.filter("preview-show");
        assert_eq!(
            results.first().map(|(cmd, _)| cmd.name.as_str()),
            Some("/lsp-preview")
        );
    }

    #[test]
    fn dynamic_command_conversion_normalizes_name() {
        let cmd = crate::command::Command {
            name: "ship".to_string(),
            description: Some("Ship it".to_string()),
            template: "Release {args}".to_string(),
            agent: Some("build".to_string()),
            model: Some("model-a".to_string()),
            #[allow(deprecated)]
            subtask: None,
            source: "test".to_string(),
        };

        let converted = CommandRegistry::from_dynamic_command(cmd);

        assert_eq!(converted.name, "/ship");
        assert_eq!(converted.description, "Ship it");
        assert_eq!(converted.template.as_deref(), Some("Release {args}"));
        assert_eq!(converted.agent.as_deref(), Some("build"));
        assert_eq!(converted.model.as_deref(), Some("model-a"));
        assert_eq!(converted.source.as_deref(), Some("test"));
    }
}
