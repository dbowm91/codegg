//! Source-span diagnostics for the Tool Program frontend.
//!
//! Diagnostics are bounded and never echo full source bodies.

use std::fmt;

/// Diagnostic error codes for the Tool Program language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticCode {
    /// Unsupported syntax (while, try, import, class, lambda, etc.)
    UnsupportedSyntax,
    /// Unbounded loop or unknown iteration count
    UnboundedLoop,
    /// Maximum nesting depth exceeded
    MaxNestingDepth,
    /// Maximum collection/literal size exceeded
    MaxCollectionSize,
    /// Built-in shadowing of reserved name
    BuiltInShadowing,
    /// Illegal attribute access on arbitrary object
    IllegalAttributeAccess,
    /// Maximum parallel width exceeded
    MaxParallelWidth,
    /// Maximum IR steps exceeded
    MaxIrSteps,
    /// Maximum call sites exceeded
    MaxCallSites,
    /// Unresolvable identifier
    UnresolvedIdentifier,
    /// Invalid call descriptor structure
    InvalidCallDescriptor,
    /// Maximum total loop iterations exceeded
    MaxTotalIterations,
    /// Source too large
    SourceTooLarge,
    /// Maximum AST node count exceeded
    MaxAstNodes,
    /// Maximum identifier length exceeded
    MaxIdentifierLength,
    /// Unsupported compiler/language version
    UnsupportedVersion,
    /// Source body too large for diagnostic span
    DiagnosticSpanTooLarge,
    /// Destructuring assignment target count mismatch
    DestructuringMismatch,
    /// Internal compiler error
    InternalError,
    /// Verification failed
    VerificationFailed,
}

impl DiagnosticCode {
    /// Return the TP error code string (e.g., "TP001").
    pub fn code_str(&self) -> &'static str {
        match self {
            Self::UnsupportedSyntax => "TP001",
            Self::UnboundedLoop => "TP002",
            Self::MaxNestingDepth => "TP003",
            Self::MaxCollectionSize => "TP004",
            Self::BuiltInShadowing => "TP005",
            Self::IllegalAttributeAccess => "TP006",
            Self::MaxParallelWidth => "TP007",
            Self::MaxIrSteps => "TP008",
            Self::MaxCallSites => "TP009",
            Self::UnresolvedIdentifier => "TP010",
            Self::InvalidCallDescriptor => "TP011",
            Self::MaxTotalIterations => "TP012",
            Self::SourceTooLarge => "TP013",
            Self::MaxAstNodes => "TP014",
            Self::MaxIdentifierLength => "TP015",
            Self::UnsupportedVersion => "TP016",
            Self::DiagnosticSpanTooLarge => "TP017",
            Self::DestructuringMismatch => "TP018",
            Self::InternalError => "TP999",
            Self::VerificationFailed => "TP998",
        }
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code_str())
    }
}

/// A bounded source span for a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSpan {
    /// Byte offset from start of source.
    pub start: usize,
    /// Byte length of the highlighted region.
    pub length: usize,
}

impl SourceSpan {
    pub fn new(start: usize, length: usize) -> Self {
        Self { start, length }
    }

    /// Create a capped context span around this diagnostic.
    /// Returns at most `max_context_bytes` of surrounding source.
    pub fn context_range(&self, source_len: usize, max_context_bytes: usize) -> (usize, usize) {
        let half = max_context_bytes / 2;
        let ctx_start = self.start.saturating_sub(half);
        let ctx_end = (self.start + self.length + half).min(source_len);
        (ctx_start, ctx_end)
    }
}

/// A diagnostic message produced during parsing, validation, or compilation.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub message: String,
    pub span: SourceSpan,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {} (offset={}, len={})",
            self.code, self.message, self.span.start, self.span.length
        )
    }
}

impl std::error::Error for Diagnostic {}

impl Diagnostic {
    pub fn new(code: DiagnosticCode, message: impl Into<String>, span: SourceSpan) -> Self {
        Self {
            code,
            message: message.into(),
            span,
        }
    }
}
