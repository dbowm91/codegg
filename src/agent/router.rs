use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TaskComplexity {
    #[default]
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
            TaskComplexity::Medium => self.medium_model.clone(),
            TaskComplexity::Complex => self.complex_model.clone(),
        }
    }

    pub fn route_from_envelope(&self, envelope: &TaskEnvelope) -> Option<String> {
        if !self.enabled {
            return None;
        }
        match envelope.complexity {
            TaskComplexity::Simple => self.simple_model.clone(),
            TaskComplexity::Medium => self.medium_model.clone(),
            TaskComplexity::Complex => self.complex_model.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MutationRisk {
    #[default]
    ReadOnly,
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TaskBreadth {
    SingleFile,
    MultiFile,
    WholeRepo,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskEnvelope {
    pub complexity: TaskComplexity,
    pub requires_repo_inspection: bool,
    pub mutation_risk: MutationRisk,
    pub breadth: TaskBreadth,
    pub needs_planning: bool,
    pub needs_tests: bool,
    pub security_sensitive: bool,
    pub likely_long_context: bool,
    pub requested_subagent: Option<String>,
}

pub fn classify_envelope(
    prompt: &str,
    active_agent: Option<&str>,
    _tools: Option<&[String]>,
) -> TaskEnvelope {
    let p = prompt.to_lowercase();

    let mutation_risk = if (p.contains("read ") || p.ends_with("read"))
        || p.contains("show ")
        || p.contains("list ")
        || p.contains("find ")
        || p.contains("view ")
        || p.contains("display ")
        || p.contains("grep ")
        || p.contains("search ")
        || p.contains("explain")
        || p.contains("describe")
    {
        MutationRisk::ReadOnly
    } else if p.contains("fix typo")
        || p.contains("update comment")
        || p.contains("rename")
        || p.contains("format")
    {
        MutationRisk::Low
    } else if p.contains("implement")
        || p.contains("fix")
        || p.contains("modify")
        || p.contains("refactor")
        || p.contains("write file")
        || p.contains("add feature")
        || p.contains("create")
        || p.contains("edit")
        || p.contains("change")
        || p.contains("update")
    {
        MutationRisk::High
    } else {
        MutationRisk::Medium
    };

    let breadth = if p.contains("repo")
        || p.contains("codebase")
        || p.contains("architecture")
        || p.contains("codebase")
        || p.contains("whole project")
        || p.contains("entire")
    {
        TaskBreadth::WholeRepo
    } else if p.contains("files")
        || p.contains("multiple")
        || p.contains("modules")
        || p.contains("across")
    {
        TaskBreadth::MultiFile
    } else if p.contains("file")
        || p.contains("single")
        || p.contains("this file")
        || p.contains(".rs")
        || p.contains(".py")
        || p.contains(".js")
        || p.contains(".ts")
        || p.contains(".go")
        || p.contains(".c")
        || p.contains(".cpp")
        || p.contains(".h")
        || p.contains(".java")
        || p.contains(".md")
        || p.contains("src/")
        || p.contains("readme")
        || p.contains("config")
    {
        TaskBreadth::SingleFile
    } else {
        TaskBreadth::Unknown
    };

    let needs_tests = p.contains("test")
        || p.contains("failing")
        || p.contains("regression")
        || p.contains("bug")
        || p.contains("broken");

    let security_sensitive = p.contains("security")
        || p.contains("vulnerability")
        || p.contains("sandbox")
        || p.contains("permission")
        || p.contains("cve")
        || p.contains("injection")
        || p.contains("xss")
        || p.contains("exploit");

    let needs_planning = p.contains("plan")
        || p.contains("architect")
        || p.contains("design")
        || p.contains("review harness")
        || p.contains("strategy");

    let likely_long_context = p.contains("architecture")
        || p.contains("codebase")
        || p.contains("repo")
        || p.contains("review")
        || p.contains("understand")
        || p.contains("analyze");

    let requires_repo_inspection = p.contains("read")
        || p.contains("show")
        || p.contains("list")
        || p.contains("find")
        || p.contains("grep")
        || p.contains("search")
        || p.contains("explore")
        || p.contains("review")
        || p.contains("analyze")
        || p.contains("architecture");

    let complexity = if security_sensitive
        || (likely_long_context && needs_planning)
        || p.contains("architecture review")
        || p.contains("codebase")
    {
        TaskComplexity::Complex
    } else if mutation_risk == MutationRisk::High
        || needs_tests
        || breadth == TaskBreadth::MultiFile
        || breadth == TaskBreadth::WholeRepo
    {
        TaskComplexity::Medium
    } else if (mutation_risk == MutationRisk::ReadOnly || mutation_risk == MutationRisk::Low)
        && breadth == TaskBreadth::SingleFile
    {
        TaskComplexity::Simple
    } else {
        TaskComplexity::Medium
    };

    let requested_subagent = active_agent.filter(|a| *a != "build");

    TaskEnvelope {
        complexity,
        requires_repo_inspection,
        mutation_risk,
        breadth,
        needs_planning,
        needs_tests,
        security_sensitive,
        likely_long_context,
        requested_subagent: requested_subagent.map(String::from),
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

    #[test]
    fn test_classify_envelope_read_only() {
        let env = classify_envelope("show me src/main.rs", None, None);
        assert_eq!(env.mutation_risk, MutationRisk::ReadOnly);
        assert_eq!(env.complexity, TaskComplexity::Simple);
    }

    #[test]
    fn test_classify_envelope_fix_typo() {
        let env = classify_envelope("fix typo in README", None, None);
        assert_eq!(env.mutation_risk, MutationRisk::Low);
        assert_eq!(env.breadth, TaskBreadth::SingleFile);
        assert_eq!(env.complexity, TaskComplexity::Simple);
    }

    #[test]
    fn test_classify_envelope_read_only_view() {
        let env = classify_envelope("view the contents of src/main.rs", None, None);
        assert_eq!(env.mutation_risk, MutationRisk::ReadOnly);
        assert_eq!(env.complexity, TaskComplexity::Simple);
    }

    #[test]
    fn test_classify_envelope_architecture_review() {
        let env = classify_envelope("review the architecture of the coding harness", None, None);
        assert_eq!(env.breadth, TaskBreadth::WholeRepo);
        assert!(env.likely_long_context);
        assert_eq!(env.complexity, TaskComplexity::Complex);
    }

    #[test]
    fn test_classify_envelope_failing_tests() {
        let env = classify_envelope("investigate failing async cancellation tests", None, None);
        assert!(env.needs_tests);
        assert_eq!(env.mutation_risk, MutationRisk::Medium);
    }

    #[test]
    fn test_classify_envelope_security() {
        let env = classify_envelope("look for prompt injection/security issues", None, None);
        assert!(env.security_sensitive);
        assert_eq!(env.complexity, TaskComplexity::Complex);
    }

    #[test]
    fn test_classify_envelope_subagent() {
        let env = classify_envelope("explore the codebase", Some("explore"), None);
        assert_eq!(env.requested_subagent.as_deref(), Some("explore"));
    }

    #[test]
    fn test_route_from_envelope() {
        let router = ModelRouter::new(
            true,
            Some("small".to_string()),
            Some("medium".to_string()),
            Some("large".to_string()),
        );
        let env = TaskEnvelope {
            complexity: TaskComplexity::Simple,
            ..Default::default()
        };
        assert_eq!(router.route_from_envelope(&env), Some("small".to_string()));
    }
}
