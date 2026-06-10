use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Debug, Default)]
pub struct ContextLedgerState {
    pub touched_files: Vec<String>,
    pub commands_run: VecDeque<String>,
    pub test_results: Vec<String>,
    pub unresolved_errors: Vec<String>,
    pub artifact_handles: Vec<String>,
}

impl ContextLedgerState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_projection(&mut self, proj: &crate::context::ToolOutputProjection, handle: &str) {
        for file in &proj.touched_files {
            if !self.touched_files.contains(file) {
                self.touched_files.push(file.clone());
            }
        }
        if self.touched_files.len() > 20 {
            self.touched_files = self.touched_files.split_off(self.touched_files.len() - 20);
        }

        for cmd in &proj.commands_run {
            self.commands_run.push_back(cmd.clone());
        }
        while self.commands_run.len() > 10 {
            self.commands_run.pop_front();
        }

        for result in &proj.test_results {
            if !self.test_results.contains(result) {
                self.test_results.push(result.clone());
            }
        }
        if self.test_results.len() > 10 {
            self.test_results = self.test_results.split_off(self.test_results.len() - 10);
        }

        for error in &proj.unresolved_errors {
            if !self.unresolved_errors.contains(error) {
                self.unresolved_errors.push(error.clone());
            }
        }
        if self.unresolved_errors.len() > 10 {
            self.unresolved_errors = self
                .unresolved_errors
                .split_off(self.unresolved_errors.len() - 10);
        }

        if !handle.is_empty() && !self.artifact_handles.contains(&handle.to_string()) {
            self.artifact_handles.push(handle.to_string());
        }
    }

    pub fn to_context_frame(&self) -> ContextFrame {
        ContextFrame {
            touched_files: self.touched_files.clone(),
            commands_run: self.commands_run.iter().cloned().collect(),
            test_results: self.test_results.clone(),
            unresolved_errors: self.unresolved_errors.clone(),
            ..Default::default()
        }
    }
}

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
