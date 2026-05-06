use crate::tui::components::completion_overlay::CompletionItem;

pub struct PromptState {
    pub prompt: crate::tui::components::prompt::PromptWidget,
    pub slash_completions: Vec<CompletionItem>,
    pub file_completions: Vec<CompletionItem>,
    pub agent_completions: Vec<CompletionItem>,
    pub completion_filter: String,
    pub show_completions: bool,
    pub completion_type: crate::tui::app::types::CompletionType,
    pub completion_sel: usize,
    pub stashed_prompts: Vec<String>,
    pub stash_pos: Option<usize>,
    pub pending_send: bool,
}
