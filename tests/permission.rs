use codegg::config::schema::{Config, PermissionConfig, PermissionRule};
use codegg::permission::{
    agent_ruleset, config_ruleset, default_ruleset, explicit_config_ruleset, merge_rulesets,
    parse_level, PathRule, PermissionChecker, PermissionLevel, PermissionRequest, PermissionResult,
    PermissionRuleset, PermissionStore, PersistentDecision, ToolRule,
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

    // Direct file edits use the resolved workspace path authority instead of
    // an unconditional Ask rule. The ruleset fallback still asks when no
    // workspace authority is available.
    let ask_tools: Vec<_> = ruleset
        .tool_rules
        .iter()
        .filter(|r| matches!(r.level, PermissionLevel::Ask))
        .map(|r| r.tool.as_str())
        .collect();

    assert!(!ask_tools.contains(&"edit"));

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
fn test_default_ruleset_keeps_explicit_asks_for_delegation() {
    let ruleset = default_ruleset();

    // Delegation and image generation remain explicit decisions; direct
    // workspace file mutation is resolved by path authority.
    let ask_tools: Vec<_> = ruleset
        .tool_rules
        .iter()
        .filter(|r| matches!(r.level, PermissionLevel::Ask))
        .map(|r| r.tool.as_str())
        .collect();

    assert!(ask_tools.contains(&"task"));
    assert!(ask_tools.contains(&"image"));
    assert!(!ask_tools.contains(&"edit"));
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
async fn unknown_mcp_tools_remain_ask_without_local_path() {
    let checker = PermissionChecker::new(None, None);
    let result = checker.check("mcp__mail__send", None, None).await;
    assert!(matches!(result, PermissionResult::Ask(_)));
}

#[tokio::test]
async fn explicit_mcp_allow_and_deny_override_default_ask() {
    let checker = PermissionChecker::new(None, None);
    checker
        .always_allow("mcp__db__update", Some("record/1"), None)
        .await;
    assert!(matches!(
        checker
            .check("mcp__db__update", Some("record/1"), None)
            .await,
        PermissionResult::Allow
    ));

    checker.always_deny("mcp__mail__send", None, None).await;
    assert!(matches!(
        checker.check("mcp__mail__send", None, None).await,
        PermissionResult::Deny
    ));
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
async fn specific_bash_patterns_override_generic_permission_fallbacks() {
    let config = Config {
        permission: Some(PermissionConfig {
            default: Some("allow".to_string()),
            bash_deny_patterns: Some(vec!["git push*".to_string()]),
            ..Default::default()
        }),
        ..Default::default()
    };
    let checker = PermissionChecker::new(Some(&config), None);
    assert!(matches!(
        checker
            .check_with_args("bash", None, Some("git push origin main"), None)
            .await,
        PermissionResult::Deny
    ));
    assert!(matches!(
        checker
            .check_with_args("bash", None, Some("git status"), None)
            .await,
        PermissionResult::Allow
    ));

    let ask_config = Config {
        permission: Some(PermissionConfig {
            bash: Some(PermissionRule::Action("ask".to_string())),
            bash_allow_patterns: Some(vec!["echo allowed*".to_string()]),
            bash_deny_patterns: Some(vec!["echo denied*".to_string()]),
            bash_ask_patterns: Some(vec!["cargo test*".to_string()]),
            ..Default::default()
        }),
        ..Default::default()
    };
    let checker = PermissionChecker::new(Some(&ask_config), None);
    assert!(matches!(
        checker
            .check_with_args("bash", None, Some("echo allowed text"), None)
            .await,
        PermissionResult::Allow
    ));
    assert!(matches!(
        checker
            .check_with_args("bash", None, Some("echo denied text"), None)
            .await,
        PermissionResult::Deny
    ));
    assert!(matches!(
        checker
            .check_with_args("bash", None, Some("cargo test --lib"), None)
            .await,
        PermissionResult::Ask(_)
    ));
}

#[tokio::test]
async fn explicit_configured_deny_cannot_be_bypassed_by_saved_broad_allow() {
    let config = Config {
        permission: Some(PermissionConfig {
            default: Some("allow".to_string()),
            bash_deny_patterns: Some(vec!["git push*".to_string()]),
            ..Default::default()
        }),
        ..Default::default()
    };
    let checker = PermissionChecker::new(Some(&config), None);
    assert!(checker.always_allow("bash", None, None).await);
    assert!(matches!(
        checker
            .check_with_args("bash", None, Some("git push origin main"), None)
            .await,
        PermissionResult::Deny
    ));
}

#[tokio::test]
async fn authorized_workspace_development_corpus_has_no_syntax_only_asks() {
    // This is a policy fixture, not an execution test. The explicit Bash
    // Allow represents an already-authorized workspace/action; each command
    // is checked through the same permission classifier without spawning it.
    let config = Config {
        permission: Some(PermissionConfig {
            bash: Some(PermissionRule::Action("allow".to_string())),
            ..Default::default()
        }),
        ..Default::default()
    };
    let checker = PermissionChecker::new(Some(&config), None);
    let commands = [
        // Rust discovery, format, build, test, and lint.
        "cargo metadata --no-deps",
        "cargo fmt --all -- --check",
        "cargo check",
        "cargo check --workspace --all-targets",
        "cargo build",
        "cargo build --release",
        "cargo test",
        "cargo test -p codegg-core",
        "cargo test --workspace",
        "cargo clippy --workspace -- -D warnings",
        "rustc --version",
        // Python and common Python runners.
        "python --version",
        "python3 --version",
        "python -m pytest",
        "python3 -m pytest tests/",
        "uv run pytest",
        "uv run pytest -q",
        "ruff check .",
        "ruff format --check .",
        "mypy src",
        "python -c 'print(1)'",
        // JavaScript, TypeScript, Go, and generic task runners.
        "npm test",
        "npm run lint",
        "pnpm test",
        "pnpm lint",
        "yarn test",
        "yarn lint",
        "go test ./...",
        "go test -race ./...",
        "make test",
        "just test",
        "cmake --build build",
        // Workspace inspection and ordinary local maintenance.
        "pwd",
        "ls -la",
        "rg -n 'TODO' src",
        "find . -name '*.rs'",
        "cat README.md",
        "head -n 20 Cargo.toml",
        "mkdir -p target/tmp",
        "rm -rf target/tmp",
        "touch src/example.rs",
        "sed -n '1,20p' src/lib.rs",
        "git diff --check",
        // Routine local Git and shell composition.
        "git status --short",
        "git diff",
        "git log --oneline -5",
        "git add src/example.rs",
        "git commit -m 'local change'",
        "git switch feature/local-work",
        "git stash push -m local",
        "git merge feature/local-work",
        "env RUSTFLAGS=-Dwarnings cargo test -p codegg-core",
        "bash -c 'cargo check'",
        "sh -c 'python -m pytest'",
        "echo \"$(pwd)\"",
        "cargo test 2>&1 | tee target/test.log",
        "printf '%s\\n' 'check' && cargo check",
        "git status && git diff --check",
        "python -c 'print(1)' && cargo check",
        "cargo test; cargo fmt --all -- --check",
        "npm test && git status --short",
        "go test ./... 2>&1 | tee target/go-test.log",
    ];

    assert!(
        commands.len() >= 50,
        "the qualification corpus must stay broad"
    );
    let mut allow_count = 0usize;
    for command in commands {
        let outcome = checker
            .check_with_args("bash", None, Some(command), None)
            .await;
        match outcome {
            PermissionResult::Allow => allow_count += 1,
            PermissionResult::Ask(request) => {
                panic!("routine command caused a policy Ask: {command}: {request:?}");
            }
            PermissionResult::Deny => {
                panic!("routine command was denied: {command}");
            }
        }
    }
    assert_eq!(allow_count, commands.len());
}

#[tokio::test]
async fn low_friction_corpus_keeps_risk_review_and_explicit_deny() {
    let checker = PermissionChecker::new(None, None);

    // A concrete destructive target and authority escalation still produce
    // review; this only checks the decision and never executes either command.
    for command in ["rm -rf /", "sudo apt-get install package"] {
        assert!(matches!(
            checker
                .check_with_args("bash", None, Some(command), None)
                .await,
            PermissionResult::Ask(_)
        ));
    }

    // An operator-selected text restriction remains a terminal invocation Deny.
    let config = Config {
        permission: Some(PermissionConfig {
            default: Some("allow".to_string()),
            bash_deny_patterns: Some(vec!["git push*".to_string()]),
            ..Default::default()
        }),
        ..Default::default()
    };
    let checker = PermissionChecker::new(Some(&config), None);
    assert!(matches!(
        checker
            .check_with_args("bash", None, Some("git push origin main"), None)
            .await,
        PermissionResult::Deny
    ));
}

#[tokio::test]
async fn current_tool_deny_cannot_be_bypassed_by_saved_allow() {
    let config = Config {
        permission: Some(PermissionConfig {
            edit: Some(PermissionRule::Action("deny".to_string())),
            ..Default::default()
        }),
        ..Default::default()
    };
    let checker = PermissionChecker::new(Some(&config), None);
    assert!(checker.always_allow("edit", None, None).await);
    assert!(matches!(
        checker.check("edit", None, None).await,
        PermissionResult::Deny
    ));
}

#[tokio::test]
async fn allow_all_bash_fallback_does_not_override_higher_priority_deny() {
    let config = Config {
        permission: Some(PermissionConfig {
            allow_all_bash: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    };
    let checker = PermissionChecker::new(Some(&config), None).with_agent_rules(PermissionRuleset {
        default: PermissionLevel::Ask,
        tool_rules: vec![ToolRule {
            tool: "bash".to_string(),
            level: PermissionLevel::Deny,
            paths: None,
            bash_patterns: Some(vec!["git push*".to_string()]),
        }],
        path_rules: Vec::new(),
    });
    assert!(matches!(
        checker
            .check_with_args("bash", None, Some("git push origin main"), None)
            .await,
        PermissionResult::Deny
    ));
    assert!(matches!(
        checker
            .check_with_args("bash", None, Some("git status"), None)
            .await,
        PermissionResult::Allow
    ));
}

#[tokio::test]
async fn legacy_exec_mode_does_not_override_configured_deny() {
    let config = Config {
        permission: Some(PermissionConfig {
            default: Some("allow".to_string()),
            bash_deny_patterns: Some(vec!["git push*".to_string()]),
            ..Default::default()
        }),
        ..Default::default()
    };
    let checker = PermissionChecker::new(Some(&config), None).with_exec_mode();
    assert!(matches!(
        checker
            .check_with_args("bash", None, Some("git push origin main"), None)
            .await,
        PermissionResult::Deny
    ));
    assert!(matches!(
        checker
            .check_with_args("bash", None, Some("git status"), None)
            .await,
        PermissionResult::Allow
    ));
}

#[tokio::test]
async fn narrow_saved_command_grant_does_not_cover_compound_shell() {
    let checker = PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
        default: PermissionLevel::Ask,
        tool_rules: vec![ToolRule {
            tool: "bash".to_string(),
            level: PermissionLevel::Ask,
            paths: None,
            bash_patterns: None,
        }],
        path_rules: Vec::new(),
    });
    assert!(
        checker
            .always_allow_scoped("bash", None, Some("cmd:cargo"), None)
            .await
    );
    assert!(matches!(
        checker
            .check_with_args("bash", None, Some("cargo test --all"), None)
            .await,
        PermissionResult::Allow
    ));
    assert!(matches!(
        checker
            .check_with_args("bash", None, Some("cargo test; ./second.sh"), None)
            .await,
        PermissionResult::Ask(_)
    ));
    assert!(matches!(
        checker
            .check_with_args("bash", None, Some("cargo test && curl example.test"), None)
            .await,
        PermissionResult::Ask(_)
    ));
}

#[tokio::test]
async fn a_saved_compound_command_grant_covers_its_segments_but_not_a_variant() {
    let command = "cargo test; ./second.sh";
    // The whole-invocation digest still exists as a scope shape, but a
    // compound command is now remembered one segment at a time.
    let scopes = codegg::permission::decision_scopes_for_shell_command(command);
    assert_eq!(scopes.len(), 2);
    assert!(scopes.iter().any(|scope| scope == "cmd:cargo"));
    let checker = PermissionChecker::new(None, None).with_agent_rules(PermissionRuleset {
        default: PermissionLevel::Ask,
        tool_rules: vec![ToolRule {
            tool: "bash".to_string(),
            level: PermissionLevel::Ask,
            paths: None,
            bash_patterns: None,
        }],
        path_rules: Vec::new(),
    });
    for scope in &scopes {
        assert!(
            checker
                .always_allow_scoped("bash", None, Some(scope), None)
                .await
        );
    }
    assert!(matches!(
        checker
            .check_with_args("bash", None, Some(command), None)
            .await,
        PermissionResult::Allow
    ));
    // Approving a segment remembers that command family, which is what makes
    // "Allow always" converge instead of re-prompting on every variant.
    assert!(matches!(
        checker
            .check_with_args("bash", None, Some("cargo test"), None)
            .await,
        PermissionResult::Allow
    ));
    // The documented invariant is unchanged: a saved `cmd:cargo` grant cannot
    // cover a compound command whose other segment was never approved.
    assert!(matches!(
        checker
            .check_with_args("bash", None, Some("cargo test; ./other.sh"), None)
            .await,
        PermissionResult::Ask(_)
    ));
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
        scope: None,
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
        scope: None,
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

// ── Agent declarations on the main loop ──────────────────────────────────────
//
// An agent's own declaration is authoritative until the operator speaks for
// that tool. Two pieces make that true on the main loop (not the subagent
// loop): the resolved agents are enveloped against *operator-written* config
// levels (`resolve_agents_with_context`, src/agent/definition.rs:580), and the
// checker is built with those rules (`build_agent_loop`,
// src/agent/agent_loop_factory.rs).

fn agent_declaring(name: &str, permissions: &[(&str, &str)]) -> codegg::agent::Agent {
    codegg::agent::Agent {
        git_read_only: false,
        name: name.to_string(),
        permissions: permissions
            .iter()
            .map(|(tool, level)| (tool.to_string(), level.to_string()))
            .collect(),
        ..Default::default()
    }
}

/// Reproduces the main-loop wiring: safety envelope over resolved agents, then
/// checker construction with the active agent's rules.
fn main_loop_checker(
    config: &Config,
    agent: &codegg::agent::Agent,
    store_path: Option<std::path::PathBuf>,
) -> PermissionChecker {
    let session_rules = PermissionRuleset::default();
    let config_rules = explicit_config_ruleset(Some(config));
    let hard_deny = vec![
        "commit".to_string(),
        "todowrite".to_string(),
        "todoread".to_string(),
    ];
    let safe_agent = agent.apply_safety_envelope(&session_rules, &config_rules, &hard_deny);
    PermissionChecker::new(Some(config), store_path)
        .with_agent_rules(codegg::agent::agent_loop_factory::active_agent_rules(
            std::slice::from_ref(&safe_agent),
        ))
        .with_active_mode(config)
}

#[test]
fn explicit_config_ruleset_is_neutral_without_operator_permission() {
    // No `[permission]` table at all: the operator has not spoken, so the
    // config layer must contribute nothing. `config_ruleset` answers a
    // different question — what to do with an unclaimed tool — and still
    // carries the built-in Ask list.
    let config = Config::default();
    let explicit = explicit_config_ruleset(Some(&config));
    assert!(
        explicit.tool_rules.is_empty(),
        "absent [permission] must not contribute tool rules: {:?}",
        explicit.tool_rules
    );
    assert!(matches!(explicit.default, PermissionLevel::Ask));

    let defaults = config_ruleset(Some(&config));
    assert!(
        defaults
            .tool_rules
            .iter()
            .any(|rule| rule.tool == "task" && matches!(rule.level, PermissionLevel::Ask)),
        "config_ruleset must keep the built-in fallback for unclaimed tools"
    );
}

#[test]
fn explicit_config_ruleset_reports_only_operator_statements() {
    let config = Config {
        permission: Some(PermissionConfig {
            task: Some(PermissionRule::Action("deny".to_string())),
            ..Default::default()
        }),
        ..Default::default()
    };
    let explicit = explicit_config_ruleset(Some(&config));
    let task_rule = explicit
        .tool_rules
        .iter()
        .find(|rule| rule.tool == "task")
        .expect("operator-set task level is reported");
    assert!(matches!(task_rule.level, PermissionLevel::Deny));
    // A tool the operator never named is not reported as a decision, so the
    // built-in `edit` Ask must not appear here.
    assert!(!explicit.tool_rules.iter().any(|rule| rule.tool == "edit"));
    // Unconfigured bash is not synthesized into a decision either.
    assert!(!explicit.tool_rules.iter().any(|rule| rule.tool == "bash"));
}

#[tokio::test]
async fn agent_allow_applies_to_task_on_the_main_loop() {
    let config = Config::default();
    let agent = agent_declaring("build", &[("task", "allow")]);

    let checker = main_loop_checker(&config, &agent, None);
    assert!(
        matches!(
            checker.check("task", None, None).await,
            PermissionResult::Allow
        ),
        "an agent's task=allow must stand when the operator configured nothing"
    );

    // Control: the same config without the declaration still asks, so this is
    // the declaration being honored and not a weakened default.
    let bare = PermissionChecker::new(Some(&config), None);
    assert!(matches!(
        bare.check("task", None, None).await,
        PermissionResult::Ask(_)
    ));
}

#[tokio::test]
async fn operator_explicit_level_beats_agent_allow() {
    let ask_config = Config {
        permission: Some(PermissionConfig {
            task: Some(PermissionRule::Action("ask".to_string())),
            ..Default::default()
        }),
        ..Default::default()
    };
    let agent = agent_declaring("build", &[("task", "allow")]);
    let checker = main_loop_checker(&ask_config, &agent, None);
    assert!(matches!(
        checker.check("task", None, None).await,
        PermissionResult::Ask(_)
    ));

    let deny_config = Config {
        permission: Some(PermissionConfig {
            task: Some(PermissionRule::Action("deny".to_string())),
            ..Default::default()
        }),
        ..Default::default()
    };
    let checker = main_loop_checker(&deny_config, &agent, None);
    assert!(matches!(
        checker.check("task", None, None).await,
        PermissionResult::Deny
    ));
}

#[tokio::test]
async fn agent_deny_applies_without_operator_config() {
    let config = Config::default();
    let agent = agent_declaring("build", &[("task", "deny"), ("image", "deny")]);
    let checker = main_loop_checker(&config, &agent, None);

    assert!(matches!(
        checker.check("task", None, None).await,
        PermissionResult::Deny
    ));
    assert!(matches!(
        checker.check("image", None, None).await,
        PermissionResult::Deny
    ));
}

#[tokio::test]
async fn tools_no_agent_declares_keep_the_default_prompt() {
    let config = Config::default();
    let agent = agent_declaring("build", &[("task", "allow")]);
    let checker = main_loop_checker(&config, &agent, None);

    for tool in ["edit", "apply_patch", "write"] {
        assert!(
            matches!(
                checker.check(tool, None, None).await,
                PermissionResult::Ask(_)
            ),
            "{tool} is declared by no agent and must still prompt"
        );
    }
    // Unlisted mutating tool with no rule anywhere: falls back to the default.
    assert!(matches!(
        checker.check("python_script", None, None).await,
        PermissionResult::Ask(_)
    ));
}

#[tokio::test]
async fn remembered_deny_beats_agent_allow() {
    let dir = TempDir::new().unwrap();
    let store_path = dir.path().join("permissions.json");

    let config = Config::default();
    let agent = agent_declaring("build", &[("task", "allow")]);
    let checker = main_loop_checker(&config, &agent, Some(store_path));
    assert!(matches!(
        checker.check("task", None, None).await,
        PermissionResult::Allow
    ));

    assert!(checker.always_deny("task", None, None).await);
    assert!(matches!(
        checker.check("task", None, None).await,
        PermissionResult::Deny
    ));
}
