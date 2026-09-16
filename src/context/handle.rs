use std::fmt;

/// Typed parser for `ctx://` artifact handles.
///
/// Handles have one of the forms:
/// `ctx://tool/{session_id}/{turn_index}/{tool_call_id}`
/// `ctx://evidence/{session_id}/{checkpoint_id}/{evidence_id}`
///
/// Tool handles are wire-compatible with all historical runtimes. Evidence
/// handles are new in M003 and resolve through the same durable
/// `FileArtifactStore` via `context_read` with exact same-session matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextHandle {
    pub session_id: String,
    pub kind: ContextHandleKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextHandleKind {
    Tool {
        turn_index: usize,
        tool_call_id: String,
    },
    Evidence {
        checkpoint_id: String,
        evidence_id: String,
    },
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
    UnsafeSegment {
        field: &'static str,
        character: char,
    },
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

impl ContextHandleKind {
    /// Stable kind discriminator used in the wire format.
    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::Tool { .. } => "tool",
            Self::Evidence { .. } => "evidence",
        }
    }

    pub fn is_tool(&self) -> bool {
        matches!(self, Self::Tool { .. })
    }

    pub fn is_evidence(&self) -> bool {
        matches!(self, Self::Evidence { .. })
    }
}

impl ContextHandle {
    /// Parse a `ctx://` handle string into a typed [`ContextHandle`].
    ///
    /// Accepted forms (exactly four `/`-separated segments after `ctx://`):
    /// `ctx://tool/{session_id}/{turn_index}/{tool_call_id}`
    /// `ctx://evidence/{session_id}/{checkpoint_id}/{evidence_id}`
    pub fn parse(input: &str) -> Result<Self, ContextHandleError> {
        let rest = input
            .strip_prefix("ctx://")
            .ok_or(ContextHandleError::InvalidScheme)?;

        let parts: Vec<&str> = rest.split('/').collect();

        if parts.len() < 4 {
            return Err(ContextHandleError::MissingSegments);
        }
        if parts.len() > 4 {
            return Err(ContextHandleError::ExtraSegments);
        }

        let mut parts = parts.into_iter();
        let kind_str = parts.next().ok_or(ContextHandleError::MissingSegments)?;
        let session_id = parts.next().ok_or(ContextHandleError::MissingSegments)?;
        let third = parts.next().ok_or(ContextHandleError::MissingSegments)?;
        let fourth = parts.next().ok_or(ContextHandleError::MissingSegments)?;

        if session_id.is_empty() {
            return Err(ContextHandleError::EmptySegment {
                field: "session_id",
            });
        }
        Self::check_segment_safe(session_id, "session_id")?;

        match kind_str {
            "tool" => {
                if fourth.is_empty() {
                    return Err(ContextHandleError::EmptySegment {
                        field: "tool_call_id",
                    });
                }
                Self::check_segment_safe(fourth, "tool_call_id")?;
                let turn_index: usize = third
                    .parse()
                    .map_err(|_| ContextHandleError::InvalidTurnIndex(third.to_string()))?;
                Ok(Self {
                    session_id: session_id.to_string(),
                    kind: ContextHandleKind::Tool {
                        turn_index,
                        tool_call_id: fourth.to_string(),
                    },
                })
            }
            "evidence" => {
                if third.is_empty() {
                    return Err(ContextHandleError::EmptySegment {
                        field: "checkpoint_id",
                    });
                }
                if fourth.is_empty() {
                    return Err(ContextHandleError::EmptySegment {
                        field: "evidence_id",
                    });
                }
                Self::check_segment_safe(third, "checkpoint_id")?;
                Self::check_segment_safe(fourth, "evidence_id")?;
                Ok(Self {
                    session_id: session_id.to_string(),
                    kind: ContextHandleKind::Evidence {
                        checkpoint_id: third.to_string(),
                        evidence_id: fourth.to_string(),
                    },
                })
            }
            other => Err(ContextHandleError::UnsupportedKind(other.to_string())),
        }
    }

    /// Build a tool handle string. Returns error if segments contain unsafe characters.
    ///
    /// Wire format is unchanged from all historical runtimes:
    /// `ctx://tool/{session_id}/{turn_index}/{tool_call_id}`.
    pub fn build_tool(
        session_id: &str,
        turn_index: usize,
        tool_call_id: &str,
    ) -> Result<String, ContextHandleError> {
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
        Self::check_segment_safe(session_id, "session_id")?;
        Self::check_segment_safe(tool_call_id, "tool_call_id")?;
        Ok(format!(
            "ctx://tool/{session_id}/{turn_index}/{tool_call_id}"
        ))
    }

    /// Build a checkpoint-scoped evidence handle string (M003).
    ///
    /// Format: `ctx://evidence/{session_id}/{checkpoint_id}/{evidence_id}`.
    /// `evidence_id` must be a deterministic host-chosen identity (ordinal /
    /// kind plus digest), never a model-invented filesystem path.
    pub fn build_evidence(
        session_id: &str,
        checkpoint_id: &str,
        evidence_id: &str,
    ) -> Result<String, ContextHandleError> {
        if session_id.is_empty() {
            return Err(ContextHandleError::EmptySegment {
                field: "session_id",
            });
        }
        if checkpoint_id.is_empty() {
            return Err(ContextHandleError::EmptySegment {
                field: "checkpoint_id",
            });
        }
        if evidence_id.is_empty() {
            return Err(ContextHandleError::EmptySegment {
                field: "evidence_id",
            });
        }
        Self::check_segment_safe(session_id, "session_id")?;
        Self::check_segment_safe(checkpoint_id, "checkpoint_id")?;
        Self::check_segment_safe(evidence_id, "evidence_id")?;
        Ok(format!(
            "ctx://evidence/{session_id}/{checkpoint_id}/{evidence_id}"
        ))
    }

    /// Render this handle back to its canonical wire string.
    pub fn render(&self) -> String {
        match &self.kind {
            ContextHandleKind::Tool {
                turn_index,
                tool_call_id,
            } => format!(
                "ctx://tool/{}/{}/{}",
                self.session_id, turn_index, tool_call_id
            ),
            ContextHandleKind::Evidence {
                checkpoint_id,
                evidence_id,
            } => format!(
                "ctx://evidence/{}/{}/{}",
                self.session_id, checkpoint_id, evidence_id
            ),
        }
    }

    /// Convenience accessor preserving the historical tool-handle call sites.
    pub fn turn_index(&self) -> Option<usize> {
        match &self.kind {
            ContextHandleKind::Tool { turn_index, .. } => Some(*turn_index),
            ContextHandleKind::Evidence { .. } => None,
        }
    }

    /// Convenience accessor preserving the historical tool-handle call sites.
    pub fn tool_call_id(&self) -> Option<&str> {
        match &self.kind {
            ContextHandleKind::Tool { tool_call_id, .. } => Some(tool_call_id),
            ContextHandleKind::Evidence { .. } => None,
        }
    }

    pub fn checkpoint_id(&self) -> Option<&str> {
        match &self.kind {
            ContextHandleKind::Evidence { checkpoint_id, .. } => Some(checkpoint_id),
            ContextHandleKind::Tool { .. } => None,
        }
    }

    pub fn evidence_id(&self) -> Option<&str> {
        match &self.kind {
            ContextHandleKind::Evidence { evidence_id, .. } => Some(evidence_id),
            ContextHandleKind::Tool { .. } => None,
        }
    }

    pub fn is_tool(&self) -> bool {
        self.kind.is_tool()
    }

    pub fn is_evidence(&self) -> bool {
        self.kind.is_evidence()
    }

    /// Check that a session_id matches exactly (not substring).
    pub fn same_session(&self, session_id: &str) -> bool {
        self.session_id == session_id
    }

    /// Check that a segment contains no `/`, control characters, or whitespace.
    fn check_segment_safe(segment: &str, field: &'static str) -> Result<(), ContextHandleError> {
        for ch in segment.chars() {
            if ch == '/' || ch.is_control() || ch.is_whitespace() {
                return Err(ContextHandleError::UnsafeSegment {
                    field,
                    character: ch,
                });
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
        assert_eq!(
            h.kind,
            ContextHandleKind::Tool {
                turn_index: 5,
                tool_call_id: "call_abc".to_string(),
            }
        );
        assert_eq!(h.session_id, "sess123");
        assert_eq!(h.turn_index(), Some(5));
        assert_eq!(h.tool_call_id(), Some("call_abc"));
        assert!(h.is_tool());
        assert!(!h.is_evidence());
    }

    #[test]
    fn test_parse_turn_zero() {
        let h = ContextHandle::parse("ctx://tool/s1/0/c1").unwrap();
        assert_eq!(h.turn_index(), Some(0));
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
        assert_eq!(err, ContextHandleError::UnsupportedKind("file".to_string()));
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
        assert_eq!(err, ContextHandleError::InvalidTurnIndex("abc".to_string()));
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
        assert_eq!(parsed.turn_index(), Some(42));
        assert_eq!(parsed.tool_call_id(), Some("call-xyz"));
        assert_eq!(parsed.render(), handle_str);
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

    #[test]
    fn test_parse_evidence_valid() {
        let h = ContextHandle::parse("ctx://evidence/sess1/ckpt-abc/ev-0001").unwrap();
        assert_eq!(h.session_id, "sess1");
        assert_eq!(h.checkpoint_id(), Some("ckpt-abc"));
        assert_eq!(h.evidence_id(), Some("ev-0001"));
        assert!(h.is_evidence());
        assert!(!h.is_tool());
        assert_eq!(h.turn_index(), None);
        assert_eq!(h.tool_call_id(), None);
        assert_eq!(h.render(), "ctx://evidence/sess1/ckpt-abc/ev-0001");
    }

    #[test]
    fn test_build_evidence_roundtrip() {
        let handle_str =
            ContextHandle::build_evidence("sess1", "ckpt-1", "ev-user-0000-abc123").unwrap();
        assert_eq!(
            handle_str,
            "ctx://evidence/sess1/ckpt-1/ev-user-0000-abc123"
        );
        let parsed = ContextHandle::parse(&handle_str).unwrap();
        assert_eq!(parsed.session_id, "sess1");
        assert_eq!(parsed.checkpoint_id(), Some("ckpt-1"));
        assert_eq!(parsed.evidence_id(), Some("ev-user-0000-abc123"));
    }

    #[test]
    fn test_tool_and_evidence_namespaces_do_not_collide() {
        let tool = ContextHandle::parse("ctx://tool/s1/0/c1").unwrap();
        let evidence = ContextHandle::parse("ctx://evidence/s1/ckpt-1/ev-0001").unwrap();
        assert!(tool.is_tool());
        assert!(evidence.is_evidence());
        assert_ne!(tool.render(), evidence.render());
    }

    #[test]
    fn test_parse_evidence_missing_segments() {
        let err = ContextHandle::parse("ctx://evidence/s1/ckpt-1").unwrap_err();
        assert_eq!(err, ContextHandleError::MissingSegments);
    }

    #[test]
    fn test_parse_evidence_extra_segments() {
        let err = ContextHandle::parse("ctx://evidence/s1/ckpt-1/ev-1/extra").unwrap_err();
        assert_eq!(err, ContextHandleError::ExtraSegments);
    }

    #[test]
    fn test_parse_evidence_empty_checkpoint() {
        let err = ContextHandle::parse("ctx://evidence/s1//ev-1").unwrap_err();
        assert_eq!(
            err,
            ContextHandleError::EmptySegment {
                field: "checkpoint_id"
            }
        );
    }

    #[test]
    fn test_parse_evidence_empty_evidence_id() {
        let err = ContextHandle::parse("ctx://evidence/s1/ckpt-1/").unwrap_err();
        assert_eq!(
            err,
            ContextHandleError::EmptySegment {
                field: "evidence_id"
            }
        );
    }

    #[test]
    fn test_build_evidence_rejects_unsafe_ids() {
        let err = ContextHandle::build_evidence("s1", "ckpt/1", "ev-1").unwrap_err();
        assert_eq!(
            err,
            ContextHandleError::UnsafeSegment {
                field: "checkpoint_id",
                character: '/'
            }
        );
        let err = ContextHandle::build_evidence("s1", "ckpt-1", "ev 1").unwrap_err();
        assert_eq!(
            err,
            ContextHandleError::UnsafeSegment {
                field: "evidence_id",
                character: ' '
            }
        );
        let err = ContextHandle::build_evidence("s 1", "ckpt-1", "ev-1").unwrap_err();
        assert_eq!(
            err,
            ContextHandleError::UnsafeSegment {
                field: "session_id",
                character: ' '
            }
        );
    }

    #[test]
    fn test_build_tool_rejects_empty_segments() {
        let err = ContextHandle::build_tool("", 0, "c1").unwrap_err();
        assert_eq!(
            err,
            ContextHandleError::EmptySegment {
                field: "session_id"
            }
        );
        let err = ContextHandle::build_tool("s1", 0, "").unwrap_err();
        assert_eq!(
            err,
            ContextHandleError::EmptySegment {
                field: "tool_call_id"
            }
        );
    }

    #[test]
    fn test_build_evidence_rejects_empty_segments() {
        let err = ContextHandle::build_evidence("", "ckpt-1", "ev-1").unwrap_err();
        assert_eq!(
            err,
            ContextHandleError::EmptySegment {
                field: "session_id"
            }
        );
        let err = ContextHandle::build_evidence("s1", "", "ev-1").unwrap_err();
        assert_eq!(
            err,
            ContextHandleError::EmptySegment {
                field: "checkpoint_id"
            }
        );
        let err = ContextHandle::build_evidence("s1", "ckpt-1", "").unwrap_err();
        assert_eq!(
            err,
            ContextHandleError::EmptySegment {
                field: "evidence_id"
            }
        );
    }

    #[test]
    fn test_evidence_same_session_exact() {
        let h = ContextHandle::parse("ctx://evidence/s1/ckpt-1/ev-1").unwrap();
        assert!(h.same_session("s1"));
        assert!(!h.same_session("s"));
        assert!(!h.same_session("s10"));
        assert!(!h.same_session("not-s1"));
    }

    #[test]
    fn test_evidence_substring_attack() {
        let h = ContextHandle::parse("ctx://evidence/not-s1/ckpt-1/ev-1").unwrap();
        assert!(!h.same_session("s1"));
    }
}
