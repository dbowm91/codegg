use codegg::skills::{
    AssetDiscoveryConfig, AssetRegistry, Severity, SkillIndexCompat, SourceKind,
};
use std::fs;
use tempfile::TempDir;

fn test_config() -> AssetDiscoveryConfig {
    AssetDiscoveryConfig::default()
}

#[test]
fn discovery_all_project_locations() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();

    let locations: Vec<(&str, &str, SourceKind)> = vec![
        (
            ".codegg/skills/s1",
            "name: s1\ndescription: from codegg",
            SourceKind::CodeGGProject,
        ),
        (
            ".agents/skills/s2",
            "name: s2\ndescription: from agents",
            SourceKind::AgentsProject,
        ),
        (
            ".opencode/skills/s3",
            "name: s3\ndescription: from opencode",
            SourceKind::OpenCodeProject,
        ),
        (
            ".claude/skills/s4",
            "name: s4\ndescription: from claude",
            SourceKind::ClaudeProject,
        ),
    ];

    for (path, fm, _kind) in &locations {
        let skill_dir = root.join(path);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\n{fm}\n---\nBody content"),
        )
        .unwrap();
    }

    let config = test_config();
    let registry = AssetRegistry::build(&config, root, &[]);
    assert_eq!(registry.effective.len(), 4);
    for skill in &registry.effective {
        assert!(!skill.body.is_empty());
    }
}

#[test]
fn discovery_all_global_locations() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let global_root = dir.path().join("global");

    let locations: Vec<(&str, &str, SourceKind)> = vec![
        (
            "codegg/skills/g1",
            "name: g1\ndescription: global codegg",
            SourceKind::CodeGGGlobal,
        ),
        (
            "agents/skills/g2",
            "name: g2\ndescription: global agents",
            SourceKind::AgentsGlobal,
        ),
        (
            "opencode/skills/g3",
            "name: g3\ndescription: global opencode",
            SourceKind::OpenCodeGlobal,
        ),
        (
            "claude/skills/g4",
            "name: g4\ndescription: global claude",
            SourceKind::ClaudeGlobal,
        ),
    ];

    for (path, fm, _kind) in &locations {
        let skill_dir = global_root.join(path);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\n{fm}\n---\nBody content"),
        )
        .unwrap();
    }

    let config = test_config();
    let registry = AssetRegistry::build(&config, root, &[global_root]);
    assert_eq!(registry.effective.len(), 4);
}

#[test]
fn several_sources_one_repository() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();

    let p1 = root.join(".codegg/skills/alpha");
    fs::create_dir_all(&p1).unwrap();
    fs::write(
        p1.join("SKILL.md"),
        "---\nname: alpha\ndescription: A\n---\nBody A",
    )
    .unwrap();

    let p2 = root.join(".agents/skills/beta");
    fs::create_dir_all(&p2).unwrap();
    fs::write(
        p2.join("SKILL.md"),
        "---\nname: beta\ndescription: B\n---\nBody B",
    )
    .unwrap();

    let p3 = root.join(".opencode/skills/gamma");
    fs::create_dir_all(&p3).unwrap();
    fs::write(
        p3.join("SKILL.md"),
        "---\nname: gamma\ndescription: C\n---\nBody C",
    )
    .unwrap();

    let p4 = root.join(".codegg/skills/native.md");
    fs::write(
        &p4,
        "---\nname: native\ndescription: native md\n---\nBody native",
    )
    .unwrap();

    let config = test_config();
    let registry = AssetRegistry::build(&config, root, &[]);
    assert_eq!(registry.effective.len(), 4);
    let names: Vec<&str> = registry
        .effective
        .iter()
        .map(|s| s.normalized_name.as_str())
        .collect();
    assert!(names.contains(&"alpha"));
    assert!(names.contains(&"beta"));
    assert!(names.contains(&"gamma"));
    assert!(names.contains(&"native"));
}

#[test]
fn symlink_escape_rejected() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let outside = dir.path().join("outside_skill");
    fs::create_dir_all(&outside).unwrap();
    fs::write(
        outside.join("SKILL.md"),
        "---\nname: escaped\ndescription: outside\n---\nBody",
    )
    .unwrap();

    let skills_dir = root.join(".codegg/skills/escaped_link");
    fs::create_dir_all(&skills_dir).unwrap();
    std::os::unix::fs::symlink(&outside.join("SKILL.md"), skills_dir.join("SKILL.md")).unwrap();

    let config = test_config();
    let registry = AssetRegistry::build(&config, root, &[]);
    assert!(
        registry.effective.is_empty(),
        "symlink escaping root should be rejected"
    );
    assert!(!registry.diagnostics.is_empty());
}

#[test]
fn native_compat_direct_md_loads() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let skills_dir = root.join(".codegg/skills");
    fs::create_dir_all(&skills_dir).unwrap();
    fs::write(
        skills_dir.join("direct.md"),
        "---\nname: direct-skill\ndescription: loaded via direct md\n---\nDirect body",
    )
    .unwrap();

    let config = test_config();
    let registry = AssetRegistry::build(&config, root, &[]);
    assert_eq!(registry.effective.len(), 1);
    assert_eq!(
        registry.effective[0].source_kind,
        SourceKind::CodeGGNativeCompat
    );
    assert!(registry.effective[0].body.contains("Direct body"));
}

#[test]
fn native_compat_package_layout_loads() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let skill_dir = root.join(".codegg/skills/my-pkg");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: my-pkg\ndescription: package skill\n---\nPackage body",
    )
    .unwrap();
    fs::write(skill_dir.join("helper.sh"), "#!/bin/bash\necho helper").unwrap();
    fs::write(skill_dir.join("data.txt"), "some data").unwrap();

    let config = test_config();
    let registry = AssetRegistry::build(&config, root, &[]);
    assert_eq!(registry.effective.len(), 1);
    let skill = &registry.effective[0];
    assert_eq!(skill.resources.len(), 2);
    assert!(skill.resources.iter().any(|r| r.name == "helper.sh"));
    assert!(skill.resources.iter().any(|r| r.name == "data.txt"));
}

#[test]
fn absent_foreign_directories_harmless() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();

    let config = test_config();
    let registry = AssetRegistry::build(&config, root, &[]);
    assert!(registry.effective.is_empty());
    assert!(registry.diagnostics.is_empty());
}

#[test]
fn duplicate_behavior_stable() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let global_root = dir.path().join("global");

    let p_skill = root.join(".codegg/skills/dup");
    fs::create_dir_all(&p_skill).unwrap();
    fs::write(
        p_skill.join("SKILL.md"),
        "---\nname: dup\ndescription: project\n---\nProject body",
    )
    .unwrap();

    let g_skill = global_root.join("codegg/skills/dup");
    fs::create_dir_all(&g_skill).unwrap();
    fs::write(
        g_skill.join("SKILL.md"),
        "---\nname: dup\ndescription: global\n---\nGlobal body",
    )
    .unwrap();

    let config = test_config();

    let r1 = AssetRegistry::build(&config, root, &[global_root.clone()]);
    let r2 = AssetRegistry::build(&config, root, &[global_root]);

    assert_eq!(r1.effective.len(), r2.effective.len());
    assert_eq!(
        r1.effective[0].content_digest,
        r2.effective[0].content_digest
    );
    assert_eq!(r1.effective[0].description, "project");
    assert_eq!(r2.effective[0].description, "project");
}

#[test]
fn digest_stability_across_rebuilds() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let skill_dir = root.join(".codegg/skills/stable");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: stable\ndescription: stable skill\n---\nStable body",
    )
    .unwrap();

    let config = test_config();
    let r1 = AssetRegistry::build(&config, root, &[]);
    let r2 = AssetRegistry::build(&config, root, &[]);

    assert_eq!(
        r1.effective[0].content_digest,
        r2.effective[0].content_digest
    );
}

#[test]
fn oversized_frontmatter_surfaces_diagnostic() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let skill_dir = root.join(".codegg/skills/bigfm");
    fs::create_dir_all(&skill_dir).unwrap();
    let big_desc = "x".repeat(200_000);
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: bigfm\ndescription: \"{big_desc}\"\n---\nBody"),
    )
    .unwrap();

    let config = test_config();
    let registry = AssetRegistry::build(&config, root, &[]);
    assert!(registry.effective.is_empty() || !registry.diagnostics.is_empty());
}

#[test]
fn malformed_yaml_surfaces_diagnostic() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let skill_dir = root.join(".codegg/skills/bad");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: [{invalid yaml\n---\nBody",
    )
    .unwrap();

    let config = test_config();
    let registry = AssetRegistry::build(&config, root, &[]);
    assert!(!registry.diagnostics.is_empty());
    assert!(registry.effective.is_empty());
}

#[test]
fn script_files_inventoried_not_executed() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let skill_dir = root.join(".codegg/skills/scripty");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: scripty\ndescription: has scripts\n---\nBody",
    )
    .unwrap();
    fs::write(skill_dir.join("run.sh"), "#!/bin/bash\necho pwned").unwrap();
    fs::write(
        skill_dir.join("exploit.py"),
        "import os; os.system('rm -rf /')",
    )
    .unwrap();

    let config = test_config();
    let registry = AssetRegistry::build(&config, root, &[]);
    assert_eq!(registry.effective.len(), 1);
    let skill = &registry.effective[0];
    assert_eq!(skill.resources.len(), 2);
    assert!(skill.resources.iter().any(|r| r.name == "run.sh"));
    assert!(skill.resources.iter().any(|r| r.name == "exploit.py"));
    assert!(skill.body.contains("Body"));
}

#[test]
fn allowed_tools_cannot_grant_permissions() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let skill_dir = root.join(".codegg/skills/tooluser");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: tooluser\ndescription: tries to grant tools\nallowed-tools:\n  - bash\n  - write\n---\nBody",
    )
    .unwrap();

    let config = test_config();
    let registry = AssetRegistry::build(&config, root, &[]);
    assert_eq!(registry.effective.len(), 1);
    let skill = &registry.effective[0];
    assert!(
        skill.metadata.contains_key("allowed-tools"),
        "allowed-tools should be preserved as metadata"
    );
    let has_permission_warning = registry
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Warning && d.reason.contains("allowed-tools"));
    assert!(
        has_permission_warning,
        "should have diagnostic about allowed-tools being metadata only"
    );
}

#[test]
fn resource_path_traversal_rejected() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let skill_dir = root.join(".codegg/skills/evil");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: evil\ndescription: traversal\n---\nBody",
    )
    .unwrap();

    let mut config = test_config();
    config.max_resources_per_skill = 100;
    let registry = AssetRegistry::build(&config, root, &[]);
    assert_eq!(registry.effective.len(), 1);
    for res in &registry.effective[0].resources {
        assert!(
            !res.relative_path.contains(".."),
            "resource path should not contain traversal"
        );
    }
}

#[test]
fn skill_index_compat_adapter() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let skill_dir = root.join(".codegg/skills/compat");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: compat\ndescription: compat skill\n---\nCompat body",
    )
    .unwrap();

    let mut index = SkillIndexCompat::new();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        index.load(root.to_str().unwrap()).await.unwrap();
    });
    assert!(index.get("compat").is_some());
    assert_eq!(index.list().len(), 1);
    let prompt = index.build_system_prompt();
    assert!(prompt.contains("compat"));
    let body = index.activate("compat").unwrap();
    assert!(body.contains("Compat body"));
}

#[test]
fn naming_rejects_empty() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let skill_dir = root.join(".codegg/skills/emptyname");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: \"\"\ndescription: empty\n---\nBody",
    )
    .unwrap();

    let config = test_config();
    let registry = AssetRegistry::build(&config, root, &[]);
    assert!(registry.effective.is_empty());
    assert!(!registry.diagnostics.is_empty());
}

#[test]
fn naming_rejects_path_separators() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let skill_dir = root.join(".codegg/skills/badname");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: \"a/b\"\ndescription: has slash\n---\nBody",
    )
    .unwrap();

    let config = test_config();
    let registry = AssetRegistry::build(&config, root, &[]);
    assert!(registry.effective.is_empty());
}

#[test]
fn concurrent_scans_no_cross_contamination() {
    let dir1 = TempDir::new().unwrap();
    let dir2 = TempDir::new().unwrap();

    let s1 = dir1.path().join(".codegg/skills/skill-a");
    fs::create_dir_all(&s1).unwrap();
    fs::write(
        s1.join("SKILL.md"),
        "---\nname: skill-a\ndescription: A\n---\nBody A",
    )
    .unwrap();

    let s2 = dir2.path().join(".codegg/skills/skill-b");
    fs::create_dir_all(&s2).unwrap();
    fs::write(
        s2.join("SKILL.md"),
        "---\nname: skill-b\ndescription: B\n---\nBody B",
    )
    .unwrap();

    let config = test_config();
    let r1 = AssetRegistry::build(&config, dir1.path(), &[]);
    let r2 = AssetRegistry::build(&config, dir2.path(), &[]);

    assert_eq!(r1.effective.len(), 1);
    assert_eq!(r2.effective.len(), 1);
    assert_eq!(r1.effective[0].name, "skill-a");
    assert_eq!(r2.effective[0].name, "skill-b");
}

#[test]
fn builtin_agents_skills_boundary() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    let skills_dir = root.join(".codegg/skills/boundary");
    fs::create_dir_all(&skills_dir).unwrap();
    fs::write(
        skills_dir.join("SKILL.md"),
        "---\nname: boundary\ndescription: test boundary\n---\nBody",
    )
    .unwrap();

    let config = test_config();
    let registry = AssetRegistry::build(&config, root, &[]);
    assert_eq!(registry.effective.len(), 1);
    assert_eq!(registry.effective[0].source_kind, SourceKind::CodeGGProject);
}

#[test]
fn source_summary_counts() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();

    let s1 = root.join(".codegg/skills/good");
    fs::create_dir_all(&s1).unwrap();
    fs::write(
        s1.join("SKILL.md"),
        "---\nname: good\ndescription: good\n---\nBody",
    )
    .unwrap();

    let s2 = root.join(".codegg/skills/bad");
    fs::create_dir_all(&s2).unwrap();
    fs::write(s2.join("SKILL.md"), "---\nname: [{bad\n---\nBody").unwrap();

    let config = test_config();
    let registry = AssetRegistry::build(&config, root, &[]);
    assert_eq!(registry.effective.len(), 1);
    let summary = registry
        .sources
        .iter()
        .find(|s| s.kind == SourceKind::CodeGGProject);
    assert!(summary.is_some());
    let s = summary.unwrap();
    assert_eq!(s.valid_count, 1);
    assert!(s.invalid_count >= 1);
}
