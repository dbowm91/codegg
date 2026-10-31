//! The legacy file-backed promotion store's in-flight records.
//!
//! M007-I (work package B) moved the durable promotion record into the
//! daemon's session storage, keyed on the canonical `WorkspaceId`. The
//! legacy store keyed its file on a **digest of a path string**, and it was
//! derived differently on each side, so a legacy record cannot be mapped
//! onto a workspace by construction.
//!
//! Such a record therefore **fails loudly**. The daemon probes for it and
//! refuses the workflow with a typed refusal naming the record, so the
//! human can act on it. A record that vanishes is a proposal the human
//! asked for and never saw again.

use std::path::{Path, PathBuf};

/// One legacy namespace's in-flight records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyPromotionProbe {
    /// The legacy file the records live in.
    pub path: PathBuf,
    /// The proposals that are neither rejected nor published.
    pub in_flight_proposals: Vec<String>,
    /// The staged requests that were never consumed.
    pub in_flight_requests: Vec<String>,
    /// Why the legacy file could not be read, when it could not.
    pub unreadable: Option<String>,
}

impl LegacyPromotionProbe {
    /// Whether the legacy store holds anything that would otherwise vanish.
    pub fn has_in_flight_records(&self) -> bool {
        !self.in_flight_proposals.is_empty() || !self.in_flight_requests.is_empty()
    }

    /// The refusal the daemon reports.
    pub fn refusal_message(&self) -> String {
        if let Some(reason) = &self.unreadable {
            return format!(
                "the legacy skill-promotion record at {} could not be read ({reason}); \
                 it must be recovered by hand rather than silently dropped",
                self.path.display()
            );
        }
        let mut message = format!(
            "the legacy skill-promotion record at {} holds {} proposal(s) awaiting review \
             and {} staged request(s); they cannot be migrated to the workspace-scoped \
             store, so they are reported rather than dropped",
            self.path.display(),
            self.in_flight_proposals.len(),
            self.in_flight_requests.len()
        );
        for id in self.in_flight_proposals.iter().chain(self.in_flight_requests.iter()) {
            message.push_str(&format!(" ({id})"));
        }
        message
    }
}

/// The legacy store's root, under the host's memory root.
///
/// The legacy store lived beside the habit candidates, so the same memory
/// root names it.
pub fn legacy_store_root(memory_root: &Path) -> PathBuf {
    memory_root.join("skill-promotions")
}

/// The legacy file one workspace's records would live in.
///
/// The legacy namespace was `project/<sha256 of the project path>`, and the
/// file was `<hex>.json` under the legacy root.
pub fn legacy_file_for(workspace_root: &Path, store_root: &Path) -> Option<PathBuf> {
    let identity = workspace_root.to_string_lossy().to_string();
    let namespace = codegg_core::memory::project_namespace(&identity);
    let suffix = namespace.strip_prefix("project/")?;
    if suffix.len() != 64 || !suffix.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(store_root.join(format!("{suffix}.json")))
}

/// Probe one workspace's legacy records.
///
/// A missing file is not a finding: there is nothing to migrate and nothing
/// to report. A file that **exists** and cannot be read is a finding: the
/// records in it are unknown, so the workflow must refuse rather than
/// assume they are empty.
pub fn probe_legacy_promotions(
    workspace_root: &Path,
    store_root: &Path,
) -> Option<LegacyPromotionProbe> {
    let path = legacy_file_for(workspace_root, store_root)?;
    if !path.is_file() {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    match serde_json::from_slice::<LegacyPromotionFile>(&bytes) {
        Ok(file) => Some(LegacyPromotionProbe {
            path,
            in_flight_proposals: file
                .proposals
                .iter()
                .filter(|proposal| !is_terminal(&proposal.status))
                .map(|proposal| proposal.id.clone())
                .collect(),
            in_flight_requests: file
                .requests
                .iter()
                .filter(|request| !request.consumed)
                .map(|request| request.id.clone())
                .collect(),
            unreadable: None,
        }),
        Err(error) => Some(LegacyPromotionProbe {
            path,
            in_flight_proposals: Vec::new(),
            in_flight_requests: Vec::new(),
            unreadable: Some(error.to_string()),
        }),
    }
}

/// A legacy proposal's status is terminal once it was rejected or published.
fn is_terminal(status: &str) -> bool {
    matches!(status, "rejected" | "published" | "superseded")
}

/// The legacy file's shape.
#[derive(Debug, serde::Deserialize)]
struct LegacyPromotionFile {
    #[serde(default)]
    requests: Vec<LegacyRequest>,
    #[serde(default)]
    proposals: Vec<LegacyProposal>,
}

#[derive(Debug, serde::Deserialize)]
struct LegacyRequest {
    #[serde(default)]
    id: String,
    #[serde(default)]
    consumed: bool,
}

#[derive(Debug, serde::Deserialize)]
struct LegacyProposal {
    #[serde(default)]
    id: String,
    #[serde(default)]
    status: String,
}
