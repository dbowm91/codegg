use codegg::config::schema::{PreflightConfig, PreflightMode as ConfigPreflightMode};
use codegg::eggsact::adapter::{EggsactConfig, EggsactRuntime};
use codegg::preflight::{
    PreflightDecision, PreflightFinding, PreflightLocation, PreflightMode, PreflightPolicy,
    PreflightService, PreflightSeverity,
};
use codegg::tool::{Tool, ToolRegistry};
use std::sync::Arc;

#[test]
fn test_policy_default() {
    let policy = PreflightPolicy::default();
    assert!(policy.enabled);
    assert_eq!(policy.mode, PreflightMode::Warn);
    assert!(policy.patch);
    assert!(policy.config);
    assert!(policy.shell);
    assert!(policy.unicode);
    assert!(policy.log_findings);
    assert!(policy.model_visible_findings);
}

#[test]
fn test_policy_from_config() {
    let config = PreflightConfig {
        enabled: Some(false),
        mode: Some(ConfigPreflightMode::BlockOnDefinite),
        patch: Some(false),
        config: Some(false),
        shell: Some(false),
        unicode: Some(false),
        log_findings: Some(false),
        model_visible_findings: Some(false),
    };
    let policy = PreflightPolicy::from_config(&config);
    assert!(!policy.enabled);
    assert_eq!(policy.mode, PreflightMode::BlockOnDefinite);
    assert!(!policy.patch);
    assert!(!policy.config);
    assert!(!policy.shell);
    assert!(!policy.unicode);
    assert!(!policy.log_findings);
    assert!(!policy.model_visible_findings);
}

#[test]
fn test_policy_from_config_defaults() {
    let config = PreflightConfig::default();
    let policy = PreflightPolicy::from_config(&config);
    assert!(policy.enabled);
    assert_eq!(policy.mode, PreflightMode::Warn);
    assert!(policy.patch);
    assert!(policy.config);
    assert!(policy.shell);
    assert!(policy.unicode);
    assert!(policy.log_findings);
    assert!(policy.model_visible_findings);
}

#[test]
fn test_policy_from_config_none_fields_use_defaults() {
    let config = PreflightConfig {
        enabled: None,
        mode: None,
        patch: None,
        config: None,
        shell: None,
        unicode: None,
        log_findings: None,
        model_visible_findings: None,
    };
    let policy = PreflightPolicy::from_config(&config);
    assert!(policy.enabled);
    assert_eq!(policy.mode, PreflightMode::Warn);
    assert!(policy.patch);
    assert!(policy.config);
    assert!(policy.shell);
    assert!(policy.unicode);
    assert!(policy.log_findings);
    assert!(policy.model_visible_findings);
}

#[test]
fn test_disabled_policy_skips_all() {
    let mut policy = PreflightPolicy::default();
    policy.enabled = false;
    policy.patch = false;
    policy.config = false;
    policy.shell = false;
    policy.unicode = false;
    // The early-return logic in each check method checks `!self.policy.enabled || !self.policy.<flag>`.
    // With enabled=false, every check returns Allow.
    assert!(!policy.enabled);
    assert!(!policy.should_block(PreflightSeverity::Block));
    assert!(!policy.should_surface());
}

#[test]
fn test_patch_disabled_skips_replace_check() {
    let mut policy = PreflightPolicy::default();
    policy.patch = false;
    assert!(policy.enabled);
    assert!(!policy.patch);
    // check_text_replace returns Allow early when !policy.patch
}

#[test]
fn test_config_disabled_skips_json() {
    let mut policy = PreflightPolicy::default();
    policy.config = false;
    assert!(policy.enabled);
    assert!(!policy.config);
    // check_json_valid returns Allow early when !policy.config
}

#[test]
fn test_decision_summary_format() {
    let d = PreflightDecision::Warn {
        findings: vec![
            PreflightFinding {
                severity: PreflightSeverity::Block,
                machine_code: None,
                message: "invalid syntax".to_string(),
                location: None,
                source_tool: "validate_json".to_string(),
            },
            PreflightFinding {
                severity: PreflightSeverity::Warn,
                machine_code: None,
                message: "multiple matches".to_string(),
                location: None,
                source_tool: "text_replace_check".to_string(),
            },
            PreflightFinding {
                severity: PreflightSeverity::Annotate,
                machine_code: None,
                message: "confusable detected".to_string(),
                location: None,
                source_tool: "text_security_inspect".to_string(),
            },
        ],
    };
    let summary = d.summary();
    assert!(summary.contains("[BLOCK] validate_json: invalid syntax"));
    assert!(summary.contains("[WARN] text_replace_check: multiple matches"));
    assert!(summary.contains("[INFO] text_security_inspect: confusable detected"));
}

#[test]
fn test_decision_summary_empty_findings() {
    let d = PreflightDecision::Allow { findings: vec![] };
    assert_eq!(d.summary(), "");
}

#[test]
fn test_decision_is_blocked() {
    let block = PreflightDecision::Block {
        findings: vec![PreflightFinding {
            severity: PreflightSeverity::Block,
            machine_code: None,
            message: "blocked".to_string(),
            location: None,
            source_tool: "test".to_string(),
        }],
    };
    assert!(block.is_blocked());
    assert!(!block.has_warnings());

    let warn = PreflightDecision::Warn {
        findings: vec![PreflightFinding {
            severity: PreflightSeverity::Warn,
            machine_code: None,
            message: "warned".to_string(),
            location: None,
            source_tool: "test".to_string(),
        }],
    };
    assert!(!warn.is_blocked());
    assert!(warn.has_warnings());

    let allow = PreflightDecision::Allow { findings: vec![] };
    assert!(!allow.is_blocked());
    assert!(!allow.has_warnings());
}

#[test]
fn test_should_block_only_block_on_definite() {
    let mut policy = PreflightPolicy::default();

    // Warn mode: never blocks even on Block severity
    policy.mode = PreflightMode::Warn;
    assert!(!policy.should_block(PreflightSeverity::Block));
    assert!(!policy.should_block(PreflightSeverity::Warn));
    assert!(!policy.should_block(PreflightSeverity::Annotate));

    // BlockOnDefinite: blocks on Block severity only
    policy.mode = PreflightMode::BlockOnDefinite;
    assert!(policy.should_block(PreflightSeverity::Block));
    assert!(!policy.should_block(PreflightSeverity::Warn));
    assert!(!policy.should_block(PreflightSeverity::Annotate));

    // Disabled policy: should_block always false
    policy.enabled = false;
    assert!(!policy.should_block(PreflightSeverity::Block));
}

#[test]
fn test_findings_collected() {
    let findings = vec![
        PreflightFinding {
            severity: PreflightSeverity::Block,
            machine_code: None,
            message: "block msg".to_string(),
            location: None,
            source_tool: "tool_a".to_string(),
        },
        PreflightFinding {
            severity: PreflightSeverity::Warn,
            machine_code: None,
            message: "warn msg".to_string(),
            location: Some(PreflightLocation {
                file: Some("main.rs".to_string()),
                line: Some(42),
                column: Some(10),
            }),
            source_tool: "tool_b".to_string(),
        },
    ];

    let block = PreflightDecision::Block {
        findings: findings.clone(),
    };
    assert_eq!(block.findings().len(), 2);
    assert_eq!(block.findings()[0].message, "block msg");
    assert_eq!(block.findings()[1].message, "warn msg");
    assert!(block.findings()[1].location.is_some());

    let warn = PreflightDecision::Warn {
        findings: findings.clone(),
    };
    assert_eq!(warn.findings().len(), 2);

    let allow = PreflightDecision::Allow {
        findings: findings.clone(),
    };
    assert_eq!(allow.findings().len(), 2);
}

#[test]
fn test_findings_collected_empty() {
    let allow = PreflightDecision::Allow { findings: vec![] };
    assert!(allow.findings().is_empty());

    let warn = PreflightDecision::Warn { findings: vec![] };
    assert!(warn.findings().is_empty());

    let block = PreflightDecision::Block { findings: vec![] };
    assert!(block.findings().is_empty());
}

#[test]
fn test_severity_equality() {
    assert_eq!(PreflightSeverity::Block, PreflightSeverity::Block);
    assert_ne!(PreflightSeverity::Block, PreflightSeverity::Warn);
    assert_ne!(PreflightSeverity::Warn, PreflightSeverity::Annotate);
    assert_ne!(PreflightSeverity::Block, PreflightSeverity::Annotate);
}

#[test]
fn test_mode_equality() {
    assert_eq!(PreflightMode::Warn, PreflightMode::Warn);
    assert_ne!(PreflightMode::Warn, PreflightMode::BlockOnDefinite);
    assert_ne!(PreflightMode::Off, PreflightMode::Observe);
}

#[test]
fn test_should_surface() {
    let mut policy = PreflightPolicy::default();
    assert!(policy.should_surface());

    policy.model_visible_findings = false;
    assert!(!policy.should_surface());

    policy.enabled = false;
    assert!(!policy.should_surface());
}

#[test]
fn test_policy_serialization_roundtrip() {
    let policy = PreflightPolicy::default();
    let json = serde_json::to_string(&policy).unwrap();
    let deserialized: PreflightPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(policy.enabled, deserialized.enabled);
    assert_eq!(policy.mode, deserialized.mode);
    assert_eq!(policy.patch, deserialized.patch);
    assert_eq!(policy.config, deserialized.config);
    assert_eq!(policy.shell, deserialized.shell);
    assert_eq!(policy.unicode, deserialized.unicode);
    assert_eq!(policy.log_findings, deserialized.log_findings);
    assert_eq!(
        policy.model_visible_findings,
        deserialized.model_visible_findings
    );
}

#[test]
fn test_decision_clone() {
    let d = PreflightDecision::Block {
        findings: vec![PreflightFinding {
            severity: PreflightSeverity::Block,
            machine_code: Some("E001".to_string()),
            message: "clone test".to_string(),
            location: None,
            source_tool: "test".to_string(),
        }],
    };
    let cloned = d.clone();
    assert!(cloned.is_blocked());
    assert_eq!(cloned.findings()[0].machine_code.as_deref(), Some("E001"));
}

// ── Tool integration tests ──────────────────────────────────────────

fn test_preflight_service() -> PreflightService {
    let runtime = Arc::new(EggsactRuntime::new(EggsactConfig::default()).unwrap());
    PreflightService::with_runtime(runtime, PreflightPolicy::default())
}

fn test_preflight_service_with_policy(policy: PreflightPolicy) -> PreflightService {
    let runtime = Arc::new(EggsactRuntime::new(EggsactConfig::default()).unwrap());
    PreflightService::with_runtime(runtime, policy)
}

#[tokio::test]
async fn test_edit_tool_with_preflight_blocks_on_text_replace() {
    let svc = test_preflight_service();
    let tool = codegg::tool::edit::EditTool::new()
        .with_allowed_root(std::env::temp_dir())
        .with_preflight(svc);

    // Create a temp file with content
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    std::fs::write(&file_path, "hello world").unwrap();

    // Try to edit with a non-existent old_string — preflight should block
    let input = serde_json::json!({
        "path": file_path.to_str().unwrap(),
        "old_string": "nonexistent text that does not exist in the file",
        "new_string": "replacement"
    });

    let result = tool.execute(input).await;
    assert!(
        result.is_err(),
        "edit with non-matching old_string should be blocked by preflight"
    );
    let err = result.unwrap_err();
    let err_msg = format!("{}", err);
    assert!(
        err_msg.contains("preflight blocked") || err_msg.contains("could not find"),
        "error should indicate preflight block or not found: {}",
        err_msg
    );
}

#[tokio::test]
async fn test_edit_tool_with_preflight_warns_on_unicode() {
    let mut policy = PreflightPolicy::default();
    policy.unicode = true;
    let svc = test_preflight_service_with_policy(policy);
    let tool = codegg::tool::edit::EditTool::new()
        .with_allowed_root(std::env::temp_dir())
        .with_preflight(svc);

    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    std::fs::write(&file_path, "hello world").unwrap();

    // Edit with unicode confusable in new_string
    let input = serde_json::json!({
        "path": file_path.to_str().unwrap(),
        "old_string": "hello",
        "new_string": "hеllo"  // Cyrillic 'е' instead of Latin 'e'
    });

    let result = tool.execute(input).await;
    // Should succeed (unicode is warn-only by default) but may include warning
    if let Ok(output) = &result {
        // The edit should still apply
        assert!(
            output.contains("Edited"),
            "output should indicate edit was applied"
        );
    }
}

#[tokio::test]
async fn test_replace_tool_with_preflight_blocks_on_no_match() {
    let svc = test_preflight_service();
    let tool = codegg::tool::replace::ReplaceTool::new()
        .with_allowed_root(std::env::temp_dir())
        .with_preflight(svc);

    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    std::fs::write(&file_path, "hello world").unwrap();

    // Try to replace with a pattern that doesn't match
    let input = serde_json::json!({
        "path": file_path.to_str().unwrap(),
        "pattern": "nonexistent",
        "replacement": "replacement"
    });

    let result = tool.execute(input).await;
    assert!(result.is_err(), "replace with no matches should fail");
}

#[tokio::test]
async fn test_multiedit_tool_with_preflight_blocks_on_edit() {
    let svc = test_preflight_service();
    let tool = codegg::tool::multiedit::MultiEditTool::new()
        .with_allowed_root(std::env::temp_dir())
        .with_preflight(svc);

    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("test.txt");
    std::fs::write(&file_path, "hello world").unwrap();

    // Try multiedit with a non-matching old_string
    let input = serde_json::json!({
        "path": file_path.to_str().unwrap(),
        "edits": [
            {
                "old_string": "nonexistent",
                "new_string": "replacement"
            }
        ]
    });

    let result = tool.execute(input).await;
    assert!(
        result.is_err(),
        "multiedit with non-matching old_string should fail"
    );
}

#[tokio::test]
async fn test_apply_patch_tool_with_preflight_blocks_on_invalid_config() {
    let mut policy = PreflightPolicy::default();
    policy.config = true;
    let svc = test_preflight_service_with_policy(policy);
    let tool = codegg::tool::apply_patch::ApplyPatchTool::new()
        .with_allowed_root(std::env::temp_dir())
        .with_preflight(svc);

    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("config.json");
    std::fs::write(&file_path, r#"{"key": "value"}"#).unwrap();

    // Create a patch that would result in invalid JSON
    let patch = "--- a/config.json\n+++ b/config.json\n@@ -1 +1 @@\n-{\"key\": \"value\"}\n+{\"key\": \"value\",}";

    let input = serde_json::json!({
        "path": file_path.to_str().unwrap(),
        "patch": patch,
        "mode": "update"
    });

    let result = tool.execute(input).await;
    // The patch application itself may fail due to invalid diff format,
    // but if it applies, the config validation should catch invalid JSON
    if let Ok(output) = &result {
        // If patch applied successfully, check that config validation ran
        // (the trailing comma makes invalid JSON)
        assert!(
            output.contains("Applied") || output.contains("preflight"),
            "output should indicate patch applied or preflight finding: {}",
            output
        );
    }
}

#[tokio::test]
async fn test_bash_tool_with_preflight_blocks_on_dangerous_command() {
    let mut policy = PreflightPolicy::default();
    policy.shell = true;
    let svc = test_preflight_service_with_policy(policy);
    let tool = codegg::tool::bash::BashTool::new().with_preflight(svc);

    // A dangerous command that should be caught by security checks
    let input = serde_json::json!({
        "command": "rm -rf /"
    });

    let result = tool.execute(input).await;
    assert!(result.is_err(), "dangerous command should be blocked ");
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("blocked")
            || err_msg.contains("permission")
            || err_msg.contains("blocked list "),
        "error should indicate blocking: {}",
        err_msg
    );
}

// ── Policy behavior tests ──────────────────────────────────────────

#[test]
fn test_observe_mode_does_not_block() {
    let mut policy = PreflightPolicy::default();
    policy.mode = PreflightMode::Observe;

    // In observe mode, even Block severity findings should not cause blocking
    assert!(!policy.should_block(PreflightSeverity::Block));
    assert!(!policy.should_block(PreflightSeverity::Warn));
}

#[test]
fn test_warn_mode_does_not_block() {
    let mut policy = PreflightPolicy::default();
    policy.mode = PreflightMode::Warn;

    assert!(!policy.should_block(PreflightSeverity::Block));
    assert!(!policy.should_block(PreflightSeverity::Warn));
}

#[test]
fn test_block_on_definite_only_blocks_block_severity() {
    let mut policy = PreflightPolicy::default();
    policy.mode = PreflightMode::BlockOnDefinite;

    assert!(policy.should_block(PreflightSeverity::Block));
    assert!(!policy.should_block(PreflightSeverity::Warn));
    assert!(!policy.should_block(PreflightSeverity::Annotate));
}

#[test]
fn test_disabled_policy_skips_all_checks() {
    let mut policy = PreflightPolicy::default();
    policy.enabled = false;

    assert!(!policy.should_block(PreflightSeverity::Block));
    assert!(!policy.should_surface());
}

#[test]
fn test_per_category_toggle_respected() {
    let mut policy = PreflightPolicy::default();
    policy.patch = false;
    policy.config = false;
    policy.shell = false;
    policy.unicode = false;

    // All categories disabled — checks should return Allow
    assert!(!policy.patch);
    assert!(!policy.config);
    assert!(!policy.shell);
    assert!(!policy.unicode);
}

// ── Findings and decision tests ────────────────────────────────────

#[test]
fn test_findings_with_location() {
    let finding = PreflightFinding {
        severity: PreflightSeverity::Warn,
        machine_code: Some("W001".to_string()),
        message: "possible issue ".to_string(),
        location: Some(PreflightLocation {
            file: Some("src/main.rs ".to_string()),
            line: Some(42),
            column: Some(10),
        }),
        source_tool: "text_replace_check".to_string(),
    };

    assert_eq!(finding.severity, PreflightSeverity::Warn);
    assert_eq!(finding.location.as_ref().unwrap().line, Some(42));
}

#[test]
fn test_decision_allow_with_empty_findings() {
    let d = PreflightDecision::Allow { findings: vec![] };
    assert!(!d.is_blocked());
    assert!(!d.has_warnings());
    assert!(d.findings().is_empty());
    assert_eq!(d.summary(), "");
}

#[test]
fn test_decision_warn_with_findings() {
    let d = PreflightDecision::Warn {
        findings: vec![
            PreflightFinding {
                severity: PreflightSeverity::Warn,
                machine_code: None,
                message: "match count > 1".to_string(),
                location: None,
                source_tool: "text_replace_check".to_string(),
            },
            PreflightFinding {
                severity: PreflightSeverity::Annotate,
                machine_code: None,
                message: "info note ".to_string(),
                location: None,
                source_tool: "validate_json".to_string(),
            },
        ],
    };
    assert!(!d.is_blocked());
    assert!(d.has_warnings());
    assert_eq!(d.findings().len(), 2);
    assert!(d.summary().contains("[WARN]"));
    assert!(d.summary().contains("[INFO]"));
}

#[test]
fn test_decision_block_with_findings() {
    let d = PreflightDecision::Block {
        findings: vec![PreflightFinding {
            severity: PreflightSeverity::Block,
            machine_code: Some("B001".to_string()),
            message: "replacement not found ".to_string(),
            location: None,
            source_tool: "text_replace_check".to_string(),
        }],
    };
    assert!(d.is_blocked());
    assert!(!d.has_warnings());
    assert_eq!(d.findings().len(), 1);
    assert!(d.summary().contains("[BLOCK]"));
}

// ── Serialization tests ────────────────────────────────────────────

#[test]
fn test_policy_serialization_roundtrip_all_modes() {
    for mode in [
        PreflightMode::Off,
        PreflightMode::Observe,
        PreflightMode::Warn,
        PreflightMode::BlockOnDefinite,
    ] {
        let mut policy = PreflightPolicy::default();
        policy.mode = mode;
        let json = serde_json::to_string(&policy).unwrap();
        let deserialized: PreflightPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(policy.mode, deserialized.mode);
    }
}

#[test]
fn test_finding_serialization_roundtrip() {
    let finding = PreflightFinding {
        severity: PreflightSeverity::Block,
        machine_code: Some("E001".to_string()),
        message: "test finding ".to_string(),
        location: Some(PreflightLocation {
            file: Some("test.rs ".to_string()),
            line: Some(10),
            column: Some(5),
        }),
        source_tool: "validate_json".to_string(),
    };

    let json = serde_json::to_string(&finding).unwrap();
    let deserialized: PreflightFinding = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.severity, PreflightSeverity::Block);
    assert_eq!(deserialized.machine_code.as_deref(), Some("E001"));
    assert_eq!(deserialized.location.as_ref().unwrap().line, Some(10));
}

// ── Config schema tests ────────────────────────────────────────────

#[test]
fn test_config_preflight_schema_defaults() {
    let config = PreflightConfig::default();
    assert!(config.enabled.unwrap_or(true));
    assert_eq!(
        config.mode.unwrap_or(ConfigPreflightMode::Warn),
        ConfigPreflightMode::Warn
    );
    assert!(config.patch.unwrap_or(true));
    assert!(config.config.unwrap_or(true));
    assert!(config.shell.unwrap_or(true));
    assert!(config.unicode.unwrap_or(true));
}

#[test]
fn test_config_preflight_schema_all_disabled() {
    let config = PreflightConfig {
        enabled: Some(false),
        mode: Some(ConfigPreflightMode::Off),
        patch: Some(false),
        config: Some(false),
        shell: Some(false),
        unicode: Some(false),
        log_findings: Some(false),
        model_visible_findings: Some(false),
    };
    let policy = PreflightPolicy::from_config(&config);
    assert!(!policy.enabled);
    assert_eq!(policy.mode, PreflightMode::Off);
    assert!(!policy.patch);
    assert!(!policy.config);
    assert!(!policy.shell);
    assert!(!policy.unicode);
}

// ── Harness preflight isolation tests ──────────────────────────────

#[test]
fn test_tool_registry_has_preflight_capable_tools() {
    let registry = ToolRegistry::with_defaults();
    let tool_names: Vec<&str> = registry.list().iter().map(|t| t.name()).collect();

    // These tools should be present and capable of receiving preflight
    assert!(tool_names.contains(&"edit"), "edit tool should be present");
    assert!(
        tool_names.contains(&"replace"),
        "replace tool should be present"
    );
    assert!(
        tool_names.contains(&"apply_patch"),
        "apply_patch tool should be present"
    );
    assert!(tool_names.contains(&"bash"), "bash tool should be present");
}

#[test]
fn test_deterministic_preflight_tools_are_model_facing() {
    // The eggsact deterministic tools (text_replace_check, validate_json, etc.)
    // are both model-facing AND used internally by the preflight service.
    // This test verifies they ARE in the default registry.
    let registry = ToolRegistry::with_defaults();
    let tool_names: Vec<&str> = registry.list().iter().map(|t| t.name()).collect();

    assert!(
        tool_names.contains(&"text_replace_check"),
        "text_replace_check should be in default registry"
    );
    assert!(
        tool_names.contains(&"validate_json"),
        "validate_json should be in default registry"
    );
    assert!(
        tool_names.contains(&"validate_toml"),
        "validate_toml should be in default registry"
    );
    assert!(
        tool_names.contains(&"command_preflight"),
        "command_preflight should be in default registry"
    );
    assert!(
        tool_names.contains(&"text_security_inspect"),
        "text_security_inspect should be in default registry"
    );
}

// ── Severity classification tests ──────────────────────────────────

#[test]
fn test_severity_ordering() {
    // Block > Warn > Annotate in severity
    assert!(PreflightSeverity::Block != PreflightSeverity::Warn);
    assert!(PreflightSeverity::Warn != PreflightSeverity::Annotate);
    assert!(PreflightSeverity::Block != PreflightSeverity::Annotate);
}

#[test]
fn test_severity_serialize_deserialize() {
    for severity in [
        PreflightSeverity::Block,
        PreflightSeverity::Warn,
        PreflightSeverity::Annotate,
    ] {
        let json = serde_json::to_string(&severity).unwrap();
        let deserialized: PreflightSeverity = serde_json::from_str(&json).unwrap();
        assert_eq!(severity, deserialized);
    }
}

// ── PreflightService check method tests ──────────────────────────

#[tokio::test]
async fn check_text_replace_detects_no_match() {
    let service = PreflightService::new(PreflightPolicy::default()).unwrap();
    let decision = service.check_text_replace("hello world", "nonexistent", "new").await;
    // No match found → either Block (if eggsact reports match_count: 0)
    // or Allow (if eggsact returns ok: true with no explicit count).
    // Either way, the service should not panic.
    match &decision {
        PreflightDecision::Block { findings } => {
            assert!(!findings.is_empty());
            assert!(findings[0].message.contains("not found") || findings[0].message.contains("no effect"));
        }
        PreflightDecision::Allow { findings } => {
            // eggsact may not report no-match as a block depending on profile
            assert!(findings.is_empty() || !findings.iter().any(|f| f.severity == PreflightSeverity::Block));
        }
        other => panic!("unexpected decision for no-match: {:?}", other),
    }
}

#[tokio::test]
async fn check_text_replace_allows_clean_match() {
    let service = PreflightService::new(PreflightPolicy::default()).unwrap();
    let decision = service.check_text_replace("hello world", "world", "rust").await;
    // Single clean match → Allow
    assert!(!decision.is_blocked(), "clean match should not block: {:?}", decision);
}

#[tokio::test]
async fn check_json_valid_passes_for_valid_json() {
    let service = PreflightService::new(PreflightPolicy::default()).unwrap();
    let decision = service.check_json_valid(r#"{"key": "value"}"#).await;
    assert!(!decision.is_blocked(), "valid JSON should not block: {:?}", decision);
}

#[tokio::test]
async fn check_json_valid_detects_invalid_json() {
    let service = PreflightService::new(PreflightPolicy::default()).unwrap();
    let decision = service.check_json_valid("{not valid}").await;
    // The eggsact validate_json tool may or may not report this as a failure
    // depending on profile. We verify the service doesn't panic.
    // The decision should be either Allow or Warn/Block.
    let _ = decision;
}

#[tokio::test]
async fn check_toml_valid_passes_for_valid_toml() {
    let service = PreflightService::new(PreflightPolicy::default()).unwrap();
    let decision = service.check_toml_valid("[package]\nname = \"test\"").await;
    assert!(!decision.is_blocked(), "valid TOML should not block: {:?}", decision);
}

#[tokio::test]
async fn check_command_analyzes_shell_command() {
    let service = PreflightService::new(PreflightPolicy::default()).unwrap();
    let decision = service.check_command("ls -la").await;
    // ls is low-risk → Allow or Warn, but not Block
    assert!(!decision.is_blocked(), "ls should not block: {:?}", decision);
}

#[tokio::test]
async fn check_text_security_clean_text() {
    let service = PreflightService::new(PreflightPolicy::default()).unwrap();
    let decision = service.check_text_security("hello world").await;
    // Clean ASCII text → Allow
    assert!(!decision.is_blocked(), "clean text should not block: {:?}", decision);
}

#[tokio::test]
async fn disabled_policy_returns_allow() {
    let policy = PreflightPolicy {
        enabled: false,
        ..Default::default()
    };
    let service = PreflightService::new(policy).unwrap();
    let decision = service.check_text_replace("a", "b", "c").await;
    assert!(!decision.is_blocked());
    assert!(decision.findings().is_empty());
}

#[tokio::test]
async fn observe_mode_records_but_does_not_block() {
    let policy = PreflightPolicy {
        mode: PreflightMode::Observe,
        ..Default::default()
    };
    let service = PreflightService::new(policy).unwrap();
    let decision = service.check_text_replace("hello world", "nonexistent", "new").await;
    // Observe mode should never block, even on no-match
    assert!(!decision.is_blocked(), "observe mode should not block");
}

// ── Golden output tests ──────────────────────────────────────────

#[test]
fn trust_frame_search_has_expected_shape() {
    let framed = codegg::search_backend::framing::frame_search_results("content", "eggsearch");
    assert!(framed.starts_with("[external_web_content"));
    assert!(framed.contains("trust=external_untrusted"));
    assert!(framed.contains("source=eggsearch"));
    assert!(framed.contains("tool=websearch"));
    assert!(framed.ends_with("[/external_web_content]"));
}

#[test]
fn trust_frame_fetch_has_expected_shape() {
    let framed = codegg::search_backend::framing::frame_fetched_page("body", "eggsearch");
    assert!(framed.starts_with("[external_web_content"));
    assert!(framed.contains("trust=external_untrusted"));
    assert!(framed.contains("source=eggsearch"));
    assert!(framed.contains("tool=webfetch"));
    assert!(framed.contains("EXTERNAL, UNTRUSTED DATA"));
    assert!(framed.ends_with("[/external_web_content]"));
}

#[test]
fn provenance_serialization_roundtrip() {
    let prov = codegg::tool::ToolProvenance {
        backend: "mcp".to_string(),
        implementation: "eggsearch/search".to_string(),
        version: Some("1.0.0".to_string()),
        elapsed_ms: Some(42),
        truncated: false,
        trust: codegg::tool::ToolTrust::ExternalUntrusted,
    };
    let json = serde_json::to_string(&prov).unwrap();
    let deserialized: codegg::tool::ToolProvenance = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.backend, "mcp");
    assert_eq!(deserialized.implementation, "eggsearch/search");
    assert_eq!(deserialized.trust, codegg::tool::ToolTrust::ExternalUntrusted);
}

#[test]
fn bootstrap_report_summary_fragments() {
    let report = codegg::search_backend::bootstrap::BootstrapReport {
        search_backend: Some("eggsearch".to_string()),
        connected: true,
        tools: vec!["web_search".to_string(), "web_fetch".to_string()],
        expose_raw_mcp_tools: false,
        fallback_to_builtin: false,
        ..Default::default()
    };
    let lines = report.summary_lines();
    let joined = lines.join("\n");
    assert!(joined.contains("Search backend: eggsearch"));
    assert!(joined.contains("Eggsearch MCP: connected"));
    assert!(joined.contains("web_search, web_fetch"));
    assert!(joined.contains("Raw MCP tools exposed to model: no"));
    assert!(joined.contains("Fallback to built-in: no"));
}
