use std::fs;
use std::path::Path;
use std::sync::Arc;

use codegg::agent::asset_context::{AssetContextBuilder, ProjectId};
use codegg::agent::asset_snapshot_builder::ProjectAssetSnapshotBuilder;
use codegg::config::schema::Config;
use codegg::mcp::{McpServerOrigin, McpService, McpTool};
use codegg::plugin::activation::{PluginActivationScope, PluginActivationStore};
use codegg::plugin::contributions::{
    PluginAssetPath, ResolvedPluginContribution, ResolvedPluginContributionSet,
    ResolvedPluginMcpServer,
};
use codegg::plugin::manifest::{PluginContributions, PluginManifest, PluginMcpServerContribution};
use codegg::plugin::registry::{PluginInfo, PluginSourceMetadata};
use codegg::plugin::PluginTrustClass;

fn asset_path(plugin: &str, version: &str, root: &Path, relative: &str) -> PluginAssetPath {
    PluginAssetPath {
        plugin_id: plugin.to_string(),
        plugin_version: version.to_string(),
        relative_path: relative.to_string(),
        path: root.join(relative),
    }
}

fn context(
    root: &Path,
    contributions: Option<Arc<ResolvedPluginContributionSet>>,
) -> codegg::agent::asset_context::AssetContext {
    let builder = AssetContextBuilder::new()
        .with_synthetic_project_id(ProjectId::new())
        .with_workspace_root(root);
    let builder = match contributions {
        Some(set) => builder.with_plugin_contributions(set),
        None => builder,
    };
    builder.build().unwrap()
}

fn contribution_set(root: &Path) -> Arc<ResolvedPluginContributionSet> {
    Arc::new(ResolvedPluginContributionSet {
        plugins: vec![ResolvedPluginContribution {
            plugin_id: "plugin:reviewer".to_string(),
            plugin_version: "1.2.3".to_string(),
            install_root: root.to_path_buf(),
            skills: vec![asset_path("plugin:reviewer", "1.2.3", root, "skills")],
            agents: vec![asset_path(
                "plugin:reviewer",
                "1.2.3",
                root,
                "agents/reviewer.md",
            )],
            instructions: vec![asset_path(
                "plugin:reviewer",
                "1.2.3",
                root,
                "instructions/review.md",
            )],
            mcp_servers: Vec::new(),
        }],
        diagnostics: Vec::new(),
    })
}

#[test]
fn active_plugin_assets_are_namespaced_and_project_assets_keep_their_identity() {
    let workspace = tempfile::tempdir().unwrap();
    let plugin = tempfile::tempdir().unwrap();
    fs::create_dir_all(plugin.path().join("skills/shared")).unwrap();
    fs::create_dir_all(plugin.path().join("agents")).unwrap();
    fs::create_dir_all(plugin.path().join("instructions")).unwrap();
    fs::create_dir_all(workspace.path().join(".codegg/skills/shared")).unwrap();
    fs::write(
        plugin.path().join("skills/shared/SKILL.md"),
        "---\nname: shared\ndescription: plugin\n---\nplugin skill",
    )
    .unwrap();
    fs::write(
        workspace.path().join(".codegg/skills/shared/SKILL.md"),
        "---\nname: shared\ndescription: project\n---\nproject skill",
    )
    .unwrap();
    fs::write(
        plugin.path().join("agents/reviewer.md"),
        "---\nname: reviewer\ndescription: plugin reviewer\nmode: primary\n---\nreview prompt",
    )
    .unwrap();
    fs::write(
        plugin.path().join("instructions/review.md"),
        "plugin instruction",
    )
    .unwrap();

    let snapshot =
        ProjectAssetSnapshotBuilder::with_default_config_doc(Arc::new(Config::default()))
            .build(&context(
                workspace.path(),
                Some(contribution_set(plugin.path())),
            ))
            .unwrap();
    assert_eq!(
        snapshot.skills.get("shared").unwrap().description,
        "project"
    );
    assert_eq!(
        snapshot.skills.get("plugin:reviewer:shared").unwrap().body,
        "\nplugin skill"
    );
    assert!(snapshot.get_agent("plugin:reviewer:reviewer").is_some());
    assert_eq!(
        snapshot.instruction_fragments()[0].plugin_id.as_deref(),
        Some("plugin:reviewer")
    );
}

#[test]
fn workspace_without_activation_cannot_see_plugin_assets_and_old_snapshot_stays_pinned() {
    let workspace = tempfile::tempdir().unwrap();
    let plugin = tempfile::tempdir().unwrap();
    fs::create_dir_all(plugin.path().join("skills/demo")).unwrap();
    fs::write(
        plugin.path().join("skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: demo\n---\nbody",
    )
    .unwrap();
    let builder = ProjectAssetSnapshotBuilder::with_default_config_doc(Arc::new(Config::default()));
    let active = builder
        .build(&context(
            workspace.path(),
            Some(contribution_set_for_skill(plugin.path())),
        ))
        .unwrap();
    let disabled = builder.build(&context(workspace.path(), None)).unwrap();

    assert!(active.skills.get("plugin:reviewer:demo").is_some());
    assert!(disabled
        .skills
        .effective
        .iter()
        .all(|skill| !skill.normalized_name.starts_with("plugin:")));
    assert!(active.skills.get("plugin:reviewer:demo").is_some());
}

fn contribution_set_for_skill(root: &Path) -> Arc<ResolvedPluginContributionSet> {
    Arc::new(ResolvedPluginContributionSet {
        plugins: vec![ResolvedPluginContribution {
            plugin_id: "plugin:reviewer".into(),
            plugin_version: "1.2.3".into(),
            install_root: root.to_path_buf(),
            skills: vec![asset_path("plugin:reviewer", "1.2.3", root, "skills")],
            agents: Vec::new(),
            instructions: Vec::new(),
            mcp_servers: Vec::new(),
        }],
        diagnostics: Vec::new(),
    })
}

#[tokio::test(flavor = "current_thread")]
async fn durable_activation_resolution_is_the_source_of_contributions() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("skills")).unwrap();
    let plugin = PluginInfo {
        id: "plugin:reviewer".into(),
        manifest: PluginManifest {
            name: "reviewer".into(),
            version: "1.0.0".into(),
            contributions: PluginContributions {
                skills: vec!["skills".into()],
                ..Default::default()
            },
            ..Default::default()
        },
        enabled: true,
        trust: PluginTrustClass::TrustedLocal,
        diagnostics: Vec::new(),
        source: Some(PluginSourceMetadata::registry_loaded(
            root.path().to_path_buf(),
        )),
    };
    let store = PluginActivationStore::in_memory();
    store
        .set(&plugin, PluginActivationScope::Global, true)
        .await
        .unwrap();
    store
        .set(
            &plugin,
            PluginActivationScope::Workspace("workspace-b".into()),
            false,
        )
        .await
        .unwrap();
    let a = store
        .resolve(std::slice::from_ref(&plugin), Some("workspace-a"))
        .await;
    let b = store.resolve(&[plugin], Some("workspace-b")).await;
    assert!(a.is_active("plugin:reviewer"));
    assert!(!b.is_active("plugin:reviewer"));
    let active_contributions =
        ResolvedPluginContributionSet::resolve(&[plugin_for_contributions(root.path())], &a);
    assert_eq!(active_contributions.plugins.len(), 1);
}

fn plugin_for_contributions(root: &Path) -> PluginInfo {
    PluginInfo {
        id: "plugin:reviewer".into(),
        manifest: PluginManifest {
            name: "reviewer".into(),
            version: "1.0.0".into(),
            contributions: PluginContributions {
                skills: vec!["skills".into()],
                ..Default::default()
            },
            ..Default::default()
        },
        enabled: true,
        trust: PluginTrustClass::TrustedLocal,
        diagnostics: Vec::new(),
        source: Some(PluginSourceMetadata::registry_loaded(root.to_path_buf())),
    }
}

#[test]
fn malformed_manifest_contribution_is_rejected_without_credential_leak() {
    let raw = r#"
name = "unsafe"
version = "1"
[contributions]
skills = ["../escape"]
[[contributions.mcp_servers]]
name = "remote"
type = "http"
url = "https://example.test/mcp"
headers = { Authorization = "secret" }
"#;
    let manifest: PluginManifest = toml::from_str(raw).unwrap();
    let error = manifest.validate_contributions().unwrap_err();
    assert!(error.contains("relative") || error.contains("credential"));
    assert!(!error.contains("secret"));
}

#[tokio::test(flavor = "current_thread")]
async fn mcp_reconciliation_removes_only_plugin_origin_and_rejects_config_collision() {
    let mut service = McpService::new();
    service.register_mock_server(
        "configured",
        vec![McpTool {
            name: "configured_tool".into(),
            description: "configured".into(),
            input_schema: serde_json::json!({}),
            server: "configured".into(),
        }],
        Box::new(|_, _| Ok("ok".into())),
    );
    service.register_mock_plugin_server(
        "plugin:alpha:owned",
        "plugin:alpha",
        "1",
        vec![],
        Box::new(|_, _| Ok("ok".into())),
    );
    let desired = ResolvedPluginMcpServer {
        plugin_id: "plugin:beta".into(),
        plugin_version: "1".into(),
        canonical_name: "configured".into(),
        declaration: PluginMcpServerContribution {
            name: "configured".into(),
            server_type: "stdio".into(),
            command: Some("echo".into()),
            ..Default::default()
        },
    };
    let report = service.reconcile_plugin_servers(&[desired]).await;
    assert_eq!(report.collisions.len(), 1);
    assert_eq!(report.removed, vec!["plugin:alpha:owned"]);
    assert!(service.server_tools().contains_key("configured"));
    assert!(!service.server_tools().contains_key("plugin:alpha:owned"));
    assert!(matches!(
        service.server_origin("configured"),
        Some(McpServerOrigin::Configured)
    ));
}

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn mcp_reconciliation_connects_stdio_alias_through_local_lifecycle() {
    let mut service = McpService::new();
    let fixture = r#"
while IFS= read -r request; do
    case "$request" in
        *'"method":"initialize"'*)
            printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"name":"fixture","version":"1"}}}'
            ;;
        *'"method":"tools/list"'*)
            printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"fixture_tool","description":"fixture","inputSchema":{"type":"object","properties":{}}}]}}'
            ;;
    esac
done
"#;
    let desired = ResolvedPluginMcpServer {
        plugin_id: "plugin:fixture".into(),
        plugin_version: "1".into(),
        canonical_name: "plugin:fixture:stdio".into(),
        declaration: PluginMcpServerContribution {
            name: "stdio".into(),
            server_type: "stdio".into(),
            command: Some("/bin/sh".into()),
            args: vec!["-c".into(), fixture.into()],
            timeout: Some(5_000),
            ..Default::default()
        },
    };

    let report = service.reconcile_plugin_servers(&[desired]).await;
    assert_eq!(report.connected, vec!["plugin:fixture:stdio"]);
    assert!(
        report.failed.is_empty(),
        "unexpected failures: {:?}",
        report.failed
    );
    assert_eq!(
        service.server_tools()["plugin:fixture:stdio"][0].name,
        "fixture_tool"
    );
    assert_eq!(
        service.server_origin("plugin:fixture:stdio"),
        Some(McpServerOrigin::Plugin {
            plugin_id: "plugin:fixture".into(),
            plugin_version: "1".into(),
        })
    );

    service.shutdown_all().await;
}

#[tokio::test(flavor = "current_thread")]
async fn mcp_reconciliation_routes_http_alias_to_remote_validation_path() {
    let mut service = McpService::new();
    let desired = ResolvedPluginMcpServer {
        plugin_id: "plugin:fixture".into(),
        plugin_version: "1".into(),
        canonical_name: "plugin:fixture:http".into(),
        declaration: PluginMcpServerContribution {
            name: "http".into(),
            server_type: "http".into(),
            url: Some("not a valid URL".into()),
            ..Default::default()
        },
    };

    let report = service.reconcile_plugin_servers(&[desired]).await;
    assert_eq!(report.connected, Vec::<String>::new());
    assert_eq!(report.failed.len(), 1);
    assert!(!report.failed[0].contains("unknown server type"));
    assert!(!service.server_tools().contains_key("plugin:fixture:http"));
}

#[tokio::test(flavor = "current_thread")]
async fn inactive_plugin_mcp_contributions_do_not_resolve_or_materialize() {
    let root = tempfile::tempdir().unwrap();
    let plugin = PluginInfo {
        id: "plugin:fixture".into(),
        manifest: PluginManifest {
            name: "fixture".into(),
            version: "1.0.0".into(),
            contributions: PluginContributions {
                mcp_servers: vec![PluginMcpServerContribution {
                    name: "stdio".into(),
                    server_type: "stdio".into(),
                    command: Some("/bin/sh".into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
            ..Default::default()
        },
        enabled: true,
        trust: PluginTrustClass::TrustedLocal,
        diagnostics: Vec::new(),
        source: Some(PluginSourceMetadata::registry_loaded(
            root.path().to_path_buf(),
        )),
    };
    let store = PluginActivationStore::in_memory();
    store
        .set(
            &plugin,
            PluginActivationScope::Workspace("workspace-a".into()),
            false,
        )
        .await
        .unwrap();
    let activation = store
        .resolve(std::slice::from_ref(&plugin), Some("workspace-a"))
        .await;
    assert!(!activation.is_active("plugin:fixture"));

    let contributions =
        ResolvedPluginContributionSet::resolve(std::slice::from_ref(&plugin), &activation);
    assert!(contributions.mcp_servers().next().is_none());
}
