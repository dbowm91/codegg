//! The `/skills` command: the effective skill registry, its precedence and its diagnostics.

use crate::tui::app::App;
use crate::tui::commands::close_command_mode;

pub(crate) fn dispatch_skills(app: &mut App, argument: &str) {
    let query = argument.chars().take(128).collect::<String>();
    let report = app.agent_state.snapshot.as_deref().map(|snapshot| {
        let registry = &snapshot.skills;
        let skipped = registry
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == crate::skills::Severity::Error)
            .count();
        let result = if query.is_empty() {
            if registry.effective.is_empty() {
                "No effective skills".to_string()
            } else {
                let mut entries = registry
                    .effective
                    .iter()
                    .take(12)
                    .map(|skill| {
                        format!(
                            "{} [{}:{} depth {}] ({} shadowed)",
                            skill.name,
                            skill.source_kind.directory_name(),
                            skill.precedence_rank,
                            skill.workspace_depth,
                            skill.shadowed_alternatives.len()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                let remaining = registry.effective.len().saturating_sub(12);
                if remaining > 0 {
                    entries.push_str(&format!("; {remaining} more (inspect with /skills <name>)"));
                }
                entries
            }
        } else if let Some(skill) = registry.get(&query) {
            format!(
                "{} — {}; source: {} (rank {}, depth {}); {} resources; shadowed: {}",
                skill.name,
                skill.description,
                skill.source_kind.directory_name(),
                skill.precedence_rank,
                skill.workspace_depth,
                skill.resources.len(),
                skill
                    .shadowed_alternatives
                    .iter()
                    .map(|alternative| alternative.source_kind.directory_name())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        } else {
            format!("No effective skill named '{query}'")
        };
        format!("{result}; {skipped} invalid candidates. Use /reload to refresh.")
    });
    if let Some(report) = report {
        let bounded: String = report.chars().take(2048).collect();
        app.messages_state.toasts.info(&bounded);
    } else {
        app.messages_state
            .toasts
            .warning("No active asset snapshot is available");
    }
    close_command_mode(app);
}
