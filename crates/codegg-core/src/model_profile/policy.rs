use crate::model_profile::types::ResolvedModelProfile;
use codegg_providers::{ContentPart, Message};

fn content_already_present(messages: &[Message], text: &str) -> bool {
    for msg in messages {
        match msg {
            Message::System { content } if content.contains(text) => {
                return true;
            }
            Message::User { content } => {
                for part in content {
                    if let ContentPart::Text { text: t } = part {
                        if t.contains(text) {
                            return true;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    false
}

pub fn should_avoid_late_system_messages(profile: &ResolvedModelProfile) -> bool {
    !profile.supports_late_system_messages || profile.prefers_user_control_messages
}

pub fn push_control_instruction(
    messages: &mut Vec<Message>,
    profile: &ResolvedModelProfile,
    content: &str,
) {
    if content_already_present(messages, content) {
        return;
    }

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
    fn test_dedup_push_control_skips_duplicate() {
        let mut messages = vec![Message::System {
            content: "Base".to_string().into(),
        }];
        let profile = infer_builtin_profile("openai/gpt-5");
        push_control_instruction(&mut messages, &profile, "unique instruction X");
        push_control_instruction(&mut messages, &profile, "unique instruction X");

        let count = messages.iter().filter(|m| {
            matches!(m, Message::System { content } if content.as_ref().contains("unique instruction X"))
        }).count();
        assert_eq!(count, 1, "Instruction should appear exactly once");
    }

    #[test]
    fn test_dedup_different_instructions_not_skipped() {
        let mut messages = vec![Message::System {
            content: "Base".to_string().into(),
        }];
        let profile = infer_builtin_profile("openai/gpt-5");
        push_control_instruction(&mut messages, &profile, "instruction A");
        push_control_instruction(&mut messages, &profile, "instruction B");

        let has_a = messages.iter().any(|m| {
            matches!(m, Message::System { content } if content.as_ref().contains("instruction A"))
        });
        let has_b = messages.iter().any(|m| {
            matches!(m, Message::System { content } if content.as_ref().contains("instruction B"))
        });
        assert!(has_a, "instruction A should be present");
        assert!(has_b, "instruction B should be present");
    }
}
