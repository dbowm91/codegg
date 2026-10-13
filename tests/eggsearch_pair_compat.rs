//! Offline CodeGG-runtime + pinned eggsearch subprocess compatibility gate.
//!
//! Run through `scripts/test-eggsearch-pair.sh`; the ignored test is kept
//! separate from routine workspace suites because it launches a real child.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use codegg::config::schema::{Config, EggsearchConfig, SearchBackendConfig, SearchConfig};
use codegg::search_backend::bootstrap::{
    bootstrap_search_runtime, EGGSEARCH_BUNDLED_COMPLETE_SURFACE, EGGSEARCH_WEBSEARCH_PROVIDER_IDS,
};
use codegg::search_backend::state;
use serde_json::Value;

const EXPECTED_VERSION: &str = "0.4.1";

fn exact_version_line(output: &str) -> Option<&str> {
    let line = output.lines().next()?.trim();
    let tokens = line.split_whitespace().collect::<Vec<_>>();
    tokens.windows(2).find_map(|pair| {
        (pair[0].eq_ignore_ascii_case("eggsearch")
            && pair[1].trim_start_matches(['v', 'V']) == EXPECTED_VERSION)
            .then_some(line)
    })
}

fn smoke_config(binary: String) -> Config {
    Config {
        search: Some(SearchConfig {
            backend: Some(SearchBackendConfig::Eggsearch),
            fallback_to_builtin: Some(false),
            eggsearch: Some(EggsearchConfig {
                command: Some(binary),
                args: Some(vec!["mcp".to_string(), "stdio".to_string()]),
                timeout_ms: Some(30_000),
                provider_status_timeout_ms: Some(15_000),
                env: Some(HashMap::new()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[tokio::test]
#[ignore = "run only via scripts/test-eggsearch-pair.sh"]
async fn pinned_eggsearch_process_pair_is_compatible_offline() {
    let binary = PathBuf::from(
        std::env::var_os("CODEGG_EGGSEARCH_BIN")
            .expect("pair harness must set CODEGG_EGGSEARCH_BIN"),
    );
    assert!(
        binary.is_absolute(),
        "eggsearch binary path must be absolute"
    );
    let binary = binary.canonicalize().expect("eggsearch binary must exist");
    let version = Command::new(&binary)
        .arg("--version")
        .output()
        .expect("exact pinned eggsearch binary must execute --version");
    assert!(version.status.success(), "eggsearch --version failed");
    let version_stdout = String::from_utf8_lossy(&version.stdout);
    let version_line = exact_version_line(&version_stdout)
        .unwrap_or_else(|| panic!("expected eggsearch {EXPECTED_VERSION}, got {version_stdout:?}"));

    let private = tempfile::tempdir().expect("private compatibility directory");
    let home = private.path().join("home");
    let config = home.join("config");
    let workspace = home.join("workspace");
    std::fs::create_dir_all(&config).expect("private config directory");
    std::fs::create_dir_all(&workspace).expect("private workspace directory");
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", &config);

    let revision = std::env::var("CODEGG_REVISION").unwrap_or_else(|_| "unknown".to_string());
    let upstream_tag = std::env::var("CODEGG_EGGSEARCH_TAG")
        .unwrap_or_else(|_| "v0.4.1 (binary-supplied)".to_string());
    println!(
        "pair_identity codegg_revision={revision} eggsearch_tag={upstream_tag} binary={} version={}",
        binary.display(),
        version_line
    );
    println!(
        "pair_isolation home={} config={} workspace={} credentials=cleared network_requests=none",
        home.display(),
        config.display(),
        workspace.display()
    );

    let (runtime, report) =
        bootstrap_search_runtime(&smoke_config(binary.to_string_lossy().into_owned())).await;
    assert!(
        report.connected,
        "MCP connection failed: {:?}",
        report.connection_error
    );
    assert_eq!(report.server_version.as_deref(), Some(EXPECTED_VERSION));
    assert_eq!(report.server_version_compatible, Some(true));
    assert_eq!(report.bundled_surface_complete, Some(true));
    let mut tools = report.tools.clone();
    let mut expected_tools = EGGSEARCH_BUNDLED_COMPLETE_SURFACE
        .iter()
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();
    tools.sort();
    expected_tools.sort();
    assert_eq!(
        tools, expected_tools,
        "pinned ten-tool MCP inventory differs"
    );

    let handle = state::mcp_service().expect("bootstrap installs one shared MCP session");
    let raw = handle
        .read()
        .await
        .call_tool(
            "eggsearch",
            "provider_status",
            serde_json::json!({"probe": false}),
        )
        .await
        .expect("provider_status(probe=false) must succeed");
    let status: Value = serde_json::from_str(&raw).expect("provider_status returns JSON");
    assert_eq!(
        status.pointer("/probe/requested").and_then(Value::as_bool),
        Some(false)
    );
    let providers = status
        .get("providers")
        .and_then(Value::as_array)
        .expect("provider_status providers descriptor array");
    let provider_ids = providers
        .iter()
        .filter_map(|provider| provider.get("id").and_then(Value::as_str))
        .collect::<std::collections::HashSet<_>>();
    for expected in EGGSEARCH_WEBSEARCH_PROVIDER_IDS {
        assert!(
            provider_ids.contains(expected),
            "upstream lacks mapped provider {expected}"
        );
    }
    assert_eq!(report.provider_inventory_compatible, Some(true));
    println!(
        "pair_contract initialize=pass tools=10 provider_status_probe=false mapped_provider_ids={} provider_contract=pass",
        EGGSEARCH_WEBSEARCH_PROVIDER_IDS.len()
    );

    // No search call is issued: provider requests cannot be accepted as an
    // offline gate because eggsearch may reject them before provider routing.
    drop(runtime);
    handle.write().await.shutdown_all().await;
    println!("pair_teardown shared_stdio_process=shutdown");
}
