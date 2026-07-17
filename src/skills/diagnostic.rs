use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Error => write!(f, "error"),
            Severity::Warning => write!(f, "warning"),
            Severity::Info => write!(f, "info"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub reason: String,
    pub location: Option<String>,
}

impl Diagnostic {
    pub fn error(reason: impl Into<String>, location: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            reason: reason.into(),
            location: Some(location.into()),
        }
    }

    pub fn warning(reason: impl Into<String>, location: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            reason: reason.into(),
            location: Some(location.into()),
        }
    }

    pub fn info(reason: impl Into<String>, location: impl Into<String>) -> Self {
        Self {
            severity: Severity::Info,
            reason: reason.into(),
            location: Some(location.into()),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.location {
            Some(loc) => write!(f, "[{}] {}: {}", self.severity, loc, self.reason),
            None => write!(f, "[{}] {}", self.severity, self.reason),
        }
    }
}
