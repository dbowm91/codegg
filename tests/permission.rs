use codegg::config::schema::{Config, PermissionConfig, PermissionRule};
use codegg::permission::{
    agent_ruleset, config_ruleset, default_ruleset, merge_rulesets, parse_level, PathRule,
    PermissionChecker, PermissionLevel, PermissionRequest, PermissionResult, PermissionRuleset,
    PermissionStore, PersistentDecision, ToolRule,
};
use std::collections::HashMap;
use tempfile::TempDir;

#[test]
fn test_parse_level_valid() {
    assert!(matches!(parse_level("allow"), PermissionLevel::Allow));
    assert!(matches!(parse_level("deny"), PermissionLevel::Deny));
    assert!(matches!(parse_level("ask"), PermissionLevel::Ask));
}

#[test]
fn test_parse_level_invalid_defaults_to_ask() {
    assert!(matches!(parse_level("invalid"), PermissionLevel::Ask));
    assert!(matches!(parse_level(""), PermissionLevel::Ask));
}

#[test]
fn test_default_ruleset_read_only_allowed() {
    // Read-only and safe-mutating tools no longer need rules: they
    // short-circuit to Allow in `PermissionChecker::check()` based on
    // their `ToolCategory`. The default ruleset is for mutating tools.
    let ruleset = default_ruleset();
    assert!(matches!(ruleset.default, PermissionLevel::Ask));

    // Mutating tools default to Ask.
    let ask_tools: Vec<_> = ruleset
        .tool_rules
        .iter()
        .filter(|r| matches!(r.level, PermissionLevel::Ask))
        .map(|r| r.tool.as_str())
        .collect();

    assert!(ask_tools.contains(&"edit"));

    // Bash is NOT in the default ruleset anymore — it's handled by the
    // destructive-pattern short-circuit in `check_with_args()`.
    // Verify the read-only short-circuit via the checker directly:
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let checker = codegg::permission::PermissionChecker::new(None, None);
        let result = checker.check_legacy("read", None).await;
        assert!(matches!(result, PermissionResult::Allow));
        let result = checker.check_legacy("glob", None).await;
        assert!(matches!(result, PermissionResult::Allow));
        let result = checker.check_legacy("grep", None).await;
        assert!(matches!(result, PermissionResult::Allow));
    });
}

#[test]
fn test_default_ruleset_destructive_tools_ask() {
    let ruleset = default_ruleset();

    // Mutating tools are listed as Ask in the default ruleset.
    let ask_tools: Vec<_> = ruleset
        .tool_rules
        .iter()
        .filter(|r| matches!(r.level, PermissionLevel::Ask))
        .map(|r| r.tool.as_str())
        .collect();

    assert!(ask_tools.contains(&"edit"));
    // Note: bash is no longer in default_ruleset (handled by destructive check)
    // Note: todowrite is no longer in default_ruleset (short-circuited to Allow)
}

#[tokio::test]
async fn test_permission_checker_default() {
    let ruleset = default_ruleset();
    let checker = PermissionChecker::new(None, None).with_session_rules(ruleset);

    let result = checker.check_legacy("read", None).await;
    assert!(matches!(result, PermissionResult::Allow));

    let result = checker.check_legacy("bash", None).await;
    assert!(matches!(result, PermissionResult::Ask(_)));
}

#[tokio::test]
async fn test_permission_checker_with_config_allow() {
    let config = Config {
        permission: Some(PermissionConfig {
            default: Some("allow".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let checker = PermissionChecker::new(Some(&config), None);
    let result = checker.check_legacy("bash", None).await;
    assert!(matches!(result, PermissionResult::Allow));
}

#[tokio::test]
async fn test_permission_checker_with_config_deny() {
    let config = Config {
        permission: Some(PermissionConfig {
            bash: Some(PermissionRule::Action("deny".to_string())),
            ..Default::default()
        }),
        ..Default::default()
    };
    let checker = PermissionChecker::new(Some(&config), None);
    let result = checker.check_legacy("bash", None).await;
    assert!(matches!(result, PermissionResult::Deny));
}

#[tokio::test]
async fn test_permission_checker_always_allow() {
    let checker = PermissionChecker::new(None, None);
    checker.always_allow_legacy("bash", None).await;

    let result = checker.check_legacy("bash", None).await;
    assert!(matches!(result, PermissionResult::Allow));
}

#[tokio::test]
async fn test_permission_checker_always_deny() {
    let checker = PermissionChecker::new(None, None);
    checker.always_deny_legacy("read", None).await;

    let result = checker.check_legacy("read", None).await;
    assert!(matches!(result, PermissionResult::Deny));
}

#[tokio::test]
async fn test_permission_checker_clear_decisions() {
    let checker = PermissionChecker::new(None, None);
    checker.always_allow_legacy("bash", None).await;

    let result = checker.check_legacy("bash", None).await;
    assert!(matches!(result, PermissionResult::Allow));

    checker.clear_decisions().await;

    let result = checker.check_legacy("bash", None).await;
    assert!(matches!(result, PermissionResult::Ask(_)));
}

#[tokio::test]
async fn test_permission_checker_with_path_rules() {
    let config = Config {
        permission: Some(PermissionConfig {
            default: Some("allow".to_string()),
            paths: Some(vec!["/tmp/*".to_string()]),
            ..Default::default()
        }),
        ..Default::default()
    };
    let checker = PermissionChecker::new(Some(&config), None);

    let result = checker.check_legacy("read", Some("/tmp/test.txt")).await;
    println!(
        "test_permission_checker_with_path_rules Result: {:?}",
        result
    );
    assert!(matches!(result, PermissionResult::Allow));
}

#[tokio::test]
async fn test_permission_checker_wildcard_tool_rule() {
    let ruleset = PermissionRuleset {
        default: PermissionLevel::Allow,
        tool_rules: vec![ToolRule {
            tool: "*".to_string(),
            level: PermissionLevel::Deny,
            paths: None,
            bash_patterns: None,
        }],
        path_rules: Vec::new(),
    };
    let checker = PermissionChecker::new(None, None);
    let checker = checker.with_session_rules(ruleset);

    let result = checker.check_legacy("any_tool", None).await;
    assert!(matches!(result, PermissionResult::Deny));
}

#[test]
fn test_merge_rulesets_b_default_overrides() {
    let a = PermissionRuleset {
        default: PermissionLevel::Allow,
        tool_rules: Vec::new(),
        path_rules: Vec::new(),
    };
    let b = PermissionRuleset {
        default: PermissionLevel::Deny,
        tool_rules: Vec::new(),
        path_rules: Vec::new(),
    };
    let merged = merge_rulesets(&a, &b);
    assert!(matches!(merged.default, PermissionLevel::Deny));
}

#[test]
fn test_merge_rulesets_b_default_ask_keeps_a() {
    let a = PermissionRuleset {
        default: PermissionLevel::Allow,
        tool_rules: Vec::new(),
        path_rules: Vec::new(),
    };
    let b = PermissionRuleset {
        default: PermissionLevel::Ask,
        tool_rules: Vec::new(),
        path_rules: Vec::new(),
    };
    let merged = merge_rulesets(&a, &b);
    assert!(matches!(merged.default, PermissionLevel::Allow));
}

#[test]
fn test_merge_rulesets_tool_rules_override() {
    let a = PermissionRuleset {
        default: PermissionLevel::Ask,
        tool_rules: vec![ToolRule {
            tool: "bash".to_string(),
            level: PermissionLevel::Allow,
            paths: None,
            bash_patterns: None,
        }],
        path_rules: Vec::new(),
    };
    let b = PermissionRuleset {
        default: PermissionLevel::Deny,
        tool_rules: vec![ToolRule {
            tool: "bash".to_string(),
            level: PermissionLevel::Deny,
            paths: None,
            bash_patterns: None,
        }],
        path_rules: Vec::new(),
    };
    let merged = merge_rulesets(&a, &b);
    assert_eq!(merged.tool_rules.len(), 1);
    assert!(matches!(merged.tool_rules[0].level, PermissionLevel::Deny));
}

#[test]
fn test_merge_rulesets_tool_rules_add() {
    let a = PermissionRuleset {
        default: PermissionLevel::Ask,
        tool_rules: vec![ToolRule {
            tool: "bash".to_string(),
            level: PermissionLevel::Allow,
            paths: None,
            bash_patterns: None,
        }],
        path_rules: Vec::new(),
    };
    let b = PermissionRuleset {
        default: PermissionLevel::Ask,
        tool_rules: vec![ToolRule {
            tool: "edit".to_string(),
            level: PermissionLevel::Deny,
            paths: None,
            bash_patterns: None,
        }],
        path_rules: Vec::new(),
    };
    let merged = merge_rulesets(&a, &b);
    assert_eq!(merged.tool_rules.len(), 2);
}

#[test]
fn test_merge_rulesets_path_rules_extend() {
    let a = PermissionRuleset {
        default: PermissionLevel::Ask,
        tool_rules: Vec::new(),
        path_rules: vec![PathRule {
            pattern: "/tmp/*".to_string(),
            level: PermissionLevel::Allow,
        }],
    };
    let b = PermissionRuleset {
        default: PermissionLevel::Ask,
        tool_rules: Vec::new(),
        path_rules: vec![PathRule {
            pattern: "/var/*".to_string(),
            level: PermissionLevel::Deny,
        }],
    };
    let merged = merge_rulesets(&a, &b);
    assert_eq!(merged.path_rules.len(), 2);
}

#[tokio::test]
async fn test_permission_store_add_and_get() {
    let dir = TempDir::new().unwrap();
    let store_path = dir.path().join("permissions.json");
    let mut store = PermissionStore::new(Some(store_path.clone()));

    store.add_decision("bash", None, PermissionLevel::Allow, None);
    let decision = store.get_decision("bash", None, None);
    assert!(matches!(decision, Some(PermissionLevel::Allow)));
}

#[tokio::test]
async fn test_permission_store_persists() {
    let dir = TempDir::new().unwrap();
    let store_path = dir.path().join("permissions.json");

    {
        let mut store = PermissionStore::new(Some(store_path.clone()));
        store.add_decision("edit", Some("/tmp/test.txt"), PermissionLevel::Allow, None);
    }

    let store = PermissionStore::new(Some(store_path));
    let decision = store.get_decision("edit", Some("/tmp/test.txt"), None);
    assert!(matches!(decision, Some(PermissionLevel::Allow)));
}

#[tokio::test]
async fn test_permission_store_clear() {
    let dir = TempDir::new().unwrap();
    let store_path = dir.path().join("permissions.json");
    let mut store = PermissionStore::new(Some(store_path));

    store.add_decision("bash", None, PermissionLevel::Allow, None);
    store.clear();
    assert!(store.get_decision("bash", None, None).is_none());
}

#[tokio::test]
async fn test_permission_store_overrides_existing() {
    let dir = TempDir::new().unwrap();
    let store_path = dir.path().join("permissions.json");
    let mut store = PermissionStore::new(Some(store_path));

    store.add_decision("bash", None, PermissionLevel::Allow, None);
    store.add_decision("bash", None, PermissionLevel::Deny, None);

    let decision = store.get_decision("bash", None, None);
    assert!(matches!(decision, Some(PermissionLevel::Deny)));
}

#[test]
fn test_agent_ruleset_from_config() {
    let mut permissions = HashMap::new();
    permissions.insert(
        "bash".to_string(),
        PermissionRule::Action("allow".to_string()),
    );
    permissions.insert(
        "write".to_string(),
        PermissionRule::Action("deny".to_string()),
    );

    let agent_config = codegg::config::schema::AgentConfig {
        permission: Some(permissions),
        ..Default::default()
    };

    let ruleset = agent_ruleset(&agent_config);
    assert_eq!(ruleset.tool_rules.len(), 2);
}

#[test]
fn test_config_ruleset_from_permission_config() {
    let config = Config {
        permission: Some(PermissionConfig {
            default: Some("deny".to_string()),
            read: Some(PermissionRule::Action("allow".to_string())),
            bash: Some(PermissionRule::Action("deny".to_string())),
            ..Default::default()
        }),
        ..Default::default()
    };

    let ruleset = config_ruleset(Some(&config));
    assert!(matches!(ruleset.default, PermissionLevel::Deny));
    assert_eq!(ruleset.tool_rules.len(), 2);
}

#[test]
fn test_config_ruleset_object_permission_rule() {
    let mut obj = HashMap::new();
    obj.insert("default".to_string(), "allow".to_string());

    let config = Config {
        permission: Some(PermissionConfig {
            bash: Some(PermissionRule::Object(obj)),
            ..Default::default()
        }),
        ..Default::default()
    };

    let ruleset = config_ruleset(Some(&config));
    let bash_rule = ruleset
        .tool_rules
        .iter()
        .find(|r| r.tool == "bash")
        .unwrap();
    assert!(matches!(bash_rule.level, PermissionLevel::Allow));
}

#[test]
fn test_config_ruleset_tools_map() {
    let mut tools = HashMap::new();
    tools.insert("custom_tool".to_string(), "allow".to_string());

    let config = Config {
        permission: Some(PermissionConfig {
            tools: Some(tools),
            ..Default::default()
        }),
        ..Default::default()
    };

    let ruleset = config_ruleset(Some(&config));
    let custom = ruleset
        .tool_rules
        .iter()
        .find(|r| r.tool == "custom_tool")
        .unwrap();
    assert!(matches!(custom.level, PermissionLevel::Allow));
}

#[test]
fn test_config_ruleset_simple_tools() {
    let config = Config {
        permission: Some(PermissionConfig {
            todowrite: Some("allow".to_string()),
            webfetch: Some("deny".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    let ruleset = config_ruleset(Some(&config));
    let todo = ruleset
        .tool_rules
        .iter()
        .find(|r| r.tool == "todowrite")
        .unwrap();
    assert!(matches!(todo.level, PermissionLevel::Allow));

    let webfetch = ruleset
        .tool_rules
        .iter()
        .find(|r| r.tool == "webfetch")
        .unwrap();
    assert!(matches!(webfetch.level, PermissionLevel::Deny));
}

#[tokio::test]
async fn test_permission_checker_session_rules_override_config() {
    let config = Config {
        permission: Some(PermissionConfig {
            default: Some("deny".to_string()),
            ..Default::default()
        }),
        ..Default::default()
    };

    let session_rules = PermissionRuleset {
        default: PermissionLevel::Allow,
        tool_rules: Vec::new(),
        path_rules: Vec::new(),
    };

    let checker = PermissionChecker::new(Some(&config), None).with_session_rules(session_rules);

    let result = checker.check_legacy("bash", None).await;
    assert!(matches!(result, PermissionResult::Allow));
}

#[tokio::test]
async fn test_permission_checker_agent_rules_override_session() {
    let session_rules = PermissionRuleset {
        default: PermissionLevel::Allow,
        tool_rules: Vec::new(),
        path_rules: Vec::new(),
    };

    let agent_rules = PermissionRuleset {
        default: PermissionLevel::Deny,
        tool_rules: Vec::new(),
        path_rules: Vec::new(),
    };

    let checker = PermissionChecker::new(None, None)
        .with_session_rules(session_rules)
        .with_agent_rules(agent_rules);

    let result = checker.check_legacy("bash", None).await;
    assert!(matches!(result, PermissionResult::Deny));
}

#[test]
fn test_persistent_decision_empty_path() {
    let decision = PersistentDecision {
        tool: "bash".to_string(),
        path: None,
        level: PermissionLevel::Allow,
        created_at: 0,
        signature: String::new(),
        session_id: None,
    };
    assert_eq!(decision.tool, "bash");
    assert!(decision.path.is_none());
}

#[test]
fn test_persistent_decision_with_path() {
    let decision = PersistentDecision {
        tool: "edit".to_string(),
        path: Some("/tmp/test.txt".to_string()),
        level: PermissionLevel::Deny,
        created_at: 12345,
        signature: String::new(),
        session_id: None,
    };
    assert_eq!(decision.path, Some("/tmp/test.txt".to_string()));
}

#[test]
fn test_permission_request() {
    let request = PermissionRequest {
        tool: "bash".to_string(),
        path: Some("/tmp/script.sh".to_string()),
        args: Some(serde_json::json!({"command": "echo hi"})),
    };
    assert_eq!(request.tool, "bash");
    assert_eq!(request.path, Some("/tmp/script.sh".to_string()));
}

#[tokio::test]
async fn test_permission_level_matching_exact_tool() {
    let ruleset = PermissionRuleset {
        default: PermissionLevel::Ask,
        tool_rules: vec![ToolRule {
            tool: "bash".to_string(),
            level: PermissionLevel::Allow,
            paths: None,
            bash_patterns: None,
        }],
        path_rules: Vec::new(),
    };
    let checker = PermissionChecker::new(None, None).with_session_rules(ruleset);
    let result = checker.check_legacy("bash", None).await;
    assert!(matches!(result, PermissionResult::Allow));
}

#[tokio::test]
async fn test_permission_level_matching_wildcard() {
    let ruleset = PermissionRuleset {
        default: PermissionLevel::Ask,
        tool_rules: vec![ToolRule {
            tool: "*".to_string(),
            level: PermissionLevel::Deny,
            paths: None,
            bash_patterns: None,
        }],
        path_rules: Vec::new(),
    };
    let checker = PermissionChecker::new(None, None).with_session_rules(ruleset);
    let result = checker.check_legacy("any_tool", None).await;
    assert!(matches!(result, PermissionResult::Deny));
}

#[tokio::test]
async fn test_permission_level_matching_unknown_tool_falls_through() {
    let ruleset = PermissionRuleset {
        default: PermissionLevel::Ask,
        tool_rules: Vec::new(),
        path_rules: Vec::new(),
    };
    let checker = PermissionChecker::new(None, None).with_session_rules(ruleset);
    let result = checker.check_legacy("unknown_tool", None).await;
    assert!(matches!(result, PermissionResult::Ask(_)));
}

#[tokio::test]
async fn test_permission_level_priority_agent_overrides_session() {
    let session_rules = PermissionRuleset {
        default: PermissionLevel::Allow,
        tool_rules: vec![ToolRule {
            tool: "bash".to_string(),
            level: PermissionLevel::Deny,
            paths: None,
            bash_patterns: None,
        }],
        path_rules: Vec::new(),
    };
    let agent_rules = PermissionRuleset {
        default: PermissionLevel::Ask,
        tool_rules: vec![ToolRule {
            tool: "bash".to_string(),
            level: PermissionLevel::Allow,
            paths: None,
            bash_patterns: None,
        }],
        path_rules: Vec::new(),
    };
    let checker = PermissionChecker::new(None, None)
        .with_session_rules(session_rules)
        .with_agent_rules(agent_rules);
    let result = checker.check_legacy("bash", None).await;
    assert!(matches!(result, PermissionResult::Allow));
}

#[tokio::test]
async fn test_permission_level_priority_config_fallback() {
    let config = Config {
        permission: Some(PermissionConfig {
            default: Some("deny".to_string()),
            bash: Some(PermissionRule::Action("allow".to_string())),
            ..Default::default()
        }),
        ..Default::default()
    };
    let session_rules = PermissionRuleset {
        default: PermissionLevel::Ask,
        tool_rules: Vec::new(),
        path_rules: Vec::new(),
    };
    let checker = PermissionChecker::new(Some(&config), None).with_session_rules(session_rules);
    let result = checker.check_legacy("bash", None).await;
    assert!(matches!(result, PermissionResult::Allow));
}

#[tokio::test]
async fn test_permission_checker_with_stored_decision() {
    let checker = PermissionChecker::new(None, None);
    checker
        .always_allow_legacy("read", Some("/tmp/test.txt"))
        .await;
    let result = checker.check_legacy("read", Some("/tmp/test.txt")).await;
    assert!(matches!(result, PermissionResult::Allow));
}

#[tokio::test]
async fn test_permission_checker_stored_decision_requires_path_match() {
    // For a non-read-only tool, a path-specific allow does NOT match a
    // different path, so the result falls through to the default (Ask).
    let checker = PermissionChecker::new(None, None);
    checker
        .always_allow_legacy("edit", Some("/tmp/test.txt"))
        .await;
    let result = checker.check_legacy("edit", Some("/tmp/other.txt")).await;
    assert!(matches!(result, PermissionResult::Ask(_)));
}

#[tokio::test]
async fn test_permission_bash_patterns_allow_matching() {
    let ruleset = PermissionRuleset {
        default: PermissionLevel::Deny,
        tool_rules: vec![ToolRule {
            tool: "bash".to_string(),
            level: PermissionLevel::Ask,
            paths: None,
            bash_patterns: Some(vec!["git *".to_string(), "ls".to_string()]),
        }],
        path_rules: Vec::new(),
    };
    let checker = PermissionChecker::new(None, None).with_session_rules(ruleset);

    let result = checker
        .check_bash_legacy(None, Some("git push origin main"))
        .await;
    assert!(matches!(result, PermissionResult::Ask(_)));

    let result = checker.check_bash_legacy(None, Some("ls")).await;
    assert!(matches!(result, PermissionResult::Ask(_)));

    let result = checker.check_bash_legacy(None, Some("rm -rf")).await;
    assert!(matches!(result, PermissionResult::Deny));
}

#[tokio::test]
async fn test_permission_bash_patterns_wildcard() {
    let ruleset = PermissionRuleset {
        default: PermissionLevel::Deny,
        tool_rules: vec![ToolRule {
            tool: "bash".to_string(),
            level: PermissionLevel::Ask,
            paths: None,
            bash_patterns: Some(vec!["*".to_string()]),
        }],
        path_rules: Vec::new(),
    };
    let checker = PermissionChecker::new(None, None).with_session_rules(ruleset);

    let result = checker.check_bash_legacy(None, Some("any command")).await;
    assert!(matches!(result, PermissionResult::Ask(_)));
}

#[tokio::test]
async fn test_permission_bash_patterns_empty_allows_all() {
    let ruleset = PermissionRuleset {
        default: PermissionLevel::Deny,
        tool_rules: vec![ToolRule {
            tool: "bash".to_string(),
            level: PermissionLevel::Ask,
            paths: None,
            bash_patterns: Some(vec![]),
        }],
        path_rules: Vec::new(),
    };
    let checker = PermissionChecker::new(None, None).with_session_rules(ruleset);

    let result = checker.check_bash_legacy(None, Some("any command")).await;
    assert!(matches!(result, PermissionResult::Ask(_)));
}

#[tokio::test]
async fn test_permission_bash_patterns_none_allows_all() {
    let ruleset = PermissionRuleset {
        default: PermissionLevel::Deny,
        tool_rules: vec![ToolRule {
            tool: "bash".to_string(),
            level: PermissionLevel::Ask,
            paths: None,
            bash_patterns: None,
        }],
        path_rules: Vec::new(),
    };
    let checker = PermissionChecker::new(None, None).with_session_rules(ruleset);

    let result = checker.check_bash_legacy(None, Some("any command")).await;
    assert!(matches!(result, PermissionResult::Ask(_)));
}
