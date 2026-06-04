use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextFrame {
    pub user_goal: Option<String>,
    pub current_task: Option<String>,
    pub constraints: Vec<String>,
    pub decisions: Vec<String>,
    pub touched_files: Vec<String>,
    pub commands_run: Vec<String>,
    pub test_results: Vec<String>,
    pub unresolved_errors: Vec<String>,
    pub security_findings: Vec<String>,
    pub next_steps: Vec<String>,
}

impl ContextFrame {
    pub fn to_control_text(&self) -> String {
        let mut lines = vec!["Current session context:".to_string()];

        if let Some(ref goal) = self.user_goal {
            lines.push(format!("- Goal: {}", goal));
        }
        if let Some(ref task) = self.current_task {
            lines.push(format!("- Active task: {}", task));
        }
        if !self.constraints.is_empty() {
            lines.push(format!("- Constraints: {}", self.constraints.join("; ")));
        }
        if !self.decisions.is_empty() {
            lines.push(format!("- Decisions: {}", self.decisions.join("; ")));
        }
        if !self.touched_files.is_empty() {
            lines.push(format!(
                "- Touched files: {}",
                self.touched_files.join(", ")
            ));
        }
        if !self.commands_run.is_empty() {
            lines.push(format!(
                "- Commands/tests: {}",
                self.commands_run.join(", ")
            ));
        }
        if !self.test_results.is_empty() {
            lines.push(format!("- Test results: {}", self.test_results.join("; ")));
        }
        if !self.unresolved_errors.is_empty() {
            lines.push(format!(
                "- Open issues: {}",
                self.unresolved_errors.join("; ")
            ));
        }
        if !self.security_findings.is_empty() {
            lines.push(format!(
                "- Security findings: {}",
                self.security_findings.join("; ")
            ));
        }
        if !self.next_steps.is_empty() {
            lines.push(format!("- Next steps: {}", self.next_steps.join("; ")));
        }

        lines.join("\n")
    }

    pub fn to_compaction_control_text(&self) -> String {
        let mut lines = vec!["[codegg compacted session state]".to_string()];

        if let Some(ref goal) = self.user_goal {
            lines.push(format!("- Goal: {}", goal));
        }
        if let Some(ref task) = self.current_task {
            lines.push(format!("- Active task: {}", task));
        }
        if !self.constraints.is_empty() {
            lines.push(format!("- Constraints: {}", self.constraints.join("; ")));
        }
        if !self.decisions.is_empty() {
            lines.push(format!("- Decisions: {}", self.decisions.join("; ")));
        }
        if !self.touched_files.is_empty() {
            lines.push(format!(
                "- Touched files: {}",
                self.touched_files.join(", ")
            ));
        }
        if !self.commands_run.is_empty() {
            lines.push(format!(
                "- Commands/tests: {}",
                self.commands_run.join(", ")
            ));
        }
        if !self.test_results.is_empty() {
            lines.push(format!("- Test results: {}", self.test_results.join("; ")));
        }
        if !self.unresolved_errors.is_empty() {
            lines.push(format!(
                "- Open issues: {}",
                self.unresolved_errors.join("; ")
            ));
        }
        if !self.security_findings.is_empty() {
            lines.push(format!(
                "- Security findings: {}",
                self.security_findings.join("; ")
            ));
        }
        if !self.next_steps.is_empty() {
            lines.push(format!("- Next steps: {}", self.next_steps.join("; ")));
        }

        lines.join("\n")
    }

    pub fn is_empty(&self) -> bool {
        self.user_goal.is_none()
            && self.current_task.is_none()
            && self.constraints.is_empty()
            && self.decisions.is_empty()
            && self.touched_files.is_empty()
            && self.commands_run.is_empty()
            && self.test_results.is_empty()
            && self.unresolved_errors.is_empty()
            && self.security_findings.is_empty()
            && self.next_steps.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_frame() {
        let frame = ContextFrame::default();
        assert!(frame.is_empty());
        let text = frame.to_control_text();
        assert_eq!(text, "Current session context:");
    }

    #[test]
    fn test_frame_with_goal_and_task() {
        let frame = ContextFrame {
            user_goal: Some("Fix the failing test".to_string()),
            current_task: Some("Investigate test_output".to_string()),
            ..Default::default()
        };
        let text = frame.to_control_text();
        assert!(text.contains("Goal: Fix the failing test"));
        assert!(text.contains("Active task: Investigate test_output"));
        assert!(!frame.is_empty());
    }

    #[test]
    fn test_frame_with_files_and_commands() {
        let frame = ContextFrame {
            touched_files: vec!["src/main.rs".to_string(), "src/lib.rs".to_string()],
            commands_run: vec!["cargo test".to_string()],
            test_results: vec!["2 passed, 0 failed".to_string()],
            ..Default::default()
        };
        let text = frame.to_control_text();
        assert!(text.contains("Touched files: src/main.rs, src/lib.rs"));
        assert!(text.contains("Commands/tests: cargo test"));
        assert!(text.contains("Test results: 2 passed, 0 failed"));
    }

    #[test]
    fn test_frame_serialization_roundtrip() {
        let frame = ContextFrame {
            user_goal: Some("Build feature X".to_string()),
            touched_files: vec!["file.rs".to_string()],
            ..Default::default()
        };
        let json = serde_json::to_string(&frame).unwrap();
        let deserialized: ContextFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.user_goal, frame.user_goal);
        assert_eq!(deserialized.touched_files, frame.touched_files);
    }
}
