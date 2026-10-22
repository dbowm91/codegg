//! Offline CodeGG native-wrapper + pinned eggsearch subprocess compatibility gate.
//!
//! Run through `scripts/test-eggsearch-pair.sh`; the ignored test launches the
//! immutable eggsearch child with a private config and workspace.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use codegg::config::schema::{Config, EggsearchConfig, SearchBackendConfig, SearchConfig};
use codegg::mcp::McpExposurePolicy;
use codegg::search_backend::bootstrap::{
    bootstrap_search_runtime, EGGSEARCH_BUNDLED_COMPLETE_SURFACE, EGGSEARCH_WEBSEARCH_PROVIDER_IDS,
};
use codegg::search_backend::state;
use codegg::tool::repo_search::RepoSearchTool;
use codegg::tool::security_search::SecuritySearchTool;
use codegg::tool::websearch::WebSearchTool;
use codegg::tool::Tool;
use serde_json::{json, Value};

const EXPECTED_VERSION: &str = "0.4.4";

fn exact_version_line(output: &str) -> Option<&str> {
    let line = output.lines().next()?.trim();
    let tokens = line.split_whitespace().collect::<Vec<_>>();
    tokens.windows(2).find_map(|pair| {
        (pair[0].eq_ignore_ascii_case("eggsearch")
            && pair[1].trim_start_matches(['v', 'V']) == EXPECTED_VERSION)
            .then_some(line)
    })
}

fn smoke_config(binary: String, private: &Path, observation: &Path, child_stderr: &Path) -> Config {
    let config_path = private.join("config").join("eggsearch").join("config.toml");
    let mut env = HashMap::new();
    env.insert(
        "HOME".to_string(),
        private.join("home").to_string_lossy().into_owned(),
    );
    env.insert(
        "XDG_CONFIG_HOME".to_string(),
        private.join("config").to_string_lossy().into_owned(),
    );
    env.insert(
        "XDG_DATA_HOME".to_string(),
        private.join("data").to_string_lossy().into_owned(),
    );
    env.insert(
        "XDG_CACHE_HOME".to_string(),
        private.join("cache").to_string_lossy().into_owned(),
    );
    env.insert(
        "CODEGG_PAIR_CHILD_SENTINEL".to_string(),
        "injected-by-codegg".to_string(),
    );
    env.insert(
        "CODEGG_PAIR_ECHO_FILE".to_string(),
        observation.to_string_lossy().into_owned(),
    );
    env.insert(
        "CODEGG_PAIR_STDERR_FILE".to_string(),
        child_stderr.to_string_lossy().into_owned(),
    );
    env.insert(
        "CODEGG_EGGSEARCH_REAL_BIN".to_string(),
        std::env::var("CODEGG_EGGSEARCH_REAL_BIN").expect("pair harness sets real binary"),
    );

    #[cfg(target_os = "windows")]
    {
        env.insert(
            "APPDATA".to_string(),
            private.join("appdata").to_string_lossy().into_owned(),
        );
        env.insert(
            "USERPROFILE".to_string(),
            private.join("profile").to_string_lossy().into_owned(),
        );
    }

    Config {
        search: Some(SearchConfig {
            backend: Some(SearchBackendConfig::Eggsearch),
            max_repo_search_output_chars: Some(256),
            eggsearch: Some(EggsearchConfig {
                command: Some(binary),
                args: Some(vec![
                    "--config".to_string(),
                    config_path.to_string_lossy().into_owned(),
                    "mcp".to_string(),
                    "stdio".to_string(),
                ]),
                timeout_ms: Some(30_000),
                provider_status_timeout_ms: Some(15_000),
                env: Some(env),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn write_offline_config(path: &Path, workspace: &Path, local_enabled: bool) {
    let workspace = workspace
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let contents = format!(
        "[search]\nmode = \"off\"\ndefault_max_results = 5\nmax_results_cap = 5\nmax_query_chars = 256\ntimeout_ms = 1000\ndefault_providers = [\"duckduckgo\"]\n\n[search.providers]\nduckduckgo = true\n\n[fetch]\nenabled = false\n\n[local]\nenabled = {local_enabled}\nroots = [\"{}\"]\nrespect_gitignore = true\n",
        workspace
    );
    std::fs::write(path, contents).expect("write private eggsearch offline config");
}

fn make_private_repo(workspace: &Path, label: &str) -> (PathBuf, String) {
    let repo = workspace.join("private-repo");
    std::fs::create_dir_all(&repo).expect("local_fixture: create private repository");
    std::fs::write(
        workspace.join("OUT_OF_ROOT.md"),
        "CODEGG_OUT_OF_ROOT_MUST_NOT_APPEAR\n",
    )
    .expect("local_fixture: write out-of-root sentinel");
    let marker = format!("CODEGG_EGGSEARCH_LOCAL_ONLY_{label}_{}", std::process::id());
    std::fs::write(
        repo.join("LOCAL_ONLY_MARKER.md"),
        format!("# Private local qualification fixture\n\n{marker}\n"),
    )
    .expect("local_fixture: write private marker");
    std::fs::write(
        repo.join("OUT_OF_SCOPE_DECOY.md"),
        "CODEGG_OUT_OF_SCOPE_DECOY_MUST_NOT_APPEAR\n",
    )
    .expect("local_fixture: write scoped decoy");
    let init = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&repo)
        .status()
        .expect("local_fixture: git must be installed");
    assert!(
        init.success(),
        "local_fixture: initialize private Git repository"
    );
    (repo, marker)
}

fn authorized_context(root: &Path) -> codegg::tool::ToolExecutionContext {
    let mut context =
        codegg::tool::ToolExecutionContext::with_backend(codegg::tool::ToolBackendKind::Mcp);
    context.cwd = root.to_path_buf();
    context.session_id = Some(format!("pair-session-{}", std::process::id()));
    context.decision_id = Some("pair-accepted-decision".into());
    context.decision_outcome = Some("allowed".into());
    context.workspace_path_policy_id = Some("pair-workspace-policy".into());
    context.workspace_path_policy_revision = Some("1".into());
    context
}

#[cfg(unix)]
fn write_echo_launcher(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let contents = r##"#!/bin/sh
case " $* " in
  *" mcp stdio "*)
    cd "$CODEGG_PAIR_WORKSPACE"
    {
      printf 'pid=%s\n' "$$"
      printf 'cwd=%s\n' "$PWD"
      printf 'home=%s\n' "${HOME-}"
      printf 'xdg_config=%s\n' "${XDG_CONFIG_HOME-}"
      printf 'xdg_data=%s\n' "${XDG_DATA_HOME-}"
      printf 'xdg_cache=%s\n' "${XDG_CACHE_HOME-}"
      printf 'child_sentinel=%s\n' "${CODEGG_PAIR_CHILD_SENTINEL-}"
      printf 'host_scope_token_present=%s\n' "${CODEGG_EGGSEARCH_HOST_SCOPE_TOKEN+x}"
      printf 'parent_secret_present=%s\n' "${BRAVE_API_KEY+x}"
      printf 'parent_proxy_present=%s\n' "${HTTP_PROXY+x}"
      printf 'parent_sentinel_present=%s\n' "${CODEGG_PAIR_PARENT_SECRET_SENTINEL+x}"
      printf 'config_arg=%s\n' "$2"
    } > "$CODEGG_PAIR_ECHO_FILE"
    ;;
esac
exec "$CODEGG_EGGSEARCH_REAL_BIN" "$@" 2>"$CODEGG_PAIR_STDERR_FILE"
"##;
    std::fs::write(path, contents).expect("write private eggsearch launcher");
    let mut permissions = std::fs::metadata(path)
        .expect("launcher metadata")
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(path, permissions).expect("make launcher executable");
}

/// Emergency process cleanup if an assertion unwinds before normal MCP shutdown.
#[cfg(unix)]
struct ChildKillGuard {
    observation: PathBuf,
    released: bool,
}

#[cfg(unix)]
impl Drop for ChildKillGuard {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        if let Some(pid) = read_observation(&self.observation)
            .get("pid")
            .and_then(|pid| pid.parse::<u32>().ok())
        {
            let _ = Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .status();
        }
    }
}

fn read_observation(path: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
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
        "version_identity: binary path must be absolute"
    );
    let binary = binary
        .canonicalize()
        .expect("version_identity: binary must exist");
    let version = Command::new(&binary)
        .arg("--version")
        .output()
        .expect("version_identity: pinned eggsearch binary must execute --version");
    assert!(
        version.status.success(),
        "version_identity: eggsearch --version failed"
    );
    let version_stdout = String::from_utf8_lossy(&version.stdout);
    let version_line = exact_version_line(&version_stdout).unwrap_or_else(|| {
        panic!(
            "version_identity: expected exact eggsearch {EXPECTED_VERSION}, got {version_stdout:?}"
        )
    });

    let private_temp =
        tempfile::tempdir().expect("child_environment: private compatibility directory");
    let keep_private = std::env::var_os("CODEGG_PAIR_KEEP_TEMP").is_some();
    let private_path = if keep_private {
        private_temp.keep()
    } else {
        private_temp.path().to_path_buf()
    };
    let home = private_path.join("home");
    let config_root = private_path.join("config");
    let eggsearch_config_dir = config_root.join("eggsearch");
    let data = private_path.join("data");
    let cache = private_path.join("cache");
    let workspace = private_path.join("workspace");
    let eggsearch_config = eggsearch_config_dir.join("config.toml");
    let observation = private_path.join("child-environment.txt");
    let child_stderr = private_path.join("child-stderr.txt");
    for dir in [
        &home,
        &config_root,
        &eggsearch_config_dir,
        &data,
        &cache,
        &workspace,
    ] {
        std::fs::create_dir_all(dir).expect("child_environment: create private directory");
    }
    let (private_repo, marker) = make_private_repo(&workspace, "workspace-a");
    let workspace_b = private_path.join("workspace-b");
    std::fs::create_dir_all(&workspace_b).expect("local_fixture: create second root");
    let (private_repo_b, marker_b) = make_private_repo(&workspace_b, "workspace-b");
    write_offline_config(&eggsearch_config, &private_repo, true);

    #[cfg(unix)]
    let command = {
        let launcher = private_path.join("eggsearch-child-launcher.sh");
        write_echo_launcher(&launcher);
        launcher
    };
    #[cfg(not(unix))]
    let command = binary.clone();

    #[cfg(unix)]
    let mut kill_guard = ChildKillGuard {
        observation: observation.clone(),
        released: false,
    };

    let revision = std::env::var("CODEGG_REVISION").unwrap_or_else(|_| "unknown".to_string());
    let upstream_tag = std::env::var("CODEGG_EGGSEARCH_TAG")
        .unwrap_or_else(|_| "0.4.4 (binary-supplied)".to_string());
    println!(
        "version_identity codegg_revision={revision} eggsearch_tag={upstream_tag} binary={} version={version_line}",
        binary.display(),
    );

    let mut config = smoke_config(
        command.to_string_lossy().into_owned(),
        &private_path,
        &observation,
        &child_stderr,
    );
    if let Some(egg) = config
        .search
        .as_mut()
        .and_then(|search| search.eggsearch.as_mut())
    {
        let env = egg
            .env
            .as_mut()
            .expect("test config has explicit child env");
        env.insert(
            "CODEGG_PAIR_WORKSPACE".to_string(),
            workspace.to_string_lossy().into_owned(),
        );
    }
    let (runtime, report) = bootstrap_search_runtime(&config).await;
    let stderr = std::fs::read_to_string(&child_stderr).unwrap_or_default();
    assert!(
        report.connected,
        "mcp_initialize: {:?}; child_stderr={stderr:?}",
        report.connection_error
    );
    assert_eq!(
        report.server_version.as_deref(),
        Some(EXPECTED_VERSION),
        "tool_schema: version"
    );
    assert_eq!(
        report.server_version_compatible,
        Some(true),
        "tool_schema: exact managed sidecar pin"
    );
    assert_eq!(
        report.bundled_surface_complete,
        Some(true),
        "tool_schema: complete surface"
    );
    let mut tools = report.tools.clone();
    let mut expected_tools = EGGSEARCH_BUNDLED_COMPLETE_SURFACE
        .iter()
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();
    tools.sort();
    expected_tools.sort();
    assert_eq!(
        tools, expected_tools,
        "tool_schema: pinned ten-tool inventory differs"
    );

    let handle = runtime
        .mcp()
        .expect("mcp_initialize: runtime has one shared MCP session");
    assert!(
        state::mcp_service().is_some_and(|global| Arc::ptr_eq(&global, &handle)),
        "mcp_initialize: runtime did not retain the bootstrap-owned MCP session"
    );
    {
        let service = handle.read().await;
        assert!(
            service
                .list_filtered_tools(&McpExposurePolicy::hide_all())
                .is_empty(),
            "tool_schema: raw MCP tools became model-visible"
        );
        for tool in service
            .server_tools()
            .get("eggsearch")
            .expect("tool_schema: discovered Eggsearch tools")
            .iter()
            .filter(|tool| {
                matches!(
                    tool.name.as_str(),
                    "repo_search" | "repo_fetch" | "repo_map"
                )
            })
        {
            assert!(
                tool.input_schema
                    .get("properties")
                    .and_then(Value::as_object)
                    .is_none_or(|properties| !properties.contains_key("host_local_scope")),
                "tool_schema: private host scope appeared in {} schema",
                tool.name
            );
        }
    }

    let raw_status = handle
        .read()
        .await
        .call_tool_structured("eggsearch", "provider_status", json!({"probe": false}))
        .await
        .expect("provider_status: probe=false must succeed");
    assert!(
        !raw_status.is_error,
        "provider_status: upstream returned isError"
    );
    let status: Value = raw_status
        .structured
        .or_else(|| serde_json::from_str(&raw_status.text).ok())
        .expect("provider_status: response must retain JSON");
    assert_eq!(
        status.get("mode").and_then(Value::as_str),
        Some("off"),
        "child_environment: explicit private config was not loaded"
    );
    assert_eq!(
        status
            .pointer("/server_capabilities/generic_search")
            .and_then(Value::as_bool),
        Some(false)
    );
    let providers = status
        .get("providers")
        .and_then(Value::as_array)
        .expect("provider_status: providers descriptor array");
    let provider_ids = providers
        .iter()
        .filter_map(|provider| provider.get("id").and_then(Value::as_str))
        .collect::<std::collections::HashSet<_>>();
    for expected in EGGSEARCH_WEBSEARCH_PROVIDER_IDS {
        assert!(
            provider_ids.contains(expected),
            "provider_status: upstream lacks mapped provider {expected}"
        );
    }
    assert_eq!(
        report.provider_inventory_compatible,
        Some(true),
        "provider_status: inventory mismatch"
    );
    println!("provider_status probe=false mode=off generic_search=false providers={} provider_contract=pass", EGGSEARCH_WEBSEARCH_PROVIDER_IDS.len());

    let poison_config = private_path.join("config").join("poisoned-config.toml");
    let poison_contents = std::fs::read_to_string(&eggsearch_config)
        .expect("child_environment: read private config for negative control")
        .replacen("mode = \"off\"", "mode = \"live\"", 1);
    std::fs::write(&poison_config, poison_contents)
        .expect("child_environment: write live-mode negative-control config");
    let mut negative_control = codegg::mcp::McpService::new();
    let mut negative_env = HashMap::new();
    for (name, value) in [
        ("HOME", home.as_path()),
        ("XDG_CONFIG_HOME", config_root.as_path()),
        ("XDG_DATA_HOME", data.as_path()),
        ("XDG_CACHE_HOME", cache.as_path()),
    ] {
        negative_env.insert(name.to_string(), value.to_string_lossy().into_owned());
    }
    #[cfg(target_os = "windows")]
    {
        negative_env.insert(
            "APPDATA".to_string(),
            private_path.join("appdata").to_string_lossy().into_owned(),
        );
        negative_env.insert(
            "USERPROFILE".to_string(),
            private_path.join("profile").to_string_lossy().into_owned(),
        );
    }
    tokio::time::timeout(
        Duration::from_secs(10),
        negative_control.connect_stdio(
            "eggsearch-negative-control",
            &binary.to_string_lossy(),
            &[
                "--config".to_string(),
                poison_config.to_string_lossy().into_owned(),
                "mcp".to_string(),
                "stdio".to_string(),
            ],
            negative_env,
            10_000,
        ),
    )
    .await
    .expect("mcp_initialize: negative-control child handshake timed out")
    .expect("mcp_initialize: negative-control child should initialize");
    let negative_tool_count = negative_control
        .server_tools()
        .get("eggsearch-negative-control")
        .map(|tools| tools.len())
        .unwrap_or_default();
    let negative_status = negative_control
        .call_tool_structured(
            "eggsearch-negative-control",
            "provider_status",
            json!({"probe": false}),
        )
        .await
        .expect("provider_status: negative control is read-only");
    let negative_mode = negative_status
        .structured
        .or_else(|| serde_json::from_str(&negative_status.text).ok())
        .and_then(|value| value.get("mode").and_then(Value::as_str).map(str::to_owned));
    negative_control.shutdown_all().await;
    assert_eq!(
        negative_tool_count, 10,
        "negative_control: handshake/tool inventory must pass"
    );
    assert_eq!(
        negative_mode.as_deref(),
        Some("live"),
        "negative_control: intentionally poisoned config was not observed"
    );
    println!(
        "negative_control handshake=pass tools=10 config_gate=fails strengthened_gate=would_fail"
    );

    #[cfg(unix)]
    {
        let observed = read_observation(&observation);
        assert_eq!(
            observed.get("cwd"),
            Some(&workspace.to_string_lossy().into_owned()),
            "child_environment: launcher cwd was not private"
        );
        assert_eq!(
            observed.get("home"),
            Some(&home.to_string_lossy().into_owned()),
            "child_environment: HOME was not injected"
        );
        assert_eq!(
            observed.get("xdg_config"),
            Some(&config_root.to_string_lossy().into_owned()),
            "child_environment: XDG_CONFIG_HOME was not injected"
        );
        assert_eq!(
            observed.get("xdg_data"),
            Some(&data.to_string_lossy().into_owned()),
            "child_environment: XDG_DATA_HOME was not injected"
        );
        assert_eq!(
            observed.get("xdg_cache"),
            Some(&cache.to_string_lossy().into_owned()),
            "child_environment: XDG_CACHE_HOME was not injected"
        );
        assert_eq!(
            observed.get("child_sentinel").map(String::as_str),
            Some("injected-by-codegg"),
            "child_environment: configured sentinel missing"
        );
        assert_eq!(
            observed.get("host_scope_token_present").map(String::as_str),
            Some("x"),
            "child_environment: private host scope token missing"
        );
        assert!(
            observed
                .get("parent_secret_present")
                .is_none_or(String::is_empty),
            "child_environment: parent credential leaked"
        );
        assert!(
            observed
                .get("parent_proxy_present")
                .is_none_or(String::is_empty),
            "child_environment: parent proxy leaked"
        );
        assert!(
            observed
                .get("parent_sentinel_present")
                .is_none_or(String::is_empty),
            "child_environment: parent sentinel leaked"
        );
        assert_eq!(
            observed.get("config_arg"),
            Some(&eggsearch_config.to_string_lossy().into_owned()),
            "child_environment: --config did not point to explicit private file"
        );
        println!("child_environment private_config=pass private_home=pass xdg_dirs=pass cwd=pass inherited_credentials=cleared inherited_proxy=cleared");
    }

    let web_input = json!({"query": "CodeGG eggsearch pair offline wrapper proof", "provider": "auto", "num_results": 1});
    let web = WebSearchTool::with_search_runtime(runtime.clone())
        .execute_structured(web_input.clone(), None)
        .await
        .expect("wrapper_request: native websearch should preserve upstream policy response");
    assert!(
        !web.success,
        "structured_fidelity: Mode::Off policy denial must not be reported successful"
    );
    assert!(
        web.output.contains("web_search") && web.output.contains("disabled by policy"),
        "upstream_policy: expected tagged web_search denial, got {:?}",
        web.output
    );
    let web_value = web
        .value
        .clone()
        .expect("structured_fidelity: web policy error JSON retained");
    let direct_web = handle
        .read()
        .await
        .call_tool_structured(
            "eggsearch",
            "web_search",
            json!({"query": "CodeGG eggsearch pair offline wrapper proof", "max_results": 1, "response_detail": "diagnostic"}),
        )
        .await
        .expect("wrapper_request: direct paired web_search call reached same child");
    assert!(
        direct_web.is_error,
        "upstream_policy: tagged web_search denial must carry isError"
    );
    let direct_web_value = direct_web
        .structured
        .or_else(|| serde_json::from_str(&direct_web.text).ok())
        .expect("structured_fidelity: direct web denial retains JSON");
    assert_eq!(
        web_value, direct_web_value,
        "structured_fidelity: wrapper changed the upstream structured policy error"
    );
    println!("wrapper_request native=websearch upstream=web_search outcome=tagged_policy_denial is_error=true structured=preserved");

    let security_input = json!({"query": "CVE-2026-0000", "max_results": 1});
    let security = SecuritySearchTool::with_search_runtime(runtime.clone())
        .execute_structured(security_input, None)
        .await
        .expect("wrapper_request: native security_search should preserve upstream policy response");
    assert!(
        !security.success,
        "structured_fidelity: Mode::Off policy denial must not be reported successful"
    );
    assert!(
        security.output.contains("security_search")
            && security.output.contains("disabled by policy"),
        "upstream_policy: expected tagged security_search denial, got {:?}",
        security.output
    );
    let security_value = security
        .value
        .clone()
        .expect("structured_fidelity: security policy error JSON retained");
    let direct_security = handle
        .read()
        .await
        .call_tool_structured(
            "eggsearch",
            "security_search",
            json!({"query": "CVE-2026-0000", "max_results": 1, "response_detail": "diagnostic"}),
        )
        .await
        .expect("wrapper_request: direct paired security_search call reached same child");
    assert!(
        direct_security.is_error,
        "upstream_policy: tagged security_search denial must carry isError"
    );
    let direct_security_value = direct_security
        .structured
        .or_else(|| serde_json::from_str(&direct_security.text).ok())
        .expect("structured_fidelity: direct security denial retains JSON");
    assert_eq!(
        security_value, direct_security_value,
        "structured_fidelity: native security wrapper differs from the paired tagged upstream call"
    );
    assert_eq!(
        runtime.mcp().as_ref().map(Arc::as_ptr),
        Some(Arc::as_ptr(&handle)),
        "mcp_initialize: wrappers switched MCP sessions"
    );
    println!("wrapper_request native=security_search upstream=security_search outcome=tagged_policy_denial is_error=true structured=preserved");

    let local_search = RepoSearchTool::with_search_runtime(runtime.clone())
        .execute_structured(
            json!({
                "query": marker,
                "include_local": true,
                "host_local_scope": {"root": private_repo_b},
                "providers": ["duckduckgo"],
                "max_results": 5
            }),
            Some(authorized_context(&private_repo)),
        )
        .await
        .expect("repo_search: private local-only search should complete through native wrapper");
    assert!(
        local_search.success,
        "repo_search: local result should be successful"
    );
    assert!(
        local_search
            .provenance
            .as_ref()
            .is_some_and(|provenance| provenance.truncated),
        "repo_search: expected bounded model-facing output truncation"
    );
    let local_value = local_search
        .value
        .clone()
        .expect("repo_search: structured retrieval dimensions must be retained");
    let local_json = serde_json::to_string(&local_value).expect("repo_search: serialize value");
    assert!(
        local_json.contains(&marker),
        "repo_search: private marker missing: {local_json}"
    );
    assert!(
        local_json.contains("LOCAL_ONLY_MARKER.md"),
        "repo_search: marker path missing"
    );
    assert!(
        !local_json.contains("CODEGG_OUT_OF_SCOPE_DECOY_MUST_NOT_APPEAR"),
        "repo_search: scoped result leaked decoy data"
    );
    assert!(
        !local_json.contains("CODEGG_OUT_OF_ROOT_MUST_NOT_APPEAR"),
        "repo_search: result escaped configured local root"
    );

    let raw_unscoped = handle
        .read()
        .await
        .call_tool_structured(
            "eggsearch",
            "repo_search",
            json!({"query": marker, "include_local": true}),
        )
        .await
        .expect("raw MCP call must be bounded and authenticated as local-disabled");
    let raw_value = raw_unscoped
        .structured
        .or_else(|| serde_json::from_str(&raw_unscoped.text).ok());
    let raw_json = serde_json::to_string(&raw_value).expect("serialize raw MCP response");
    assert!(
        !raw_json.contains("LOCAL_ONLY_MARKER.md"),
        "raw MCP call without CodeGG workspace authority leaked local file data: {raw_json}"
    );
    let queried = local_value
        .get("providers_queried")
        .and_then(Value::as_array)
        .expect("repo_search: providers_queried inventory");
    assert_eq!(queried, &[json!("local_workspace")]);
    assert!(local_value.get("retrieval_summary").is_some());
    assert!(local_value.get("warnings").is_some());
    assert!(local_value.get("structured_warnings").is_some());
    assert!(
        local_value
            .pointer("/telemetry/subqueries")
            .and_then(Value::as_array)
            .is_none_or(|rows| rows.iter().all(|row| row
                .get("providers_attempted")
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty))),
        "repo_search: remote provider attempts reported"
    );
    let routing = local_value
        .pointer("/telemetry/routing_decision")
        .expect("repo_search: routing decision telemetry");
    assert_eq!(
        routing.get("reason").and_then(Value::as_str),
        Some("local-only execution policy denies remote dispatch")
    );
    assert!(routing
        .get("selected_providers")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty));
    assert!(
        local_json.contains("local_trusted"),
        "repo_search: local provenance missing"
    );
    println!("wrapper_request native=repo_search upstream=repo_search mode=off local_marker=pass providers_queried=local_workspace remote_attempts=0 local_provenance=pass");

    let repo_search_b = RepoSearchTool::with_search_runtime(runtime.clone());
    let repo_search_a = RepoSearchTool::with_search_runtime(runtime.clone());
    let search_b = repo_search_b.execute_structured(
        json!({"query": marker_b, "include_local": true, "max_results": 5}),
        Some(authorized_context(&private_repo_b)),
    );
    let search_a = repo_search_a.execute_structured(
        json!({"query": marker, "include_local": true, "max_results": 5}),
        Some(authorized_context(&private_repo)),
    );
    let (second_search, first_again) = tokio::join!(search_b, search_a);
    let second_search = second_search.expect("repo_search: second authorized workspace search");
    let first_again = first_again.expect("repo_search: concurrent workspace A search");
    let second_json = serde_json::to_string(&second_search.value).expect("serialize second scope");
    assert!(
        second_json.contains(&marker_b),
        "workspace B marker missing"
    );
    assert!(
        !second_json.contains(&marker),
        "workspace A leaked into workspace B"
    );
    let concurrent_a_json =
        serde_json::to_string(&first_again.value).expect("serialize concurrent workspace A");
    assert!(
        concurrent_a_json.contains(&marker),
        "concurrent workspace A marker missing"
    );
    assert!(
        !concurrent_a_json.contains(&marker_b),
        "concurrent workspace B leaked into A"
    );

    let first_warm = RepoSearchTool::with_search_runtime(runtime.clone())
        .execute_structured(
            json!({"query": marker, "include_local": true, "max_results": 5}),
            Some(authorized_context(&private_repo)),
        )
        .await
        .expect("repo_search: warm-cache workspace A search");
    let first_again_json = serde_json::to_string(&first_warm.value).expect("serialize warm scope");
    assert!(
        first_again_json.contains(&marker),
        "workspace A warm cache lost its marker"
    );
    assert!(
        !first_again_json.contains(&marker_b),
        "workspace B leaked into warm workspace A search"
    );

    let no_match = RepoSearchTool::with_search_runtime(runtime.clone())
        .execute_structured(
            json!({"query": "CODEGG_NO_MATCH_7d6f8162", "include_local": true}),
            Some(authorized_context(&private_repo)),
        )
        .await
        .expect("repo_search: scoped local no-match should return truthful structured coverage");
    assert!(
        no_match.success,
        "repo_search: local no-match is a completed local query"
    );
    let no_match_value = no_match
        .value
        .expect("repo_search: no-match structured value");
    assert!(no_match_value
        .get("groups")
        .and_then(Value::as_array)
        .is_none_or(|groups| groups.iter().all(|group| group
            .get("results")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty))));
    assert_eq!(
        no_match_value
            .get("providers_queried")
            .and_then(Value::as_array),
        Some(&vec![json!("local_workspace")])
    );
    assert!(no_match_value.get("retrieval_summary").is_some());
    assert!(no_match_value
        .pointer("/telemetry/subqueries")
        .and_then(Value::as_array)
        .is_none_or(|rows| rows.iter().all(|row| row
            .get("providers_attempted")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty))));
    assert!(no_match_value
        .pointer("/telemetry/routing_decision/selected_providers")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty));
    println!(
        "wrapper_request native=repo_search outcome=scoped_local_no_match remote_coverage=skipped"
    );

    let local_denied = RepoSearchTool::with_search_runtime(runtime.clone())
        .execute_structured(
            json!({"query": marker, "include_local": false}),
            Some(authorized_context(&private_repo)),
        )
        .await
        .expect("repo_search: include_local=false should preserve typed policy response");
    assert!(
        !local_denied.success,
        "repo_search: disabled local inclusion cannot report success"
    );
    assert!(
        local_denied.output.contains("disabled by policy"),
        "repo_search: expected policy denial, got {:?}",
        local_denied.output
    );
    println!("wrapper_request native=repo_search include_local=false outcome=policy_denial");

    let package_selection = RepoSearchTool::with_search_runtime(runtime.clone())
        .execute_structured(
            json!({"query": "crate serde", "include_local": true, "providers": ["duckduckgo"], "package": "serde", "ecosystem": "crates_io", "include_security_context": true}),
            Some(authorized_context(&private_repo)),
        )
        .await
        .expect("repo_search: package/security context should retain local-only result");
    assert!(
        package_selection.success,
        "repo_search: package/security request was not a completed local query"
    );
    let package_value = package_selection
        .value
        .expect("repo_search: package structured value");
    assert!(package_value
        .get("providers_queried")
        .and_then(Value::as_array)
        .is_some_and(|providers| providers
            .iter()
            .any(|provider| provider == "local_workspace")
            && providers
                .iter()
                .all(|provider| provider == "local_workspace")));
    assert_ne!(
        package_value
            .pointer("/package_resolution/verified")
            .and_then(Value::as_bool),
        Some(true),
        "repo_search: package registry coordinate was falsely verified"
    );
    assert_eq!(
        package_value
            .pointer("/package_resolution/coordinate/ecosystem")
            .and_then(Value::as_str),
        Some("crates_io")
    );
    assert!(package_value
        .pointer("/package_resolution/warnings")
        .and_then(Value::as_array)
        .is_some_and(|warnings| warnings.iter().any(|warning| warning
            .as_str()
            .is_some_and(|message| message.contains("skipped by local-only execution policy")))));
    assert!(package_value
        .get("security_context")
        .is_none_or(Value::is_null));
    assert!(package_value
        .get("warnings")
        .and_then(Value::as_array)
        .is_some_and(|warnings| warnings.iter().any(|warning| warning
            .get("message")
            .and_then(Value::as_str)
            .is_some_and(|message| message.contains("package_security_skipped")))));
    println!(
        "wrapper_request native=repo_search package_and_security_context remote_resolution=skipped"
    );

    let disabled_config_path = private_path.join("config").join("disabled-local.toml");
    write_offline_config(&disabled_config_path, &private_repo, false);
    let mut disabled_config = smoke_config(
        binary.to_string_lossy().into_owned(),
        &private_path,
        &observation,
        &child_stderr,
    );
    if let Some(egg) = disabled_config
        .search
        .as_mut()
        .and_then(|search| search.eggsearch.as_mut())
    {
        if let Some(config_arg) = egg.args.as_mut().and_then(|args| args.get_mut(1)) {
            *config_arg = disabled_config_path.to_string_lossy().into_owned();
        }
    }
    let mut disabled_service = codegg::mcp::McpService::new();
    tokio::time::timeout(
        Duration::from_secs(10),
        disabled_service.connect_stdio_host_scoped(
            "eggsearch",
            &binary.to_string_lossy(),
            &disabled_config
                .search
                .as_ref()
                .and_then(|search| search.eggsearch.as_ref())
                .and_then(|egg| egg.args.clone())
                .expect("disabled_local: explicit child config args"),
            HashMap::from([
                ("HOME".to_string(), home.to_string_lossy().into_owned()),
                (
                    "XDG_CONFIG_HOME".to_string(),
                    config_root.to_string_lossy().into_owned(),
                ),
                (
                    "XDG_DATA_HOME".to_string(),
                    data.to_string_lossy().into_owned(),
                ),
                (
                    "XDG_CACHE_HOME".to_string(),
                    cache.to_string_lossy().into_owned(),
                ),
            ]),
            10_000,
        ),
    )
    .await
    .expect("disabled_local: bounded real child handshake")
    .expect("disabled_local: real child handshake");
    let disabled_search_config = disabled_config
        .search
        .as_ref()
        .expect("disabled_local: resolved search config");
    let disabled_mcp = Arc::new(tokio::sync::RwLock::new(disabled_service));
    let disabled_runtime =
        codegg::search_backend::SearchRuntimeContext::from_config(disabled_search_config)
            .with_mcp(disabled_mcp.clone());
    let disabled_result = RepoSearchTool::with_search_runtime(disabled_runtime.clone())
        .execute_structured(json!({"query": marker, "include_local": true}), None)
        .await
        .expect("disabled_local: local request should return classified policy denial");
    assert!(
        !disabled_result.success,
        "disabled_local: absent backend cannot report complete search"
    );
    assert!(
        disabled_result.output.contains("disabled by policy"),
        "disabled_local: expected explicit denial, got {:?}",
        disabled_result.output
    );
    tokio::time::timeout(
        Duration::from_secs(5),
        disabled_mcp.write().await.shutdown_all(),
    )
    .await
    .expect("disabled_local: bounded child shutdown");
    println!("wrapper_request native=repo_search local_backend=disabled outcome=policy_denial");

    let malformed_config_path = private_path.join("config").join("malformed.toml");
    std::fs::write(&malformed_config_path, "[search\nmode = \"off\"\n")
        .expect("negative_control: write malformed private config");
    let mut malformed_service = codegg::mcp::McpService::new();
    let malformed_result = tokio::time::timeout(
        Duration::from_secs(5),
        malformed_service.connect_stdio(
            "eggsearch-malformed-config",
            &binary.to_string_lossy(),
            &[
                "--config".to_string(),
                malformed_config_path.to_string_lossy().into_owned(),
                "mcp".to_string(),
                "stdio".to_string(),
            ],
            HashMap::from([
                ("HOME".to_string(), home.to_string_lossy().into_owned()),
                (
                    "XDG_CONFIG_HOME".to_string(),
                    config_root.to_string_lossy().into_owned(),
                ),
                (
                    "XDG_DATA_HOME".to_string(),
                    data.to_string_lossy().into_owned(),
                ),
                (
                    "XDG_CACHE_HOME".to_string(),
                    cache.to_string_lossy().into_owned(),
                ),
            ]),
            5_000,
        ),
    )
    .await
    .expect("negative_control: malformed config handshake must be bounded");
    tokio::time::timeout(Duration::from_secs(5), malformed_service.shutdown_all())
        .await
        .expect("negative_control: malformed child cleanup must be bounded");
    assert!(
        malformed_result.is_err(),
        "negative_control: malformed config unexpectedly initialized MCP"
    );
    println!(
        "negative_control stage=config_identity malformed_private_config=rejected bounded=true"
    );

    for (label, input, expected) in [
        (
            "retired_google_news",
            json!({"query": "x", "provider": "google_news"}),
            "retired",
        ),
        (
            "unknown_provider",
            json!({"query": "x", "provider": "missing_provider"}),
            "unsupported provider",
        ),
        (
            "non_string_provider",
            json!({"query": "x", "provider": 7}),
            "must be a string",
        ),
    ] {
        let error = WebSearchTool::with_search_runtime(runtime.clone())
            .execute_structured(input, None)
            .await
            .expect_err("wrapper_request: invalid provider must reject before MCP transport");
        let message = error.to_string();
        assert!(
            message.contains(expected),
            "wrapper_request: {label} error class/message changed: {message}"
        );
        println!("wrapper_request negative={label} outcome=local_validation_error mcp=not_invoked");
    }
    let invalid_repo_provider = RepoSearchTool::with_search_runtime(runtime.clone())
        .execute_structured(
            json!({"query": "x", "providers": ["missing_provider"]}),
            None,
        )
        .await
        .expect_err("wrapper_request: invalid repository provider must reject before MCP");
    assert!(
        invalid_repo_provider
            .to_string()
            .contains("unsupported provider"),
        "wrapper_request: unexpected invalid repo provider error: {invalid_repo_provider}"
    );
    println!("wrapper_request negative=repo_search_unknown_provider outcome=local_validation_error mcp=not_invoked");
    // The stable alias remains covered against the exact tagged inventory by
    // search_backend_arg_mapping; the real child's provider inventory above
    // proves its destination id is present.
    println!("tool_schema native_websearch_tools=1 native_specialist_tools=1 tagged_tools=10 fake_mcp=covered separately");

    drop(runtime);
    tokio::time::timeout(Duration::from_secs(5), handle.write().await.shutdown_all())
        .await
        .expect("teardown: MCP process shutdown timed out");
    #[cfg(unix)]
    {
        let pid = read_observation(&observation)
            .get("pid")
            .cloned()
            .expect("teardown: child pid observation missing");
        let still_running = Command::new("kill")
            .args(["-0", &pid])
            .output()
            .is_ok_and(|output| output.status.success());
        assert!(
            !still_running,
            "teardown: eggsearch child {pid} remained alive after shutdown"
        );
        kill_guard.released = true;
        println!("teardown shared_stdio_process=shutdown child_pid={pid} alive=false");
    }
    println!("pair_coverage real_native_wrappers=3 tagged_tools=10 live_provider_calls=0 network_containment=not_claimed repo_search_mode_off_local_only=pass");

    if !keep_private {
        std::fs::remove_dir_all(&private_path)
            .expect("teardown: remove private home/config/workspace");
        assert!(
            !private_path.exists(),
            "teardown: temporary config or workspace persisted"
        );
    }
}
