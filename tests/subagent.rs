#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use codegg::agent::worker::{SubAgentPool, SubAgentRequest, SubAgentResult};
    use codegg::agent::{Agent, AgentMode};
    use codegg::config::schema::{Config, SubagentConfig};
    use codegg::provider::{
        ChatEvent, ChatRequest, EventStream, ModelInfo, Provider, ProviderError, ProviderRegistry,
        TokenUsage,
    };
    use sqlx::SqlitePool;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    // =============================================================================
    // TEST PROVIDER FOR SUBAGENT LIFECYCLE TESTS
    // =============================================================================

    /// A deterministic provider that returns scripted responses for subagent testing.
    /// This provider can be registered with ProviderRegistry for real subagent tests.
    #[derive(Clone)]
    struct SubagentTestProvider {
        responses: Vec<Vec<ChatEvent>>,
        requests: Arc<Mutex<Vec<ChatRequest>>>,
        response_index: Arc<Mutex<usize>>,
        id: String,
    }

    impl SubagentTestProvider {
        fn new(id: &str, responses: Vec<Vec<ChatEvent>>) -> Self {
            Self {
                responses,
                requests: Arc::new(Mutex::new(Vec::new())),
                response_index: Arc::new(Mutex::new(0)),
                id: id.to_string(),
            }
        }
    }

    #[async_trait]
    impl Provider for SubagentTestProvider {
        fn id(&self) -> &str {
            &self.id
        }

        fn name(&self) -> &str {
            "Subagent Test Provider"
        }

        fn clone_box(&self) -> Box<dyn Provider> {
            Box::new(self.clone())
        }

        async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError> {
            self.requests.lock().await.push(request.clone());

            let mut idx = self.response_index.lock().await;
            let events = if *idx < self.responses.len() {
                self.responses[*idx].clone()
            } else {
                vec![ChatEvent::Finish {
                    stop_reason: "stop".to_string().into(),
                    usage: TokenUsage::default(),
                }]
            };
            *idx += 1;

            let stream = futures::stream::iter(events.into_iter().map(Ok::<_, ProviderError>));
            Ok(Box::pin(stream))
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

    // =============================================================================
    // HELPER FUNCTIONS
    // =============================================================================

    /// Wait for a task to reach a terminal state (Completed, Failed, or Interrupted).
    /// Returns the task if it completed within the timeout, or None if timeout exceeded.
    async fn wait_for_task_result(
        task_store: &Arc<tokio::sync::Mutex<codegg::tool::task::TaskStore>>,
        task_id: u64,
        max_attempts: u32,
        interval_ms: u64,
    ) -> Option<codegg::tool::task::SubAgentTask> {
        for _ in 0..max_attempts {
            let store = task_store.lock().await;
            if let Some(task) = store.get_task(task_id).await {
                match task.status {
                    codegg::tool::task::TaskStatus::Completed
                    | codegg::tool::task::TaskStatus::Failed
                    | codegg::tool::task::TaskStatus::Interrupted => {
                        return Some(task);
                    }
                    _ => {}
                }
            }
            drop(store);
            tokio::time::sleep(std::time::Duration::from_millis(interval_ms)).await;
        }
        None
    }

    /// Create a task in the store and send a subagent request.
    /// Returns the created task ID.
    async fn create_task_and_send(
        task_store: &Arc<tokio::sync::Mutex<codegg::tool::task::TaskStore>>,
        spawner: &codegg::agent::worker::SubAgentSpawner,
        mut request: SubAgentRequest,
    ) -> Result<u64, String> {
        let created_id = task_store
            .lock()
            .await
            .create_task(
                request.description.clone(),
                request.prompt.clone(),
                request.agent.clone(),
                request.parent_id.clone(),
                request.denied_tools.clone(),
                request.allowed_paths.clone(),
            )
            .await;
        request.task_id = created_id;
        spawner.send_async(request).await?;
        Ok(created_id)
    }

    /// Create a test agent with the given name and model.
    fn create_test_agent(name: &str, model: &str, system_prompt: Option<&str>) -> Agent {
        Agent {
            name: name.to_string(),
            role: None,
            description: format!("Test agent {}", name),
            mode: AgentMode::Primary,
            mode_name: None,
            model: Some(model.to_string()),
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: system_prompt.map(|s| s.to_string()),
            permissions: std::collections::HashMap::new(),
            hidden: false,
            thinking_budget: None,
            reasoning_effort: None,
        }
    }

    /// Create a test provider registry with a deterministic provider.
    fn create_test_provider_registry() -> (ProviderRegistry, Arc<Mutex<Vec<ChatRequest>>>) {
        let responses = vec![vec![
            ChatEvent::TextDelta("Subagent response".to_string().into()),
            ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: TokenUsage::default(),
            },
        ]];

        let provider = SubagentTestProvider::new("test", responses);
        let requests = provider.requests.clone();

        let mut registry = ProviderRegistry::new();
        registry.register(provider);

        (registry, requests)
    }

    /// Create a test pool with required tables for subagent testing.
    async fn create_test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();

        // Create required tables
        create_session_table(&pool).await;
        create_message_table(&pool).await;
        create_part_table(&pool).await;
        create_todo_table(&pool).await;
        create_permission_table(&pool).await;
        create_session_share_table(&pool).await;
        create_task_table(&pool).await;

        pool
    }

    async fn create_session_table(pool: &SqlitePool) {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS session (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                workspace_id TEXT,
                parent_id TEXT,
                slug TEXT NOT NULL,
                directory TEXT NOT NULL,
                title TEXT NOT NULL,
                version TEXT NOT NULL,
                share_url TEXT,
                summary_additions INTEGER,
                summary_deletions INTEGER,
                summary_files INTEGER,
                summary_diffs TEXT,
                revert TEXT,
                permission TEXT,
                tags TEXT,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                time_compacting INTEGER,
                time_archived INTEGER,
                time_deleted INTEGER
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
    }

    async fn create_message_table(pool: &SqlitePool) {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS message (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
    }

    async fn create_part_table(pool: &SqlitePool) {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS part (
                id TEXT PRIMARY KEY,
                message_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
    }

    async fn create_todo_table(pool: &SqlitePool) {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS todo (
                session_id TEXT NOT NULL,
                content TEXT NOT NULL,
                status TEXT NOT NULL,
                priority TEXT NOT NULL,
                position INTEGER NOT NULL,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                PRIMARY KEY (session_id, position)
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
    }

    async fn create_permission_table(pool: &SqlitePool) {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS permission (
                project_id TEXT PRIMARY KEY,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
    }

    async fn create_session_share_table(pool: &SqlitePool) {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS session_share (
                session_id TEXT PRIMARY KEY,
                id TEXT NOT NULL,
                secret TEXT NOT NULL,
                url TEXT NOT NULL,
                share_expires_at INTEGER,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
    }

    async fn create_task_table(pool: &SqlitePool) {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS task (
                id INTEGER PRIMARY KEY,
                parent_id TEXT,
                session_id TEXT NOT NULL,
                description TEXT NOT NULL,
                prompt TEXT NOT NULL,
                agent TEXT NOT NULL,
                status TEXT NOT NULL,
                result TEXT,
                denied_tools TEXT,
                time_created INTEGER NOT NULL,
                time_updated INTEGER NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
    }

    // =============================================================================
    // PACKET 9: REAL SUBAGENT LIFECYCLE TESTS
    // =============================================================================

    /// Test 1: Real subagent lifecycle with deterministic provider.
    /// Verifies: task status transitions, result text stored, provider receives prompt,
    /// parent session ID propagated.
    #[tokio::test]
    async fn test_real_subagent_lifecycle() {
        let pool = create_test_pool().await;
        let (provider_registry, provider_requests) = create_test_provider_registry();

        // Create agent with model matching the provider ID
        let agent = create_test_agent("test-agent", "test/model", Some("You are a test agent"));

        let config = Config::default();
        let session_store = Arc::new(codegg::session::SessionStore::new(pool.clone()));

        let subagent_pool = SubAgentPool::new(
            &config,
            vec![agent.clone()],
            provider_registry,
            session_store,
            Some(pool.clone()),
        )
        .await;

        let spawner = subagent_pool.spawner();
        let task_store = subagent_pool.task_store();

        // Send subagent request - create task first so subsequent operations work
        let request = SubAgentRequest {
            task_id: 1,
            prompt: "Hello from parent".to_string(),
            agent: "test-agent".to_string(),
            parent_id: Some("parent-session-123".to_string()),
            denied_tools: vec![],
            allowed_paths: vec![],
            description: "Test task".to_string(),
            depth: 0,
            max_tool_calls: None,
        };

        let created_id = create_task_and_send(&task_store, &spawner, request.clone()).await;
        assert!(
            created_id.is_ok(),
            "create_task_and_send should succeed: {:?}",
            created_id.err()
        );

        // Wait for task completion using bounded polling
        let completed_task = wait_for_task_result(&task_store, created_id.unwrap(), 100, 50).await;
        assert!(
            completed_task.is_some(),
            "Task should complete within timeout"
        );

        let task = completed_task.unwrap();
        assert!(
            matches!(task.status, codegg::tool::task::TaskStatus::Completed),
            "Task should be Completed, got {:?}",
            task.status
        );

        // Verify stored result text contains deterministic provider output
        assert!(
            task.result
                .as_ref()
                .is_some_and(|r| r.contains("Subagent response")),
            "Stored result should contain 'Subagent response', got: {:?}",
            task.result
        );

        // Verify the provider received the expected prompt
        let requests = provider_requests.lock().await;
        assert!(
            !requests.is_empty(),
            "Provider should have received requests"
        );

        // Check that the request contains the expected prompt
        let has_expected_prompt = requests.iter().any(|req| {
            req.messages.iter().any(|msg| {
                if let codegg::provider::Message::User { content } = msg {
                    content.iter().any(|p| {
                        if let codegg::provider::ContentPart::Text { text } = p {
                            text.contains("Hello from parent")
                        } else {
                            false
                        }
                    })
                } else {
                    false
                }
            })
        });
        assert!(
            has_expected_prompt,
            "Provider should have received the expected prompt"
        );

        // Check system prompt if provided
        if agent.system_prompt.is_some() {
            let has_system_prompt = requests.iter().any(|req| {
                req.messages.iter().any(|msg| {
                    matches!(msg, codegg::provider::Message::System { content } if content.contains("test agent"))
                })
            });
            assert!(
                has_system_prompt,
                "Provider should have received the system prompt"
            );
        }

        // Verify task is not left running
        let store = task_store.lock().await;
        let final_task = store.get_task(1).await;
        assert!(
            final_task.is_none()
                || !matches!(
                    final_task.as_ref().unwrap().status,
                    codegg::tool::task::TaskStatus::Running
                ),
            "Task should not be left in Running state"
        );
    }

    /// Test 2: Nonexistent agent sets failed result.
    #[tokio::test]
    async fn test_nonexistent_agent_sets_failed_result() {
        let pool = create_test_pool().await;
        let (provider_registry, _provider_requests) = create_test_provider_registry();

        let config = Config::default();
        let session_store = Arc::new(codegg::session::SessionStore::new(pool.clone()));

        let subagent_pool = SubAgentPool::new(
            &config,
            vec![], // No agents registered
            provider_registry,
            session_store,
            Some(pool.clone()),
        )
        .await;

        let spawner = subagent_pool.spawner();
        let task_store = subagent_pool.task_store();

        let request = SubAgentRequest {
            task_id: 1,
            prompt: "Test".to_string(),
            agent: "nonexistent-agent".to_string(),
            parent_id: Some("parent".to_string()),
            denied_tools: vec![],
            allowed_paths: vec![],
            description: "Test".to_string(),
            depth: 0,
            max_tool_calls: None,
        };

        let created_id = create_task_and_send(&task_store, &spawner, request.clone()).await;
        assert!(created_id.is_ok(), "create_task_and_send should succeed");

        // Wait for task to complete using bounded polling
        let completed_task = wait_for_task_result(&task_store, created_id.unwrap(), 100, 50).await;
        assert!(
            completed_task.is_some(),
            "Task should complete within timeout"
        );

        let task = completed_task.unwrap();
        assert!(
            matches!(task.status, codegg::tool::task::TaskStatus::Failed),
            "Task should be Failed for nonexistent agent, got {:?}",
            task.status
        );

        // Verify failed result contains the missing agent name
        assert!(
            task.result
                .as_ref()
                .is_some_and(|r| r.contains("nonexistent-agent")),
            "Failed result should contain 'nonexistent-agent', got: {:?}",
            task.result
        );
    }

    /// Test 3: Nonexistent provider sets failed result.
    #[tokio::test]
    async fn test_nonexistent_provider_sets_failed_result() {
        let pool = create_test_pool().await;

        // Create agent with model pointing to nonexistent provider
        let agent = create_test_agent("test-agent", "nonexistent/model", None);

        let provider_registry = ProviderRegistry::new(); // Empty registry

        let config = Config::default();
        let session_store = Arc::new(codegg::session::SessionStore::new(pool.clone()));

        let subagent_pool = SubAgentPool::new(
            &config,
            vec![agent],
            provider_registry,
            session_store,
            Some(pool.clone()),
        )
        .await;

        let spawner = subagent_pool.spawner();
        let task_store = subagent_pool.task_store();

        let request = SubAgentRequest {
            task_id: 1,
            prompt: "Test".to_string(),
            agent: "test-agent".to_string(),
            parent_id: Some("parent".to_string()),
            denied_tools: vec![],
            allowed_paths: vec![],
            description: "Test".to_string(),
            depth: 0,
            max_tool_calls: None,
        };

        let created_id = create_task_and_send(&task_store, &spawner, request.clone()).await;
        assert!(created_id.is_ok(), "create_task_and_send should succeed");

        // Wait for task to complete using bounded polling
        let completed_task = wait_for_task_result(&task_store, created_id.unwrap(), 100, 50).await;
        assert!(
            completed_task.is_some(),
            "Task should complete within timeout"
        );

        let task = completed_task.unwrap();
        assert!(
            matches!(task.status, codegg::tool::task::TaskStatus::Failed),
            "Task should be Failed for nonexistent provider, got {:?}",
            task.status
        );

        // Verify failed result contains the missing provider name
        assert!(
            task.result
                .as_ref()
                .is_some_and(|r| r.contains("nonexistent")),
            "Failed result should contain 'nonexistent', got: {:?}",
            task.result
        );
    }

    /// Test 4: Depth equal to max depth returns error before queueing.
    #[tokio::test]
    async fn test_max_depth_returns_error_before_queueing() {
        let pool = create_test_pool().await;
        let (provider_registry, _provider_requests) = create_test_provider_registry();

        let config = Config {
            subagent: Some(SubagentConfig {
                max_concurrent: Some(5),
                max_depth: Some(3),
            }),
            ..Default::default()
        };

        let session_store = Arc::new(codegg::session::SessionStore::new(pool.clone()));

        let subagent_pool = SubAgentPool::new(
            &config,
            vec![],
            provider_registry,
            session_store,
            Some(pool.clone()),
        )
        .await;

        let spawner = subagent_pool.spawner();

        // Depth equal to max_depth (3) should fail
        let request = SubAgentRequest {
            task_id: 1,
            prompt: "Test".to_string(),
            agent: "test-agent".to_string(),
            parent_id: None,
            denied_tools: vec![],
            allowed_paths: vec![],
            description: "Test".to_string(),
            depth: 3, // Equal to max_depth
            max_tool_calls: None,
        };

        let result = spawner.send_async(request.clone()).await;
        assert!(
            result.is_err(),
            "Should return error for depth >= max_depth"
        );
        assert!(
            result.unwrap_err().contains("max depth"),
            "Error should mention max depth"
        );

        // Verify it returns error BEFORE queueing - task should not exist in store
        let task_store = subagent_pool.task_store();
        let store = task_store.lock().await;
        let task_in_store = store.get_task(1).await;
        assert!(
            task_in_store.is_none(),
            "Task should not be created when depth >= max_depth (error before queueing), got: {:?}",
            task_in_store
        );
        drop(store);

        // Depth greater than max_depth should also fail
        let request2 = SubAgentRequest {
            task_id: 2,
            prompt: "Test".to_string(),
            agent: "test-agent".to_string(),
            parent_id: None,
            denied_tools: vec![],
            allowed_paths: vec![],
            description: "Test".to_string(),
            depth: 4, // Greater than max_depth
            max_tool_calls: None,
        };

        let result2 = spawner.send_async(request2).await;
        assert!(
            result2.is_err(),
            "Should return error for depth > max_depth"
        );

        // Verify it also returns error BEFORE queueing - task 2 should not exist
        let store = task_store.lock().await;
        let task2_in_store = store.get_task(2).await;
        assert!(
            task2_in_store.is_none(),
            "Task 2 should not be created when depth > max_depth (error before queueing), got: {:?}",
            task2_in_store
        );
    }

    /// Test 5: Denied-tool filtering.
    /// Verifies that denied tools are excluded from subagent tool registry
    /// and provider request tool definitions do not include the denied tool.
    #[tokio::test]
    async fn test_denied_tool_filtering() {
        // Create a provider that records tool definitions from requests
        #[derive(Clone)]
        struct RecordingProvider {
            requests: Arc<Mutex<Vec<ChatRequest>>>,
        }

        impl RecordingProvider {
            fn new() -> Self {
                Self {
                    requests: Arc::new(Mutex::new(Vec::new())),
                }
            }
        }

        #[async_trait]
        impl Provider for RecordingProvider {
            fn id(&self) -> &str {
                "recording"
            }

            fn name(&self) -> &str {
                "Recording Provider"
            }

            fn clone_box(&self) -> Box<dyn Provider> {
                Box::new(self.clone())
            }

            async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError> {
                self.requests.lock().await.push(request.clone());

                let events = vec![
                    ChatEvent::TextDelta("Done".to_string().into()),
                    ChatEvent::Finish {
                        stop_reason: "stop".to_string().into(),
                        usage: TokenUsage::default(),
                    },
                ];

                let stream = futures::stream::iter(events.into_iter().map(Ok::<_, ProviderError>));
                Ok(Box::pin(stream))
            }

            async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
                Ok(vec![ModelInfo {
                    id: "recording/model".to_string(),
                    name: "Recording Model".to_string(),
                    provider: "recording".to_string(),
                    context_window: 4096,
                    max_output_tokens: Some(2048),
                    supports_tools: true,
                    supports_vision: false,
                    variants: vec![],
                }])
            }
        }

        let pool = create_test_pool().await;

        let recording_provider = RecordingProvider::new();
        let requests = recording_provider.requests.clone();

        let mut provider_registry = ProviderRegistry::new();
        provider_registry.register(recording_provider);

        let agent = create_test_agent("test-agent", "recording/model", None);

        let config = Config::default();
        let session_store = Arc::new(codegg::session::SessionStore::new(pool.clone()));

        let subagent_pool = SubAgentPool::new(
            &config,
            vec![agent],
            provider_registry,
            session_store,
            Some(pool.clone()),
        )
        .await;

        let spawner = subagent_pool.spawner();
        let task_store = subagent_pool.task_store();

        // Send request with denied tools
        let request = SubAgentRequest {
            task_id: 1,
            prompt: "Test with denied tools".to_string(),
            agent: "test-agent".to_string(),
            parent_id: Some("parent".to_string()),
            denied_tools: vec!["bash".to_string(), "write".to_string()],
            allowed_paths: vec![],
            description: "Test".to_string(),
            depth: 0,
            max_tool_calls: None,
        };

        let created_id = create_task_and_send(&task_store, &spawner, request.clone()).await;
        assert!(created_id.is_ok(), "create_task_and_send should succeed");

        // Wait for task to complete using bounded polling
        let completed_task = wait_for_task_result(&task_store, created_id.unwrap(), 100, 50).await;
        assert!(
            completed_task.is_some(),
            "Task should complete within timeout"
        );

        // Verify provider request does not include denied tools in tool definitions
        let requests = requests.lock().await;
        assert!(
            !requests.is_empty(),
            "Provider should have received requests"
        );

        let mut found_non_denied_tool = false;
        for req in requests.iter() {
            if let Some(tools) = &req.tools {
                for tool in tools {
                    assert!(
                        tool.name != "bash" && tool.name != "write",
                        "Denied tool '{}' should not be in provider request",
                        tool.name
                    );
                    found_non_denied_tool = true;
                }
            }
        }
        // At least one non-denied tool should remain
        assert!(
            found_non_denied_tool,
            "At least one non-denied tool should remain in provider request"
        );
    }

    /// Test 6: Strengthened concurrency test.
    /// Uses controlled barriers to prove no more than max_concurrent tasks run simultaneously.
    #[tokio::test]
    async fn test_concurrency_with_barrier() {
        // Use Notify for better control
        let notify = Arc::new(tokio::sync::Notify::new());
        let running_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let max_observed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let completed_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        // Create a provider that waits on a Notify before responding
        #[derive(Clone)]
        struct ControlledProvider {
            notify: Arc<tokio::sync::Notify>,
            running_count: Arc<std::sync::atomic::AtomicUsize>,
            max_observed: Arc<std::sync::atomic::AtomicUsize>,
            completed_count: Arc<std::sync::atomic::AtomicUsize>,
            requests: Arc<Mutex<Vec<ChatRequest>>>,
        }

        impl ControlledProvider {
            fn new(
                notify: Arc<tokio::sync::Notify>,
                running_count: Arc<std::sync::atomic::AtomicUsize>,
                max_observed: Arc<std::sync::atomic::AtomicUsize>,
                completed_count: Arc<std::sync::atomic::AtomicUsize>,
            ) -> Self {
                Self {
                    notify,
                    running_count,
                    max_observed,
                    completed_count,
                    requests: Arc::new(Mutex::new(Vec::new())),
                }
            }
        }

        #[async_trait]
        impl Provider for ControlledProvider {
            fn id(&self) -> &str {
                "controlled"
            }

            fn name(&self) -> &str {
                "Controlled Provider"
            }

            fn clone_box(&self) -> Box<dyn Provider> {
                Box::new(self.clone())
            }

            async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError> {
                self.requests.lock().await.push(request.clone());

                // Increment running count
                let current = self
                    .running_count
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                    + 1;

                // Update max observed
                let mut max = self.max_observed.load(std::sync::atomic::Ordering::SeqCst);
                while current > max {
                    match self.max_observed.compare_exchange_weak(
                        max,
                        current,
                        std::sync::atomic::Ordering::SeqCst,
                        std::sync::atomic::Ordering::SeqCst,
                    ) {
                        Ok(_) => break,
                        Err(new_max) => max = new_max,
                    }
                }

                // Wait until test allows us to proceed
                self.notify.notified().await;

                // Decrement running count
                self.running_count
                    .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);

                // Increment completed count
                self.completed_count
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

                let events = vec![
                    ChatEvent::TextDelta("Done".to_string().into()),
                    ChatEvent::Finish {
                        stop_reason: "stop".to_string().into(),
                        usage: TokenUsage::default(),
                    },
                ];

                let stream = futures::stream::iter(events.into_iter().map(Ok::<_, ProviderError>));
                Ok(Box::pin(stream))
            }

            async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
                Ok(vec![ModelInfo {
                    id: "controlled/model".to_string(),
                    name: "Controlled Model".to_string(),
                    provider: "controlled".to_string(),
                    context_window: 4096,
                    max_output_tokens: Some(2048),
                    supports_tools: true,
                    supports_vision: false,
                    variants: vec![],
                }])
            }
        }

        let pool = create_test_pool().await;

        let provider = ControlledProvider::new(
            notify.clone(),
            running_count.clone(),
            max_observed.clone(),
            completed_count.clone(),
        );

        let mut provider_registry = ProviderRegistry::new();
        provider_registry.register(provider);

        let agent = create_test_agent("test-agent", "controlled/model", None);

        let config = Config {
            subagent: Some(SubagentConfig {
                max_concurrent: Some(2),
                max_depth: Some(3),
            }),
            ..Default::default()
        };

        let session_store = Arc::new(codegg::session::SessionStore::new(pool.clone()));

        let subagent_pool = SubAgentPool::new(
            &config,
            vec![agent],
            provider_registry,
            session_store,
            Some(pool.clone()),
        )
        .await;

        assert_eq!(subagent_pool.max_concurrent(), 2);

        let spawner = subagent_pool.spawner();

        // Enqueue 3 tasks (max_concurrent is 2)
        for i in 0..3 {
            let request = SubAgentRequest {
                task_id: i as u64,
                prompt: format!("Task {}", i),
                agent: "test-agent".to_string(),
                parent_id: None,
                denied_tools: vec![],
                allowed_paths: vec![],
                description: format!("Task {}", i),
                depth: 0,
                max_tool_calls: None,
            };
            let result = spawner.send_async(request).await;
            assert!(result.is_ok(), "send_async should succeed for task {}", i);
        }

        // Give tasks time to start
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        // At this point, max 2 should be running (the 3rd is queued)
        let max = max_observed.load(std::sync::atomic::Ordering::SeqCst);
        assert!(
            max <= 2,
            "Max concurrent tasks observed: {}, should be <= 2",
            max
        );

        // Release one task at a time to verify concurrency
        // Release first task
        notify.notify_one();
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Release second task
        notify.notify_one();
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Now the 3rd task should be running (since one of the first 2 completed)
        // Release third task
        notify.notify_one();

        // Wait for all tasks to complete by polling completed_count
        for _ in 0..100 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let completed = completed_count.load(std::sync::atomic::Ordering::SeqCst);
            if completed >= 3 {
                break;
            }
        }

        // Wait for active_count to become 0
        for _ in 0..100 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let active = subagent_pool.active_count();
            if active == 0 {
                break;
            }
        }

        // Verify all tasks eventually completed
        let active = subagent_pool.active_count();
        assert_eq!(
            active, 0,
            "All tasks should have completed, active={}",
            active
        );

        // Also verify completed count
        let completed = completed_count.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(completed, 3, "All 3 tasks should have completed");
    }
    #[test]
    fn test_subagent_request_builder() {
        let request = SubAgentRequest {
            task_id: 123,
            prompt: "Test prompt".to_string(),
            agent: "test".to_string(),
            parent_id: Some("parent-session".to_string()),
            denied_tools: vec!["bash".to_string()],
            allowed_paths: vec![],
            description: "Test task".to_string(),
            depth: 0,
            max_tool_calls: None,
        };
        assert_eq!(request.task_id, 123);
        assert_eq!(request.agent, "test");
        assert_eq!(request.denied_tools, vec!["bash"]);
        assert_eq!(request.depth, 0);
        assert_eq!(request.parent_id, Some("parent-session".to_string()));
    }

    #[test]
    fn test_subagent_result_success() {
        let result = SubAgentResult::success(1, "output".to_string());
        assert!(result.success);
        assert_eq!(result.task_id, 1);
        assert_eq!(result.result, "output");
    }

    #[test]
    fn test_subagent_result_failure() {
        let result = SubAgentResult::failure(1, "error message".to_string());
        assert!(!result.success);
        assert_eq!(result.task_id, 1);
        assert_eq!(result.result, "error message");
    }

    #[test]
    fn test_subagent_request_with_different_depths() {
        let request_depth_0 = SubAgentRequest {
            task_id: 1,
            prompt: "depth 0".to_string(),
            agent: "test".to_string(),
            parent_id: None,
            denied_tools: vec![],
            allowed_paths: vec![],
            description: "test".to_string(),
            depth: 0,
            max_tool_calls: None,
        };

        let request_depth_2 = SubAgentRequest {
            task_id: 2,
            prompt: "depth 2".to_string(),
            agent: "test".to_string(),
            parent_id: Some("parent".to_string()),
            denied_tools: vec![],
            allowed_paths: vec![],
            description: "test".to_string(),
            depth: 2,
            max_tool_calls: None,
        };

        assert_eq!(request_depth_0.depth, 0);
        assert_eq!(request_depth_2.depth, 2);
        assert_eq!(request_depth_2.parent_id, Some("parent".to_string()));
    }

    #[test]
    fn test_subagent_result_with_empty_result() {
        let result = SubAgentResult::success(42, String::new());
        assert!(result.success);
        assert_eq!(result.result, String::new());
    }

    #[test]
    fn test_subagent_request_clone() {
        let request = SubAgentRequest {
            task_id: 1,
            prompt: "Test prompt".to_string(),
            agent: "test".to_string(),
            parent_id: Some("parent".to_string()),
            denied_tools: vec!["bash".to_string(), "write".to_string()],
            allowed_paths: vec![],
            description: "Test description".to_string(),
            depth: 1,
            max_tool_calls: None,
        };
        let cloned = request.clone();
        assert_eq!(cloned.task_id, request.task_id);
        assert_eq!(cloned.prompt, request.prompt);
        assert_eq!(cloned.agent, request.agent);
        assert_eq!(cloned.parent_id, request.parent_id);
        assert_eq!(cloned.denied_tools, request.denied_tools);
        assert_eq!(cloned.description, request.description);
        assert_eq!(cloned.depth, request.depth);
    }

    #[test]
    fn test_subagent_result_clone() {
        let result = SubAgentResult::success(1, "result".to_string());
        let cloned = result.clone();
        assert_eq!(cloned.task_id, result.task_id);
        assert_eq!(cloned.success, result.success);
        assert_eq!(cloned.result, result.result);
    }

    #[tokio::test]
    async fn test_subagent_spawner_send_async_not_blocking() {
        use codegg::agent::worker::SubAgentPool;
        use codegg::config::schema::Config;
        use codegg::provider::ProviderRegistry;
        use std::sync::Arc;

        let pool = create_test_pool().await;
        let config = Config::default();
        let provider_registry = ProviderRegistry::new();
        let session_store = Arc::new(codegg::session::SessionStore::new(pool.clone()));

        let subagent_pool = SubAgentPool::new(
            &config,
            vec![],
            provider_registry,
            session_store,
            Some(pool.clone()),
        )
        .await;

        let spawner = subagent_pool.spawner();
        let request = SubAgentRequest {
            task_id: 1,
            prompt: "test".to_string(),
            agent: "nonexistent".to_string(),
            parent_id: None,
            denied_tools: vec![],
            allowed_paths: vec![],
            description: "test".to_string(),
            depth: 0,
            max_tool_calls: None,
        };

        let result = spawner.send_async(request).await;
        assert!(
            result.is_ok(),
            "send_async should not block or panic: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_subagent_pool_concurrent_execution() {
        use codegg::agent::worker::SubAgentPool;
        use codegg::config::schema::{Config, SubagentConfig};
        use codegg::provider::ProviderRegistry;
        use std::sync::Arc;

        let pool = create_test_pool().await;

        let mut config = Config::default();
        config.subagent = Some(SubagentConfig {
            max_concurrent: Some(2),
            max_depth: Some(3),
        });

        let provider_registry = ProviderRegistry::new();
        let session_store = Arc::new(codegg::session::SessionStore::new(pool.clone()));

        let subagent_pool = SubAgentPool::new(
            &config,
            vec![],
            provider_registry,
            session_store,
            Some(pool.clone()),
        )
        .await;

        assert_eq!(subagent_pool.max_concurrent(), 2);

        let spawner = subagent_pool.spawner();
        let barrier = Arc::new(tokio::sync::Barrier::new(3));

        let handle1 = tokio::spawn({
            let spawner = spawner.clone();
            let barrier = Arc::clone(&barrier);
            async move {
                let req = SubAgentRequest {
                    task_id: 1,
                    prompt: "task 1".to_string(),
                    agent: "nonexistent".to_string(),
                    parent_id: None,
                    denied_tools: vec![],
                    allowed_paths: vec![],
                    description: "task 1".to_string(),
                    depth: 0,
                    max_tool_calls: None,
                };
                let _ = spawner.send_async(req).await;
                barrier.wait().await;
            }
        });

        let handle2 = tokio::spawn({
            let spawner = spawner.clone();
            let barrier = Arc::clone(&barrier);
            async move {
                let req = SubAgentRequest {
                    task_id: 2,
                    prompt: "task 2".to_string(),
                    agent: "nonexistent".to_string(),
                    parent_id: None,
                    denied_tools: vec![],
                    allowed_paths: vec![],
                    description: "task 2".to_string(),
                    depth: 0,
                    max_tool_calls: None,
                };
                let _ = spawner.send_async(req).await;
                barrier.wait().await;
            }
        });

        barrier.wait().await;

        let active = subagent_pool.active_count();
        assert!(
            active <= 2,
            "active_count {} should be <= max_concurrent 2",
            active
        );

        handle1.await.unwrap();
        handle2.await.unwrap();
    }

    // =============================================================================
    // PACKET 2: SHUTDOWN DURING ACTIVE WORK TEST
    // =============================================================================

    /// Test: Shutdown during active work should set Interrupted status.
    /// Verifies:
    /// 1. shutdown() returns without hanging
    /// 2. active_count() becomes 0 after shutdown
    /// 3. Task status is Interrupted (not Failed, not Completed)
    /// 4. Task is not later changed to Completed
    #[tokio::test]
    async fn test_shutdown_during_active_work() {
        use codegg::tool::task::TaskStatus;

        // Create a provider that blocks until notified
        let notify = Arc::new(tokio::sync::Notify::new());

        #[derive(Clone)]
        struct BlockingProvider {
            notify: Arc<tokio::sync::Notify>,
            requests: Arc<Mutex<Vec<ChatRequest>>>,
        }

        impl BlockingProvider {
            fn new(notify: Arc<tokio::sync::Notify>) -> Self {
                Self {
                    notify,
                    requests: Arc::new(Mutex::new(Vec::new())),
                }
            }
        }

        #[async_trait]
        impl Provider for BlockingProvider {
            fn id(&self) -> &str {
                "blocking"
            }

            fn name(&self) -> &str {
                "Blocking Provider"
            }

            fn clone_box(&self) -> Box<dyn Provider> {
                Box::new(self.clone())
            }

            async fn stream(&self, request: &ChatRequest) -> Result<EventStream, ProviderError> {
                self.requests.lock().await.push(request.clone());
                // Wait until notified to simulate long-running task
                self.notify.notified().await;

                let events = vec![
                    ChatEvent::TextDelta("Done".to_string().into()),
                    ChatEvent::Finish {
                        stop_reason: "stop".to_string().into(),
                        usage: TokenUsage::default(),
                    },
                ];
                let stream = futures::stream::iter(events.into_iter().map(Ok::<_, ProviderError>));
                Ok(Box::pin(stream))
            }

            async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
                Ok(vec![ModelInfo {
                    id: "blocking/model".to_string(),
                    name: "Blocking Model".to_string(),
                    provider: "blocking".to_string(),
                    context_window: 4096,
                    max_output_tokens: Some(2048),
                    supports_tools: true,
                    supports_vision: false,
                    variants: vec![],
                }])
            }
        }

        let pool = create_test_pool().await;

        let blocking_provider = BlockingProvider::new(notify.clone());
        let mut provider_registry = ProviderRegistry::new();
        provider_registry.register(blocking_provider);

        let agent = create_test_agent("blocking-agent", "blocking/model", None);

        let config = Config::default();
        let session_store = Arc::new(codegg::session::SessionStore::new(pool.clone()));

        let subagent_pool = SubAgentPool::new(
            &config,
            vec![agent],
            provider_registry,
            session_store,
            Some(pool.clone()),
        )
        .await;

        let spawner = subagent_pool.spawner();
        let task_store = subagent_pool.task_store();

        // Create and send a task
        let request = SubAgentRequest {
            task_id: 999,
            prompt: "Block me".to_string(),
            agent: "blocking-agent".to_string(),
            parent_id: Some("parent".to_string()),
            denied_tools: vec![],
            allowed_paths: vec![],
            description: "Blocking task".to_string(),
            depth: 0,
            max_tool_calls: None,
        };

        let created_id = create_task_and_send(&task_store, &spawner, request).await;
        assert!(created_id.is_ok(), "create_task_and_send should succeed");
        let task_id = created_id.unwrap();

        // Wait for task to start running (active_count > 0)
        for _ in 0..100 {
            if subagent_pool.active_count() > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        assert!(
            subagent_pool.active_count() > 0,
            "Task should be running (active_count > 0)"
        );

        // Call shutdown while task is running
        subagent_pool.shutdown().await;

        // Assert shutdown returns and active_count is 0
        assert_eq!(
            subagent_pool.active_count(),
            0,
            "active_count should be 0 after shutdown"
        );

        // Check task status - should be Interrupted
        let store = task_store.lock().await;
        let task = store.get_task(task_id).await;
        assert!(task.is_some(), "Task should exist after shutdown");

        let task = task.unwrap();
        assert_eq!(
            task.status,
            TaskStatus::Interrupted,
            "Task status should be Interrupted after shutdown, got {:?}",
            task.status
        );

        // Verify task is not Completed
        assert_ne!(
            task.status,
            TaskStatus::Completed,
            "Task should NOT be Completed after shutdown"
        );

        // Also verify the result contains indication of cancellation
        assert!(
            task.result
                .as_ref()
                .is_some_and(|r| r.contains("cancelled") || r.contains("Task cancelled")),
            "Task result should indicate cancellation, got: {:?}",
            task.result
        );
    }
}
