use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskComplexity {
    Simple,
    Medium,
    Complex,
}

impl TaskComplexity {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskComplexity::Simple => "simple",
            TaskComplexity::Medium => "medium",
            TaskComplexity::Complex => "complex",
        }
    }
}

#[allow(dead_code)]
pub struct ModelRouter {
    enabled: bool,
    simple_model: Option<String>,
    medium_model: Option<String>,
    complex_model: Option<String>,
}

impl ModelRouter {
    pub fn new(
        enabled: bool,
        simple_model: Option<String>,
        medium_model: Option<String>,
        complex_model: Option<String>,
    ) -> Self {
        Self {
            enabled,
            simple_model,
            medium_model,
            complex_model,
        }
    }

    pub fn from_config(config: &crate::config::schema::Config) -> Self {
        let enabled = config.auto_route_models.unwrap_or(false);
        Self::new(
            enabled,
            config.small_model.clone(),
            config.medium_model.clone(),
            config.model.clone(),
        )
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn classify(&self, prompt: &str, tool_name: &str) -> TaskComplexity {
        if self.classify_by_tool(tool_name) == TaskComplexity::Complex {
            return TaskComplexity::Complex;
        }

        let complexity = self.classify_by_content(prompt);
        tracing::debug!(
            "Task classified as {:?} (tool: {}, prompt_len: {})",
            complexity,
            tool_name,
            prompt.len()
        );
        complexity
    }

    fn classify_by_tool(&self, tool_name: &str) -> TaskComplexity {
        match tool_name {
            "read" | "cat" | "ls" | "glob" | "list" => TaskComplexity::Simple,
            "edit" | "write" | "grep" | "search" => TaskComplexity::Medium,
            "debug" | "plan" | "review" | "architect" | "analyze" => TaskComplexity::Complex,
            _ => TaskComplexity::Medium,
        }
    }

    fn classify_by_content(&self, prompt: &str) -> TaskComplexity {
        let prompt_lower = prompt.to_lowercase();

        let complex_keywords = [
            "debug",
            "analyze",
            "plan",
            "architect",
            "review",
            "design",
            "optimize",
            "refactor",
            "investigate",
            "troubleshoot",
            "complex",
            "difficult",
            "understand the codebase",
            "architecture",
            "performance issue",
        ];
        let medium_keywords = [
            "edit",
            "write",
            "create",
            "modify",
            "change",
            "update",
            "add",
            "fix",
            "implement",
            "function",
            "feature",
            "improve",
        ];
        let simple_keywords = [
            "read", "show", "list", "find", "get", "look", "view", "display", "what is", "cat",
            "ls", "glob", "grep", "search",
        ];

        let complex_count = complex_keywords
            .iter()
            .filter(|kw| prompt_lower.contains(*kw))
            .count();
        if complex_count >= 2
            || prompt_lower.contains("debug this")
            || prompt_lower.contains("analyze the")
        {
            return TaskComplexity::Complex;
        }

        if complex_count == 1 {
            return TaskComplexity::Medium;
        }

        let medium_count = medium_keywords
            .iter()
            .filter(|kw| prompt_lower.contains(*kw))
            .count();
        if medium_count >= 2 {
            return TaskComplexity::Medium;
        }

        let simple_count = simple_keywords
            .iter()
            .filter(|kw| prompt_lower.contains(*kw))
            .count();
        if simple_count >= 2 || prompt.len() < 50 {
            return TaskComplexity::Simple;
        }

        TaskComplexity::Medium
    }

    pub fn route_model(&self, complexity: TaskComplexity) -> Option<String> {
        if !self.enabled {
            return None;
        }

        match complexity {
            TaskComplexity::Simple => self.simple_model.clone(),
            TaskComplexity::Medium => None,
            TaskComplexity::Complex => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_simple_tools() {
        let router = ModelRouter::new(false, None, None, None);
        assert_eq!(
            router.classify("show me the file", "read"),
            TaskComplexity::Simple
        );
        assert_eq!(router.classify("list files", "ls"), TaskComplexity::Simple);
        assert_eq!(
            router.classify("find pattern", "glob"),
            TaskComplexity::Simple
        );
    }

    #[test]
    fn test_classify_complex_tools() {
        let router = ModelRouter::new(false, None, None, None);
        assert_eq!(
            router.classify("debug this issue", "debug"),
            TaskComplexity::Complex
        );
        assert_eq!(
            router.classify("plan the architecture", "plan"),
            TaskComplexity::Complex
        );
    }

    #[test]
    fn test_classify_by_content() {
        let router = ModelRouter::new(false, None, None, None);
        assert_eq!(
            router.classify("read the file and show me what is inside", "read"),
            TaskComplexity::Simple
        );
        assert_eq!(
            router.classify("analyze the codebase architecture", "read"),
            TaskComplexity::Complex
        );
        assert_eq!(
            router.classify("edit the function to add new feature", "edit"),
            TaskComplexity::Medium
        );
    }

    #[test]
    fn test_disabled_router_returns_none() {
        let router = ModelRouter::new(false, Some("gpt-4o-mini".to_string()), None, None);
        assert!(router.route_model(TaskComplexity::Simple).is_none());
    }

    #[test]
    fn test_enabled_router_returns_model() {
        let router = ModelRouter::new(true, Some("gpt-4o-mini".to_string()), None, None);
        assert_eq!(
            router.route_model(TaskComplexity::Simple),
            Some("gpt-4o-mini".to_string())
        );
    }
}
