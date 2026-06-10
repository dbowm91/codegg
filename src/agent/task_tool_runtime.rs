use std::sync::Arc;
use tokio::sync::Mutex;

use crate::agent::worker::SubAgentSpawner;
use crate::tool::task::TaskStore;

/// Narrow runtime DTO for constructing the task/subagent tool.
///
/// Extracts the two capabilities a `TaskTool` needs from `SubAgentPool`
/// without coupling the tool factory to the full pool type.
pub struct TaskToolRuntime {
    store: Arc<Mutex<TaskStore>>,
    spawner: Option<SubAgentSpawner>,
}

impl TaskToolRuntime {
    pub fn new(store: Arc<Mutex<TaskStore>>, spawner: Option<SubAgentSpawner>) -> Self {
        Self { store, spawner }
    }

    pub fn from_subagent_pool(pool: &Arc<crate::agent::worker::SubAgentPool>) -> Self {
        Self {
            store: pool.task_store(),
            spawner: Some(pool.spawner()),
        }
    }

    pub fn store(&self) -> Arc<Mutex<TaskStore>> {
        self.store.clone()
    }

    pub fn spawner(&self) -> Option<SubAgentSpawner> {
        self.spawner.clone()
    }
}
