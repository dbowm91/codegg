use std::fmt;

/// Typed parser for `ctx://` artifact handles.
///
/// Handles have the form: `ctx://tool/{session_id}/{turn_index}/{tool_call_id}`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextHandle {
    pub kind: ContextHandleKind,
    pub session_id: String,
    pub turn_index: usize,
    pub tool_call_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextHandleKind {
    Tool,
}

/// Errors returned when parsing or building a [`ContextHandle`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextHandleError {
    /// The scheme is not `ctx://`.
    InvalidScheme,
    /// The handle kind is not `tool`.
    UnsupportedKind(String),
    /// The handle does not have the expected number of path segments.
    MissingSegments,
    /// The handle has too many path segments.
    ExtraSegments,
    /// A required segment is empty.
    EmptySegment { field: &'static str },
    /// The turn_index segment is not a valid `usize`.
    InvalidTurnIndex(String),
    /// A segment contains `/`, control characters, or whitespace.
    UnsafeSegment { field: &'static str, character: char },
}

impl fmt::Display for ContextHandleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidScheme => write!(f, "handle must start with ctx://"),
            Self::UnsupportedKind(k) => write!(f, "unsupported handle kind: {k}"),
            Self::MissingSegments => write!(f, "handle has too few path segments"),
            Self::ExtraSegments => write!(f, "handle has too many path segments"),
            Self::EmptySegment { field } => write!(f, "{field} segment must not be empty"),
            Self::InvalidTurnIndex(s) => write!(f, "invalid turn_index: {s}"),
            Self::UnsafeSegment { field, character } => {
                write!(
                    f,
                    "{field} segment contains unsafe character: {character:?}"
                )
            }
        }
    }
}

impl std::error::Error for ContextHandleError {}

impl ContextHandle {
    /// Parse a `ctx://` handle string into a typed [`ContextHandle`].
    pub fn parse(input: &str) -> Result<Self, ContextHandleError> {
        let rest = input
            .strip_prefix("ctx://")
            .ok_or(ContextHandleError::InvalidScheme)?;

        let mut parts: Vec<&str> = rest.split('/').collect();

        // Must have exactly: kind / session_id / turn_index / tool_call_id
        if parts.len() < 4 {
            return Err(ContextHandleError::MissingSegments);
        }
        if parts.len() > 4 {
            return Err(ContextHandleError::ExtraSegments);
        }

        let kind_str = parts.remove(0);
        let kind = match kind_str {
            "tool" => ContextHandleKind::Tool,
            other => return Err(ContextHandleError::UnsupportedKind(other.to_string())),
        };

        let session_id = parts.remove(0);
        let turn_str = parts.remove(0);
        let tool_call_id = parts.remove(0);

        // Validate segments are non-empty
        if session_id.is_empty() {
            return Err(ContextHandleError::EmptySegment {
                field: "session_id",
            });
        }
        if tool_call_id.is_empty() {
            return Err(ContextHandleError::EmptySegment {
                field: "tool_call_id",
            });
        }

        // Validate no unsafe characters in segments
        Self::check_segment_safe(session_id, "session_id")?;
        Self::check_segment_safe(tool_call_id, "tool_call_id")?;

        // Parse turn_index
        let turn_index: usize = turn_str
            .parse()
            .map_err(|_| ContextHandleError::InvalidTurnIndex(turn_str.to_string()))?;

        Ok(Self {
            kind,
            session_id: session_id.to_string(),
            turn_index,
            tool_call_id: tool_call_id.to_string(),
        })
    }

    /// Build a tool handle string. Returns error if segments contain unsafe characters.
    pub fn build_tool(
        session_id: &str,
        turn_index: usize,
        tool_call_id: &str,
    ) -> Result<String, ContextHandleError> {
        Self::check_segment_safe(session_id, "session_id")?;
        Self::check_segment_safe(tool_call_id, "tool_call_id")?;
        Ok(format!(
            "ctx://tool/{session_id}/{turn_index}/{tool_call_id}"
        ))
    }

    /// Check that a session_id matches exactly (not substring).
    pub fn same_session(&self, session_id: &str) -> bool {
        self.session_id == session_id
    }

    /// Check that a segment contains no `/`, control characters, or whitespace.
    fn check_segment_safe(segment: &str, field: &'static str) -> Result<(), ContextHandleError> {
        for ch in segment.chars() {
            if ch == '/' || ch.is_control() || ch.is_whitespace() {
                return Err(ContextHandleError::UnsafeSegment { field, character: ch });
            }
        }
        Ok(())
    }
}

/// Clamp a byte index to the nearest valid UTF-8 character boundary (rounding down).
pub fn clamp_to_char_boundary(s: &str, mut idx: usize) -> usize {
    idx = idx.min(s.len());
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid() {
        let h = ContextHandle::parse("ctx://tool/sess123/5/call_abc").unwrap();
        assert_eq!(h.kind, ContextHandleKind::Tool);
        assert_eq!(h.session_id, "sess123");
        assert_eq!(h.turn_index, 5);
        assert_eq!(h.tool_call_id, "call_abc");
    }

    #[test]
    fn test_parse_turn_zero() {
        let h = ContextHandle::parse("ctx://tool/s1/0/c1").unwrap();
        assert_eq!(h.turn_index, 0);
    }

    #[test]
    fn test_parse_invalid_scheme() {
        let err = ContextHandle::parse("http://tool/s1/0/c1").unwrap_err();
        assert_eq!(err, ContextHandleError::InvalidScheme);
    }

    #[test]
    fn test_parse_no_scheme() {
        let err = ContextHandle::parse("tool/s1/0/c1").unwrap_err();
        assert_eq!(err, ContextHandleError::InvalidScheme);
    }

    #[test]
    fn test_parse_unsupported_kind() {
        let err = ContextHandle::parse("ctx://file/s1/0/c1").unwrap_err();
        assert_eq!(
            err,
            ContextHandleError::UnsupportedKind("file".to_string())
        );
    }

    #[test]
    fn test_parse_missing_segments() {
        let err = ContextHandle::parse("ctx://tool/s1/0").unwrap_err();
        assert_eq!(err, ContextHandleError::MissingSegments);
    }

    #[test]
    fn test_parse_too_few_segments() {
        let err = ContextHandle::parse("ctx://tool/s1").unwrap_err();
        assert_eq!(err, ContextHandleError::MissingSegments);
    }

    #[test]
    fn test_parse_extra_segments() {
        let err = ContextHandle::parse("ctx://tool/s1/0/c1/extra").unwrap_err();
        assert_eq!(err, ContextHandleError::ExtraSegments);
    }

    #[test]
    fn test_parse_empty_session_id() {
        let err = ContextHandle::parse("ctx://tool//0/c1").unwrap_err();
        assert_eq!(
            err,
            ContextHandleError::EmptySegment {
                field: "session_id"
            }
        );
    }

    #[test]
    fn test_parse_empty_tool_call_id() {
        let err = ContextHandle::parse("ctx://tool/s1/0/").unwrap_err();
        assert_eq!(
            err,
            ContextHandleError::EmptySegment {
                field: "tool_call_id"
            }
        );
    }

    #[test]
    fn test_parse_invalid_turn_index() {
        let err = ContextHandle::parse("ctx://tool/s1/abc/c1").unwrap_err();
        assert_eq!(
            err,
            ContextHandleError::InvalidTurnIndex("abc".to_string())
        );
    }

    #[test]
    fn test_parse_negative_turn_index() {
        // -1 doesn't parse as usize
        let err = ContextHandle::parse("ctx://tool/s1/-1/c1").unwrap_err();
        assert!(matches!(err, ContextHandleError::InvalidTurnIndex(_)));
    }

    #[test]
    fn test_parse_slash_in_session_id() {
        let err = ContextHandle::parse("ctx://tool/not/s1/0/c1").unwrap_err();
        // With 5 segments, we get ExtraSegments
        assert_eq!(err, ContextHandleError::ExtraSegments);
    }

    #[test]
    fn test_parse_whitespace_in_session_id() {
        let err = ContextHandle::parse("ctx://tool/s 1/0/c1").unwrap_err();
        assert_eq!(
            err,
            ContextHandleError::UnsafeSegment {
                field: "session_id",
                character: ' '
            }
        );
    }

    #[test]
    fn test_parse_control_char_in_tool_call_id() {
        let err = ContextHandle::parse("ctx://tool/s1/0/c\n1").unwrap_err();
        assert_eq!(
            err,
            ContextHandleError::UnsafeSegment {
                field: "tool_call_id",
                character: '\n'
            }
        );
    }

    #[test]
    fn test_build_tool_valid() {
        let handle = ContextHandle::build_tool("sess123", 5, "call_abc").unwrap();
        assert_eq!(handle, "ctx://tool/sess123/5/call_abc");
    }

    #[test]
    fn test_build_tool_rejects_slash() {
        let err = ContextHandle::build_tool("s/1", 0, "c1").unwrap_err();
        assert_eq!(
            err,
            ContextHandleError::UnsafeSegment {
                field: "session_id",
                character: '/'
            }
        );
    }

    #[test]
    fn test_build_tool_rejects_whitespace() {
        let err = ContextHandle::build_tool("s1", 0, "c 1").unwrap_err();
        assert_eq!(
            err,
            ContextHandleError::UnsafeSegment {
                field: "tool_call_id",
                character: ' '
            }
        );
    }

    #[test]
    fn test_same_session_exact() {
        let h = ContextHandle::parse("ctx://tool/s1/0/c1").unwrap();
        assert!(h.same_session("s1"));
        assert!(!h.same_session("not-s1"));
        assert!(!h.same_session("s"));
        assert!(!h.same_session("s10"));
    }

    #[test]
    fn test_roundtrip() {
        let handle_str = ContextHandle::build_tool("my-session", 42, "call-xyz").unwrap();
        let parsed = ContextHandle::parse(&handle_str).unwrap();
        assert_eq!(parsed.session_id, "my-session");
        assert_eq!(parsed.turn_index, 42);
        assert_eq!(parsed.tool_call_id, "call-xyz");
    }

    #[test]
    fn test_substring_attack() {
        // session "s1" must not match a handle with session "not-s1"
        let h = ContextHandle::parse("ctx://tool/not-s1/0/c1").unwrap();
        assert!(!h.same_session("s1"));
    }

    #[test]
    fn test_clamp_to_char_boundary_ascii() {
        let s = "hello world";
        assert_eq!(clamp_to_char_boundary(s, 5), 5);
        assert_eq!(clamp_to_char_boundary(s, 100), 11);
    }

    #[test]
    fn test_clamp_to_char_boundary_multibyte() {
        let s = "héllo"; // é is 2 bytes: byte 1 (0xC3) and byte 2 (0xA9)
        // Byte index 1 is the START of é, which IS a valid boundary
        assert_eq!(clamp_to_char_boundary(s, 1), 1);
        // Byte index 2 is the SECOND byte of é, NOT a boundary — clamps down to 1
        assert_eq!(clamp_to_char_boundary(s, 2), 1);
        // Byte index 3 is start of 'l', valid boundary
        assert_eq!(clamp_to_char_boundary(s, 3), 3);
    }

    #[test]
    fn test_clamp_to_char_boundary_emoji() {
        let s = "hi🚀"; // 🚀 is 4 bytes: byte 2-5
        assert_eq!(clamp_to_char_boundary(s, 3), 2); // middle of emoji, clamps down to start
        assert_eq!(clamp_to_char_boundary(s, 4), 2); // also middle
        assert_eq!(clamp_to_char_boundary(s, 5), 2); // also middle
        assert_eq!(clamp_to_char_boundary(s, 6), 6); // after emoji
    }

    #[test]
    fn test_error_display() {
        let err = ContextHandleError::InvalidScheme;
        assert!(err.to_string().contains("ctx://"));

        let err = ContextHandleError::EmptySegment {
            field: "session_id",
        };
        assert!(err.to_string().contains("session_id"));
    }
}
