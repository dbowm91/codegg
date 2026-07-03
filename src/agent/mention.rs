use crate::agent::Agent;

#[derive(Debug, Clone, PartialEq)]
pub struct MentionContext {
    pub trigger_pos: usize,
    pub query: String,
}

pub fn parse_mention(input: &str, cursor: usize) -> Option<MentionContext> {
    let before_cursor = &input[..cursor.min(input.len())];

    if let Some(pos) = before_cursor.rfind('@') {
        let is_start = pos == 0 || before_cursor.chars().nth(pos.saturating_sub(1)) == Some(' ');
        if is_start {
            let after_at = &before_cursor[pos + 1..];
            let token_end = after_at
                .find(|c: char| c.is_whitespace())
                .unwrap_or(after_at.len());
            let query = format!("@{}", &after_at[..token_end]);
            return Some(MentionContext {
                trigger_pos: pos,
                query,
            });
        }
    }
    None
}

pub fn filter_agents(agents: &[Agent], query: &str) -> Vec<Agent> {
    let query = query.trim_start_matches('@').to_lowercase();

    if query.is_empty() {
        return agents.to_vec();
    }

    agents
        .iter()
        .filter(|a| {
            let name_match = a.name.to_lowercase().contains(&query);
            let desc_match = a.description.to_lowercase().contains(&query);
            name_match || desc_match
        })
        .cloned()
        .collect()
}

pub fn find_mention_trigger(input: &str, cursor: usize) -> Option<usize> {
    let before_cursor = &input[..cursor.min(input.len())];

    if let Some(pos) = before_cursor.rfind('@') {
        let is_start = pos == 0 || before_cursor.chars().nth(pos.saturating_sub(1)) == Some(' ');
        if is_start {
            return Some(pos);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentMode;
    use std::collections::HashMap;

    fn make_agent(name: &str, description: &str) -> Agent {
        Agent {
            name: name.to_string(),
            role: None,
            description: description.to_string(),
            mode: AgentMode::Primary,
            mode_name: None,
            model: None,
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: None,
            permissions: HashMap::new(),
            hidden: false,
            thinking_budget: None,
            fallback_model: None,
            reasoning_effort: None,
            runtime_kind: None,
        }
    }

    #[test]
    fn test_parse_mention_at_start() {
        let ctx = parse_mention("@build", 6).unwrap();
        assert_eq!(ctx.trigger_pos, 0);
        assert_eq!(ctx.query, "@build");
    }

    #[test]
    fn test_parse_mention_with_space() {
        let ctx = parse_mention("use @build agent", 14).unwrap();
        assert_eq!(ctx.trigger_pos, 4);
        assert_eq!(ctx.query, "@build");
    }

    #[test]
    fn test_parse_mention_partial() {
        let ctx = parse_mention("@bu", 3).unwrap();
        assert_eq!(ctx.query, "@bu");
    }

    #[test]
    fn test_parse_mention_no_at() {
        assert!(parse_mention("hello world", 5).is_none());
    }

    #[test]
    fn test_parse_mention_mid_word() {
        assert!(parse_mention("user@host", 5).is_none());
    }

    #[test]
    fn test_filter_agents_empty_query() {
        let agents = vec![
            make_agent("build", "Builds code"),
            make_agent("review", "Reviews code"),
        ];
        let filtered = filter_agents(&agents, "@");
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filter_agents_by_name() {
        let agents = vec![
            make_agent("build", "Builds code"),
            make_agent("review", "Reviews code"),
        ];
        let filtered = filter_agents(&agents, "@bu");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "build");
    }

    #[test]
    fn test_filter_agents_by_description() {
        let agents = vec![
            make_agent("build", "Compiles code"),
            make_agent("review", "Reviews PRs"),
        ];
        let filtered = filter_agents(&agents, "@compile");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "build");
    }
}
