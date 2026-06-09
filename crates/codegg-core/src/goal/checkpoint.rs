use crate::error::AppError;
use crate::goal::model::Goal;
use std::path::{Path, PathBuf};

pub fn goal_artifact_dir(project_dir: impl AsRef<Path>) -> PathBuf {
    project_dir.as_ref().join(".codegg").join("goals")
}

pub async fn create_checkpoint_file(
    project_dir: impl AsRef<Path>,
    goal: &Goal,
    plan_excerpt: Option<&str>,
) -> Result<PathBuf, AppError> {
    let dir = goal_artifact_dir(project_dir);
    tokio::fs::create_dir_all(&dir).await?;
    let path = dir.join(format!("{}.checkpoint.md", goal.id));

    let plan_source = goal.plan_path.as_deref().unwrap_or("none");
    let remaining = if goal.completion_criteria.is_empty() {
        "Unspecified. Derive from objective and plan.".to_string()
    } else {
        goal.completion_criteria
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{}. {}", i + 1, c))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let excerpt = plan_excerpt.unwrap_or("No plan file provided.");

    let content = format!(
        r#"# Goal Checkpoint

## Objective

{}

## Plan Source

{}

## Current Phase

Not started.

## Completed

None yet.

## In Progress

None yet.

## Remaining

{}

## Decisions

None recorded.

## Known Issues

None recorded.

## Open Questions

None recorded.

## Next Action

Inspect the repository and identify the first concrete implementation step.

## Plan Excerpt

{}
"#,
        goal.objective, plan_source, remaining, excerpt
    );

    tokio::fs::write(&path, content).await?;
    Ok(path)
}

pub async fn read_checkpoint_excerpt(
    path: impl AsRef<Path>,
    max_chars: usize,
) -> Result<Option<String>, AppError> {
    match tokio::fs::read_to_string(path.as_ref()).await {
        Ok(content) => {
            let truncated = if content.len() > max_chars {
                content[..max_chars].to_string()
            } else {
                content
            };
            Ok(Some(truncated))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub async fn append_checkpoint_update(
    path: impl AsRef<Path>,
    update: &crate::goal::model::GoalProgressUpdate,
) -> Result<(), AppError> {
    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    let mut block = format!("\n---\n\n### Update: {}\n\n", timestamp);

    if let Some(ref phase) = update.current_phase {
        block.push_str(&format!("**Phase:** {}\n\n", phase));
    }
    if let Some(ref summary) = update.progress_summary {
        block.push_str(&format!("**Progress:** {}\n\n", summary));
    }
    if let Some(ref next) = update.next_action {
        block.push_str(&format!("**Next action:** {}\n\n", next));
    }
    if !update.completed_items.is_empty() {
        block.push_str("**Completed:**\n");
        for item in &update.completed_items {
            block.push_str(&format!("- {}\n", item));
        }
        block.push('\n');
    }
    if !update.remaining_items.is_empty() {
        block.push_str("**Remaining:**\n");
        for item in &update.remaining_items {
            block.push_str(&format!("- {}\n", item));
        }
        block.push('\n');
    }
    if !update.open_questions.is_empty() {
        block.push_str("**Open questions:**\n");
        for q in &update.open_questions {
            block.push_str(&format!("- {}\n", q));
        }
        block.push('\n');
    }

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    use tokio::io::AsyncWriteExt;
    file.write_all(block.as_bytes()).await?;
    Ok(())
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
            plan_path: None,
            checkpoint_path: None,
            current_phase: None,
            progress_summary: String::new(),
            next_action: None,
            completion_criteria: vec!["Criteria 1".to_string()],
            open_questions: vec![],
            budget: GoalBudget::default(),
            usage: GoalUsage::default(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            started_at: None,
            completed_at: None,
        }
    }

    #[tokio::test]
    async fn test_create_checkpoint_file() {
        let dir = tempfile::tempdir().unwrap();
        let goal = test_goal();
        let path = create_checkpoint_file(dir.path(), &goal, None)
            .await
            .unwrap();
        assert!(path.exists());
        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("Implement something cool"));
        assert!(content.contains("No plan file provided."));
    }

    #[tokio::test]
    async fn test_read_checkpoint_excerpt() {
        let dir = tempfile::tempdir().unwrap();
        let goal = test_goal();
        let path = create_checkpoint_file(dir.path(), &goal, Some("Plan excerpt here"))
            .await
            .unwrap();
        let excerpt = read_checkpoint_excerpt(&path, 4000).await.unwrap();
        assert!(excerpt.is_some());
        assert!(excerpt.unwrap().contains("Plan excerpt here"));
    }

    #[tokio::test]
    async fn test_read_checkpoint_excerpt_max_chars() {
        let dir = tempfile::tempdir().unwrap();
        let goal = test_goal();
        let path = create_checkpoint_file(dir.path(), &goal, Some(&"x".repeat(5000)))
            .await
            .unwrap();
        let excerpt = read_checkpoint_excerpt(&path, 100).await.unwrap();
        assert!(excerpt.unwrap().len() <= 100);
    }

    #[tokio::test]
    async fn test_append_checkpoint_update() {
        let dir = tempfile::tempdir().unwrap();
        let goal = test_goal();
        let path = create_checkpoint_file(dir.path(), &goal, None)
            .await
            .unwrap();
        let update = GoalProgressUpdate {
            current_phase: Some("Phase 1".to_string()),
            progress_summary: Some("Started work".to_string()),
            next_action: Some("Run tests".to_string()),
            completed_items: vec!["Setup".to_string()],
            remaining_items: vec!["Testing".to_string()],
            open_questions: vec![],
        };
        append_checkpoint_update(&path, &update).await.unwrap();
        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("Phase 1"));
        assert!(content.contains("Started work"));
        assert!(content.contains("Run tests"));
        assert!(content.contains("Setup"));
        assert!(content.contains("Testing"));
    }

    #[tokio::test]
    async fn test_read_checkpoint_not_found() {
        let result = read_checkpoint_excerpt("/nonexistent/path.md", 100)
            .await
            .unwrap();
        assert!(result.is_none());
    }
}
