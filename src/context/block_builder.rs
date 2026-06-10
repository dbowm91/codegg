use super::block::{ContextBlock, ContextBlockKind, Lossiness};
use super::tool_hash::tool_definitions_hash;
use crate::agent::context_frame::ContextFrame;
use crate::provider::ToolDefinition;

pub struct ContextBlockBuilder {
    session_id: String,
    model_id: String,
}

impl ContextBlockBuilder {
    pub fn new(session_id: &str, model_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            model_id: model_id.to_string(),
        }
    }

    pub fn build_system_prompt_block(&self, system_text: &str) -> ContextBlock {
        let source = format!("system:{}", self.model_id);
        ContextBlock::new(
            ContextBlockKind::SystemPrompt,
            &source,
            system_text.to_string(),
            100,
            true,
            Lossiness::Lossless,
        )
    }

    pub fn build_model_profile_block(&self, profile_text: &str) -> ContextBlock {
        let source = format!("profile:{}", self.model_id);
        ContextBlock::new(
            ContextBlockKind::ModelProfile,
            &source,
            profile_text.to_string(),
            90,
            true,
            Lossiness::Lossless,
        )
    }

    pub fn build_tool_definitions_block(&self, definitions: &[ToolDefinition]) -> ContextBlock {
        let hash = tool_definitions_hash(definitions);
        let source = format!("tools:{hash}");
        ContextBlock::new(
            ContextBlockKind::ToolDefinitions,
            &source,
            String::new(),
            80,
            true,
            Lossiness::Lossless,
        )
    }

    pub fn build_session_frame_block(&self, frame: &ContextFrame) -> Option<ContextBlock> {
        if frame.is_empty() {
            return None;
        }
        let source = format!("frame:session:{}", self.session_id);
        Some(ContextBlock::new(
            ContextBlockKind::SessionFrame,
            &source,
            frame.to_control_text(),
            60,
            false,
            Lossiness::ProjectedRecoverable,
        ))
    }

    pub fn build_goal_context_block(&self, goal_text: &str) -> Option<ContextBlock> {
        if goal_text.is_empty() {
            return None;
        }
        Some(ContextBlock::new(
            ContextBlockKind::GoalContext,
            &format!("goal:{}", self.session_id),
            goal_text.to_string(),
            70,
            false,
            Lossiness::ProjectedRecoverable,
        ))
    }

    pub fn build_memory_context_block(&self, memory_text: &str) -> Option<ContextBlock> {
        if memory_text.is_empty() {
            return None;
        }
        Some(ContextBlock::new(
            ContextBlockKind::MemoryContext,
            &format!("memory:{}", self.session_id),
            memory_text.to_string(),
            65,
            false,
            Lossiness::ProjectedRecoverable,
        ))
    }

    pub fn build_todo_reminder_block(&self, todo_text: &str) -> Option<ContextBlock> {
        if todo_text.is_empty() {
            return None;
        }
        Some(ContextBlock::new(
            ContextBlockKind::TodoReminder,
            &format!("todo:{}", self.session_id),
            todo_text.to_string(),
            40,
            false,
            Lossiness::SummaryOnly,
        ))
    }

    pub fn build_control_instruction_block(&self, instruction_text: &str) -> ContextBlock {
        ContextBlock::new(
            ContextBlockKind::ControlInstruction,
            &format!("control:{}", self.session_id),
            instruction_text.to_string(),
            30,
            false,
            Lossiness::SummaryOnly,
        )
    }

    pub fn build_artifact_summary_block(
        &self,
        summary: &str,
        artifact_count: usize,
    ) -> Option<ContextBlock> {
        if summary.is_empty() {
            return None;
        }
        Some(ContextBlock::new(
            ContextBlockKind::ArtifactSummary,
            &format!("artifacts:{}:{}", self.session_id, artifact_count),
            summary.to_string(),
            20,
            false,
            Lossiness::SummaryOnly,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn build_all(
        &self,
        system_text: &str,
        profile_text: &str,
        definitions: &[ToolDefinition],
        frame: &ContextFrame,
        goal_text: Option<&str>,
        memory_text: Option<&str>,
        todo_text: Option<&str>,
        control_text: Option<&str>,
        artifact_summary: Option<&str>,
        artifact_count: usize,
    ) -> Vec<ContextBlock> {
        let mut blocks = Vec::new();

        blocks.push(self.build_system_prompt_block(system_text));
        blocks.push(self.build_model_profile_block(profile_text));
        blocks.push(self.build_tool_definitions_block(definitions));

        if let Some(block) = self.build_session_frame_block(frame) {
            blocks.push(block);
        }
        if let Some(text) = goal_text {
            if let Some(block) = self.build_goal_context_block(text) {
                blocks.push(block);
            }
        }
        if let Some(text) = memory_text {
            if let Some(block) = self.build_memory_context_block(text) {
                blocks.push(block);
            }
        }
        if let Some(text) = todo_text {
            if let Some(block) = self.build_todo_reminder_block(text) {
                blocks.push(block);
            }
        }
        if let Some(text) = control_text {
            blocks.push(self.build_control_instruction_block(text));
        }
        if let Some(text) = artifact_summary {
            if let Some(block) = self.build_artifact_summary_block(text, artifact_count) {
                blocks.push(block);
            }
        }

        blocks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::context_frame::ContextFrame;
    use crate::context::block::CacheClass;
    use serde_json::json;

    fn sample_tool_def(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            description: format!("Tool {name}"),
            parameters: json!({"type": "object"}),
            defer_loading: None,
        }
    }

    #[test]
    fn identical_state_produces_identical_blocks() {
        let builder = ContextBlockBuilder::new("sess1", "claude-3");
        let defs = vec![sample_tool_def("bash")];
        let frame = ContextFrame {
            touched_files: vec!["a.rs".into()],
            ..Default::default()
        };

        let a = builder.build_all(
            "sys", "prof", &defs, &frame, None, None, None, None, None, 0,
        );
        let b = builder.build_all(
            "sys", "prof", &defs, &frame, None, None, None, None, None, 0,
        );

        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(x.id, y.id, "block id mismatch for kind {:?}", x.kind);
            assert_eq!(
                x.content_hash, y.content_hash,
                "content hash mismatch for kind {:?}",
                x.kind
            );
        }
    }

    #[test]
    fn changing_touched_files_changes_frame_hash() {
        let builder = ContextBlockBuilder::new("sess1", "claude-3");

        let frame_a = ContextFrame {
            touched_files: vec!["a.rs".into()],
            ..Default::default()
        };
        let frame_b = ContextFrame {
            touched_files: vec!["b.rs".into()],
            ..Default::default()
        };

        let block_a = builder.build_session_frame_block(&frame_a).unwrap();
        let block_b = builder.build_session_frame_block(&frame_b).unwrap();

        assert_eq!(
            block_a.id, block_b.id,
            "id should be same (source unchanged)"
        );
        assert_ne!(
            block_a.content_hash, block_b.content_hash,
            "content hash should differ"
        );
    }

    #[test]
    fn empty_frame_returns_none() {
        let builder = ContextBlockBuilder::new("sess1", "claude-3");
        let frame = ContextFrame::default();
        assert!(builder.build_session_frame_block(&frame).is_none());
    }

    #[test]
    fn cache_class_tiers() {
        let builder = ContextBlockBuilder::new("s", "m");

        let sys = builder.build_system_prompt_block("s");
        assert_eq!(sys.kind.tier(), CacheClass::StablePrefix);

        let prof = builder.build_model_profile_block("p");
        assert_eq!(prof.kind.tier(), CacheClass::StablePrefix);

        let tools = builder.build_tool_definitions_block(&[]);
        assert_eq!(tools.kind.tier(), CacheClass::SlowChanging);

        let goal = builder.build_goal_context_block("g").unwrap();
        assert_eq!(goal.kind.tier(), CacheClass::SlowChanging);

        let mem = builder.build_memory_context_block("m").unwrap();
        assert_eq!(mem.kind.tier(), CacheClass::SlowChanging);

        let frame = ContextFrame {
            touched_files: vec!["x.rs".into()],
            ..Default::default()
        };
        let sf = builder.build_session_frame_block(&frame).unwrap();
        assert_eq!(sf.kind.tier(), CacheClass::Volatile);

        let todo = builder.build_todo_reminder_block("t").unwrap();
        assert_eq!(todo.kind.tier(), CacheClass::Volatile);

        let art = builder.build_artifact_summary_block("sum", 1).unwrap();
        assert_eq!(art.kind.tier(), CacheClass::Volatile);

        let ctrl = builder.build_control_instruction_block("c");
        assert_eq!(ctrl.kind.tier(), CacheClass::NeverCache);
    }

    #[test]
    fn empty_optionals_return_none() {
        let builder = ContextBlockBuilder::new("s", "m");
        assert!(builder.build_goal_context_block("").is_none());
        assert!(builder.build_memory_context_block("").is_none());
        assert!(builder.build_todo_reminder_block("").is_none());
        assert!(builder.build_artifact_summary_block("", 0).is_none());
    }

    #[test]
    fn build_all_collects_non_none() {
        let builder = ContextBlockBuilder::new("s", "m");
        let frame = ContextFrame::default();
        let blocks = builder.build_all("sys", "prof", &[], &frame, None, None, None, None, None, 0);
        // system, profile, tool_definitions = 3 required; frame=None, goal=None, etc.
        assert_eq!(blocks.len(), 3);
    }

    #[test]
    fn build_all_with_all_optionals() {
        let builder = ContextBlockBuilder::new("s", "m");
        let frame = ContextFrame {
            current_task: Some("do stuff".into()),
            ..Default::default()
        };
        let blocks = builder.build_all(
            "sys",
            "prof",
            &[],
            &frame,
            Some("fix bug"),
            Some("learned X"),
            Some("todo: Y"),
            Some("be careful"),
            Some("ran 5 tools"),
            5,
        );
        // 3 required + frame + goal + memory + todo + control + artifact = 9
        assert_eq!(blocks.len(), 9);
    }

    #[test]
    fn priorities_match_expected_values() {
        let builder = ContextBlockBuilder::new("s", "m");

        assert_eq!(builder.build_system_prompt_block("s").priority, 100);
        assert_eq!(builder.build_model_profile_block("p").priority, 90);
        assert_eq!(builder.build_tool_definitions_block(&[]).priority, 80);
        assert_eq!(builder.build_goal_context_block("g").unwrap().priority, 70);
        assert_eq!(
            builder.build_memory_context_block("m").unwrap().priority,
            65
        );
        let frame = ContextFrame {
            current_task: Some("t".into()),
            ..Default::default()
        };
        assert_eq!(
            builder.build_session_frame_block(&frame).unwrap().priority,
            60
        );
        assert_eq!(builder.build_todo_reminder_block("t").unwrap().priority, 40);
        assert_eq!(builder.build_control_instruction_block("c").priority, 30);
        assert_eq!(
            builder
                .build_artifact_summary_block("a", 1)
                .unwrap()
                .priority,
            20
        );
    }
}
