use std::collections::HashSet;

use crate::context::artifact::estimate_tokens;
use crate::context::effective_cost::{EffectiveCostAction, EffectiveCostAnalysis};
use codegg_config::schema::{ContextPolicyConfig, VolatileTailPolicyMode};
use codegg_providers::{ContentPart, Message};

// ── Phase 2: Candidate analysis ────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VolatileTailCandidateKind {
    ToolResult,
    AssistantNarration,
    UserMessage,
    ControlInstruction,
}

#[derive(Debug, Clone)]
pub struct VolatileTailCandidate {
    pub message_index: usize,
    pub kind: VolatileTailCandidateKind,
    pub estimated_tokens: usize,
    pub has_recovery_handle: bool,
    pub safe_to_compact: bool,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct VolatileTailAnalysis {
    pub total_volatile_tail_tokens: usize,
    pub candidate_tokens: usize,
    pub preserved_recent_messages: usize,
    pub candidates: Vec<VolatileTailCandidate>,
}

/// Classify a message into a volatile-tail candidate kind.
fn classify_message(msg: &Message) -> Option<VolatileTailCandidateKind> {
    match msg {
        Message::Tool { .. } => Some(VolatileTailCandidateKind::ToolResult),
        Message::Assistant { content, .. } => {
            if !content.is_empty() {
                Some(VolatileTailCandidateKind::AssistantNarration)
            } else {
                None
            }
        }
        Message::User { content } => {
            // Treat text-only single-part user messages as potential control instructions
            if content.len() == 1 {
                if let ContentPart::Text { text } = &content[0] {
                    let t = text.as_ref();
                    if t.starts_with("[[") || t.contains("SYSTEM:") || t.starts_with("<control") {
                        return Some(VolatileTailCandidateKind::ControlInstruction);
                    }
                }
            }
            Some(VolatileTailCandidateKind::UserMessage)
        }
        Message::System { .. } => None,
    }
}

/// Check if a message content starts with the compacted tombstone marker.
fn is_already_compacted(msg: &Message) -> bool {
    match msg {
        Message::Tool { content, .. } => content.starts_with("[compacted volatile tool result]"),
        _ => false,
    }
}

/// Check if a message content contains a recovery handle (ctx://...).
fn has_recovery_handle(msg: &Message) -> bool {
    let content: &str = match msg {
        Message::Tool { content, .. } => content.as_ref(),
        _ => return false,
    };
    content.contains("ctx://")
}

/// Check if an assistant message has tool calls attached.
fn assistant_has_tool_calls(msg: &Message) -> bool {
    match msg {
        Message::Assistant { tool_calls, .. } => !tool_calls.is_empty(),
        _ => false,
    }
}

/// Run candidate analysis on a slice of messages.
///
/// Only inspects messages after the stable/system prefix. Preserves the last
/// `preserve_recent` transcript messages. Pure and non-mutating.
pub fn analyze_volatile_tail(
    messages: &[Message],
    preserve_recent: usize,
    compact_tool_results_only: bool,
) -> VolatileTailAnalysis {
    // Find the end of the system prefix
    let prefix_end = messages
        .iter()
        .position(|m| !matches!(m, Message::System { .. }))
        .unwrap_or(messages.len());

    let tail = &messages[prefix_end..];
    let total_volatile_tail_tokens: usize = tail.iter().map(estimate_message_tokens).sum();

    let recent_start = tail.len().saturating_sub(preserve_recent);
    let recent = &tail[recent_start..];

    let mut candidates = Vec::new();
    let mut candidate_tokens = 0usize;

    for (i, msg) in tail[..recent_start].iter().enumerate() {
        let absolute_index = prefix_end + i;

        if is_already_compacted(msg) {
            continue;
        }

        let kind = match classify_message(msg) {
            Some(k) => k,
            None => continue,
        };

        let tokens = estimate_message_tokens(msg);
        let recovery = has_recovery_handle(msg);
        let has_tc = assistant_has_tool_calls(msg);

        let (safe, reason) = match kind {
            VolatileTailCandidateKind::ToolResult => {
                if recovery {
                    (
                        true,
                        "older volatile tool result with recovery handle".into(),
                    )
                } else {
                    (
                        false,
                        "tool result lacks recovery handle; skipping by default".into(),
                    )
                }
            }
            VolatileTailCandidateKind::AssistantNarration => {
                if compact_tool_results_only {
                    (
                        false,
                        "assistant narration skipped in tool-results-only mode".into(),
                    )
                } else {
                    (
                        false,
                        "assistant narration not compacted in first pass".into(),
                    )
                }
            }
            VolatileTailCandidateKind::UserMessage => {
                (false, "user messages preserved in first pass".into())
            }
            VolatileTailCandidateKind::ControlInstruction => {
                if compact_tool_results_only {
                    (
                        false,
                        "control instruction skipped in tool-results-only mode".into(),
                    )
                } else {
                    (true, "machine-generated control instruction".into())
                }
            }
        };

        // Never compact assistant messages with tool calls
        if has_tc {
            candidates.push(VolatileTailCandidate {
                message_index: absolute_index,
                kind,
                estimated_tokens: tokens,
                has_recovery_handle: recovery,
                safe_to_compact: false,
                reason: "assistant message with tool calls preserved".into(),
            });
            continue;
        }

        if safe {
            candidate_tokens += tokens;
        }

        candidates.push(VolatileTailCandidate {
            message_index: absolute_index,
            kind,
            estimated_tokens: tokens,
            has_recovery_handle: recovery,
            safe_to_compact: safe,
            reason,
        });
    }

    VolatileTailAnalysis {
        total_volatile_tail_tokens,
        candidate_tokens,
        preserved_recent_messages: recent.len(),
        candidates,
    }
}

/// Estimate tokens for a single message.
pub fn estimate_message_tokens(msg: &Message) -> usize {
    match msg {
        Message::System { content } => estimate_tokens(content.as_ref()),
        Message::User { content } => content
            .iter()
            .map(|p| match p {
                ContentPart::Text { text } => estimate_tokens(text.as_ref()),
                _ => 10, // image placeholder
            })
            .sum(),
        Message::Assistant {
            content,
            tool_calls,
        } => {
            let text_tokens: usize = content
                .iter()
                .map(|p| match p {
                    ContentPart::Text { text } => estimate_tokens(text.as_ref()),
                    _ => 10,
                })
                .sum();
            let tc_tokens: usize = tool_calls
                .iter()
                .map(|tc| {
                    estimate_tokens(tc.name.as_ref())
                        + estimate_tokens(tc.arguments.to_string().as_str())
                })
                .sum();
            text_tokens + tc_tokens
        }
        Message::Tool {
            tool_call_id,
            content,
        } => estimate_tokens(tool_call_id.as_ref()) + estimate_tokens(content.as_ref()),
    }
}

// ── Phase 3: Tombstone formatting ──────────────────────────────────────────

/// Format a compacted tombstone for a tool result message.
pub fn format_tombstone(original_tokens: usize, recovery_handle: Option<&str>) -> String {
    match recovery_handle {
        Some(handle) => {
            format!(
                "[compacted volatile tool result]\n\
                 original_estimated_tokens={}\n\
                 reason=older volatile tail compacted by context policy\n\
                 recovery_handle={}\n\
                 Use context_read with the recovery_handle if full output is needed.",
                original_tokens, handle
            )
        }
        None => {
            format!(
                "[compacted volatile tool result]\n\
                 original_estimated_tokens={}\n\
                 reason=older volatile tail compacted by context policy (no recovery handle)\n\
                 Use context_read with the recovery_handle if full output is needed.",
                original_tokens
            )
        }
    }
}

/// Extract a recovery handle from message content, if present.
pub fn extract_recovery_handle(content: &str) -> Option<String> {
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("recovery_handle=") {
            let handle = rest.trim();
            if handle.starts_with("ctx://") {
                return Some(handle.to_string());
            }
        }
    }
    // Also handle inline ctx:// references in tool result content
    for word in content.split_whitespace() {
        if word.starts_with("ctx://") {
            return Some(word.to_string());
        }
    }
    None
}

// ── Phase 4: Decision logic ────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VolatileTailDecisionKind {
    Noop,
    WarnOnly,
    Compact,
}

#[derive(Debug, Clone)]
pub struct VolatileTailDecision {
    pub kind: VolatileTailDecisionKind,
    pub reason: String,
    pub recommended_action: EffectiveCostAction,
    pub candidate_count: usize,
    pub candidate_tokens: usize,
    pub planned_compaction_tokens: usize,
}

/// Decide whether volatile-tail compaction should fire.
pub fn decide_volatile_tail(
    analysis: &EffectiveCostAnalysis,
    config: &ContextPolicyConfig,
    plan: &VolatileTailPlan,
) -> VolatileTailDecision {
    if !config.volatile_tail_compaction() {
        return VolatileTailDecision {
            kind: VolatileTailDecisionKind::Noop,
            reason: "volatile tail compaction disabled".into(),
            recommended_action: analysis.recommended_action,
            candidate_count: 0,
            candidate_tokens: 0,
            planned_compaction_tokens: 0,
        };
    }

    let mode = config.volatile_tail_mode();

    if mode == VolatileTailPolicyMode::Observe {
        return VolatileTailDecision {
            kind: VolatileTailDecisionKind::Noop,
            reason: "volatile tail mode is observe; no action".into(),
            recommended_action: analysis.recommended_action,
            candidate_count: plan.candidates.len(),
            candidate_tokens: plan.candidate_tokens,
            planned_compaction_tokens: plan.planned_tokens,
        };
    }

    // Check effective-cost signal requirement
    if config.require_effective_cost_signal()
        && analysis.recommended_action != EffectiveCostAction::CompactVolatileTailFirst
    {
        return VolatileTailDecision {
            kind: VolatileTailDecisionKind::Noop,
            reason: format!(
                "effective-cost signal is {:?}; CompactVolatileTailFirst required",
                analysis.recommended_action
            ),
            recommended_action: analysis.recommended_action,
            candidate_count: plan.candidates.len(),
            candidate_tokens: plan.candidate_tokens,
            planned_compaction_tokens: plan.planned_tokens,
        };
    }

    // Check minimum tokens threshold
    if plan.candidate_tokens < config.min_volatile_tokens_for_compaction() {
        return VolatileTailDecision {
            kind: VolatileTailDecisionKind::Noop,
            reason: format!(
                "candidate tokens {} below minimum {}",
                plan.candidate_tokens,
                config.min_volatile_tokens_for_compaction()
            ),
            recommended_action: analysis.recommended_action,
            candidate_count: plan.candidates.len(),
            candidate_tokens: plan.candidate_tokens,
            planned_compaction_tokens: plan.planned_tokens,
        };
    }

    // Check that at least one candidate has a recovery handle
    if !plan.candidates.iter().any(|c| c.has_recovery_handle) {
        return VolatileTailDecision {
            kind: VolatileTailDecisionKind::Noop,
            reason: "no candidates with recovery handles; skipping".into(),
            recommended_action: analysis.recommended_action,
            candidate_count: plan.candidates.len(),
            candidate_tokens: plan.candidate_tokens,
            planned_compaction_tokens: plan.planned_tokens,
        };
    }

    match mode {
        VolatileTailPolicyMode::Compact => VolatileTailDecision {
            kind: VolatileTailDecisionKind::Compact,
            reason: format!(
                "compact {} candidates ({} tokens) from volatile tail",
                plan.safe_candidates.len(),
                plan.planned_tokens
            ),
            recommended_action: analysis.recommended_action,
            candidate_count: plan.candidates.len(),
            candidate_tokens: plan.candidate_tokens,
            planned_compaction_tokens: plan.planned_tokens,
        },
        VolatileTailPolicyMode::Warn => VolatileTailDecision {
            kind: VolatileTailDecisionKind::WarnOnly,
            reason: format!(
                "warn: would compact {} candidates ({} tokens) from volatile tail",
                plan.safe_candidates.len(),
                plan.planned_tokens
            ),
            recommended_action: analysis.recommended_action,
            candidate_count: plan.candidates.len(),
            candidate_tokens: plan.candidate_tokens,
            planned_compaction_tokens: plan.planned_tokens,
        },
        VolatileTailPolicyMode::Observe => unreachable!("handled above"),
    }
}

// ── Phase 5: Pure compaction planner ───────────────────────────────────────

#[derive(Debug, Clone)]
pub struct VolatileTailPlan {
    /// All candidates identified (including ones marked safe_to_compact=false for diagnostics).
    pub candidates: Vec<VolatileTailCandidate>,
    /// Total tokens across all safe candidates.
    pub candidate_tokens: usize,
    /// Safe candidates sorted oldest-first, respecting budget cap.
    pub safe_candidates: Vec<VolatileTailCandidate>,
    /// Total tokens to be compacted (sum of safe_candidates within budget).
    pub planned_tokens: usize,
}

/// Plan volatile-tail compaction. Pure and non-mutating.
///
/// Identifies candidates, sorts oldest first, prefers tool results with
/// recovery handles, and stops when `max_compacted_tail_tokens` would be exceeded.
pub fn plan_volatile_tail_compaction(
    messages: &[Message],
    _analysis: &EffectiveCostAnalysis,
    config: &ContextPolicyConfig,
) -> VolatileTailPlan {
    let analysis_result = analyze_volatile_tail(
        messages,
        config.preserve_recent_messages(),
        config.compact_tool_results_only_first(),
    );

    // Filter to safe candidates only, sorted oldest first
    let mut safe: Vec<VolatileTailCandidate> = analysis_result
        .candidates
        .iter()
        .filter(|c| c.safe_to_compact && c.has_recovery_handle)
        .cloned()
        .collect();

    // Sort oldest first (lower index = older)
    safe.sort_by_key(|c| c.message_index);

    let max_tokens = config.max_compacted_tail_tokens();
    let mut planned_tokens = 0usize;
    let mut selected = Vec::new();

    for candidate in safe {
        if planned_tokens + candidate.estimated_tokens > max_tokens {
            break;
        }
        planned_tokens += candidate.estimated_tokens;
        selected.push(candidate);
    }

    VolatileTailPlan {
        candidates: analysis_result.candidates,
        candidate_tokens: analysis_result.candidate_tokens,
        safe_candidates: selected,
        planned_tokens,
    }
}

// ── Phase 7: Apply compaction ──────────────────────────────────────────────

/// Apply volatile-tail compaction to messages. Mutates only selected `Message::Tool`
/// contents. Preserves message order and count. Returns the count of applied compactions.
pub fn apply_volatile_tail_compaction(messages: &mut [Message], plan: &VolatileTailPlan) -> usize {
    let indices: HashSet<usize> = plan
        .safe_candidates
        .iter()
        .map(|c| c.message_index)
        .collect();
    let mut applied = 0;

    for (i, msg) in messages.iter_mut().enumerate() {
        if !indices.contains(&i) {
            continue;
        }
        if let Message::Tool {
            tool_call_id: _,
            content,
        } = msg
        {
            let original_tokens = estimate_tokens(content.as_ref());
            let recovery = extract_recovery_handle(content.as_ref());
            let tombstone = format_tombstone(original_tokens, recovery.as_deref());
            *content = std::sync::Arc::from(tombstone);
            applied += 1;
        }
    }

    applied
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::effective_cost::{EffectiveCostAction, EffectiveCostAnalysis};
    use codegg_config::schema::{ContextPolicyConfig, VolatileTailPolicyMode};
    use codegg_providers::{ContentPart, Message, ToolCall};
    use std::sync::Arc;

    fn tool_msg(id: &str, content: &str) -> Message {
        Message::Tool {
            tool_call_id: Arc::new(id.to_string()),
            content: Arc::new(content.to_string()),
        }
    }

    fn user_msg(text: &str) -> Message {
        Message::User {
            content: vec![ContentPart::Text {
                text: Arc::new(text.to_string()),
            }],
        }
    }

    fn assistant_msg(text: &str) -> Message {
        Message::Assistant {
            content: vec![ContentPart::Text {
                text: Arc::new(text.to_string()),
            }],
            tool_calls: vec![],
        }
    }

    fn assistant_msg_with_tc(text: &str) -> Message {
        Message::Assistant {
            content: vec![ContentPart::Text {
                text: Arc::new(text.to_string()),
            }],
            tool_calls: vec![ToolCall {
                id: Arc::new("tc1".to_string()),
                name: Arc::new("bash".to_string()),
                arguments: serde_json::json!({"command": "ls"}),
            }],
        }
    }

    fn system_msg(text: &str) -> Message {
        Message::System {
            content: Arc::new(text.to_string()),
        }
    }

    fn default_config() -> ContextPolicyConfig {
        ContextPolicyConfig {
            volatile_tail_compaction: Some(true),
            volatile_tail_mode: Some(VolatileTailPolicyMode::Compact),
            min_volatile_tokens_for_compaction: Some(100),
            preserve_recent_messages: Some(3),
            max_compacted_tail_tokens: Some(50000),
            require_effective_cost_signal: Some(false),
            compact_tool_results_only_first: Some(true),
            ..Default::default()
        }
    }

    fn make_analysis(action: EffectiveCostAction) -> EffectiveCostAnalysis {
        EffectiveCostAnalysis {
            input_tokens: 10000,
            cached_input_tokens: 1000,
            uncached_input_tokens: 9000,
            cache_hit_rate: 0.1,
            stable_prefix_tokens: 1000,
            slow_changing_tokens: 2000,
            volatile_tokens: 7000,
            recommended_action: action,
            reason: "test".into(),
        }
    }

    #[test]
    fn test_analyze_preserves_recent_messages() {
        let msgs = vec![
            system_msg("sys"),
            tool_msg("t1", "output1 ctx://tool/s/0/t1"),
            tool_msg("t2", "output2 ctx://tool/s/0/t2"),
            tool_msg("t3", "output3 ctx://tool/s/0/t3"),
            user_msg("hello"),
            assistant_msg("hi"),
            tool_msg("t4", "output4 ctx://tool/s/0/t4"),
            tool_msg("t5", "output5 ctx://tool/s/0/t5"),
            tool_msg("t6", "output6 ctx://tool/s/0/t6"),
        ];

        let analysis = analyze_volatile_tail(&msgs, 3, true);
        // Last 3 messages (t4, t5, t6) should be preserved (not candidates)
        let candidate_indices: Vec<usize> = analysis
            .candidates
            .iter()
            .map(|c| c.message_index)
            .collect();
        assert!(!candidate_indices.contains(&6));
        assert!(!candidate_indices.contains(&7));
        assert!(!candidate_indices.contains(&8));
        // t1, t2, t3 should be candidates
        assert!(candidate_indices.contains(&1));
        assert!(candidate_indices.contains(&2));
        assert!(candidate_indices.contains(&3));
    }

    #[test]
    fn test_analyze_selects_tool_results_with_handles() {
        let msgs = vec![
            system_msg("sys"),
            tool_msg("t1", "output ctx://tool/s/0/t1"),
            tool_msg("t2", "no handle output"),
            user_msg("hello"),
        ];

        let analysis = analyze_volatile_tail(&msgs, 1, true);
        let with_handle: Vec<_> = analysis
            .candidates
            .iter()
            .filter(|c| c.has_recovery_handle)
            .collect();
        assert_eq!(with_handle.len(), 1);
        assert_eq!(with_handle[0].message_index, 1);
    }

    #[test]
    fn test_analyze_skips_no_handle_by_default() {
        let msgs = vec![
            system_msg("sys"),
            tool_msg("t1", "no handle"),
            user_msg("hello"),
        ];

        let analysis = analyze_volatile_tail(&msgs, 1, true);
        let safe: Vec<_> = analysis
            .candidates
            .iter()
            .filter(|c| c.safe_to_compact)
            .collect();
        assert!(safe.is_empty());
    }

    #[test]
    fn test_analyze_preserves_user_messages() {
        let msgs = vec![system_msg("sys"), user_msg("hello")];

        let analysis = analyze_volatile_tail(&msgs, 0, true);
        let user_candidates: Vec<_> = analysis
            .candidates
            .iter()
            .filter(|c| c.kind == VolatileTailCandidateKind::UserMessage && c.safe_to_compact)
            .collect();
        assert!(user_candidates.is_empty());
    }

    #[test]
    fn test_analyze_preserves_assistant_tool_calls() {
        let msgs = vec![
            system_msg("sys"),
            assistant_msg_with_tc("let me run that"),
            user_msg("hello"),
        ];

        let analysis = analyze_volatile_tail(&msgs, 1, true);
        let tc_msgs: Vec<_> = analysis
            .candidates
            .iter()
            .filter(|c| c.message_index == 1)
            .collect();
        assert_eq!(tc_msgs.len(), 1);
        assert!(!tc_msgs[0].safe_to_compact);
    }

    #[test]
    fn test_budget_cap_respected() {
        let config = ContextPolicyConfig {
            volatile_tail_compaction: Some(true),
            volatile_tail_mode: Some(VolatileTailPolicyMode::Compact),
            min_volatile_tokens_for_compaction: Some(0),
            preserve_recent_messages: Some(0),
            max_compacted_tail_tokens: Some(5),
            require_effective_cost_signal: Some(false),
            compact_tool_results_only_first: Some(true),
            ..Default::default()
        };

        // Each message has 2 words -> ~3 tokens
        let msgs = vec![
            system_msg("sys"),
            tool_msg("t1", "word1 word2 ctx://tool/s/0/t1"),
            tool_msg("t2", "word3 word4 ctx://tool/s/0/t2"),
            tool_msg("t3", "word5 word6 ctx://tool/s/0/t3"),
        ];

        let analysis = make_analysis(EffectiveCostAction::CompactVolatileTailFirst);
        let plan = plan_volatile_tail_compaction(&msgs, &analysis, &config);

        // Budget of 5 tokens, each message ~3 tokens, so only 1 should be selected
        assert!(plan.planned_tokens <= 5);
        assert!(plan.safe_candidates.len() < 3);
    }

    #[test]
    fn test_tombstone_preserves_contract() {
        let msgs = vec![
            system_msg("sys"),
            tool_msg("t1", "output content ctx://tool/s/0/t1"),
            tool_msg("t2", "output2 ctx://tool/s/0/t2"),
            tool_msg("t3", "output3 ctx://tool/s/0/t3"),
            user_msg("hello"),
        ];

        let config = default_config();
        let analysis = make_analysis(EffectiveCostAction::CompactVolatileTailFirst);
        let plan = plan_volatile_tail_compaction(&msgs, &analysis, &config);

        let mut msgs_clone = msgs.clone();
        let applied = apply_volatile_tail_compaction(&mut msgs_clone, &plan);

        assert_eq!(applied, 1);
        // Message count unchanged
        assert_eq!(msgs_clone.len(), msgs.len());
        // Tool call id preserved
        match &msgs_clone[1] {
            Message::Tool {
                tool_call_id,
                content,
            } => {
                assert_eq!(tool_call_id.as_ref(), "t1");
                assert!(content.starts_with("[compacted volatile tool result]"));
                assert!(content.contains("ctx://tool/s/0/t1"));
            }
            _ => panic!("expected Tool message"),
        }
    }

    #[test]
    fn test_idempotence() {
        let msgs = vec![
            system_msg("sys"),
            tool_msg("t1", "output ctx://tool/s/0/t1"),
            tool_msg("t2", "output2 ctx://tool/s/0/t2"),
            tool_msg("t3", "output3 ctx://tool/s/0/t3"),
            user_msg("hello"),
        ];

        let config = default_config();
        let analysis = make_analysis(EffectiveCostAction::CompactVolatileTailFirst);
        let plan = plan_volatile_tail_compaction(&msgs, &analysis, &config);

        let mut msgs_clone = msgs.clone();
        apply_volatile_tail_compaction(&mut msgs_clone, &plan);

        // Run analysis again on compacted messages
        let plan2 = plan_volatile_tail_compaction(&msgs_clone, &analysis, &config);
        // Already compacted message should not be a safe candidate
        let compacted_idx = msgs_clone
            .iter()
            .position(|m| matches!(m, Message::Tool { content, .. } if content.starts_with("[compacted volatile tool result]")))
            .unwrap();
        assert!(!plan2
            .safe_candidates
            .iter()
            .any(|c| c.message_index == compacted_idx));
    }

    #[test]
    fn test_warn_mode_nonmutation() {
        let msgs = vec![
            system_msg("sys"),
            tool_msg("t1", "output ctx://tool/s/0/t1"),
            user_msg("hello"),
        ];

        let config = ContextPolicyConfig {
            volatile_tail_compaction: Some(true),
            volatile_tail_mode: Some(VolatileTailPolicyMode::Warn),
            min_volatile_tokens_for_compaction: Some(0),
            preserve_recent_messages: Some(1),
            max_compacted_tail_tokens: Some(50000),
            require_effective_cost_signal: Some(false),
            compact_tool_results_only_first: Some(true),
            ..Default::default()
        };

        let analysis = make_analysis(EffectiveCostAction::CompactVolatileTailFirst);
        let plan = plan_volatile_tail_compaction(&msgs, &analysis, &config);
        let decision = decide_volatile_tail(&analysis, &config, &plan);

        assert_eq!(decision.kind, VolatileTailDecisionKind::WarnOnly);
        // Messages should NOT be mutated by warn mode (no apply call)
        assert!(
            matches!(&msgs[1], Message::Tool { content, .. } if !content.starts_with("[compacted"))
        );
    }

    #[test]
    fn test_compact_mode_mutation_scope() {
        let msgs = vec![
            system_msg("sys"),
            tool_msg("t1", "output1 ctx://tool/s/0/t1"),
            user_msg("hello"),
            tool_msg("t2", "output2 ctx://tool/s/0/t2"),
            assistant_msg("response"),
        ];

        let config = default_config();
        let analysis = make_analysis(EffectiveCostAction::CompactVolatileTailFirst);
        let plan = plan_volatile_tail_compaction(&msgs, &analysis, &config);

        let mut msgs_clone = msgs.clone();
        apply_volatile_tail_compaction(&mut msgs_clone, &plan);

        // Message count and order preserved
        assert_eq!(msgs_clone.len(), msgs.len());
        // User message untouched
        assert!(matches!(&msgs_clone[2], Message::User { .. }));
        // Assistant message untouched
        assert!(matches!(&msgs_clone[4], Message::Assistant { .. }));
    }

    #[test]
    fn test_tombstone_content_format() {
        let tombstone = format_tombstone(4312, Some("ctx://tool/s1/0/c1"));
        assert!(tombstone.starts_with("[compacted volatile tool result]"));
        assert!(tombstone.contains("original_estimated_tokens=4312"));
        assert!(tombstone.contains("recovery_handle=ctx://tool/s1/0/c1"));
        assert!(tombstone.contains("context_read"));
    }

    #[test]
    fn test_tombstone_no_handle_format() {
        let tombstone = format_tombstone(100, None);
        assert!(tombstone.starts_with("[compacted volatile tool result]"));
        assert!(tombstone.contains("original_estimated_tokens=100"));
        assert!(tombstone.contains("no recovery handle"));
    }

    #[test]
    fn test_extract_recovery_handle() {
        let content = "result output\nrecovery_handle=ctx://tool/s1/0/c1\nmore text";
        assert_eq!(
            extract_recovery_handle(content),
            Some("ctx://tool/s1/0/c1".to_string())
        );
    }

    #[test]
    fn test_extract_recovery_handle_inline() {
        let content = "output at ctx://tool/s1/0/c1 end";
        assert_eq!(
            extract_recovery_handle(content),
            Some("ctx://tool/s1/0/c1".to_string())
        );
    }
}
