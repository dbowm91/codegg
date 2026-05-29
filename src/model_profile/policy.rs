use crate::model_profile::types::ResolvedModelProfile;
use crate::provider::{ContentPart, Message};

pub fn apply_startup_profile_policy(
    messages: &mut Vec<Message>,
    profile: &ResolvedModelProfile,
) {
    if profile.requires_explicit_tool_contract {
        inject_tool_contract(messages, profile);
    }

    if profile.prefers_small_patches {
        inject_small_patch_contract(messages, profile);
    }

    inject_todo_discipline(messages, profile);
}

fn inject_tool_contract(messages: &mut Vec<Message>, profile: &ResolvedModelProfile) {
    let contract = "Tool-use contract: For repository/file/code/doc tasks, emit structured tool calls before giving conclusions. Do not only describe intended tool use in plain text. If tools are available and the task requires repository knowledge, inspect the repository with tools before finalizing.";

    inject_control_text(messages, profile, contract);
}

fn inject_small_patch_contract(messages: &mut Vec<Message>, profile: &ResolvedModelProfile) {
    let contract = "Patch discipline: Prefer small, targeted edits. Do not rewrite unrelated files. Inspect the relevant file region before editing when possible.";

    inject_control_text(messages, profile, contract);
}

fn inject_todo_discipline(messages: &mut Vec<Message>, profile: &ResolvedModelProfile) {
    use crate::model_profile::types::TodoMode;
    let text = match profile.task_state_policy.mode {
        TodoMode::Disabled => return,
        TodoMode::SparsePlan => "Task planning: Use todos only for non-trivial multi-step work. Keep the list short. Maintain exactly one in-progress item. Update it at meaningful milestones, not after every minor read.",
        TodoMode::ExplicitTodo => "Task planning: For multi-step coding work, keep a short todo list. Keep exactly one item in_progress. Mark items completed only after verification. Update the list when task direction changes.",
        TodoMode::GuidedCurrentTask => "Task planning: Follow the active task reminder. Do not create or rewrite the global todo list unless explicitly allowed. Complete the current task, report blockers, then proceed.",
    };
    inject_control_text(messages, profile, text);
}

fn inject_control_text(
    messages: &mut Vec<Message>,
    profile: &ResolvedModelProfile,
    text: &str,
) {
    if let Some(Message::System { content }) = messages.first_mut() {
        let merged = format!("{content}\n\n{text}");
        *content = merged.into();
        return;
    }

    if profile.prefers_user_control_messages {
        messages.insert(
            0,
            Message::User {
                content: vec![ContentPart::Text {
                    text: format!("Instruction: {text}").into(),
                }],
            },
        );
    } else {
        messages.insert(
            0,
            Message::System {
                content: text.to_string().into(),
            },
        );
    }
}

pub fn should_avoid_late_system_messages(profile: &ResolvedModelProfile) -> bool {
    !profile.supports_late_system_messages || profile.prefers_user_control_messages
}

pub fn push_control_instruction(
    messages: &mut Vec<Message>,
    profile: &ResolvedModelProfile,
    content: &str,
) {
    if should_avoid_late_system_messages(profile) {
        if let Some(Message::System {
            content: system_content,
        }) = messages.first_mut()
        {
            let merged = format!("{system_content}\n\n{content}");
            *system_content = merged.into();
            return;
        }

        messages.push(Message::User {
            content: vec![ContentPart::Text {
                text: format!("Instruction: {content}").into(),
            }],
        });
        return;
    }

    messages.push(Message::System {
        content: content.to_string().into(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_profile::resolve::infer_builtin_profile;

    #[test]
    fn test_tool_contract_injected_when_required() {
        let mut messages = vec![Message::System {
            content: "Base system prompt".to_string().into(),
        }];
        let profile = infer_builtin_profile("minimax/minimax-2.7");
        apply_startup_profile_policy(&mut messages, &profile);

        match &messages[0] {
            Message::System { content } => {
                assert!(content.contains("Tool-use contract"));
                assert!(content.contains("Patch discipline"));
                assert!(content.contains("Base system prompt"));
            }
            _ => panic!("Expected system message"),
        }
    }

    #[test]
    fn test_no_system_message_prefers_user_control() {
        let mut messages = vec![];
        let profile = infer_builtin_profile("ollama/qwen2.5-coder:32b");
        apply_startup_profile_policy(&mut messages, &profile);

        // tool contract + small patch + todo discipline = 3 user messages
        assert_eq!(messages.len(), 3);
        let has_tool_contract = messages.iter().any(|m| {
            matches!(m, Message::User { content } if content.iter().any(|p| matches!(p, ContentPart::Text { text } if text.contains("Tool-use contract"))))
        });
        assert!(has_tool_contract, "Expected a user message with Tool-use contract");
    }

    #[test]
    fn test_no_injection_when_not_required() {
        let mut messages = vec![Message::System {
            content: "Base system prompt".to_string().into(),
        }];
        let profile = infer_builtin_profile("openai/gpt-5");
        apply_startup_profile_policy(&mut messages, &profile);

        // Frontier models don't get tool contract or small patch, but do get todo discipline
        assert_eq!(messages.len(), 1);
        match &messages[0] {
            Message::System { content } => {
                assert!(content.contains("Base system prompt"));
                assert!(content.contains("Task planning"));
            }
            _ => panic!("Expected system message"),
        }
    }

    #[test]
    fn test_push_control_avoids_late_system() {
        let mut messages = vec![Message::System {
            content: "Base".to_string().into(),
        }];
        let profile = infer_builtin_profile("minimax/minimax-2.7");
        push_control_instruction(&mut messages, &profile, "new instruction");

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            Message::System { content } => {
                assert!(content.contains("new instruction"));
            }
            _ => panic!("Expected merged system message"),
        }
    }

    #[test]
    fn test_push_control_allows_late_system() {
        let mut messages = vec![Message::System {
            content: "Base".to_string().into(),
        }];
        let profile = infer_builtin_profile("openai/gpt-5");
        push_control_instruction(&mut messages, &profile, "new instruction");

        assert_eq!(messages.len(), 2);
        match &messages[1] {
            Message::System { content } => {
                assert_eq!(content.as_ref(), "new instruction");
            }
            _ => panic!("Expected new system message"),
        }
    }

    #[test]
    fn test_todo_discipline_sparse_plan() {
        let mut messages = vec![Message::System {
            content: "Base".to_string().into(),
        }];
        let mut profile = infer_builtin_profile("openai/gpt-5");
        profile.task_state_policy = crate::model_profile::types::TaskStatePolicy::sparse_plan();
        apply_startup_profile_policy(&mut messages, &profile);
        match &messages[0] {
            Message::System { content } => {
                assert!(content.contains("Task planning"));
                assert!(content.contains("meaningful milestones"));
            }
            _ => panic!("Expected system message"),
        }
    }

    #[test]
    fn test_todo_discipline_disabled() {
        let mut messages = vec![Message::System {
            content: "Base".to_string().into(),
        }];
        let mut profile = infer_builtin_profile("openai/gpt-5");
        profile.task_state_policy = crate::model_profile::types::TaskStatePolicy::disabled();
        apply_startup_profile_policy(&mut messages, &profile);
        match &messages[0] {
            Message::System { content } => {
                assert!(!content.contains("Task planning"));
            }
            _ => panic!("Expected system message"),
        }
    }
}
