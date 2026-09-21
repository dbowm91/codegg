//! Bounded, versioned current-state projection for advisor experiments.
//!
//! This DTO deliberately accepts only host-owned state summaries. It never
//! carries transcript history, tool arguments/results, credentials, or chain
//! of thought. The same constructor is used by runtime request preparation and
//! offline benchmark fixtures.

use crate::agent::context_frame::ContextFrame;

pub const ADVISOR_CONTEXT_V2_SCHEMA_VERSION: u16 = 2;
pub const MAX_ADVISOR_CONTEXT_V2_BYTES: usize = 8 * 1024;
const MAX_FIELD_BYTES: usize = 2 * 1024;
const MAX_NEXT_STEPS: usize = 2;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdvisorContextV2 {
    pub current_objective: Option<String>,
    pub current_task: Option<String>,
    pub next_steps: Vec<String>,
    pub unresolved_signal: Option<String>,
    pub capability_cue: Option<String>,
}

impl AdvisorContextV2 {
    pub fn from_context_frame(
        current_prompt: Option<&str>,
        original_prompt: Option<&str>,
        frame: &ContextFrame,
        work_plan_task: Option<&str>,
        work_plan_next_steps: &[String],
        capability_cue: Option<&str>,
    ) -> Self {
        let objective = frame
            .user_goal
            .as_deref()
            .or_else(|| current_prompt.and_then(nonempty))
            .or_else(|| original_prompt.and_then(nonempty))
            .and_then(safe_text);
        let current_task = work_plan_task
            .and_then(safe_text)
            .or_else(|| frame.current_task.as_deref().and_then(safe_text));
        let mut next_steps = work_plan_next_steps
            .iter()
            .chain(frame.next_steps.iter())
            .filter_map(|step| safe_text(step))
            .take(MAX_NEXT_STEPS)
            .collect::<Vec<_>>();
        next_steps.dedup();
        let unresolved_signal = frame
            .unresolved_errors
            .iter()
            .filter_map(|error| safe_text(error))
            .take(2)
            .collect::<Vec<_>>();
        Self {
            current_objective: objective,
            current_task,
            next_steps,
            unresolved_signal: (!unresolved_signal.is_empty())
                .then(|| unresolved_signal.join("; ")),
            capability_cue: capability_cue.and_then(safe_text),
        }
    }

    /// Deterministic adapter for frozen benchmark cases. New context-v2
    /// training/dev cases can use the same serializer without changing the
    /// frozen test labels or lineage.
    pub fn from_benchmark_context(context: &str) -> Self {
        Self {
            current_objective: safe_text(context),
            ..Self::default()
        }
    }

    pub fn serialize(&self) -> String {
        let mut output = format!("advisor-context-v{ADVISOR_CONTEXT_V2_SCHEMA_VERSION}\n");
        for (label, value) in [
            ("current_objective", self.current_objective.as_deref()),
            ("current_task", self.current_task.as_deref()),
            ("unresolved_signal", self.unresolved_signal.as_deref()),
            ("capability_cue", self.capability_cue.as_deref()),
        ] {
            if let Some(value) = value {
                append_bounded(&mut output, label, value);
            }
        }
        if !self.next_steps.is_empty() {
            append_bounded(&mut output, "next_steps", &self.next_steps.join(" | "));
        }
        truncate_utf8(&mut output, MAX_ADVISOR_CONTEXT_V2_BYTES);
        output
    }
}

fn nonempty(value: &str) -> Option<&str> {
    (!value.trim().is_empty()).then_some(value)
}

/// Conservative field-level filter. Secret-like assignments are omitted as a
/// whole rather than attempting to preserve surrounding prose.
fn safe_text(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.contains('\0') {
        return None;
    }
    let lower = value.to_ascii_lowercase();
    let sensitive_keys = [
        "api_key=",
        "api-key=",
        "access_token=",
        "access-token=",
        "client_secret=",
        "password=",
        "credential=",
        "bearer ",
        "sk-",
        "ghp_",
    ];
    if sensitive_keys.iter().any(|key| lower.contains(key)) {
        return Some("[redacted advisor field]".into());
    }
    Some(truncate_string(value, MAX_FIELD_BYTES))
}

fn append_bounded(output: &mut String, label: &str, value: &str) {
    let line = format!("{label}: {value}\n");
    if output.len() + line.len() <= MAX_ADVISOR_CONTEXT_V2_BYTES {
        output.push_str(&line);
    } else {
        let remaining = MAX_ADVISOR_CONTEXT_V2_BYTES.saturating_sub(output.len());
        if remaining > label.len() + 3 {
            let mut partial = format!("{label}: ");
            partial.push_str(&truncate_string(
                value,
                remaining.saturating_sub(partial.len()),
            ));
            output.push_str(&partial);
        }
    }
}

fn truncate_string(value: &str, max_bytes: usize) -> String {
    let mut value = value.to_string();
    truncate_utf8(&mut value, max_bytes);
    value
}

fn truncate_utf8(value: &mut String, max_bytes: usize) {
    if value.len() <= max_bytes {
        return;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> ContextFrame {
        ContextFrame {
            user_goal: Some("inspect architecture".into()),
            current_task: Some("stale task".into()),
            next_steps: vec![
                "run focused tests".into(),
                "update docs".into(),
                "drop me".into(),
            ],
            unresolved_errors: vec!["compiler error in parser".into()],
            ..ContextFrame::default()
        }
    }

    #[test]
    fn active_goal_and_work_plan_task_outrank_origin_prompt() {
        let context = AdvisorContextV2::from_context_frame(
            Some("latest user request"),
            Some("inspect architecture"),
            &frame(),
            Some("fix failing Rust tests"),
            &["rerun the failing suite".into()],
            None,
        );
        assert_eq!(
            context.current_objective.as_deref(),
            Some("inspect architecture")
        );
        assert_eq!(
            context.current_task.as_deref(),
            Some("fix failing Rust tests")
        );
        assert!(context.serialize().contains("fix failing Rust tests"));
    }

    #[test]
    fn fallback_and_unresolved_projection_are_bounded() {
        let no_state = ContextFrame {
            unresolved_errors: vec!["é".repeat(20_000)],
            ..ContextFrame::default()
        };
        let context = AdvisorContextV2::from_context_frame(
            Some("current objective"),
            Some("origin"),
            &no_state,
            None,
            &[],
            Some("rust repository"),
        );
        let serialized = context.serialize();
        assert!(serialized.len() <= MAX_ADVISOR_CONTEXT_V2_BYTES);
        assert!(serialized.contains("current objective"));
        assert!(serialized.is_char_boundary(serialized.len()));
    }

    #[test]
    fn secret_like_fields_never_enter_projection() {
        let secret = ContextFrame {
            user_goal: Some("deploy api_key=do-not-leak".into()),
            unresolved_errors: vec!["password=do-not-leak".into()],
            ..ContextFrame::default()
        };
        let serialized =
            AdvisorContextV2::from_context_frame(None, None, &secret, None, &[], None).serialize();
        assert!(!serialized.contains("do-not-leak"));
        assert!(serialized.contains("[redacted advisor field]"));
    }

    #[test]
    fn benchmark_adapter_is_deterministic() {
        let first = AdvisorContextV2::from_benchmark_context("read project files").serialize();
        let second = AdvisorContextV2::from_benchmark_context("read project files").serialize();
        assert_eq!(first, second);
    }
}
