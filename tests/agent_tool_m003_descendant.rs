//! M003 descendant tool authority ceilings: parent-to-child-to-broker trace.
//!
//! Proves the same host-attested effective ceiling at model discovery,
//! provider definitions, cache identity, and broker invocation:
//! exact-name and capability denies enforce independently, forged calls
//! fail before execution, nested delegation only narrows, durable
//! payloads round-trip (legacy absents stay absent), and the model-driven
//! `task` path fails closed without a snapshot.

mod common;

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use codegg::agent::parent_authority::{
        decode_parent_authority, encode_parent_authority, ParentAuthorityMintInput,
        ParentDenyReason, ParentToolAuthoritySnapshot,
    };
    use codegg::agent::tool_surface::{ResolvedToolSurface, ToolOmissionReason};
    use codegg::agent::worker::{SubAgentPool, SubAgentRequest};
    use codegg::agent::{Agent, AgentMode};
    use codegg::config::schema::Config;
    use codegg::provider::{
        ChatEvent, ChatRequest, EventStream, ModelInfo, Provider, ProviderError, ProviderRegistry,
        TokenUsage, ToolDefinition,
    };
    use codegg::tool::task::{TaskStatus, TaskStore, TaskTool};
    use codegg::tool::{BrokerAuthority, BrokerInvocationContext, ToolCaller, ToolContract};
    use codegg::tool::{Tool, ToolRegistry};
    use std::collections::{BTreeMap, BTreeSet, HashMap};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn def(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.into(),
            description: name.into(),
            parameters: serde_json::json!({"type": "object"}),
            defer_loading: None,
        }
    }

    fn parent_surface_for(names: &[&str], denied: &[&str]) -> ResolvedToolSurface {
        let denied_set: BTreeSet<String> = denied.iter().map(|s| s.to_string()).collect();
        ResolvedToolSurface::resolve(
            names.iter().map(|n| def(n)),
            &denied_set,
            &BTreeSet::new(),
            false,
            true,
            None,
        )
        .expect("parent surface resolves")
    }

    fn mint_for(
        surface: ResolvedToolSurface,
        session: &str,
        workspace: &str,
    ) -> ParentToolAuthoritySnapshot {
        mint_for_with_generation(surface, session, workspace, "mcp-gen-1")
    }

    fn mint_for_with_generation(
        surface: ResolvedToolSurface,
        session: &str,
        workspace: &str,
        mcp_generation: &str,
    ) -> ParentToolAuthoritySnapshot {
        let eligible_effects = surface
            .tools
            .iter()
            .map(|tool| (tool.canonical_name.clone(), "non_idempotent".to_string()))
            .collect();
        ParentToolAuthoritySnapshot::mint(ParentAuthorityMintInput {
            parent_session_id: session.into(),
            parent_turn_id: Some("turn-1".into()),
            parent_run_id: None,
            parent_agent: "build".into(),
            workspace_scope: workspace.into(),
            surface,
            mcp_generation: mcp_generation.into(),
            plugin_activation_rev: String::new(),
            policy_revision: "config:1".into(),
            max_effect_class: "non_idempotent".into(),
            eligible_effects,
            now_ms: chrono::Utc::now().timestamp_millis(),
        })
    }

    fn child_resolve(
        names: &[&str],
        snapshot: &ParentToolAuthoritySnapshot,
    ) -> ResolvedToolSurface {
        ResolvedToolSurface::resolve_with_parent_authority(
            names.iter().map(|n| def(n)),
            &BTreeSet::new(),
            &BTreeSet::new(),
            false,
            true,
            &BTreeMap::new(),
            &BTreeMap::new(),
            snapshot,
        )
        .expect("child surface resolves")
    }

    // --- surface: exact names ------------------------------------------------

    #[test]
    fn exact_name_denies_despite_shared_capability() {
        // Parent sees `read` but is denied `git_query` (both read-side).
        let parent = parent_surface_for(&["read", "git_query"], &["git_query"]);
        assert!(parent.tools.iter().any(|t| t.canonical_name == "read"));
        assert!(!parent.tools.iter().any(|t| t.canonical_name == "git_query"));
        let snapshot = mint_for(parent, "s-1", "/w");

        // The child cannot discover or invoke `git_query` even though it
        // shares read-side capability bits with `read`.
        let child = child_resolve(&["read", "git_query", "write"], &snapshot);
        let names: Vec<_> = child
            .tools
            .iter()
            .map(|t| t.canonical_name.as_str())
            .collect();
        assert!(names.contains(&"read"));
        assert!(!names.contains(&"git_query"));
        assert!(child
            .omissions
            .iter()
            .any(|o| o.canonical_name == "git_query"
                && o.reason == ToolOmissionReason::ParentCeiling));

        // Invocation-time agreement: the same snapshot denies by name.
        let now = chrono::Utc::now().timestamp_millis();
        assert!(snapshot.check_tool("read", "read_only", now, "/w").is_ok());
        assert_eq!(
            snapshot.check_tool("git_query", "read_only", now, "/w"),
            Err(ParentDenyReason::ParentCeiling)
        );
    }

    #[test]
    fn plugin_exact_name_denies_without_capability_amplification() {
        // `PluginExecute` is shared; the exact denied name still loses.
        let parent = parent_surface_for(
            &["plugin__one__read", "plugin__one__write"],
            &["plugin__one__write"],
        );
        let snapshot = mint_for(parent, "s-1", "/w");
        let child = child_resolve(&["plugin__one__read", "plugin__one__write"], &snapshot);
        let names: Vec<_> = child
            .tools
            .iter()
            .map(|t| t.canonical_name.as_str())
            .collect();
        assert!(names.contains(&"plugin__one__read"));
        assert!(!names.contains(&"plugin__one__write"));
    }

    #[test]
    fn mcp_server_tool_scoping_holds_after_reconnect_identities() {
        // Parent saw one MCP tool; a sibling tool appearing later
        // (reconnect / list expansion) is denied by exact name.
        let parent = parent_surface_for(&["mcp__db__query"], &[]);
        let snapshot = mint_for(parent, "s-1", "/w");
        let child = child_resolve(&["mcp__db__query", "mcp__db__admin"], &snapshot);
        let names: Vec<_> = child
            .tools
            .iter()
            .map(|t| t.canonical_name.as_str())
            .collect();
        assert!(names.contains(&"mcp__db__query"));
        assert!(!names.contains(&"mcp__db__admin"));
    }

    // --- surface: capability independence ------------------------------------

    #[test]
    fn capability_and_name_denies_are_independent() {
        // Caps allow FsWrite but the name is absent → name deny.
        let full = mint_for(parent_surface_for(&["read", "write"], &[]), "s-1", "/w");
        let mut name_narrowed = full.clone();
        name_narrowed.eligible_tools.remove("write");
        let child = child_resolve(&["read", "write"], &name_narrowed);
        assert!(!child.tools.iter().any(|t| t.canonical_name == "write"));

        // Name present but caps lack FsWrite → capability deny.
        let read_only_parent = parent_surface_for(&["read", "write"], &["write"]);
        let mut cap_narrowed = mint_for(read_only_parent, "s-1", "/w");
        cap_narrowed.eligible_tools.insert("write".into());
        let child = child_resolve(&["read", "write"], &cap_narrowed);
        assert!(!child.tools.iter().any(|t| t.canonical_name == "write"));
        assert!(child
            .omissions
            .iter()
            .any(|o| o.canonical_name == "write" && o.reason == ToolOmissionReason::ParentCeiling));
    }

    #[test]
    fn every_capability_family_denies_without_parent_bit() {
        // Parent with read-only caps denies one representative per
        // mutating family at both discovery and invocation layers.
        let parent = parent_surface_for(&["read"], &[]);
        let snapshot = mint_for(parent, "s-1", "/w");
        let now = chrono::Utc::now().timestamp_millis();
        for tool in [
            "write",
            "terminal",
            "websearch",
            "image",
            "memory_search",
            "goal_update_progress",
            "work_order",
            "git_query",
            "plugin__x__run",
            "mcp__srv__tool",
        ] {
            let child = child_resolve(&["read", tool], &snapshot);
            assert!(
                !child.tools.iter().any(|t| t.canonical_name == tool),
                "{tool} must be absent from the child surface"
            );
            assert_eq!(
                snapshot.check_tool(tool, "non_idempotent", now, "/w"),
                Err(ParentDenyReason::ParentCeiling),
                "{tool} must be denied at invocation"
            );
        }
        // `tool_search` is absent too when the parent never had it:
        // there is no default-allow for discovery itself.
        let child = child_resolve(&["read", "tool_search"], &snapshot);
        assert!(!child
            .tools
            .iter()
            .any(|t| t.canonical_name == "tool_search"));
    }

    // --- surface: aliases, fingerprint, nesting ------------------------------

    #[test]
    fn alias_resurrection_and_collisions_fail_closed() {
        let parent = parent_surface_for(&["read"], &[]);
        let snapshot = mint_for(parent, "s-1", "/w");
        // A wire alias pointing at a denied canonical name stays denied:
        // canonicalization precedes membership.
        let aliases = BTreeMap::from([("gh__query".to_string(), "git_query".to_string())]);
        let surface = ResolvedToolSurface::resolve_with_parent_authority(
            [def("read"), def("gh__query")],
            &BTreeSet::new(),
            &BTreeSet::new(),
            false,
            true,
            &aliases,
            &BTreeMap::new(),
            &snapshot,
        )
        .unwrap();
        assert!(!surface
            .tools
            .iter()
            .any(|t| t.canonical_name == "git_query"));

        // Alias collisions still fail the whole resolution under a ceiling.
        let colliding = BTreeMap::from([("read_v2".to_string(), "read".to_string())]);
        let err = ResolvedToolSurface::resolve_with_parent_authority(
            [def("read"), def("read_v2")],
            &BTreeSet::new(),
            &BTreeSet::new(),
            false,
            true,
            &colliding,
            &BTreeMap::new(),
            &snapshot,
        );
        // Both names are ceiling-eligible? No — `read_v2` canonicalizes to
        // `read` which IS eligible, so the collision error must fire first.
        assert!(err.is_err());

        // Nested narrowing cannot resurrect through aliases either.
        let child_surface = child_resolve(&["read"], &snapshot);
        let narrowed = snapshot.narrow_for_child(&child_surface);
        let grandchild = child_resolve(&["read", "gh__query"], &narrowed);
        assert!(!grandchild
            .tools
            .iter()
            .any(|t| t.canonical_name == "git_query"));
    }

    #[test]
    fn child_fingerprint_binds_parent_snapshot() {
        let a = mint_for(parent_surface_for(&["read", "write"], &[]), "s-1", "/w");
        let b = mint_for(parent_surface_for(&["read"], &[]), "s-1", "/w");
        let fa = child_resolve(&["read"], &a).fingerprint;
        let fb = child_resolve(&["read"], &b).fingerprint;
        assert_ne!(fa, fb, "parent ceiling must enter cache identity");
        let fa2 = child_resolve(&["read"], &a).fingerprint;
        assert_eq!(fa, fa2);
    }

    // --- forgery matrix --------------------------------------------------------

    #[test]
    fn forged_stale_and_cross_scope_snapshots_fail_closed() {
        let now = chrono::Utc::now().timestamp_millis();
        let snapshot = mint_for(parent_surface_for(&["read"], &[]), "s-1", "/w");

        // Forged eligible set (fingerprint kept) fails integrity.
        let mut forged = snapshot.clone();
        forged.eligible_tools.insert("task".into());
        assert!(!forged.verify_integrity());
        assert_eq!(
            forged.validate(now, "/w"),
            Err(ParentDenyReason::InvalidParentReceipt)
        );

        // Stale fingerprint field fails integrity.
        let mut stale_fp = snapshot.clone();
        stale_fp.snapshot_fingerprint = "sha256:0".into();
        assert!(!stale_fp.verify_integrity());

        // Workspace mismatch fails.
        assert_eq!(
            snapshot.validate(now, "/other"),
            Err(ParentDenyReason::InvalidParentReceipt)
        );

        // Expiry fails.
        assert_eq!(
            snapshot.validate(snapshot.expires_at_ms + 1, "/w"),
            Err(ParentDenyReason::InvalidParentReceipt)
        );

        // Contract effect widening fails even for an eligible name.
        let read_only_ceiling = ParentToolAuthoritySnapshot::mint(ParentAuthorityMintInput {
            parent_session_id: "s-1".into(),
            parent_turn_id: Some("turn-1".into()),
            parent_run_id: None,
            parent_agent: "build".into(),
            workspace_scope: "/w".into(),
            surface: parent_surface_for(&["read"], &[]),
            mcp_generation: "mcp-gen-1".into(),
            plugin_activation_rev: String::new(),
            policy_revision: "config:1".into(),
            max_effect_class: "read_only".into(),
            eligible_effects: BTreeMap::from([("read".into(), "read_only".into())]),
            now_ms: now,
        });
        assert_eq!(
            read_only_ceiling.check_tool("read", "non_idempotent", now, "/w"),
            Err(ParentDenyReason::ForbiddenEffect)
        );
        assert!(read_only_ceiling
            .check_tool("read", "read_only", now, "/w")
            .is_ok());
    }

    // --- durable transport -----------------------------------------------------

    #[test]
    fn payload_authority_roundtrips_and_legacy_absent_stays_absent() {
        let snapshot = mint_for(parent_surface_for(&["read"], &[]), "s-1", "/w");
        let encoded = encode_parent_authority(&snapshot).unwrap();
        let decoded = decode_parent_authority(&encoded).unwrap();
        assert_eq!(decoded, snapshot);
        assert_eq!(
            decode_parent_authority("{corrupt"),
            Err(ParentDenyReason::InvalidParentReceipt)
        );

        // Additive scheduler field round-trips on both variants.
        let run = codegg_core::jobs::JobPayload::SubagentRun {
            prompt: "p".into(),
            agent: "a".into(),
            model: None,
            parent_id: None,
            denied_tools: vec![],
            allowed_paths: vec![],
            max_tool_calls: None,
            task_id: codegg_core::identity::AgentTaskId::new(),
            run_id: codegg_core::identity::AgentRunId::new(),
            delegation_key: "dk".into(),
            base_commit: None,
            parent_authority: Some(encoded.clone()),
        };
        let json = serde_json::to_value(&run).unwrap();
        assert_eq!(
            json.get("parent_authority").and_then(|v| v.as_str()),
            Some(encoded.as_str())
        );
        let back: codegg_core::jobs::JobPayload = serde_json::from_value(json).unwrap();
        assert!(matches!(
            back,
            codegg_core::jobs::JobPayload::SubagentRun { .. }
        ));

        // Legacy payloads without the field decode as None (system
        // delegation), never as a silent ceiling.
        let legacy = serde_json::json!({
            "kind": "subagent",
            "prompt": "p", "agent": "a", "model": null,
            "parent_id": null, "denied_tools": [], "allowed_paths": [],
            "max_tool_calls": null,
        });
        let back: codegg_core::jobs::JobPayload = serde_json::from_value(legacy).unwrap();
        match back {
            codegg_core::jobs::JobPayload::Subagent {
                parent_authority, ..
            } => assert_eq!(parent_authority, None),
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    // --- broker manifest ---------------------------------------------------------

    fn manifest_ctx(allowed: &[&str]) -> (BrokerInvocationContext, ToolContract) {
        let now = chrono::Utc::now().timestamp_millis();
        let grant = {
            let g = codegg_core::jobs::ToolAuthorityGrant {
                schema_version: 1,
                grant_id: "m003-grant".into(),
                principal_ref: "test".into(),
                workspace_id: "ws-test".into(),
                workspace_path_policy_id: "workspace:ws-test".into(),
                session_id: None,
                agent_id: None,
                turn_id: None,
                permission_mode: None,
                policy_revision: "test-v1".into(),
                allowed_caller_class: "agent".into(),
                allowed_effect_class: "non_idempotent".into(),
                manifest_digest: "sha256:test".into(),
                source_digest: String::new(),
                ir_digest: String::new(),
                contract_digest: String::new(),
                contract_snapshot_json: String::new(),
                issued_at: now,
                expires_at: None,
                revoked_at: None,
                decision_digest: String::new(),
            };
            let decision_digest = g.compute_digest();
            codegg_core::jobs::ToolAuthorityGrant {
                decision_digest,
                ..g
            }
        };
        let mut ctx = BrokerInvocationContext {
            caller: ToolCaller::Agent,
            cwd: std::path::PathBuf::from("/w"),
            session_id: None,
            workspace_id: Some("ws-test".into()),
            agent_id: None,
            turn_id: None,
            job_id: None,
            attempt_id: None,
            permission_mode: None,
            timeout_ms: None,
            submission_key: None,
            authority: BrokerAuthority::from_grant(grant),
            cancellation: None,
            deadline: None,
            principal_ref: Some("test".into()),
            workspace_path_policy_id: Some("workspace:ws-test".into()),
            allowed_tools: Some(allowed.iter().map(|s| s.to_string()).collect()),
            // M003: broker manifest under test; no delegation snapshot needed.
            parent_tool_authority: None,
            current_policy_revision: Some("test-v1".into()),
            execution_audit: None,
            audit_emitter: None,
        };
        let _ = &mut ctx;
        let contract = ToolContract::legacy("probe", serde_json::json!({"type": "object"}));
        (ctx, contract)
    }

    #[test]
    fn broker_manifest_denies_outside_ceiling_before_execution() {
        // The child broker binds the parent eligible set: `read` passes,
        // a same-capability outsider fails before any side effect.
        let (ctx, contract) = manifest_ctx(&["read"]);
        assert!(ctx
            .authority
            .verify_grant_scope("read", &contract, &ctx)
            .is_ok());
        let err = ctx
            .authority
            .verify_grant_scope("git_query", &contract, &ctx)
            .expect_err("outside-ceiling tool must fail");
        assert!(err.contains("manifest violation"), "got: {err}");
    }

    // --- task launch fail-closed ---------------------------------------------------

    #[tokio::test]
    async fn task_launch_without_authority_fails_closed() {
        let pool = crate::common::pool::isolated_pool().await;
        let registry = ProviderRegistry::new();
        let session_store = Arc::new(codegg::session::SessionStore::new(pool.clone()));
        let subagent_pool = SubAgentPool::new(
            &Config::default(),
            vec![],
            registry,
            session_store,
            Some(pool),
        )
        .await;
        let store = Arc::new(Mutex::new(TaskStore::new()));
        let tool = TaskTool::new(
            store,
            Some(subagent_pool.spawner()),
            Some("sess-m003".to_string()),
            vec![],
        );
        let input = serde_json::json!({
            "action": "spawn",
            "description": "no ceiling",
            "prompt": "do work",
            "agent": "general",
        });
        // No ctx ceiling, no registration ceiling → fail closed.
        let err = format!(
            "{:?}",
            tool.execute(input.clone()).await.expect_err("must fail")
        );
        assert!(err.contains("parent tool authority"), "got: {err}");

        // A host-minted ctx ceiling lets the same delegation launch.
        let snapshot = mint_for(parent_surface_for(&["read"], &[]), "sess-m003", "/w");
        let mut exec =
            codegg::tool::ToolExecutionContext::with_backend(codegg::tool::ToolBackendKind::Native);
        exec.session_id = Some("sess-m003".to_string());
        exec.parent_tool_authority = Some(snapshot);
        let out = tool
            .execute_structured(input, Some(exec))
            .await
            .expect("host ceiling launches");
        assert!(out.success);
        assert!(
            out.output.contains("Subagent spawned"),
            "got: {}",
            out.output
        );
    }

    // --- worker end to end -----------------------------------------------------------

    #[derive(Clone)]
    struct ScriptedProvider {
        responses: Vec<Vec<ChatEvent>>,
        requests: Arc<Mutex<Vec<ChatRequest>>>,
        index: Arc<Mutex<usize>>,
        id: String,
    }

    #[async_trait]
    impl Provider for ScriptedProvider {
        fn id(&self) -> &str {
            &self.id
        }
        fn name(&self) -> &str {
            "M003 scripted provider"
        }
        fn clone_box(&self) -> Box<dyn Provider> {
            Box::new(self.clone())
        }
        async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError> {
            self.requests.lock().await.push(request.clone());
            let mut idx = self.index.lock().await;
            let events = if *idx < self.responses.len() {
                self.responses[*idx].clone()
            } else {
                vec![ChatEvent::Finish {
                    stop_reason: "stop".to_string().into(),
                    usage: TokenUsage::default(),
                }]
            };
            *idx += 1;
            Ok(Box::pin(futures_util::stream::iter(
                events.into_iter().map(Ok::<_, ProviderError>),
            )))
        }
        async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(vec![ModelInfo {
                id: format!("{}/model", self.id),
                name: "Test Model".to_string(),
                provider: self.id.clone(),
                context_window: 4096,
                max_output_tokens: Some(2048),
                supports_tools: true,
                supports_vision: false,
                variants: vec![],
            }])
        }
    }

    fn test_agent(name: &str, task_allow: bool) -> Agent {
        let mut permissions = HashMap::new();
        if task_allow {
            permissions.insert("task".to_string(), "allow".to_string());
        }
        Agent {
            name: name.to_string(),
            role: None,
            description: format!("M003 {name}"),
            mode: AgentMode::Primary,
            mode_name: None,
            model: Some("test/model".to_string()),
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: None,
            permissions,
            hidden: false,
            thinking_budget: None,
            reasoning_effort: None,
            fallback_model: None,
            runtime_kind: None,
        }
    }

    async fn wait_completed(
        store: &Arc<Mutex<TaskStore>>,
        task_id: u64,
    ) -> codegg::tool::task::SubAgentTask {
        for _ in 0..4_000 {
            if let Some(task) = store.lock().await.get_task(task_id).await {
                match task.status {
                    TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Interrupted => {
                        return task
                    }
                    _ => {}
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let last = store.lock().await.get_task(task_id).await;
        panic!("task {task_id} did not finish; last state: {last:?}");
    }

    fn tool_names(request: &ChatRequest) -> Vec<String> {
        request
            .tools
            .clone()
            .unwrap_or_default()
            .iter()
            .map(|d| d.name.clone())
            .collect()
    }

    /// Spawn one child under `snapshot`. An empty `denied` set leaves the
    /// ceiling as the only narrowing force. Returns provider requests
    /// plus the finished child task.
    async fn spawn_under_ceiling(
        snapshot: ParentToolAuthoritySnapshot,
        agent: Agent,
        responses: Vec<Vec<ChatEvent>>,
        workspace: std::path::PathBuf,
        denied: Vec<String>,
    ) -> (
        Vec<ChatRequest>,
        codegg::tool::task::SubAgentTask,
        Arc<Mutex<Vec<ChatRequest>>>,
        Arc<Mutex<TaskStore>>,
    ) {
        let pool = crate::common::pool::isolated_pool().await;
        let provider = ScriptedProvider {
            responses,
            requests: Arc::new(Mutex::new(Vec::new())),
            index: Arc::new(Mutex::new(0)),
            id: "test".to_string(),
        };
        let requests = provider.requests.clone();
        let mut registry = ProviderRegistry::new();
        registry.register(provider);
        let session_store = Arc::new(codegg::session::SessionStore::new(pool.clone()));
        let subagent_pool = SubAgentPool::new(
            &Config::default(),
            vec![agent.clone()],
            registry,
            session_store,
            Some(pool),
        )
        .await;
        let task_store = subagent_pool.task_store();
        let created = task_store
            .lock()
            .await
            .create_task(
                "m003".to_string(),
                "ceiling probe".to_string(),
                agent.name.clone(),
                Some(snapshot.parent_session_id.clone()),
                vec![],
                vec![workspace.to_string_lossy().into_owned()],
            )
            .await;
        let request = SubAgentRequest {
            task_id: created,
            run_id: None,
            prompt: "probe the ceiling".to_string(),
            agent: agent.name.clone(),
            parent_id: Some(snapshot.parent_session_id.clone()),
            parent_run_id: None,
            denied_tools: denied,
            allowed_paths: vec![workspace.to_string_lossy().into_owned()],
            description: "m003 probe".to_string(),
            depth: 1,
            max_tool_calls: None,
            parent_model: None,
            workspace_root: Some(workspace),
            workspace_locks: None,
            parent_sandbox_profile: None,
            sandbox_profile: None,
            parent_authority: Some(snapshot),
        };
        subagent_pool
            .spawner()
            .send_async(request)
            .await
            .expect("spawn");
        let task = match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            wait_completed(&task_store, created),
        )
        .await
        {
            Ok(task) => task,
            Err(_) => {
                let requests = requests.lock().await;
                panic!(
                    "task {created} did not finish; provider requests: {:?}",
                    requests.iter().map(tool_names).collect::<Vec<_>>()
                );
            }
        };
        let reqs = requests.lock().await.clone();
        (reqs, task, requests, task_store)
    }

    fn text_finish(text: &str) -> Vec<ChatEvent> {
        vec![
            ChatEvent::TextDelta(text.to_string().into()),
            ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            },
        ]
    }

    fn nested_spawn_call(workspace: &std::path::Path) -> Vec<ChatEvent> {
        vec![
            ChatEvent::ToolCall(codegg::provider::ToolCall {
                id: Arc::new("nested-task-1".into()),
                name: Arc::new("task".into()),
                arguments: serde_json::json!({
                    "action": "spawn",
                    "description": "M003 nested child",
                    "prompt": "complete the nested probe",
                    "agent": "test-agent",
                    "allowed_paths": [workspace.to_string_lossy()],
                }),
            }),
            ChatEvent::Finish {
                stop_reason: "tool_calls".to_string().into(),
                usage: TokenUsage::default(),
            },
        ]
    }

    #[tokio::test]
    async fn worker_child_provider_surface_honors_ceiling() {
        let workspace = std::env::temp_dir().join(format!("m003-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&workspace);
        // Parent surface denies `write`; the snapshot is the only ceiling.
        let parent = ResolvedToolSurface::from_registry(
            &ToolRegistry::with_defaults(),
            &BTreeSet::from(["write".to_string()]),
            &BTreeSet::new(),
            false,
            None,
        )
        .unwrap();
        assert!(!parent.tools.iter().any(|t| t.canonical_name == "write"));
        // Empty pinned generation: the test pool carries a live default
        // MCP service the test never pinned; exact-name filtering is the
        // enforcement under test, and staleness has its own matrix test.
        let snapshot =
            mint_for_with_generation(parent, "sess-m003-e2e", &workspace.to_string_lossy(), "");
        let (reqs, task, _, _) = spawn_under_ceiling(
            snapshot,
            test_agent("test-agent", false),
            vec![text_finish("done")],
            workspace,
            // Deliberately empty: the snapshot is the only ceiling.
            vec![],
        )
        .await;
        assert_eq!(
            task.status,
            TaskStatus::Completed,
            "child result: {:?}",
            task.result
        );
        assert!(
            !reqs.is_empty(),
            "child must have issued a provider request"
        );
        let names = tool_names(&reqs[0]);
        assert!(!names.contains(&"write".to_string()), "tools: {names:?}");
        assert!(names.contains(&"read".to_string()), "tools: {names:?}");
    }

    #[tokio::test]
    async fn worker_child_withholds_nested_task() {
        let workspace = std::env::temp_dir().join(format!("m003-nest-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&workspace);
        // Snapshot without `task`, but the agent definition allows it:
        // the ceiling (not the role) withholds nested delegation.
        let parent = ResolvedToolSurface::from_registry(
            &ToolRegistry::with_defaults(),
            &BTreeSet::from(["task".to_string()]),
            &BTreeSet::new(),
            false,
            None,
        )
        .unwrap();
        let snapshot =
            mint_for_with_generation(parent, "sess-m003-nest", &workspace.to_string_lossy(), "");
        assert!(!snapshot.eligible_tools.contains("task"));
        let (reqs, task, _, _) = spawn_under_ceiling(
            snapshot,
            test_agent("test-agent", true),
            vec![text_finish("no delegation needed")],
            workspace,
            vec![],
        )
        .await;
        assert_eq!(
            task.status,
            TaskStatus::Completed,
            "child result: {:?}",
            task.result
        );
        // `task` is never advertised on any provider request, so no
        // hand-crafted nested call could resolve — and the broker
        // manifest (unit-tested above) would deny it regardless.
        // (A forged in-turn call is not scripted here: an unregistered
        // tool parks on approval-ask with no responder in this harness,
        // which is pre-existing permission behavior, not the ceiling.)
        assert!(!reqs.is_empty());
        for req in &reqs {
            assert!(
                !tool_names(req).contains(&"task".to_string()),
                "task must never be advertised"
            );
        }
    }

    #[tokio::test]
    async fn worker_child_advertises_and_executes_authorized_nested_task() {
        let workspace =
            std::env::temp_dir().join(format!("m003-nest-allow-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&workspace);
        // The host parent surface includes task. The child role also allows
        // delegation, so its functional TaskTool must be installed before
        // the child provider surface is resolved.
        let parent = parent_surface_for(&["read", "task", "tool_search"], &[]);
        let snapshot = mint_for_with_generation(
            parent,
            "sess-m003-nest-allow",
            &workspace.to_string_lossy(),
            "",
        );
        let (mut reqs, task, live_requests, task_store) = spawn_under_ceiling(
            snapshot,
            test_agent("test-agent", true),
            vec![
                nested_spawn_call(&workspace),
                text_finish("grandchild completed"),
            ],
            workspace,
            vec![],
        )
        .await;
        assert_eq!(
            task.status,
            TaskStatus::Completed,
            "child result: {:?}",
            task.result
        );
        let grandchild = wait_completed(&task_store, 2).await;
        assert_eq!(
            grandchild.status,
            TaskStatus::Completed,
            "grandchild result: {:?}",
            grandchild.result
        );
        for _ in 0..200 {
            reqs = live_requests.lock().await.clone();
            if reqs
                .iter()
                .any(|request| request_texts(request).contains("complete the nested probe"))
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(!reqs.is_empty(), "child must issue a provider request");
        assert!(
            tool_names(&reqs[0]).contains(&"task".to_string()),
            "authorized child surface omitted task: {:?}",
            tool_names(&reqs[0])
        );
        assert!(
            tool_names(&reqs[0]).contains(&"tool_search".to_string()),
            "authorized child surface omitted tool_search: {:?}",
            tool_names(&reqs[0])
        );
        assert!(
            reqs.len() >= 2,
            "nested task did not start a grandchild provider turn"
        );
        assert!(
            reqs.iter()
                .any(|request| request.messages.iter().any(|message| {
                    matches!(message, codegg::provider::Message::Tool { content, .. }
                    if content.contains("Subagent spawned for task: M003 nested child"))
                })),
            "child provider transcript has no successful nested task result: {:?}",
            reqs.iter()
                .flat_map(|request| request.messages.iter())
                .filter_map(|message| match message {
                    codegg::provider::Message::Tool { content, .. } => Some(content.to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        );
        assert!(
            reqs.iter()
                .any(|request| request_texts(request).contains("complete the nested probe")),
            "no grandchild provider request received the nested prompt"
        );
    }

    fn request_texts(request: &ChatRequest) -> String {
        use codegg::provider::{ContentPart, Message};
        request
            .messages
            .iter()
            .filter_map(|message| match message {
                Message::User { content } => Some(
                    content
                        .iter()
                        .filter_map(|part| match part {
                            ContentPart::Text { text } => Some(text.as_ref().clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[tokio::test]
    async fn concurrent_children_keep_distinct_ceilings() {
        use tokio::sync::Mutex as TokioMutex;
        let workspace = std::env::temp_dir().join(format!("m003-conc-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&workspace);
        let pool = crate::common::pool::isolated_pool().await;
        let provider = ScriptedProvider {
            responses: vec![text_finish("done"), text_finish("done")],
            requests: Arc::new(Mutex::new(Vec::new())),
            index: Arc::new(Mutex::new(0)),
            id: "test".to_string(),
        };
        let requests = provider.requests.clone();
        let mut registry = ProviderRegistry::new();
        registry.register(provider);
        let session_store = Arc::new(codegg::session::SessionStore::new(pool.clone()));
        let agent = test_agent("test-agent", false);
        let subagent_pool = SubAgentPool::new(
            &Config::default(),
            vec![agent.clone()],
            registry,
            session_store,
            Some(pool),
        )
        .await;
        let task_store = subagent_pool.task_store();
        let spawner = subagent_pool.spawner();

        // Two siblings under different ceilings: alpha denies `write`,
        // beta allows it. Denied sets stay empty so the snapshots are
        // the only narrowing force.
        let full = ResolvedToolSurface::from_registry(
            &ToolRegistry::with_defaults(),
            &BTreeSet::new(),
            &BTreeSet::new(),
            false,
            None,
        )
        .unwrap();
        let narrowed = ResolvedToolSurface::from_registry(
            &ToolRegistry::with_defaults(),
            &BTreeSet::from(["write".to_string()]),
            &BTreeSet::new(),
            false,
            None,
        )
        .unwrap();
        let scope = workspace.to_string_lossy().into_owned();
        let mk_request = |tag: String,
                          snapshot: ParentToolAuthoritySnapshot,
                          store: Arc<TokioMutex<TaskStore>>| {
            let workspace = workspace.clone();
            let scope = scope.clone();
            let agent = agent.clone();
            let spawner = spawner.clone();
            let prompt = format!("probe {tag}");
            async move {
                let created = store
                    .lock()
                    .await
                    .create_task(
                        format!("m003-{tag}"),
                        prompt.clone(),
                        agent.name.clone(),
                        Some(snapshot.parent_session_id.clone()),
                        vec![],
                        vec![scope],
                    )
                    .await;
                let request = SubAgentRequest {
                    task_id: created,
                    run_id: None,
                    prompt,
                    agent: agent.name.clone(),
                    parent_id: Some(snapshot.parent_session_id.clone()),
                    parent_run_id: None,
                    denied_tools: vec![],
                    allowed_paths: vec![workspace.to_string_lossy().into_owned()],
                    description: format!("m003 {tag}"),
                    depth: 1,
                    max_tool_calls: None,
                    parent_model: None,
                    workspace_root: Some(workspace),
                    workspace_locks: None,
                    parent_sandbox_profile: None,
                    sandbox_profile: None,
                    parent_authority: Some(snapshot),
                };
                spawner.send_async(request).await.expect("spawn");
                created
            }
        };
        let alpha_snap = mint_for_with_generation(narrowed, "sess-m003-conc", &scope, "");
        let beta_snap = mint_for_with_generation(full, "sess-m003-conc", &scope, "");
        let (alpha_id, beta_id) = tokio::join!(
            mk_request("alpha".to_string(), alpha_snap, task_store.clone()),
            mk_request("beta".to_string(), beta_snap, task_store.clone())
        );
        let alpha = wait_completed(&task_store, alpha_id).await;
        let beta = wait_completed(&task_store, beta_id).await;
        assert_eq!(alpha.status, TaskStatus::Completed, "{:?}", alpha.result);
        assert_eq!(beta.status, TaskStatus::Completed, "{:?}", beta.result);

        // Attribute provider requests by prompt and assert per-child
        // ceilings held under concurrency.
        let reqs = requests.lock().await.clone();
        assert!(reqs.len() >= 2, "both children must issue requests");
        let mut alpha_saw_write = false;
        let mut beta_saw_write = false;
        let mut alpha_seen = false;
        let mut beta_seen = false;
        for req in &reqs {
            let texts = request_texts(req);
            let names = tool_names(req);
            if texts.contains("probe alpha") {
                alpha_seen = true;
                alpha_saw_write |= names.contains(&"write".to_string());
            }
            if texts.contains("probe beta") {
                beta_seen = true;
                beta_saw_write |= names.contains(&"write".to_string());
            }
        }
        assert!(alpha_seen && beta_seen, "both prompts must be observed");
        assert!(!alpha_saw_write, "alpha ceiling must withhold write");
        assert!(beta_saw_write, "beta ceiling must retain write");
    }

    #[tokio::test]
    async fn worker_child_rejects_forged_snapshot_before_execution() {
        let workspace = std::env::temp_dir().join(format!("m003-forge-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&workspace);
        let parent = ResolvedToolSurface::from_registry(
            &ToolRegistry::with_defaults(),
            &BTreeSet::new(),
            &BTreeSet::new(),
            false,
            None,
        )
        .unwrap();
        let mut forged =
            mint_for_with_generation(parent, "sess-m003-forge", &workspace.to_string_lossy(), "");
        // Attacker adds `task` but cannot re-mint the fingerprint.
        forged.eligible_tools.insert("task".into());
        let (reqs, task, _, _) = spawn_under_ceiling(
            forged,
            test_agent("test-agent", true),
            vec![text_finish("must never run")],
            workspace,
            vec![],
        )
        .await;
        // The worker fails the delegation closed before any provider
        // request or tool execution.
        assert_eq!(
            task.status,
            TaskStatus::Failed,
            "child result: {:?}",
            task.result
        );
        assert!(
            task.result
                .as_deref()
                .unwrap_or_default()
                .contains("InvalidParentReceipt"),
            "child result: {:?}",
            task.result
        );
        assert!(reqs.is_empty(), "no provider request may issue");
    }
}
