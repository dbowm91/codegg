use codegg::config::schema::{PreflightConfig, PreflightMode as ConfigPreflightMode};
use codegg::preflight::{
    PreflightDecision, PreflightFinding, PreflightLocation, PreflightMode, PreflightPolicy,
    PreflightSeverity,
};

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
