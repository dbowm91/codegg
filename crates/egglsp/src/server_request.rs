//! Server-request dispatcher for LSP JSON-RPC.
//!
//! When the language server sends a request to the client (e.g.
//! `workspace/configuration`, `client/registerCapability`), this module
//! classifies the method and returns an appropriate response. The background
//! reader in [`crate::client`] calls [`dispatch_server_request`] and writes
//! the reply back via [`crate::writer::LspWriter`].

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Configuration for workspace/configuration responses.
#[derive(Debug, Clone, Default)]
pub struct ServerRequestContext {
    pub server_id: String,
    pub root: PathBuf,
    pub configuration: serde_json::Value,
    pub workspace_folders: Vec<lsp_types::WorkspaceFolder>,
    pub dynamic_registrations: Arc<RwLock<DynamicRegistrationState>>,
}

/// Bounded dynamic registration state.
#[derive(Debug, Default)]
pub struct DynamicRegistrationState {
    registrations: HashMap<String, DynamicRegistration>,
}

/// A single dynamic registration entry.
#[derive(Debug, Clone, PartialEq)]
pub struct DynamicRegistration {
    pub id: String,
    pub method: String,
    pub register_options: Option<serde_json::Value>,
}

/// Response to a server request.
#[derive(Debug)]
pub enum ServerRequestReply {
    Result(serde_json::Value),
    Error {
        code: i64,
        message: String,
        data: Option<serde_json::Value>,
    },
}

/// Maximum number of dynamic registrations we track.
const MAX_REGISTRATIONS: usize = 256;

impl DynamicRegistrationState {
    /// Create an empty state.
    pub fn new() -> Self {
        Self {
            registrations: HashMap::new(),
        }
    }

    /// Register a new capability.
    ///
    /// If `id` already exists this is a replacement (no cap check).
    /// If `id` is new and the cap is reached, returns `Err`.
    pub fn register(
        &mut self,
        id: String,
        method: String,
        options: Option<serde_json::Value>,
    ) -> Result<(), String> {
        let is_new = !self.registrations.contains_key(&id);
        if is_new && self.registrations.len() >= MAX_REGISTRATIONS {
            return Err(format!(
                "dynamic registration limit ({}) reached",
                MAX_REGISTRATIONS
            ));
        }
        self.registrations.insert(
            id.clone(),
            DynamicRegistration {
                id,
                method,
                register_options: options,
            },
        );
        Ok(())
    }

    /// Unregister by id. Tolerates unknown ids.
    pub fn unregister(&mut self, id: &str) {
        self.registrations.remove(id);
    }

    /// Look up a registration by id.
    pub fn get(&self, id: &str) -> Option<&DynamicRegistration> {
        self.registrations.get(id)
    }

    /// Atomically register a batch of capabilities.
    ///
    /// All entries are validated and capacity-checked before any mutation.
    /// Returns `Ok(())` if all entries fit, or `Err` with the first failure reason.
    pub fn register_batch(
        &mut self,
        registrations: Vec<(String, String, Option<serde_json::Value>)>,
    ) -> Result<(), String> {
        // Count new IDs (not already present).
        let new_id_count = registrations
            .iter()
            .filter(|(id, _, _)| !self.registrations.contains_key(id))
            .count();

        // Check capacity before any mutation.
        if self.registrations.len() + new_id_count > MAX_REGISTRATIONS {
            return Err(format!(
                "dynamic registration limit ({}) would be exceeded",
                MAX_REGISTRATIONS
            ));
        }

        // Apply all entries.
        for (id, method, options) in registrations {
            self.registrations.insert(
                id.clone(),
                DynamicRegistration {
                    id,
                    method,
                    register_options: options,
                },
            );
        }
        Ok(())
    }

    /// Current count of tracked registrations.
    pub fn count(&self) -> usize {
        self.registrations.len()
    }

    /// Return a deterministic snapshot of registrations sorted by ID.
    pub fn snapshot(&self) -> Vec<DynamicRegistration> {
        let mut registrations: Vec<_> = self.registrations.values().cloned().collect();
        registrations.sort_by(|a, b| a.id.cmp(&b.id));
        registrations
    }

    /// Remove all registrations.
    pub fn clear(&mut self) {
        self.registrations.clear();
    }
}

/// Dispatch a server request based on its method.
pub async fn dispatch_server_request(
    context: &ServerRequestContext,
    method: &str,
    params: serde_json::Value,
) -> ServerRequestReply {
    match method {
        "workspace/configuration" => handle_configuration(context, &params).await,
        "workspace/workspaceFolders" => handle_workspace_folders(context).await,
        "client/registerCapability" => handle_register_capability(context, &params).await,
        "client/unregisterCapability" => handle_unregister_capability(context, &params).await,
        "window/workDoneProgress/create" => ServerRequestReply::Result(serde_json::Value::Null),
        "workspace/applyEdit" => ServerRequestReply::Result(serde_json::json!({
            "applied": false,
            "failureReason": "Codegg does not permit implicit language-server edits; request a preview and apply it through the authorized patch path",
        })),
        _ => {
            debug!(method, "unknown server request method");
            ServerRequestReply::Error {
                code: -32601,
                message: "Method not found".to_string(),
                data: None,
            }
        }
    }
}

async fn handle_configuration(
    context: &ServerRequestContext,
    params: &serde_json::Value,
) -> ServerRequestReply {
    let items = match params.get("items").and_then(|v| v.as_array()) {
        Some(items) => items,
        None => {
            warn!(
                server_id = %context.server_id,
                "workspace/configuration: missing 'items' array in params"
            );
            return ServerRequestReply::Error {
                code: -32602,
                message: "Invalid params: missing 'items' array".to_string(),
                data: None,
            };
        }
    };

    let mut values: Vec<serde_json::Value> = Vec::with_capacity(items.len());

    for item in items {
        let section = item.get("section").and_then(|s| s.as_str());
        let scope_uri = item.get("scopeUri").and_then(|s| s.as_str());

        // Check scope: if a scopeUri is given, reject if outside root.
        if let Some(uri) = scope_uri {
            if let Ok(parsed) = url::Url::parse(uri) {
                if let Ok(path) = parsed.to_file_path() {
                    if !context.root.as_os_str().is_empty() && !path.starts_with(&context.root) {
                        values.push(serde_json::Value::Null);
                        continue;
                    }
                }
            }
        }

        // Match section against configuration keys.
        match section {
            Some(sec) => {
                let val = context
                    .configuration
                    .get(sec)
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                values.push(val);
            }
            None => {
                values.push(serde_json::Value::Null);
            }
        }
    }

    ServerRequestReply::Result(serde_json::Value::Array(values))
}

async fn handle_workspace_folders(context: &ServerRequestContext) -> ServerRequestReply {
    ServerRequestReply::Result(
        serde_json::to_value(&context.workspace_folders)
            .unwrap_or(serde_json::Value::Array(Vec::new())),
    )
}

async fn handle_register_capability(
    context: &ServerRequestContext,
    params: &serde_json::Value,
) -> ServerRequestReply {
    let registrations = match params.get("registrations").and_then(|v| v.as_array()) {
        Some(regs) if !regs.is_empty() => regs,
        _ => {
            warn!(
                server_id = %context.server_id,
                "client/registerCapability: missing or empty 'registrations' array in params"
            );
            return ServerRequestReply::Error {
                code: -32602,
                message: "Invalid params: missing 'registrations' array".to_string(),
                data: None,
            };
        }
    };

    // Validate ALL entries first — reject the whole request if ANY is malformed.
    let mut parsed: Vec<(String, String, Option<serde_json::Value>)> =
        Vec::with_capacity(registrations.len());
    for reg in registrations {
        let id = match reg.get("id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => {
                return ServerRequestReply::Error {
                    code: -32602,
                    message: "Invalid params: missing registration id".to_string(),
                    data: None,
                };
            }
        };
        let method = match reg.get("method").and_then(|v| v.as_str()) {
            Some(m) => m.to_string(),
            None => {
                return ServerRequestReply::Error {
                    code: -32602,
                    message: "Invalid params: missing registration method".to_string(),
                    data: None,
                };
            }
        };
        let register_options = reg.get("registerOptions").cloned();
        parsed.push((id, method, register_options));
    }

    // Deduplicate by ID: last-write-wins within a single request.
    let mut deduped: Vec<(String, String, Option<serde_json::Value>)> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for entry in parsed {
        match seen.get(&entry.0) {
            Some(&idx) => deduped[idx] = entry,
            None => {
                seen.insert(entry.0.clone(), deduped.len());
                deduped.push(entry);
            }
        }
    }

    // Atomically apply all entries — batch is validated before any mutation.
    let mut state = context.dynamic_registrations.write().await;
    if let Err(msg) = state.register_batch(deduped) {
        return ServerRequestReply::Error {
            code: -32600,
            message: msg,
            data: None,
        };
    }
    ServerRequestReply::Result(serde_json::Value::Null)
}

async fn handle_unregister_capability(
    context: &ServerRequestContext,
    params: &serde_json::Value,
) -> ServerRequestReply {
    // Try LSP array shape: "unregisterations" (official) or "unregistrations" (compat).
    let items = params
        .get("unregisterations")
        .or_else(|| params.get("unregistrations"))
        .and_then(|v| v.as_array());

    if let Some(items) = items {
        if items.is_empty() {
            return ServerRequestReply::Error {
                code: -32602,
                message: "Invalid params: empty 'unregisterations' array".to_string(),
                data: None,
            };
        }

        // Validate each item has an ID, then unregister all.
        let mut ids: Vec<String> = Vec::with_capacity(items.len());
        for item in items {
            match item.get("id").and_then(|v| v.as_str()) {
                Some(id) => ids.push(id.to_string()),
                None => {
                    return ServerRequestReply::Error {
                        code: -32602,
                        message: "Invalid params: missing unregister id in array entry".to_string(),
                        data: None,
                    };
                }
            }
        }

        let mut state = context.dynamic_registrations.write().await;
        for id in &ids {
            state.unregister(id);
        }
        return ServerRequestReply::Result(serde_json::Value::Null);
    }

    // Fall back to single `id` field for backward compatibility.
    let id = match params.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => {
            return ServerRequestReply::Error {
                code: -32602,
                message: "Invalid params: missing 'unregisterations' array or 'id' field"
                    .to_string(),
                data: None,
            };
        }
    };

    let mut state = context.dynamic_registrations.write().await;
    state.unregister(&id);
    ServerRequestReply::Result(serde_json::Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn make_context() -> ServerRequestContext {
        let mut config = serde_json::Map::new();
        config.insert(
            "rust-analyzer".to_string(),
            serde_json::json!({"checkOnSave": true}),
        );
        config.insert(
            "pyright".to_string(),
            serde_json::json!({"typeCheckingMode": "strict"}),
        );

        let uri: lsp_types::Uri = "file:///workspace".parse().expect("valid URI");

        ServerRequestContext {
            server_id: "test".to_string(),
            root: PathBuf::from("/workspace"),
            configuration: serde_json::Value::Object(config),
            workspace_folders: vec![lsp_types::WorkspaceFolder {
                uri,
                name: "workspace".to_string(),
            }],
            dynamic_registrations: Arc::new(RwLock::new(DynamicRegistrationState::new())),
        }
    }

    // ── Configuration tests ──────────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn configuration_matching_section() {
        let ctx = make_context();
        let params = serde_json::json!({
            "items": [{"section": "rust-analyzer"}]
        });
        let reply = dispatch_server_request(&ctx, "workspace/configuration", params).await;
        match reply {
            ServerRequestReply::Result(val) => {
                let arr = val.as_array().unwrap();
                assert_eq!(arr.len(), 1);
                assert_eq!(arr[0]["checkOnSave"], true);
            }
            _ => panic!("expected Result"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn configuration_unknown_section_returns_null() {
        let ctx = make_context();
        let params = serde_json::json!({
            "items": [{"section": "unknown.server"}]
        });
        let reply = dispatch_server_request(&ctx, "workspace/configuration", params).await;
        match reply {
            ServerRequestReply::Result(val) => {
                let arr = val.as_array().unwrap();
                assert_eq!(arr.len(), 1);
                assert!(arr[0].is_null());
            }
            _ => panic!("expected Result"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn configuration_empty_items() {
        let ctx = make_context();
        let params = serde_json::json!({"items": []});
        let reply = dispatch_server_request(&ctx, "workspace/configuration", params).await;
        match reply {
            ServerRequestReply::Result(val) => {
                assert_eq!(val.as_array().unwrap().len(), 0);
            }
            _ => panic!("expected Result"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn configuration_missing_items_is_invalid() {
        let ctx = make_context();
        let params = serde_json::json!({});
        let reply = dispatch_server_request(&ctx, "workspace/configuration", params).await;
        match reply {
            ServerRequestReply::Error { code, .. } => assert_eq!(code, -32602),
            _ => panic!("expected Error"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn configuration_multiple_items() {
        let ctx = make_context();
        let params = serde_json::json!({
            "items": [
                {"section": "rust-analyzer"},
                {"section": "unknown"},
                {"section": "pyright"}
            ]
        });
        let reply = dispatch_server_request(&ctx, "workspace/configuration", params).await;
        match reply {
            ServerRequestReply::Result(val) => {
                let arr = val.as_array().unwrap();
                assert_eq!(arr.len(), 3);
                assert_eq!(arr[0]["checkOnSave"], true);
                assert!(arr[1].is_null());
                assert_eq!(arr[2]["typeCheckingMode"], "strict");
            }
            _ => panic!("expected Result"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn configuration_scope_outside_root_returns_null() {
        let ctx = make_context();
        let params = serde_json::json!({
            "items": [{"section": "rust-analyzer", "scopeUri": "file:///other/path"}]
        });
        let reply = dispatch_server_request(&ctx, "workspace/configuration", params).await;
        match reply {
            ServerRequestReply::Result(val) => {
                let arr = val.as_array().unwrap();
                assert!(arr[0].is_null());
            }
            _ => panic!("expected Result"),
        }
    }

    // ── Workspace folders tests ──────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn workspace_folders_returns_current_root() {
        let ctx = make_context();
        let params = serde_json::json!({});
        let reply = dispatch_server_request(&ctx, "workspace/workspaceFolders", params).await;
        match reply {
            ServerRequestReply::Result(val) => {
                let arr = val.as_array().unwrap();
                assert_eq!(arr.len(), 1);
                assert_eq!(arr[0]["name"], "workspace");
            }
            _ => panic!("expected Result"),
        }
    }

    // ── Register capability tests ────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn register_capability_records_registration() {
        let ctx = make_context();
        let params = serde_json::json!({
            "registrations": [{
                "id": "reg-1",
                "method": "textDocument/didOpen",
                "registerOptions": {}
            }]
        });
        let reply = dispatch_server_request(&ctx, "client/registerCapability", params).await;
        assert!(matches!(reply, ServerRequestReply::Result(_)));
        let state = ctx.dynamic_registrations.read().await;
        assert_eq!(state.count(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn register_capability_at_limit_returns_error() {
        let ctx = make_context();
        {
            let mut state = ctx.dynamic_registrations.write().await;
            for i in 0..MAX_REGISTRATIONS {
                state
                    .register(format!("id-{i}"), "test/method".to_string(), None)
                    .unwrap();
            }
        }
        let params = serde_json::json!({
            "registrations": [{"id": "overflow", "method": "x"}]
        });
        let reply = dispatch_server_request(&ctx, "client/registerCapability", params).await;
        match reply {
            ServerRequestReply::Error { code, .. } => assert_eq!(code, -32600),
            _ => panic!("expected Error at limit"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn register_missing_registrations_is_invalid() {
        let ctx = make_context();
        let params = serde_json::json!({});
        let reply = dispatch_server_request(&ctx, "client/registerCapability", params).await;
        match reply {
            ServerRequestReply::Error { code, .. } => assert_eq!(code, -32602),
            _ => panic!("expected Error"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn register_capability_multiple_registrations() {
        let ctx = make_context();
        let params = serde_json::json!({
            "registrations": [
                {"id": "r1", "method": "textDocument/didOpen"},
                {"id": "r2", "method": "textDocument/didClose"},
                {"id": "r3", "method": "textDocument/didSave", "registerOptions": {"includeText": true}}
            ]
        });
        let reply = dispatch_server_request(&ctx, "client/registerCapability", params).await;
        assert!(matches!(reply, ServerRequestReply::Result(_)));
        let state = ctx.dynamic_registrations.read().await;
        assert_eq!(state.count(), 3);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn register_capability_replacement_at_cap_succeeds() {
        let ctx = make_context();
        {
            let mut state = ctx.dynamic_registrations.write().await;
            for i in 0..MAX_REGISTRATIONS {
                state
                    .register(format!("id-{i}"), "test/method".to_string(), None)
                    .unwrap();
            }
        }
        // Replacing an existing ID at the cap should succeed.
        let params = serde_json::json!({
            "registrations": [{"id": "id-0", "method": "replaced/method"}]
        });
        let reply = dispatch_server_request(&ctx, "client/registerCapability", params).await;
        assert!(matches!(reply, ServerRequestReply::Result(_)));
        let state = ctx.dynamic_registrations.read().await;
        assert_eq!(state.count(), MAX_REGISTRATIONS);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn register_capability_new_above_cap_fails() {
        let ctx = make_context();
        {
            let mut state = ctx.dynamic_registrations.write().await;
            for i in 0..MAX_REGISTRATIONS {
                state
                    .register(format!("id-{i}"), "test/method".to_string(), None)
                    .unwrap();
            }
        }
        // Adding a brand-new ID at the cap should fail, leaving state unchanged.
        let params = serde_json::json!({
            "registrations": [{"id": "brand-new", "method": "x"}]
        });
        let reply = dispatch_server_request(&ctx, "client/registerCapability", params).await;
        match reply {
            ServerRequestReply::Error { code, .. } => assert_eq!(code, -32600),
            _ => panic!("expected Error"),
        }
        let state = ctx.dynamic_registrations.read().await;
        assert_eq!(state.count(), MAX_REGISTRATIONS);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn register_capability_malformed_entry_rejects_all() {
        let ctx = make_context();
        let params = serde_json::json!({
            "registrations": [
                {"id": "good-1", "method": "textDocument/didOpen"},
                {"method": "textDocument/didClose"}
            ]
        });
        let reply = dispatch_server_request(&ctx, "client/registerCapability", params).await;
        match reply {
            ServerRequestReply::Error { code, message, .. } => {
                assert_eq!(code, -32602);
                assert!(message.contains("missing registration id"));
            }
            _ => panic!("expected Error for malformed entry"),
        }
        // Nothing should have been registered.
        let state = ctx.dynamic_registrations.read().await;
        assert_eq!(state.count(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn register_capability_duplicate_ids_last_write_wins() {
        let ctx = make_context();
        let params = serde_json::json!({
            "registrations": [
                {"id": "dup", "method": "first/method", "registerOptions": {"v": 1}},
                {"id": "dup", "method": "second/method", "registerOptions": {"v": 2}}
            ]
        });
        let reply = dispatch_server_request(&ctx, "client/registerCapability", params).await;
        assert!(matches!(reply, ServerRequestReply::Result(_)));
        let state = ctx.dynamic_registrations.read().await;
        assert_eq!(state.count(), 1);
        // The second entry should have won.
        let reg = state.get("dup").expect("registration exists");
        assert_eq!(reg.method, "second/method");
        assert_eq!(reg.register_options, Some(serde_json::json!({"v": 2})));
    }

    // ── Unregister capability tests ──────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn unregister_capability_removes_registration() {
        let ctx = make_context();
        {
            let mut state = ctx.dynamic_registrations.write().await;
            state
                .register("reg-1".into(), "test/method".into(), None)
                .unwrap();
        }
        let params = serde_json::json!({"id": "reg-1"});
        let reply = dispatch_server_request(&ctx, "client/unregisterCapability", params).await;
        assert!(matches!(reply, ServerRequestReply::Result(_)));
        let state = ctx.dynamic_registrations.read().await;
        assert_eq!(state.count(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unregister_unknown_id_succeeds() {
        let ctx = make_context();
        let params = serde_json::json!({"id": "nonexistent"});
        let reply = dispatch_server_request(&ctx, "client/unregisterCapability", params).await;
        assert!(matches!(reply, ServerRequestReply::Result(_)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unregister_missing_id_is_invalid() {
        let ctx = make_context();
        let params = serde_json::json!({});
        let reply = dispatch_server_request(&ctx, "client/unregisterCapability", params).await;
        match reply {
            ServerRequestReply::Error { code, .. } => assert_eq!(code, -32602),
            _ => panic!("expected Error"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unregister_capability_multiple_unregistrations() {
        let ctx = make_context();
        {
            let mut state = ctx.dynamic_registrations.write().await;
            state.register("r1".into(), "m1".into(), None).unwrap();
            state.register("r2".into(), "m2".into(), None).unwrap();
            state.register("r3".into(), "m3".into(), None).unwrap();
        }
        let params = serde_json::json!({
            "unregisterations": [
                {"id": "r1", "method": "m1"},
                {"id": "r3", "method": "m3"}
            ]
        });
        let reply = dispatch_server_request(&ctx, "client/unregisterCapability", params).await;
        assert!(matches!(reply, ServerRequestReply::Result(_)));
        let state = ctx.dynamic_registrations.read().await;
        assert_eq!(state.count(), 1);
        assert!(state.get("r2").is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unregister_capability_protocol_spelling() {
        let ctx = make_context();
        {
            let mut state = ctx.dynamic_registrations.write().await;
            state.register("r1".into(), "m1".into(), None).unwrap();
        }
        let params = serde_json::json!({
            "unregisterations": [{"id": "r1", "method": "m1"}]
        });
        let reply = dispatch_server_request(&ctx, "client/unregisterCapability", params).await;
        assert!(matches!(reply, ServerRequestReply::Result(_)));
        let state = ctx.dynamic_registrations.read().await;
        assert_eq!(state.count(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unregister_capability_compat_spelling() {
        let ctx = make_context();
        {
            let mut state = ctx.dynamic_registrations.write().await;
            state.register("r1".into(), "m1".into(), None).unwrap();
        }
        let params = serde_json::json!({
            "unregistrations": [{"id": "r1", "method": "m1"}]
        });
        let reply = dispatch_server_request(&ctx, "client/unregisterCapability", params).await;
        assert!(matches!(reply, ServerRequestReply::Result(_)));
        let state = ctx.dynamic_registrations.read().await;
        assert_eq!(state.count(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unregister_capability_unknown_ids_succeeds() {
        let ctx = make_context();
        let params = serde_json::json!({
            "unregisterations": [
                {"id": "unknown-1", "method": "m1"},
                {"id": "unknown-2", "method": "m2"}
            ]
        });
        let reply = dispatch_server_request(&ctx, "client/unregisterCapability", params).await;
        assert!(matches!(reply, ServerRequestReply::Result(_)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unregister_capability_empty_array_is_invalid() {
        let ctx = make_context();
        let params = serde_json::json!({
            "unregisterations": []
        });
        let reply = dispatch_server_request(&ctx, "client/unregisterCapability", params).await;
        match reply {
            ServerRequestReply::Error { code, .. } => assert_eq!(code, -32602),
            _ => panic!("expected Error for empty array"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unregister_capability_array_missing_id_is_invalid() {
        let ctx = make_context();
        let params = serde_json::json!({
            "unregisterations": [{"method": "m1"}]
        });
        let reply = dispatch_server_request(&ctx, "client/unregisterCapability", params).await;
        match reply {
            ServerRequestReply::Error { code, .. } => assert_eq!(code, -32602),
            _ => panic!("expected Error for missing id in array entry"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unregister_mixed_valid_and_malformed_rejects_all_atomically() {
        let ctx = make_context();
        {
            let mut state = ctx.dynamic_registrations.write().await;
            state.register("r1".into(), "m1".into(), None).unwrap();
            state.register("r2".into(), "m2".into(), None).unwrap();
        }
        // One valid entry followed by one malformed entry (missing id).
        let params = serde_json::json!({
            "unregisterations": [
                {"id": "r1", "method": "m1"},
                {"method": "m2"}
            ]
        });
        let reply = dispatch_server_request(&ctx, "client/unregisterCapability", params).await;
        match reply {
            ServerRequestReply::Error { code, message, .. } => {
                assert_eq!(code, -32602);
                assert!(
                    message.contains("missing"),
                    "error should mention missing id"
                );
            }
            _ => panic!("expected Error for malformed entry"),
        }
        // Nothing should have been unregistered — atomic rejection.
        let state = ctx.dynamic_registrations.read().await;
        assert_eq!(state.count(), 2, "both registrations should remain");
        assert!(state.get("r1").is_some());
        assert!(state.get("r2").is_some());
    }

    // ── WorkDoneProgress/create test ─────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn work_done_progress_create_returns_null() {
        let ctx = make_context();
        let params = serde_json::json!({"token": "progress-1"});
        let reply = dispatch_server_request(&ctx, "window/workDoneProgress/create", params).await;
        match reply {
            ServerRequestReply::Result(val) => assert!(val.is_null()),
            _ => panic!("expected Result(null)"),
        }
    }

    // ── ApplyEdit test ──────────────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn apply_edit_always_rejected() {
        let ctx = make_context();
        let params = serde_json::json!({
            "edit": {
                "documentChanges": []
            }
        });
        let reply = dispatch_server_request(&ctx, "workspace/applyEdit", params).await;
        match reply {
            ServerRequestReply::Result(val) => {
                assert_eq!(val["applied"], false);
                let reason = val["failureReason"]
                    .as_str()
                    .expect("failureReason should be a string");
                assert!(!reason.is_empty(), "failureReason should be non-empty");
            }
            ServerRequestReply::Error { .. } => panic!("expected Result, not Error"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn apply_edit_with_changes_refused_as_result() {
        let ctx = make_context();
        let params = serde_json::json!({
            "edit": {
                "changes": {
                    "file:///src/main.rs": [
                        {
                            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 5 } },
                            "newText": "hello"
                        }
                    ]
                }
            }
        });
        let reply = dispatch_server_request(&ctx, "workspace/applyEdit", params).await;
        match reply {
            ServerRequestReply::Result(val) => {
                assert_eq!(val["applied"], false);
                let reason = val["failureReason"]
                    .as_str()
                    .expect("failureReason should be a string");
                assert!(!reason.is_empty());
            }
            ServerRequestReply::Error { .. } => panic!("expected Result, not Error"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn apply_edit_with_document_changes_refused_as_result() {
        let ctx = make_context();
        let params = serde_json::json!({
            "edit": {
                "documentChanges": [
                    {
                        "textDocument": { "uri": "file:///src/main.rs", "version": 1 },
                        "edits": [
                            {
                                "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 5 } },
                                "newText": "hello"
                            }
                        ]
                    }
                ]
            }
        });
        let reply = dispatch_server_request(&ctx, "workspace/applyEdit", params).await;
        match reply {
            ServerRequestReply::Result(val) => {
                assert_eq!(val["applied"], false);
                let reason = val["failureReason"]
                    .as_str()
                    .expect("failureReason should be a string");
                assert!(!reason.is_empty());
            }
            ServerRequestReply::Error { .. } => panic!("expected Result, not Error"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn apply_edit_with_both_changes_and_document_changes() {
        let ctx = make_context();
        let params = serde_json::json!({
            "edit": {
                "changes": {
                    "file:///src/main.rs": [
                        {
                            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 5 } },
                            "newText": "hello"
                        }
                    ]
                },
                "documentChanges": [
                    {
                        "textDocument": { "uri": "file:///src/lib.rs", "version": 1 },
                        "edits": [
                            {
                                "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 5 } },
                                "newText": "world"
                            }
                        ]
                    }
                ]
            }
        });
        let reply = dispatch_server_request(&ctx, "workspace/applyEdit", params).await;
        match reply {
            ServerRequestReply::Result(val) => {
                assert_eq!(val["applied"], false);
                let reason = val["failureReason"]
                    .as_str()
                    .expect("failureReason should be a string");
                assert!(!reason.is_empty());
            }
            ServerRequestReply::Error { .. } => panic!("expected Result, not Error"),
        }
    }

    // ── Unknown method test ──────────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn unknown_method_returns_not_found() {
        let ctx = make_context();
        let reply = dispatch_server_request(&ctx, "unknown/method", serde_json::json!({})).await;
        match reply {
            ServerRequestReply::Error {
                code,
                message,
                data,
            } => {
                assert_eq!(code, -32601);
                assert_eq!(message, "Method not found");
                assert!(data.is_none());
            }
            _ => panic!("expected Error"),
        }
    }

    // ── DynamicRegistrationState tests ───────────────────────────────

    #[test]
    fn empty_dynamic_registrations_state() {
        let state = DynamicRegistrationState::new();
        assert_eq!(state.count(), 0);
    }

    #[test]
    fn register_then_unregister() {
        let mut state = DynamicRegistrationState::new();
        state
            .register("r1".into(), "test/method".into(), None)
            .unwrap();
        assert_eq!(state.count(), 1);
        state.unregister("r1");
        assert_eq!(state.count(), 0);
    }

    #[test]
    fn multiple_registrations_tracked() {
        let mut state = DynamicRegistrationState::new();
        state.register("r1".into(), "m1".into(), None).unwrap();
        state
            .register("r2".into(), "m2".into(), Some(serde_json::json!({"x": 1})))
            .unwrap();
        state.register("r3".into(), "m3".into(), None).unwrap();
        assert_eq!(state.count(), 3);
        state.unregister("r2");
        assert_eq!(state.count(), 2);
    }

    #[test]
    fn clear_removes_all() {
        let mut state = DynamicRegistrationState::new();
        state.register("r1".into(), "m1".into(), None).unwrap();
        state.register("r2".into(), "m2".into(), None).unwrap();
        state.clear();
        assert_eq!(state.count(), 0);
    }

    #[test]
    fn register_at_exact_limit() {
        let mut state = DynamicRegistrationState::new();
        for i in 0..MAX_REGISTRATIONS {
            state
                .register(format!("id-{i}"), "test/m".into(), None)
                .unwrap();
        }
        assert_eq!(state.count(), MAX_REGISTRATIONS);
        assert!(state
            .register("overflow".into(), "test/m".into(), None)
            .is_err());
    }

    // ── Dispatch timeout tests ──────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn dispatch_completes_within_timeout() {
        // All current handlers are fast and local — verify they complete
        // well within the 5-second SERVER_REQUEST_TIMEOUT.
        let ctx = make_context();
        let params = serde_json::json!({"items": [{"section": "test"}]});
        let start = std::time::Instant::now();
        let reply = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            dispatch_server_request(&ctx, "workspace/configuration", params),
        )
        .await;
        let elapsed = start.elapsed();
        assert!(reply.is_ok(), "dispatch should complete within timeout");
        assert!(
            elapsed < std::time::Duration::from_secs(1),
            "dispatch took {:?}, expected < 1s",
            elapsed
        );
    }

    #[test]
    fn server_request_timeout_is_reasonable() {
        // The SERVER_REQUEST_TIMEOUT constant in client.rs should be generous
        // enough for fast local handlers but short enough to prevent stalling
        // stdout consumption.
        let timeout = crate::client::LspClient::SERVER_REQUEST_TIMEOUT;
        assert!(
            timeout >= std::time::Duration::from_secs(2),
            "timeout should be at least 2s, got {:?}",
            timeout
        );
        assert!(
            timeout <= std::time::Duration::from_secs(30),
            "timeout should be at most 30s, got {:?}",
            timeout
        );
    }

    #[test]
    fn register_batch_rejects_over_capacity() {
        let mut state = DynamicRegistrationState::new();
        for i in 0..MAX_REGISTRATIONS {
            state
                .register(format!("id-{i}"), "test/m".into(), None)
                .unwrap();
        }
        // 256 existing + 2 new = over cap
        let result = state.register_batch(vec![
            ("new-1".into(), "m1".into(), None),
            ("new-2".into(), "m2".into(), None),
        ]);
        assert!(result.is_err());
        assert_eq!(state.count(), MAX_REGISTRATIONS);
    }

    #[test]
    fn register_batch_replacement_at_cap_succeeds() {
        let mut state = DynamicRegistrationState::new();
        for i in 0..MAX_REGISTRATIONS {
            state
                .register(format!("id-{i}"), "test/m".into(), None)
                .unwrap();
        }
        let result = state.register_batch(vec![("id-0".into(), "replaced".into(), None)]);
        assert!(result.is_ok());
        assert_eq!(state.count(), MAX_REGISTRATIONS);
    }

    #[test]
    fn register_batch_mixed_replacement_and_new_at_cap() {
        let mut state = DynamicRegistrationState::new();
        // Fill to 255 (one below cap) so replacement + one new = 256, at cap.
        for i in 0..MAX_REGISTRATIONS - 1 {
            state
                .register(format!("id-{i}"), "test/m".into(), None)
                .unwrap();
        }
        let result = state.register_batch(vec![
            ("id-0".into(), "replaced".into(), None),
            ("brand-new".into(), "m".into(), None),
        ]);
        assert!(result.is_ok());
        assert_eq!(state.count(), MAX_REGISTRATIONS);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn configuration_uses_context_configuration_field() {
        let uri: lsp_types::Uri = "file:///workspace".parse().expect("valid URI");
        // Simulate a context where configuration comes from workspace_configuration
        // (which takes precedence over initialization in service.rs).
        let mut config = serde_json::Map::new();
        config.insert(
            "my-server".to_string(),
            serde_json::json!({"customSetting": "from-ws-config"}),
        );
        let ctx = ServerRequestContext {
            server_id: "my-server".to_string(),
            root: PathBuf::from("/workspace"),
            configuration: serde_json::Value::Object(config),
            workspace_folders: vec![lsp_types::WorkspaceFolder {
                uri,
                name: "workspace".to_string(),
            }],
            dynamic_registrations: Arc::new(RwLock::new(DynamicRegistrationState::new())),
        };
        let params = serde_json::json!({
            "items": [{"section": "my-server"}]
        });
        let reply = dispatch_server_request(&ctx, "workspace/configuration", params).await;
        match reply {
            ServerRequestReply::Result(val) => {
                let arr = val.as_array().unwrap();
                assert_eq!(arr[0]["customSetting"], "from-ws-config");
            }
            _ => panic!("expected Result"),
        }
    }
}
