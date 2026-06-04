use crate::goal::model::Goal;

const CHECKPOINT_EXCERPT_LIMIT: usize = 4000;

pub fn render_goal_context(goal: &Goal, checkpoint_excerpt: Option<&str>) -> String {
    let mut out = String::with_capacity(2048);
    out.push_str("## Active Codegg Goal\n\n");
    out.push_str("Objective:\n");
    out.push_str(&goal.objective);
    out.push_str("\n\n");

    out.push_str(&format!("Status: {}\n", goal.status_as_str()));

    if let Some(ref phase) = goal.current_phase {
        out.push_str(&format!("Current phase: {}\n", phase));
    }

    if !goal.progress_summary.is_empty() {
        out.push_str(&format!("Progress:\n{}\n", goal.progress_summary));
    }

    if let Some(ref next) = goal.next_action {
        out.push_str(&format!("Next action:\n{}\n", next));
    }

    if !goal.completion_criteria.is_empty() {
        out.push_str("\nCompletion criteria:\n");
        for (i, c) in goal.completion_criteria.iter().enumerate() {
            out.push_str(&format!("{}. {}\n", i + 1, c));
        }
    }

    if !goal.open_questions.is_empty() {
        out.push_str("\nOpen questions:\n");
        for q in &goal.open_questions {
            out.push_str(&format!("- {}\n", q));
        }
    }

    if let Some(excerpt) = checkpoint_excerpt {
        let truncated = if excerpt.len() > CHECKPOINT_EXCERPT_LIMIT {
            &excerpt[..CHECKPOINT_EXCERPT_LIMIT]
        } else {
            excerpt
        };
        out.push_str(&format!("\nCheckpoint excerpt:\n{}\n", truncated));
    }

    out
}

pub fn render_goal_status(goal: &Goal) -> String {
    let mut out = String::with_capacity(1024);
    out.push_str(&format!("Goal: {}\n", goal.title));
    out.push_str(&format!("Status: {}\n", goal.status_as_str()));
    out.push_str(&format!("Objective: {}\n", goal.objective));

    if let Some(ref phase) = goal.current_phase {
        out.push_str(&format!("Phase: {}\n", phase));
    }
    if !goal.progress_summary.is_empty() {
        out.push_str(&format!("Progress: {}\n", goal.progress_summary));
    }
    if let Some(ref next) = goal.next_action {
        out.push_str(&format!("Next: {}\n", next));
    }

    out.push_str(&format!(
        "Usage: {} turns, {} input tokens, {} output tokens, {} tool calls\n",
        goal.usage.turns_used,
        goal.usage.input_tokens,
        goal.usage.output_tokens,
        goal.usage.tool_calls,
    ));

    if let Some(ref path) = goal.checkpoint_path {
        out.push_str(&format!("Checkpoint: {}\n", path));
    }
    if let Some(ref path) = goal.plan_path {
        out.push_str(&format!("Plan: {}\n", path));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::goal::model::*;
    use chrono::Utc;

    fn test_goal() -> Goal {
        Goal {
            id: "test-goal-id".to_string(),
            session_id: "sess1".to_string(),
            project_id: "/tmp/test".to_string(),
            title: "Test Goal".to_string(),
            objective: "Implement something cool".to_string(),
            status: GoalStatus::Active,
            plan_path: Some("plans/test.md".to_string()),
            checkpoint_path: Some(".codegg/goals/test.checkpoint.md".to_string()),
            current_phase: Some("Planning".to_string()),
            progress_summary: "Gathering requirements".to_string(),
            next_action: Some("Read source files".to_string()),
            completion_criteria: vec!["All tests pass".to_string(), "Code reviewed".to_string()],
            open_questions: vec!["Should we use approach A?".to_string()],
            budget: GoalBudget::default(),
            usage: GoalUsage {
                turns_used: 5,
                input_tokens: 10000,
                output_tokens: 3000,
                tool_calls: 12,
                wallclock_secs: 600,
            },
            created_at: Utc::now(),
            updated_at: Utc::now(),
            started_at: None,
            completed_at: None,
        }
    }

    #[test]
    fn test_render_goal_context_includes_all_fields() {
        let goal = test_goal();
        let ctx = render_goal_context(&goal, Some("Plan excerpt here"));
        assert!(ctx.contains("Implement something cool"));
        assert!(ctx.contains("active"));
        assert!(ctx.contains("Planning"));
        assert!(ctx.contains("Gathering requirements"));
        assert!(ctx.contains("Read source files"));
        assert!(ctx.contains("1. All tests pass"));
        assert!(ctx.contains("2. Code reviewed"));
        assert!(ctx.contains("Should we use approach A?"));
        assert!(ctx.contains("Plan excerpt here"));
    }

    #[test]
    fn test_render_goal_context_caps_checkpoint_excerpt() {
        let goal = test_goal();
        let long_excerpt = "x".repeat(5000);
        let ctx = render_goal_context(&goal, Some(&long_excerpt));
        // Should contain the excerpt but truncated
        let checkpoint_start = ctx.find("Checkpoint excerpt:").unwrap();
        let excerpt_section = &ctx[checkpoint_start..];
        // The excerpt section should be shorter than the full long excerpt
        assert!(excerpt_section.len() < 5000);
    }

    #[test]
    fn test_render_goal_context_handles_empty_optionals() {
        let goal = Goal {
            current_phase: None,
            progress_summary: String::new(),
            next_action: None,
            completion_criteria: vec![],
            open_questions: vec![],
            ..test_goal()
        };
        let ctx = render_goal_context(&goal, None);
        assert!(ctx.contains("Implement something cool"));
        assert!(!ctx.contains("Current phase:"));
        assert!(!ctx.contains("Progress:"));
        assert!(!ctx.contains("Next action:"));
        assert!(!ctx.contains("Completion criteria:"));
        assert!(!ctx.contains("Open questions:"));
        assert!(!ctx.contains("Checkpoint excerpt:"));
    }

    #[test]
    fn test_render_goal_status() {
        let goal = test_goal();
        let status = render_goal_status(&goal);
        assert!(status.contains("Test Goal"));
        assert!(status.contains("active"));
        assert!(status.contains("5 turns"));
        assert!(status.contains("10000 input tokens"));
        assert!(status.contains("plans/test.md"));
    }

    #[test]
    fn test_status_as_str() {
        let goal = test_goal();
        assert_eq!(goal.status_as_str(), "active");
    }
}
