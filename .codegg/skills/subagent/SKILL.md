---
name: subagent
description: SubAgentPool, SubAgentSpawner, worker task infrastructure for parallel agent execution
version: 1.1.0
tags: [agent, worker, parallel, spawner]
---

# Skill: SubAgent Infrastructure

This skill covers the subagent infrastructure in opencode-rs, which enables the main agent to spawn independent subagents for parallel task execution.

## Architecture Overview

```
Parent Agent → SubAgentPool → Worker Tasks (up to 5 concurrent)
                ↓
            TaskTool → SubAgentSpawner → WorkerRequest channel
```

## Key Components

### SubAgentPool (`src/agent/worker.rs`)

```rust
pub struct SubAgentPool {
    shutdown_tx: broadcast::Sender<()>,
    active_count: Arc<AtomicUsize>,
    max_concurrent: usize,  // 5
    max_depth: usize,      // 3
    task_store: Arc<Mutex<TaskStore>>,
    workers: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    request_tx: mpsc::Sender<WorkerRequest>,
    cancel_token: CancellationToken,
    active_handles: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}
```

impl SubAgentPool {
    pub async fn new(config: &Config, ...) -> Self {
        // Creates worker loop task on initialization
        let pool = Self { ... };
        pool.start_worker_loop(request_rx);
        pool
    }

    pub async fn shutdown(&self) {
        // 1. Signal cancellation via cancel_token
        // 2. Drop request_tx to stop accepting new work
        // 3. Wait briefly (up to 1s) for cooperative cancellation
        // 4. Abort only as fallback
        // 5. Await all handles for clean shutdown
    }
}

impl SubAgentPool {
    pub async fn new(config: Config) -> Self {
```

**Key features**:
- Worker loop spawns on `new()` initialization
- Uses bounded semaphore for max 5 concurrent workers
- Broadcast channel for shutdown signaling
- Proper cleanup on `shutdown()` method

### SubAgentRequest
```rust
#[derive(Debug, Clone)]
pub struct SubAgentRequest {
    pub task_id: u64,
    pub prompt: String,
    pub agent: String,
    pub parent_id: Option<String>,
    pub denied_tools: Vec<String>,
    pub description: String,
    pub depth: usize,  // Current nesting depth
}
```

### TaskStatus (`src/tool/task.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Interrupted,  // Set when task is cancelled during shutdown
}
```

**Key change (Packet 2)**: `TaskStatus` now derives `PartialEq` for proper comparison. Tasks cancelled during shutdown are marked `Interrupted` (not `Failed`).

### TaskStore (`src/tool/task.rs`)

```rust
impl TaskStore {
    pub fn new() -> Self;
    pub async fn create_task(...) -> u64;
    pub async fn update_status(&self, id: u64, status: TaskStatus);
    pub async fn set_result(&self, id: u64, result: String);
    pub async fn set_failed(&self, id: u64, error: String);
    pub async fn set_interrupted(&self, id: u64, msg: String);  // NEW in Packet 2
    pub async fn get_task(&self, id: u64) -> Option<SubAgentTask>;
}
```

**Key change (Packet 2)**: Added `set_interrupted()` method that preserves `TaskStatus::Interrupted` without overwriting with `Failed`.

### SubAgentSpawner

```rust
#[derive(Clone)]
pub struct SubAgentSpawner {
    pool: SubAgentPool,
}

impl SubAgentSpawner {
    // Internal helper to enqueue request (checks max_depth, sends to channel)
    fn enqueue_request(&self, request: SubAgentRequest) -> Result<oneshot::Receiver<SubAgentResult>, String>;

    // Shared response handler
    async fn handle_response(
        task_id: u64,
        result: Result<SubAgentResult, tokio::sync::oneshot::error::RecvError>,
        task_store: Arc<TokioMutex<TaskStore>>,
    );

    // Async send - queues request and spawns handler task
    pub async fn send(&self, request: SubAgentRequest) -> Result<(), String> {
        let task_id = request.task_id;
        let response_rx = self.enqueue_request(request)?;
        let task_store = Arc::clone(&self.pool.task_store);

        tokio::spawn(async move {
            Self::handle_response(task_id, response_rx.await, task_store).await;
        });

        Ok(())
    }

    // Async send for use from async contexts (e.g., TaskTool::execute())
    pub async fn send_async(&self, request: SubAgentRequest) -> Result<(), String> {
        let task_id = request.task_id;
        let response_rx = self.enqueue_request(request)?;
        let task_store = Arc::clone(&self.pool.task_store);

        tokio::spawn(async move {
            Self::handle_response(task_id, response_rx.await, task_store).await;
        });

        Ok(())
    }
}
```

**Note**: Both `send` and `send_async` are now async and share the same implementation via `handle_response`. The separate implementations were deduplicated in 2026-05-22.

### SubAgentPool Configuration

```rust
pub struct SubagentConfig {
    pub max_concurrent: Option<usize>,  // Default: 5
    pub max_depth: Option<usize>,        // Default: 3
}

// Creating pool with config:
let config = Config {
    subagent: Some(SubagentConfig {
        max_concurrent: Some(2),
        max_depth: Some(3),
    }),
    ..Default::default()
};

let subagent_pool = SubAgentPool::new(
    &config,
    agents,
    provider_registry,
    session_store,
    Some(pool.clone()),
).await;

// Get spawner for sending requests
let spawner = subagent_pool.spawner();
```

### SubagentConfig

```rust
pub struct SubagentConfig {
    pub max_concurrent: Option<usize>,  // Default: 5
    pub max_depth: Option<usize>,        // Default: 3
}
```

## Integration Points

### TaskTool (`src/tool/task.rs`)

TaskTool can be created with a SubAgentPool:

```rust
pub fn new_with_pool(
    pool: Arc<SubAgentPool>,
    parent_session_id: Option<String>,
    denied_tools: Vec<String>,
) -> Self
```

The `spawner` field is `Option<SubAgentSpawner>` - when `None`, tasks are queued but not executed:

```rust
if let Some(ref spawner) = self.spawner {
    spawner.send(req)?;
} else {
    // Return pending status - no spawner configured
}
```

### Test Providers for Subagent Testing (Packet 10)

**SubagentTestProvider** - Deterministic provider for subagent tests:
```rust
#[derive(Clone)]
struct SubagentTestProvider {
    responses: Vec<Vec<ChatEvent>>,  // Scripted responses
    requests: Arc<Mutex<Vec<ChatRequest>>>,
    response_index: Arc<Mutex<usize>>,
    id: String,
}

// Register with ProviderRegistry for real subagent tests
let mut registry = ProviderRegistry::new();
registry.register(SubagentTestProvider::new("test", responses));
```

**ControlledProvider** - Uses Notify for concurrency testing:
```rust
#[derive(Clone)]
struct ControlledProvider {
    notify: Arc<tokio::sync::Notify>,
    running_count: Arc<AtomicUsize>,
    max_observed: Arc<AtomicUsize>,
    completed_count: Arc<AtomicUsize>,
}

// Used with barrier to prove no more than max_concurrent tasks run
// Verify max_observed <= max_concurrent
```

### Real Subagent Lifecycle Tests (Packet 10)

Key test patterns in `tests/subagent.rs`:

1. **test_real_subagent_lifecycle()** - Verifies task status transitions, result text stored, provider receives prompt, parent session ID propagated

2. **test_nonexistent_agent_sets_failed_result()** - Nonexistent agent should set failed result

3. **test_nonexistent_provider_sets_failed_result()** - Nonexistent provider should set failed result

4. **test_max_depth_returns_error_before_queueing()** - Depth >= max_depth returns error

5. **test_denied_tool_filtering()** - Denied tools excluded from provider request tool definitions

6. **test_concurrency_with_barrier()** - Uses Notify barriers to verify max_concurrent respected

```rust
// Example: Testing with send_async()
let result = spawner.send_async(request).await;
assert!(result.is_ok());

// Wait for completion
tokio::time::sleep(Duration::from_millis(500)).await;

// Verify provider received expected prompt
let requests = provider_requests.lock().await;
assert!(requests.iter().any(|req| /* check prompt */ ));
```

### Subagent Events (`src/bus/events.rs`)

Events for subagent lifecycle (handled in TUI):

```rust
SubagentStarted { session_id: String, task_id: u64, agent: String, description: String },
SubagentProgress { session_id: String, task_id: u64, agent: String, message: String },
SubagentCompleted { session_id: String, task_id: u64, agent: String, result_summary: String },
SubagentFailed { session_id: String, task_id: u64, agent: String, error: String },
```

### TUI Event Handling (`src/tui/mod.rs`)

The TUI handles subagent events in the event loop:

```rust
AppEvent::SubagentStarted { task_id: _, agent, description, .. } => {
    app.messages_state.toasts.add(Toast::info(&format!("Subagent '{}' started: {}", agent, description)));
}
AppEvent::SubagentProgress { task_id: _, agent, message, .. } => {
    app.messages_state.toasts.add(Toast::info(&format!("[{}] {}", agent, message)));
}
AppEvent::SubagentCompleted { task_id: _, agent, result_summary: _, .. } => {
    app.messages_state.toasts.add(Toast::success(&format!("Subagent '{}' completed", agent)));
}
AppEvent::SubagentFailed { task_id: _, agent, error, .. } => {
    app.messages_state.toasts.add(Toast::error(&format!("Subagent '{}' failed: {}", agent, error)));
}
```

### Task Persistence (SQLite)

Tasks are persisted to SQLite via migration v9 (`src/session/schema.rs`):

```sql
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
```

## Implementation Status

| Component | Status |
|-----------|--------|
| SubAgentPool struct | Implemented |
| SubAgentRequest/Result | Implemented |
| SubAgentSpawner | Implemented |
| TaskTool integration | Implemented |
| Task SQLite persistence | Implemented |
| Subagent events | Defined in bus/events.rs |
| **Worker task spawning** | **IMPLEMENTED** - bounded worker loop |
| **max_depth recursion limit** | **IMPLEMENTED** - SubAgentSpawner checks depth |
| TUI event handling | Implemented |
| **Packet 10: send_async()** | **✅ Complete** - Non-blocking async send |
| **Packet 10: Real lifecycle tests** | **✅ Complete** - SubagentTestProvider, ControlledProvider |
| **Packet 10: Concurrency tests** | **✅ Complete** - Barrier-based verification |

## TaskStore

In-memory task storage with SQLite persistence:

```rust
pub struct TaskStore {
    pool: Option<SqlitePool>,
    tasks: Mutex<HashMap<u64, SubAgentResult>>,
}

impl TaskStore {
    pub fn new(pool: Option<SqlitePool>) -> Self;
    pub async fn save_task(&self, task: &SubAgentResult) -> Result<(), AppError>;
    pub async fn load_tasks(&self, session_id: &str) -> Result<Vec<SubAgentResult>, AppError>;
    pub async fn update_status_in_db(&self, task_id: u64, status: TaskStatus) -> Result<(), AppError>;
}
```

## Worker Loop Implementation

Each request is now spawned as its own Tokio task with proper concurrency control:

```rust
fn start_worker_loop(&self, mut request_rx: mpsc::Receiver<WorkerRequest>) {
    let handle = tokio::spawn(async move {
        let sem = Arc::new(Semaphore::new(max_concurrent));

        loop {
            tokio::select! {
                biased;
                _ = shutdown_rx.recv() => break,
                Some(WorkerRequest { request, response_tx }) = request_rx.recv() => {
                    // Capture values for spawned task
                    let sem = Arc::clone(&sem);
                    let active_count = Arc::clone(&self.active_count);
                    let task_store = Arc::clone(&self.task_store);

                    // Spawn each request as its own task
                    tokio::spawn(async move {
                        // Permit acquired INSIDE the spawned task - held for duration
                        let _permit = sem.acquire().await.unwrap();
                        active_count.fetch_add(1, Ordering::SeqCst);

                        let result = run_subagent_task(request, task_store).await;
                        let _ = response_tx.send(result);

                        active_count.fetch_sub(1, Ordering::SeqCst);
                    });
                }
                else => break,
            }
        }
    });

    self.workers.lock().push(handle);
}
```

Key improvements (2026-04-30):
- Each request spawns its own Tokio task via `tokio::spawn()`
- Semaphore permit acquired INSIDE spawned task, not before
- `active_count` properly tracks concurrent tasks
- `response_tx` resolved exactly once per request

## Usage in Agent Loop

When TaskTool executes with `action == "spawn"`:

1. Creates a new task in TaskStore with status `Running`
2. Sends `SubAgentRequest` via spawner
3. Returns task ID to parent LLM

When TaskTool executes with `action == "get"`:

1. Retrieves task from TaskStore by ID
2. Returns formatted task result

## Important Notes

- **Concurrency limit**: Max 5 concurrent subagents via semaphore
- **Recursion limit**: max_depth (default: 3) prevents infinite nesting; `SubAgentSpawner::send()` rejects if depth exceeded
- **Session sharing**: Uses `parent_id` reference, not separate session_id
- **Denied tools**: Subagents inherit parent's denied_tools list
- **TaskStore**: In-memory task storage with full SQLite persistence - tasks survive application restarts. The `TaskStore` now has `pool: Option<SqlitePool>` field and methods `save_task`, `load_tasks`, and `update_status_in_db` for persistence.
- **Shutdown**: Call `pool.shutdown()` for clean termination
- **Shutdown semantics (Packet 2)**:
  - Uses `CancellationToken` for cooperative cancellation
  - RAII-style `ActiveCountGuard` ensures `active_count` is properly decremented
  - Waits briefly (up to 1s) for cooperative cancellation before aborting
  - Tasks interrupted during shutdown are marked `TaskStatus::Interrupted` (not `Failed`)
  - `set_interrupted()` method preserves Interrupted status
- **@ Mentions**: Can invoke subagents via `@agent_name` in TUI input (CompletionOverlay extension)