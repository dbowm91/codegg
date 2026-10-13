//! Immutable per-turn tool descriptor snapshot (M002).
//!
//! One snapshot pins the exact descriptor universe a turn advertises,
//! discovers, and invokes:
//!
//! - MCP server origin/identity, connection liveness, and descriptor
//!   generation (`McpService::catalog_fingerprint`, no credentials);
//! - visible `mcp__*` revision (registry single origin, sorted);
//! - permitted tool names for this turn (post deny/plan/disable filtering,
//!   pre provider palette);
//! - permission policy revision;
//! - plugin activation revision (deterministic, sorted active ids);
//! - acting agent identity (authority owner);
//! - exposure hidden-server set;
//! - resolved `ResolvedToolSurface` fingerprint (filled after resolution,
//!   diagnostics only — never part of the cache key, which would be circular).
//!
//! Refresh lifecycle: the registry is rebuilt from the live inventory at
//! every turn boundary (turn/worker async pass + `AgentLoop::new` safety
//! net). In-flight turns keep their pinned adapters; a concurrent
//! `tools/list_changed`, reconnect, or re-registration never mutates an
//! active turn's authority — execution fails closed on generation mismatch
//! and the next turn picks the refreshed inventory up. No continuous polling
//! is invented for closure; when the protocol library cannot deliver a
//! notification, turn-boundary rebuild is the bounded refresh.

use std::collections::{BTreeMap, BTreeSet};

/// One MCP server's bounded identity for snapshot purposes (no credentials,
/// endpoints, tokens, or transport config).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct McpServerIdentity {
    /// Configured server name.
    pub server: String,
    /// Origin label (`configured` or `plugin:<id>@<version>`).
    pub origin: String,
    /// Liveness label (`connected`, `disconnected`, etc.).
    pub status: String,
    /// Number of tools currently advertised by this server.
    pub tool_count: usize,
}

/// Immutable per-turn descriptor snapshot (inputs only; see module docs).
#[derive(Debug, Clone)]
pub struct ToolDescriptorSnapshot {
    /// Global `McpService::catalog_fingerprint` at turn start (empty when no service).
    pub mcp_generation: String,
    /// Sorted visible `mcp__*` surface revision (registry single origin).
    pub mcp_visible_revision: String,
    /// Bounded per-server identities (sorted by server).
    pub servers: Vec<McpServerIdentity>,
    /// Permitted names for this turn (registry-visible, pre-surface).
    pub permitted_names: BTreeSet<String>,
    /// Permission policy revision (`config:<hash>`).
    pub policy_revision: String,
    /// Deterministic plugin activation revision (sorted active ids joined).
    pub plugin_activation_revision: String,
    /// Acting agent name (authority owner).
    pub agent_name: String,
    /// Hidden servers from the exposure policy (sorted).
    pub hidden_servers: Vec<String>,
    /// Resolved surface fingerprint (filled after resolution; diagnostics only).
    pub surface_fingerprint: Option<String>,
}

impl ToolDescriptorSnapshot {
    /// Stable fingerprint of the snapshot inputs (excludes `surface_fingerprint`).
    ///
    /// Used as the cache identity for `tool_def_cache`: any input change
    /// (MCP inventory, origins, permitted set, policy, plugin activation,
    /// agent, hidden set) invalidates the cached definitions. Contains no
    /// credentials or transport configuration.
    pub fn fingerprint(&self) -> String {
        let canonical = serde_json::json!({
            "mcp_generation": self.mcp_generation,
            "mcp_visible_revision": self.mcp_visible_revision,
            "servers": self.servers.iter().map(|s| serde_json::json!({
                "server": s.server,
                "origin": s.origin,
                "status": s.status,
                "tool_count": s.tool_count,
            })).collect::<Vec<_>>(),
            "permitted": self.permitted_names.iter().collect::<Vec<_>>(),
            "policy": self.policy_revision,
            "plugin_activation": self.plugin_activation_revision,
            "agent": self.agent_name,
            "hidden": self.hidden_servers,
        });
        crate::context::stable_hash_hex(canonical.to_string())
    }

    /// Build the pre-surface snapshot from already-cloned turn inputs.
    ///
    /// Takes no locks: the caller snapshots the service (via `snapshot_mcp_identities`)
    /// and the registry before calling. `permitted_names` should be the
    /// registry-visible set (post `expose_in_definitions`, pre model/plan filtering);
    /// the resolved surface narrows it further and fills `surface_fingerprint`.
    #[allow(clippy::too_many_arguments)]
    pub fn builder(
        mcp_generation: String,
        mcp_visible_revision: String,
        servers: Vec<McpServerIdentity>,
        permitted_names: BTreeSet<String>,
        policy_revision: String,
        plugin_activation_revision: String,
        agent_name: String,
        hidden_servers: Vec<String>,
    ) -> Self {
        let mut hidden = hidden_servers;
        hidden.sort();
        Self {
            mcp_generation,
            mcp_visible_revision,
            servers,
            permitted_names,
            policy_revision,
            plugin_activation_revision,
            agent_name,
            hidden_servers: hidden,
            surface_fingerprint: None,
        }
    }

    /// Attach the resolved surface fingerprint (diagnostics only).
    pub fn with_surface(mut self, fingerprint: String) -> Self {
        self.surface_fingerprint = Some(fingerprint);
        self
    }
}

/// Snapshot the MCP server identities without holding the guard across the caller.
///
/// Uses `try_read` so definition building never blocks on a write-locked
/// service; on contention returns an empty identity set with an empty
/// generation (the turn-boundary registration already installed the live
/// adapters, so definitions stay correct — only the diagnostic generation
/// degrades, and the next turn refreshes).
pub fn snapshot_mcp_identities(
    mcp_service: Option<&std::sync::Arc<tokio::sync::RwLock<crate::mcp::McpService>>>,
) -> (String, Vec<McpServerIdentity>) {
    let Some(service) = mcp_service else {
        return (String::new(), Vec::new());
    };
    let Ok(guard) = service.try_read() else {
        tracing::debug!("MCP service write-locked during snapshot; using empty generation");
        return (String::new(), Vec::new());
    };
    let generation = guard.catalog_fingerprint();
    let mut servers: Vec<McpServerIdentity> = Vec::new();
    let tools = guard.server_tools();
    for (name, status) in guard.server_status() {
        let origin = match guard.server_origin(name) {
            Some(crate::mcp::McpServerOrigin::Configured) => "configured".to_string(),
            Some(crate::mcp::McpServerOrigin::Plugin {
                plugin_id,
                plugin_version,
            }) => {
                format!("plugin:{plugin_id}@{plugin_version}")
            }
            None => "unknown".to_string(),
        };
        let status_label = match status {
            crate::mcp::McpServerStatus::Connected => "connected",
            crate::mcp::McpServerStatus::Connecting => "connecting",
            crate::mcp::McpServerStatus::Disconnected => "disconnected",
            crate::mcp::McpServerStatus::Error(_) => "error",
        }
        .to_string();
        let tool_count = tools.get(name).map(|t| t.len()).unwrap_or(0);
        servers.push(McpServerIdentity {
            server: name.to_string(),
            origin,
            status: status_label,
            tool_count,
        });
    }
    servers.sort();
    (generation, servers)
}

/// Deterministic plugin activation revision (sorted active ids joined).
///
/// Empty when no plugin service or no pinned activation (stable across turns
/// with no plugins). Contains only plugin ids, no schemas or credentials.
pub fn plugin_activation_revision(
    plugin_service: Option<&std::sync::Arc<crate::plugin::service::PluginService>>,
) -> String {
    let Some(service) = plugin_service else {
        return String::new();
    };
    // `pinned_activation` is sync and cheap; active ids are already bounded.
    if let Some(activation) = service.pinned_activation() {
        let mut ids: Vec<String> = activation.active_plugin_ids().into_iter().collect();
        ids.sort();
        ids.join(",")
    } else {
        String::new()
    }
}

/// Confine plugin `dispatch_tool_definition` output to the authorized surface.
///
/// Rules (M002 §5D):
/// - Additions outside the authorized registry/surface names are rejected
///   (skipped). An untrusted hook cannot create invocation authority.
/// - Changed names are additions under a new name and are rejected the same way.
/// - Invalid schemas (missing name/description/parameters, non-object
///   parameters) are rejected.
/// - Collisions are impossible for surviving entries: they reuse an existing
///   authorized name with its canonical schema.
/// - Permission-expanding modifications are rejected: surviving entries keep
///   their canonical `parameters` and `defer_loading` exactly; only a bounded
///   description rewording (16 KiB, NUL-stripped) may survive. Plugin metadata
///   is never an effect-class or capability grant.
/// - Allowed removals (hook returns a subset) survive: fewer advertised tools
///   is always safe.
/// - `defer_loading` is preserved from the canonical definition, never
///   quietly cleared to `None`.
///
/// Returns the confined definitions in the hook's order (stable). Logs a
/// bounded diagnostic per rejection class (counts + names, no schemas).
pub fn confine_hook_definitions(
    hook_value: &serde_json::Value,
    canonical: &BTreeMap<String, crate::provider::ToolDefinition>,
) -> Vec<crate::provider::ToolDefinition> {
    let Some(tools) = hook_value.get("tools").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut confined = Vec::with_capacity(tools.len());
    let mut rejected_additions = 0usize;
    let mut rejected_invalid = 0usize;
    let mut clamped_descriptions = 0usize;
    for entry in tools {
        let Some(name) = entry.get("name").and_then(|v| v.as_str()) else {
            rejected_invalid += 1;
            continue;
        };
        let Some(canonical_def) = canonical.get(name) else {
            // Addition outside authorized names (including renamed tools).
            rejected_additions += 1;
            continue;
        };
        let Some(description) = entry.get("description").and_then(|v| v.as_str()) else {
            rejected_invalid += 1;
            continue;
        };
        // Parameters must be present and an object to be well-formed, but the
        // surviving value is always the canonical schema — hook-supplied
        // parameters never become authority.
        let parameters_valid = entry.get("parameters").is_some_and(|p| p.is_object());
        if !parameters_valid {
            rejected_invalid += 1;
            continue;
        }
        // Bounded description rewording only.
        let mut new_description = description.replace('\0', "");
        if new_description.len() > crate::mcp::protocol::MAX_MCP_DESCRIPTION_BYTES {
            let mut end = crate::mcp::protocol::MAX_MCP_DESCRIPTION_BYTES;
            while !new_description.is_char_boundary(end) {
                end -= 1;
            }
            new_description.truncate(end);
            clamped_descriptions += 1;
        }
        confined.push(crate::provider::ToolDefinition {
            name: canonical_def.name.clone(),
            description: new_description,
            parameters: canonical_def.parameters.clone(),
            defer_loading: canonical_def.defer_loading,
        });
    }
    if rejected_additions > 0 || rejected_invalid > 0 || clamped_descriptions > 0 {
        tracing::debug!(
            rejected_additions,
            rejected_invalid,
            clamped_descriptions,
            surviving = confined.len(),
            "confined plugin tool-definition hook output to the authorized surface"
        );
    }
    confined
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::{BTreeMap, BTreeSet};

    fn canonical_map() -> BTreeMap<String, crate::provider::ToolDefinition> {
        BTreeMap::from([
            (
                "read".to_string(),
                crate::provider::ToolDefinition {
                    name: "read".into(),
                    description: "read files".into(),
                    parameters: json!({"type": "object"}),
                    defer_loading: None,
                },
            ),
            (
                "mcp__db__query".to_string(),
                crate::provider::ToolDefinition {
                    name: "mcp__db__query".into(),
                    description: "query".into(),
                    parameters: json!({"type": "object", "properties": {"q": {"type": "string"}}}),
                    defer_loading: None,
                },
            ),
        ])
    }

    #[test]
    fn hook_confinement_rejects_additions_and_preserves_canonical_schema() {
        let canonical = canonical_map();
        let hook = json!({
            "tools": [
                {"name": "read", "description": "reworded read", "parameters": {"type": "object", "properties": {"evil": {"type": "string"}}}},
                {"name": "mcp__db__query", "description": "reworded query", "parameters": {"type": "object"}},
                {"name": "mcp__evil__injected", "description": "injected", "parameters": {"type": "object"}},
                {"name": "read_renamed", "description": "renamed", "parameters": {"type": "object"}},
                {"name": "read", "description": "missing params"},
                {"description": "no name", "parameters": {"type": "object"}},
            ]
        });
        let confined = confine_hook_definitions(&hook, &canonical);
        // Only the two authorized names survive, in hook order.
        assert_eq!(confined.len(), 2);
        assert_eq!(confined[0].name, "read");
        assert_eq!(confined[0].description, "reworded read");
        // Parameters are canonical, not hook-supplied (no widening).
        assert_eq!(confined[0].parameters, json!({"type": "object"}));
        assert_eq!(confined[1].name, "mcp__db__query");
        assert_eq!(
            confined[1].parameters,
            json!({"type": "object", "properties": {"q": {"type": "string"}}})
        );
        // Deferral preserved (not cleared to None when canonical has Some).
        let mut with_defer = canonical.clone();
        with_defer.get_mut("read").unwrap().defer_loading = Some(true);
        let hook2 = json!({"tools": [{"name": "read", "description": "x", "parameters": {"type": "object"}}]});
        let confined2 = confine_hook_definitions(&hook2, &with_defer);
        assert_eq!(confined2[0].defer_loading, Some(true));
    }

    #[test]
    fn hook_removal_subset_survives() {
        let canonical = canonical_map();
        let hook = json!({"tools": [
            {"name": "read", "description": "read files", "parameters": {"type": "object"}}
        ]});
        let confined = confine_hook_definitions(&hook, &canonical);
        assert_eq!(confined.len(), 1);
        assert_eq!(confined[0].name, "read");
    }

    #[test]
    fn snapshot_fingerprint_changes_on_any_input() {
        let base = ToolDescriptorSnapshot::builder(
            "gen1".into(),
            "rev1".into(),
            vec![McpServerIdentity {
                server: "db".into(),
                origin: "configured".into(),
                status: "connected".into(),
                tool_count: 2,
            }],
            BTreeSet::from(["read".to_string(), "mcp__db__query".to_string()]),
            "config:1".into(),
            "plugin-a".into(),
            "build".into(),
            vec![],
        );
        let fp = base.fingerprint();
        assert_ne!(base.clone().with_surface("surf".into()).fingerprint(), "");
        // Surface fingerprint must not affect the cache identity.
        assert_eq!(base.clone().with_surface("surf".into()).fingerprint(), fp);
        let mut changed = base.clone();
        changed.policy_revision = "config:2".into();
        assert_ne!(changed.fingerprint(), fp);
        let mut changed2 = base.clone();
        changed2.agent_name = "plan".into();
        assert_ne!(changed2.fingerprint(), fp);
        let mut changed3 = base;
        changed3.plugin_activation_revision = "plugin-b".into();
        assert_ne!(changed3.fingerprint(), fp);
    }
}
