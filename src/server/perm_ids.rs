//! Scoped pending-request ID parsing shared by server adapters.
//!
//! Pending permissions/questions are registered in the bus registries
//! under their simple registry key (`{tool_call_id}-{tool_name}`,
//! `q-{uuid}`) together with an owning `session_id`. Protocol-level IDs
//! handed to remote clients are prefixed:
//!
//! - `perm:<session_id>:<turn_id>:<simple_perm_id>`
//! - `question:<session_id>:<turn_id>:<simple_question_id>`
//!
//! Responding requires splitting the prefix back out so the scoped
//! registry APIs (`respond_scoped` / `answer_question_scoped`) can
//! verify ownership. This mirrors the parsing performed by the daemon's
//! `CoreRequest::PermissionRespond` / `CoreRequest::QuestionRespond`
//! handlers in `src/core/daemon.rs`.

/// Parse a prefixed pending-request ID into `(session_id, simple_id)`.
/// Returns `None` for unprefixed or malformed IDs.
pub(crate) fn parse_scoped_pending_id(id: &str) -> Option<(String, String)> {
    let (prefix, rest) = id.split_once(':')?;
    if prefix != "perm" && prefix != "question" {
        return None;
    }
    let mut parts = rest.splitn(3, ':');
    let session_id = parts.next()?;
    let _turn_id = parts.next()?;
    let simple_id = parts.next()?;
    Some((session_id.to_string(), simple_id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::parse_scoped_pending_id;

    #[test]
    fn parses_permission_id() {
        let got = parse_scoped_pending_id("perm:s1:t2:abc-def");
        assert_eq!(got, Some(("s1".to_string(), "abc-def".to_string())));
    }

    #[test]
    fn parses_question_id_with_empty_turn() {
        let got = parse_scoped_pending_id("question:sess::q-123");
        assert_eq!(got, Some(("sess".to_string(), "q-123".to_string())));
    }

    #[test]
    fn rejects_unprefixed_and_foreign_prefixes() {
        assert_eq!(parse_scoped_pending_id("abc-def"), None);
        assert_eq!(parse_scoped_pending_id("other:s1:t2:p3"), None);
    }

    #[test]
    fn rejects_malformed_ids() {
        assert_eq!(parse_scoped_pending_id("perm:s1"), None);
        assert_eq!(parse_scoped_pending_id("perm:s1:t2"), None);
        assert_eq!(parse_scoped_pending_id("perm:"), None);
    }
}
