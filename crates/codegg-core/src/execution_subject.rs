//! Durable, CodeGG-native description of the Git input observed by an execution.
use serde::{Deserialize, Serialize};

pub const EXECUTION_SUBJECT_SCHEMA_VERSION: u16 = 1;
pub const MAX_EXECUTION_SUBJECT_JSON_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionSubjectKind {
    Git,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionSubjectState {
    Clean,
    Dirty,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionSubjectRevision {
    pub schema_version: u16,
    pub subject_kind: ExecutionSubjectKind,
    pub repository_identity: String,
    pub revision: String,
    pub state: ExecutionSubjectState,
    pub dirty_digest: Option<String>,
}

impl ExecutionSubjectRevision {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != EXECUTION_SUBJECT_SCHEMA_VERSION {
            return Err("unsupported subject schema");
        }
        if self.repository_identity.is_empty()
            || self.repository_identity.len() > 256
            || self.repository_identity.contains('/')
        {
            return Err("invalid repository identity");
        }
        if self.revision.is_empty()
            || self.revision.len() > 128
            || !self.revision.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("invalid Git revision");
        }
        match (self.state, self.dirty_digest.as_deref()) {
            (ExecutionSubjectState::Clean, None) => Ok(()),
            (ExecutionSubjectState::Dirty, Some(d))
                if d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()) =>
            {
                Ok(())
            }
            _ => Err("subject state and dirty digest disagree"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectUnavailableReason {
    NotGit,
    CaptureFailed,
    UnsafePath,
    BoundsExceeded,
    LegacyMissingProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectDisposition {
    Started,
    Stable,
    Drifted,
    Unavailable(SubjectUnavailableReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectSealKind {
    LiveExecutionEnd,
    SnapshotMaterialized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionSubjectProvenance {
    pub schema_version: u16,
    pub captured: Option<ExecutionSubjectRevision>,
    pub sealed: Option<ExecutionSubjectRevision>,
    pub disposition: SubjectDisposition,
    pub seal_kind: Option<SubjectSealKind>,
}

impl ExecutionSubjectProvenance {
    pub fn unavailable(reason: SubjectUnavailableReason) -> Self {
        Self {
            schema_version: 1,
            captured: None,
            sealed: None,
            disposition: SubjectDisposition::Unavailable(reason),
            seal_kind: None,
        }
    }
    pub fn started(subject: ExecutionSubjectRevision) -> Result<Self, &'static str> {
        subject.validate()?;
        Ok(Self {
            schema_version: 1,
            captured: Some(subject),
            sealed: None,
            disposition: SubjectDisposition::Started,
            seal_kind: None,
        })
    }
    pub fn seal(
        &mut self,
        subject: ExecutionSubjectRevision,
        kind: SubjectSealKind,
    ) -> Result<(), &'static str> {
        subject.validate()?;
        let captured = self
            .captured
            .as_ref()
            .ok_or("cannot seal without start subject")?;
        if self.sealed.is_some() {
            return Err("provenance is already sealed");
        }
        self.disposition = if captured == &subject {
            SubjectDisposition::Stable
        } else {
            SubjectDisposition::Drifted
        };
        self.sealed = Some(subject);
        self.seal_kind = Some(kind);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subject(revision: &str) -> ExecutionSubjectRevision {
        ExecutionSubjectRevision {
            schema_version: 1,
            subject_kind: ExecutionSubjectKind::Git,
            repository_identity: "codegg-workspace:ws-1".into(),
            revision: revision.into(),
            state: ExecutionSubjectState::Clean,
            dirty_digest: None,
        }
    }

    #[test]
    fn start_and_seal_classifies_stable_or_drifted_and_is_terminal() {
        let mut stable = ExecutionSubjectProvenance::started(subject(&"a".repeat(40))).unwrap();
        stable
            .seal(subject(&"a".repeat(40)), SubjectSealKind::LiveExecutionEnd)
            .unwrap();
        assert_eq!(stable.disposition, SubjectDisposition::Stable);
        assert!(stable
            .seal(subject(&"a".repeat(40)), SubjectSealKind::LiveExecutionEnd)
            .is_err());

        let mut drifted = ExecutionSubjectProvenance::started(subject(&"a".repeat(40))).unwrap();
        drifted
            .seal(subject(&"b".repeat(40)), SubjectSealKind::LiveExecutionEnd)
            .unwrap();
        assert_eq!(drifted.disposition, SubjectDisposition::Drifted);
    }
}
