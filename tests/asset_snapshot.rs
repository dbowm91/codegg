//! Integration tests for the unified `ProjectAssetSnapshot` runtime
//! surface (Runtime Assets Milestone 002).
//!
//! These tests exercise the production builder from outside the
//! library and assert the invariants promised by the plan: two
//! concurrently active projects never cross-contaminate; the root
//! application and daemon turn runtime can consume the unified
//! snapshot; skill discovery remains source-aware; and unchanged
//! inputs reconstruct identical fingerprints across rebuilds.

use std::fs;
use std::sync::Arc;

use codegg::agent::asset_context::{AssetContextBuilder, ProjectId};
use codegg::agent::asset_snapshot::{compute_snapshot_fingerprint, ProjectAssetSnapshot};
use codegg::agent::asset_snapshot_builder::{ProjectAssetSnapshotBuilder, SnapshotBuilderConfig};
use codegg::agent::instructions::{InstructionFragment, ProjectInstructionResolver};
use codegg::agent::resolve_agents_with_context;
use codegg::config::schema::Config;
use codegg::skills::AssetDiscoveryConfig;
use tempfile::TempDir;

fn make_config() -> Arc<Config> {
    Arc::new(Config::default())
}

fn default_builder() -> ProjectAssetSnapshotBuilder {
    ProjectAssetSnapshotBuilder::new(
        SnapshotBuilderConfig {
            asset_discovery: AssetDiscoveryConfig::default(),
        },
        make_config(),
    )
}

fn ctx_for(root: &std::path::Path) -> codegg::agent::asset_context::AssetContext {
    AssetContextBuilder::new()
        .with_synthetic_project_id(ProjectId::new())
        .with_workspace_root(root)
        .build()
        .expect("context should build for test tmpdir")
}

#[test]
fn snapshot_isolates_two_concurrent_projects() {
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();

    fs::write(a.path().join("AGENTS.md"), "project-a-only instructions").unwrap();
    fs::write(b.path().join("AGENTS.md"), "project-b-only instructions").unwrap();

    fs::create_dir_all(a.path().join(".codegg/agents")).unwrap();
    fs::create_dir_all(b.path().join(".codegg/agents")).unwrap();
    fs::write(
        a.path().join(".codegg/agents/reviewer.toml"),
        "name = \"reviewer\"\nmode = \"subagent\"\ndescription = \"a-only\"\nprompt = \"a prompt\"\n",
    )
    .unwrap();
    fs::write(
        b.path().join(".codegg/agents/reviewer.toml"),
        "name = \"reviewer\"\nmode = \"subagent\"\ndescription = \"b-only\"\nprompt = \"b prompt\"\n",
    )
    .unwrap();

    let snap_a = default_builder().build(&ctx_for(a.path())).unwrap();
    let snap_b = default_builder().build(&ctx_for(b.path())).unwrap();

    assert_ne!(snap_a.fingerprint, snap_b.fingerprint);
    assert_eq!(snap_a.instruction_block(), "project-a-only instructions");
    assert_eq!(snap_b.instruction_block(), "project-b-only instructions");

    let reviewer_a = snap_a
        .get_agent("reviewer")
        .expect("project a reviewer must be resolved");
    let reviewer_b = snap_b
        .get_agent("reviewer")
        .expect("project b reviewer must be resolved");
    assert_ne!(
        reviewer_a.content_digest(),
        reviewer_b.content_digest(),
        "same-named agents from distinct projects must have different digests"
    );
    assert!(reviewer_a
        .agent
        .system_prompt
        .as_deref()
        .unwrap_or("")
        .contains("a prompt"));
    assert!(reviewer_b
        .agent
        .system_prompt
        .as_deref()
        .unwrap_or("")
        .contains("b prompt"));
}

#[test]
fn resolve_agents_with_context_matches_snapshot_agents() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".codegg/agents")).unwrap();
    fs::write(
        dir.path().join(".codegg/agents/snapshot-vs-resolver.toml"),
        "name = \"snapshot-vs-resolver\"\nmode = \"subagent\"\ndescription = \"alignment\"\nprompt = \"shared prompt\"\n",
    )
    .unwrap();

    let ctx = ctx_for(dir.path());
    let config = make_config();
    let via_resolver =
        resolve_agents_with_context(&config, Some(dir.path())).expect("resolver must succeed");

    let snap = default_builder().build(&ctx).unwrap();

    let from_resolver = via_resolver
        .iter()
        .find(|a| a.name == "snapshot-vs-resolver")
        .expect("resolver must surface the new agent");
    let from_snapshot = snap
        .get_agent("snapshot-vs-resolver")
        .expect("snapshot must surface the new agent");

    assert_eq!(
        from_resolver.system_prompt,
        from_snapshot.agent.system_prompt
    );
    assert_eq!(from_resolver.description, from_snapshot.agent.description);
}

#[test]
fn identical_inputs_produce_identical_fingerprints_across_rebuilds() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("AGENTS.md"), "stable instructions").unwrap();
    let ctx = ctx_for(dir.path());

    let a: ProjectAssetSnapshot = default_builder().build(&ctx).unwrap();
    let b: ProjectAssetSnapshot = default_builder().build(&ctx).unwrap();
    assert_eq!(a.fingerprint, b.fingerprint);

    // Fingerprint is derived from per-asset digests only — no clock.
    let manual = compute_snapshot_fingerprint(&a.agents, &a.skills, &a.instructions);
    assert_eq!(a.fingerprint, manual);
}

#[test]
fn snapshot_reflects_instruction_walk_across_project_layers() {
    // Layout:
    //   <root>/AGENTS.md           -> OUTER
    //   <root>/sub/AGENTS.md       -> INNER
    // The walk must surface INNER before OUTER.
    let root = TempDir::new().unwrap();
    let sub = root.path().join("sub");
    fs::create_dir(&sub).unwrap();
    fs::write(root.path().join("AGENTS.md"), "OUTER").unwrap();
    fs::write(sub.join("AGENTS.md"), "INNER").unwrap();

    let ctx = ctx_for(&sub);
    let snap = default_builder().build(&ctx).unwrap();

    let frags: &[InstructionFragment] = snap.instruction_fragments();
    assert!(frags.len() >= 2, "expected INNER + OUTER, got {frags:?}");
    assert_eq!(frags[0].content, "INNER");
    assert_eq!(frags[1].content, "OUTER");
    assert!(snap.instruction_block().starts_with("INNER\n\nOUTER"));
}

#[test]
fn instruction_resolver_rejects_unrelated_paths() {
    let project = TempDir::new().unwrap();
    let unrelated = TempDir::new().unwrap();
    fs::write(project.path().join("AGENTS.md"), "PROJECT").unwrap();
    fs::write(unrelated.path().join("AGENTS.md"), "UNRELATED").unwrap();

    let ctx = ctx_for(project.path());
    let resolved = ProjectInstructionResolver::with_defaults().resolve(&ctx);
    assert!(resolved.fragments.iter().any(|f| f.content == "PROJECT"));
    assert!(!resolved.fragments.iter().any(|f| f.content == "UNRELATED"));
}

#[test]
fn changed_agent_file_changes_only_expected_fingerprint() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".codegg/agents")).unwrap();
    fs::write(
        dir.path().join(".codegg/agents/watcher.toml"),
        "name = \"watcher\"\nmode = \"subagent\"\ndescription = \"v1\"\nprompt = \"v1 prompt\"\n",
    )
    .unwrap();
    let ctx = ctx_for(dir.path());
    let before = default_builder().build(&ctx).unwrap();

    fs::write(
        dir.path().join(".codegg/agents/watcher.toml"),
        "name = \"watcher\"\nmode = \"subagent\"\ndescription = \"v2\"\nprompt = \"v2 prompt\"\n",
    )
    .unwrap();
    let after = default_builder().build(&ctx).unwrap();

    assert_ne!(before.fingerprint, after.fingerprint);
    let before_watcher = before.get_agent("watcher").unwrap();
    let after_watcher = after.get_agent("watcher").unwrap();
    assert_ne!(
        before_watcher.content_digest(),
        after_watcher.content_digest()
    );
    assert_eq!(before_watcher.agent.description, "v1");
    assert_eq!(after_watcher.agent.description, "v2");
}

#[test]
fn snapshot_skills_reflect_project_source_aware_discovery() {
    let dir = TempDir::new().unwrap();
    let agents_dir = dir.path().join(".agents/skills/agents-only-skill");
    fs::create_dir_all(&agents_dir).unwrap();
    fs::write(
        agents_dir.join("SKILL.md"),
        "---\nname: agents-only-skill\ndescription: only in .agents\n---\n",
    )
    .unwrap();

    let ctx = ctx_for(dir.path());
    let snap = default_builder().build(&ctx).unwrap();
    assert!(
        snap.skills
            .effective
            .iter()
            .any(|s| s.normalized_name == "agents-only-skill"),
        "snapshot must surface the .agents/-only skill"
    );
}
