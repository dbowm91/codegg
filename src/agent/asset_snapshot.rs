//! Unified project/workspace-scoped asset snapshot.
//!
//! Runtime Assets Milestone 2 produces one immutable
//! [`ProjectAssetSnapshot`] per workspace. The snapshot bundles the
//! resolved effective agents, the source-aware skills
//! [`AssetRegistry`], the resolved project instructions, their
//! per-asset digests, and a combined fingerprint.
//!
//! The snapshot is the only object that daemon/runtime consumers of
//! agents, skills, and instructions should hold. Building a snapshot is
//! the single disk-touching operation; everything downstream consumes
//! the immutable view.
//!
//! Milestone 3 will own the publication generation and refresh
//! coordination that swaps snapshots in. This module does not claim
//! generation ownership.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::agent::asset_context::AssetContext;
use crate::agent::instructions::{InstructionDiagnostic, InstructionFragment};
use crate::agent::registry::{AgentDiagnostic, ResolvedAgent};
use crate::skills::AssetRegistry;

/// One immutable snapshot of all project/workspace runtime assets.
#[derive(Debug, Clone)]
pub struct ProjectAssetSnapshot {
    /// Explicit context this snapshot was built from. The context is
    /// retained for diagnostics and for snapshot equality checks; it is
    /// never used as a project identity (the canonical `ProjectId`
    /// comes from `ProjectStorage`).
    pub context: Arc<AssetContext>,
    /// Effective agents indexed by normalized name.
    pub agents: BTreeMap<String, ResolvedAgent>,
    /// All agent-resolution diagnostics collected during the build.
    pub agent_diagnostics: Vec<AgentDiagnostic>,
    /// Source-aware skills registry.
    pub skills: Arc<AssetRegistry>,
    /// Resolved project-instruction fragments in deterministic order.
    pub instructions: Vec<InstructionFragment>,
    /// Effective merged instruction text (empty when no fragments).
    pub instruction_text: String,
    /// Per-fragment instruction diagnostics.
    pub instruction_diagnostics: Vec<InstructionDiagnostic>,
    /// Combined snapshot fingerprint. Stable across unchanged builds.
    pub fingerprint: String,
    /// Build metadata. Not part of the fingerprint.
    pub build_metadata: SnapshotBuildMetadata,
}

/// Immutable runtime-asset identity captured at an execution boundary.
///
/// The snapshot itself is retained by the turn as an `Arc`, while this small
/// value is the bounded audit record that can cross turn/agent-run and
/// durable-metadata boundaries.  Skill digests are indexed by normalized
/// name, so a later skill activation can be recorded without rereading the
/// workspace or trusting a mutable registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeAssetPin {
    pub generation: u64,
    pub fingerprint: String,
    #[serde(default)]
    pub skill_digests: BTreeMap<String, String>,
    #[serde(default)]
    pub activated_skill_digests: BTreeMap<String, String>,
}

impl RuntimeAssetPin {
    pub const MAX_SKILL_DIGESTS: usize = 256;

    pub fn from_snapshot(generation: u64, snapshot: &ProjectAssetSnapshot) -> Self {
        let skill_digests = snapshot
            .skills
            .effective
            .iter()
            .take(Self::MAX_SKILL_DIGESTS)
            .map(|skill| (skill.normalized_name.clone(), skill.content_digest.clone()))
            .collect();
        Self {
            generation,
            fingerprint: snapshot.fingerprint.clone(),
            skill_digests,
            activated_skill_digests: BTreeMap::new(),
        }
    }

    /// Record a skill activation against the captured digest.  This never
    /// resolves a path or reads the filesystem.  Unknown skills are rejected
    /// so an audit record cannot be forged by naming an asset absent from the
    /// pinned snapshot.
    pub fn record_skill_activation(&mut self, name: &str) -> Result<&str, RuntimeAssetPinError> {
        let normalized = name.trim().to_lowercase();
        let digest = self
            .skill_digests
            .get(&normalized)
            .ok_or_else(|| RuntimeAssetPinError::UnknownSkill(normalized.clone()))?;
        if self.activated_skill_digests.len() >= Self::MAX_SKILL_DIGESTS
            && !self.activated_skill_digests.contains_key(&normalized)
        {
            return Err(RuntimeAssetPinError::ActivationLimit);
        }
        self.activated_skill_digests
            .insert(normalized, digest.clone());
        Ok(digest)
    }

    pub fn activated_skill_digest(&self, name: &str) -> Option<&str> {
        self.activated_skill_digests
            .get(&name.trim().to_lowercase())
            .map(String::as_str)
    }

    /// Convert the captured identity to the additive durable run-store
    /// projection. Only digests and generation metadata cross the boundary.
    pub fn to_run_provenance(&self) -> codegg_core::run_store::RunAssetProvenance {
        codegg_core::run_store::RunAssetProvenance {
            generation: Some(self.generation),
            fingerprint: Some(self.fingerprint.clone()),
            activated_skill_digests: self.activated_skill_digests.values().cloned().collect(),
        }
        .bounded()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeAssetPinError {
    #[error("skill '{0}' is not present in the pinned snapshot")]
    UnknownSkill(String),
    #[error("activated skill digest limit reached")]
    ActivationLimit,
}

impl ProjectAssetSnapshot {
    pub fn runtime_asset_pin(&self, generation: u64) -> RuntimeAssetPin {
        RuntimeAssetPin::from_snapshot(generation, self)
    }
}

/// Build metadata for a snapshot. Not part of the snapshot fingerprint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotBuildMetadata {
    pub built_at: DateTime<Utc>,
    pub build_duration: Duration,
    pub config_revision: u64,
}

impl ProjectAssetSnapshot {
    /// Total number of effective agents.
    pub fn agent_count(&self) -> usize {
        self.agents.len()
    }

    /// Look up an agent by exact name.
    pub fn get_agent(&self, name: &str) -> Option<&ResolvedAgent> {
        self.agents.get(name)
    }

    /// Iterator over all effective agents in deterministic order.
    pub fn agents(&self) -> impl Iterator<Item = (&str, &ResolvedAgent)> {
        self.agents.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Iterate over instruction fragments in deterministic order.
    pub fn instruction_fragments(&self) -> &[InstructionFragment] {
        &self.instructions
    }

    /// Build the public instruction block for inclusion in system prompts.
    pub fn instruction_block(&self) -> &str {
        &self.instruction_text
    }

    /// Build the system-prompt skill listing from the embedded skills
    /// registry.
    pub fn build_skill_prompt(&self) -> String {
        self.skills.build_system_prompt()
    }
}

/// Construct a stable fingerprint from a snapshot's resolved content.
///
/// The fingerprint is derived from sorted, semantically meaningful fields
/// only: agent digests, skill digests, instruction digests. It must not
/// depend on wall-clock time, map iteration order, or absolute paths
/// (paths live in provenance only).
pub fn compute_snapshot_fingerprint(
    agents: &BTreeMap<String, ResolvedAgent>,
    skills: &AssetRegistry,
    instructions: &[InstructionFragment],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"agents\n");
    for (name, agent) in agents {
        hasher.update(name.as_bytes());
        hasher.update(b":");
        hasher.update(agent.content_digest().as_bytes());
        hasher.update(b"\n");
    }
    hasher.update(b"skills\n");
    for skill in skills.effective.iter() {
        hasher.update(skill.normalized_name.as_bytes());
        hasher.update(b":");
        hasher.update(skill.content_digest.as_bytes());
        hasher.update(b"\n");
    }
    hasher.update(b"instructions\n");
    for frag in instructions {
        hasher.update(frag.content_digest.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

/// Internal builder inputs that the snapshot builder produces before
/// assembling the immutable snapshot.
#[derive(Debug)]
pub(crate) struct BuiltSnapshot {
    pub agents: BTreeMap<String, ResolvedAgent>,
    pub agent_diagnostics: Vec<AgentDiagnostic>,
    pub skills: Arc<AssetRegistry>,
    pub instructions: Vec<InstructionFragment>,
    pub instruction_text: String,
    pub instruction_diagnostics: Vec<InstructionDiagnostic>,
    pub build_metadata: SnapshotBuildMetadata,
}

impl BuiltSnapshot {
    pub fn assemble(self, context: Arc<AssetContext>) -> ProjectAssetSnapshot {
        let fingerprint =
            compute_snapshot_fingerprint(&self.agents, &self.skills, &self.instructions);
        ProjectAssetSnapshot {
            context,
            agents: self.agents,
            agent_diagnostics: self.agent_diagnostics,
            skills: self.skills,
            instructions: self.instructions,
            instruction_text: self.instruction_text,
            instruction_diagnostics: self.instruction_diagnostics,
            fingerprint,
            build_metadata: self.build_metadata,
        }
    }
}

/// Surface area for the snapshot builder. The builder is created by
/// [`crate::agent::asset_snapshot_builder::ProjectAssetSnapshotBuilder`].
pub trait SnapshotBuilder: Send + Sync {
    fn build(&self, ctx: &AssetContext) -> Result<ProjectAssetSnapshot, SnapshotBuildError>;
}

/// Errors produced during snapshot construction.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SnapshotBuildError {
    #[error("invalid context: {0}")]
    Context(String),
    #[error("agent resolution failed: {0}")]
    Agent(String),
    #[error("skill resolution failed: {0}")]
    Skill(String),
    #[error("instruction resolution failed: {0}")]
    Instruction(String),
    #[error("agent digest missing for agent '{0}'")]
    MissingAgentDigest(String),
}

/// Project-root-relative path used for diagnostics.
pub fn short_path(path: &std::path::Path) -> String {
    path.file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

/// Helper for tests: returns the project root derived from the context.
pub fn workspace_root(ctx: &AssetContext) -> PathBuf {
    ctx.workspace_root().to_path_buf()
}

/// Helper for tests: returns all global roots derived from the context.
pub fn global_roots(ctx: &AssetContext) -> Vec<PathBuf> {
    ctx.global_roots().to_vec()
}

/// Re-export [`InstructionResolution`] for callers that build a snapshot
/// piecemeal (mostly for tests and the builder seam).
pub use crate::agent::instructions::InstructionResolution as ResolvedInstructions;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_asset_pin_records_only_known_skill_digests() {
        let mut pin = RuntimeAssetPin {
            generation: 4,
            fingerprint: "snapshot-fingerprint".to_string(),
            skill_digests: BTreeMap::from([("review".to_string(), "digest-1".to_string())]),
            activated_skill_digests: BTreeMap::new(),
        };

        assert_eq!(pin.record_skill_activation("REVIEW").unwrap(), "digest-1");
        assert_eq!(pin.activated_skill_digest("review"), Some("digest-1"));
        assert!(matches!(
            pin.record_skill_activation("missing"),
            Err(RuntimeAssetPinError::UnknownSkill(_))
        ));

        let encoded = serde_json::to_string(&pin).unwrap();
        let decoded: RuntimeAssetPin = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, pin);
        assert!(!encoded.contains("/"));
        let provenance = pin.to_run_provenance();
        assert_eq!(provenance.generation, Some(4));
        assert_eq!(
            provenance.fingerprint.as_deref(),
            Some("snapshot-fingerprint")
        );
        assert_eq!(provenance.activated_skill_digests, vec!["digest-1"]);
    }
}
