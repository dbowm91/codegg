//! AppContext - shared state for components
//!
//! Provides access to application state without requiring components to hold direct references.

use crate::tui::theme::Theme;
use std::sync::Arc;

pub struct AppContext {
    pub theme: Arc<Theme>,
    pub session_id: Option<String>,
    pub session_title: Option<String>,
    pub agent_name: String,
    pub model_name: String,
    pub token_in: usize,
    pub token_out: usize,
    pub is_connected: bool,
}

impl AppContext {
    pub fn new(theme: Arc<Theme>) -> Self {
        Self {
            theme,
            session_id: None,
            session_title: None,
            agent_name: "build".to_string(),
            model_name: String::new(),
            token_in: 0,
            token_out: 0,
            is_connected: true,
        }
    }

    pub fn with_session(mut self, session_id: String, title: String) -> Self {
        self.session_id = Some(session_id);
        self.session_title = Some(title);
        self
    }

    pub fn with_agent_model(mut self, agent: String, model: String) -> Self {
        self.agent_name = agent;
        self.model_name = model;
        self
    }

    pub fn with_tokens(mut self, token_in: usize, token_out: usize) -> Self {
        self.token_in = token_in;
        self.token_out = token_out;
        self
    }
}
