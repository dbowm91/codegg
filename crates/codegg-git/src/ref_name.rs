use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RefError {
    #[error("empty ref name")]
    Empty,
    #[error("ref name contains illegal characters: {0}")]
    IllegalCharacters(String),
    #[error("ref name starts with '-': {0}")]
    StartsWithDash(String),
    #[error("ref contains '..': {0}")]
    DoubleDot(String),
    #[error("ref ends with '.lock': {0}")]
    LockSuffix(String),
    #[error("ref contains '~', '^', ':', '?', '*', '[', '\\': {0}")]
    SpecialCharacters(String),
    #[error("not a valid object id: {0}")]
    InvalidObjectId(String),
}

/// Validated branch name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BranchName(String);

impl BranchName {
    pub fn new(name: &str) -> Result<Self, RefError> {
        validate_ref_name(name)?;
        Ok(Self(name.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BranchName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Validated ref name (branch, tag, or symbolic ref).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RefName(String);

impl RefName {
    pub fn new(name: &str) -> Result<Self, RefError> {
        validate_ref_name(name)?;
        Ok(Self(name.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RefName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Validated remote name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RemoteName(String);

impl RemoteName {
    pub fn new(name: &str) -> Result<Self, RefError> {
        if name.is_empty() {
            return Err(RefError::Empty);
        }
        if name.starts_with('-') {
            return Err(RefError::StartsWithDash(name.to_owned()));
        }
        if name.contains('\0') || name.contains("..") || name.contains(' ') {
            return Err(RefError::IllegalCharacters(name.to_owned()));
        }
        Ok(Self(name.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RemoteName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Validated 40-char hex object id (SHA-1) or 64-char hex (SHA-256).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObjectId(String);

impl ObjectId {
    pub fn new(id: &str) -> Result<Self, RefError> {
        if id.is_empty() {
            return Err(RefError::InvalidObjectId(id.to_owned()));
        }
        if !id.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(RefError::InvalidObjectId(id.to_owned()));
        }
        if id.len() != 40 && id.len() != 64 {
            return Err(RefError::InvalidObjectId(id.to_owned()));
        }
        Ok(Self(id.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Revision expression — raw, not validated (too many forms to parse safely).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RevisionExpr(String);

impl RevisionExpr {
    pub fn new(expr: &str) -> Result<Self, RefError> {
        if expr.is_empty() {
            return Err(RefError::Empty);
        }
        Ok(Self(expr.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validate a ref name per git-check-ref-format rules (simplified).
fn validate_ref_name(name: &str) -> Result<(), RefError> {
    if name.is_empty() {
        return Err(RefError::Empty);
    }
    if name.starts_with('-') {
        return Err(RefError::StartsWithDash(name.to_owned()));
    }
    if name.contains("..") {
        return Err(RefError::DoubleDot(name.to_owned()));
    }
    if name.ends_with(".lock") {
        return Err(RefError::LockSuffix(name.to_owned()));
    }
    if name.contains(['~', '^', ':', '?', '*', '[', '\\']) {
        return Err(RefError::SpecialCharacters(name.to_owned()));
    }
    if name.contains('\0') {
        return Err(RefError::IllegalCharacters(name.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_name_valid() {
        assert_eq!(BranchName::new("main").unwrap().as_str(), "main");
    }

    #[test]
    fn branch_name_rejects_empty() {
        assert!(matches!(BranchName::new(""), Err(RefError::Empty)));
    }

    #[test]
    fn branch_name_rejects_dash() {
        assert!(matches!(
            BranchName::new("-feature"),
            Err(RefError::StartsWithDash(_))
        ));
    }

    #[test]
    fn branch_name_rejects_double_dot() {
        assert!(matches!(
            BranchName::new("a..b"),
            Err(RefError::DoubleDot(_))
        ));
    }

    #[test]
    fn branch_name_rejects_lock_suffix() {
        assert!(matches!(
            BranchName::new("main.lock"),
            Err(RefError::LockSuffix(_))
        ));
    }

    #[test]
    fn branch_name_rejects_special_chars() {
        assert!(matches!(
            BranchName::new("feat~1"),
            Err(RefError::SpecialCharacters(_))
        ));
    }

    #[test]
    fn ref_name_valid() {
        assert_eq!(RefName::new("v1.0.0").unwrap().as_str(), "v1.0.0");
    }

    #[test]
    fn remote_name_valid() {
        assert_eq!(RemoteName::new("origin").unwrap().as_str(), "origin");
    }

    #[test]
    fn remote_name_rejects_space() {
        assert!(matches!(
            RemoteName::new("my remote"),
            Err(RefError::IllegalCharacters(_))
        ));
    }

    #[test]
    fn object_id_valid_sha1() {
        let id = "a".repeat(40);
        assert_eq!(ObjectId::new(&id).unwrap().as_str(), id);
    }

    #[test]
    fn object_id_valid_sha256() {
        let id = "a".repeat(64);
        assert_eq!(ObjectId::new(&id).unwrap().as_str(), id);
    }

    #[test]
    fn object_id_rejects_wrong_length() {
        let id = "a".repeat(20);
        assert!(matches!(
            ObjectId::new(&id),
            Err(RefError::InvalidObjectId(_))
        ));
    }

    #[test]
    fn object_id_rejects_non_hex() {
        let id = format!("{:0>40}", "g");
        assert!(matches!(
            ObjectId::new(&id),
            Err(RefError::InvalidObjectId(_))
        ));
    }

    #[test]
    fn revision_expr_valid() {
        assert_eq!(RevisionExpr::new("HEAD~3").unwrap().as_str(), "HEAD~3");
    }

    #[test]
    fn revision_expr_rejects_empty() {
        assert!(matches!(RevisionExpr::new(""), Err(RefError::Empty)));
    }
}
