use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::info;

use crate::agent::team::{AgentRole, Team, TeamError, TeamMessage};

pub const TEAM_BASE_DIR: &str = ".opencode/team";

#[derive(Debug, Clone)]
pub struct TeamConfig {
    pub name: String,
    pub agents: Vec<TeamAgentConfig>,
    pub base_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct TeamAgentConfig {
    pub name: String,
    pub instructions: String,
    pub capabilities: Vec<String>,
}

impl From<TeamConfig> for Vec<AgentRole> {
    fn from(config: TeamConfig) -> Self {
        config
            .agents
            .into_iter()
            .map(|a| AgentRole {
                name: a.name,
                instructions: a.instructions,
                capabilities: a.capabilities,
            })
            .collect()
    }
}

pub struct TeamManager {
    teams: RwLock<HashMap<String, Arc<Team>>>,
    team_configs: RwLock<HashMap<String, TeamConfig>>,
    shutdown_txs: RwLock<HashMap<String, broadcast::Sender<()>>>,
}

impl TeamManager {
    pub fn new() -> Self {
        Self {
            teams: RwLock::new(HashMap::new()),
            team_configs: RwLock::new(HashMap::new()),
            shutdown_txs: RwLock::new(HashMap::new()),
        }
    }

    pub async fn create_team(&self, config: TeamConfig) -> Result<Arc<Team>, TeamError> {
        let name = config.name.clone();
        let base_dir = config.base_dir.join(TEAM_BASE_DIR);

        let agents: Vec<AgentRole> = config.clone().into();
        let team = Arc::new(Team::new(name.clone(), agents, base_dir));

        {
            let mut teams = self.teams.write().await;
            teams.insert(config.name.clone(), team.clone());
        }
        {
            let mut configs = self.team_configs.write().await;
            configs.insert(config.name.clone(), config);
        }
        {
            let mut txs = self.shutdown_txs.write().await;
            let (shutdown_tx, _) = broadcast::channel(1);
            txs.insert(name.clone(), shutdown_tx);
        }

        info!("team '{}' created successfully", name);
        Ok(team)
    }

    pub async fn get_team(&self, name: &str) -> Option<Arc<Team>> {
        let teams = self.teams.read().await;
        teams.get(name).cloned()
    }

    async fn require_team(&self, name: &str) -> Result<Arc<Team>, TeamError> {
        self.get_team(name)
            .await
            .ok_or_else(|| TeamError::NotFound(name.to_string()))
    }

    pub async fn list_teams(&self) -> Vec<String> {
        let teams = self.teams.read().await;
        teams.keys().cloned().collect()
    }

    pub async fn shutdown_team(&self, name: &str) -> Result<(), TeamError> {
        let tx = {
            let txs = self.shutdown_txs.write().await;
            txs.get(name).cloned()
        };

        if let Some(tx) = tx {
            let _ = tx.send(());
            info!("shutdown signal sent to team '{}'", name);
        }

        {
            let mut teams = self.teams.write().await;
            teams.remove(name);
        }
        {
            let mut configs = self.team_configs.write().await;
            configs.remove(name);
        }
        {
            let mut txs = self.shutdown_txs.write().await;
            txs.remove(name);
        }

        Ok(())
    }

    pub async fn send_message(&self, team_name: &str, message: &TeamMessage) -> Result<(), TeamError> {
        self.require_team(team_name).await?.send_message(message)
    }

    pub async fn deliver_messages(&self, team_name: &str, agent_name: &str) -> Result<Vec<TeamMessage>, TeamError> {
        self.require_team(team_name).await?.deliver_messages(agent_name)
    }

    pub async fn get_team_status(&self, team_name: &str) -> Result<crate::agent::team::TeamStatus, TeamError> {
        self.require_team(team_name).await?.get_status()
    }
}

impl Default for TeamManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct TaskDependency {
    pub task_id: String,
    pub depends_on: Vec<String>,
}

pub struct SharedTaskList {
    tasks: RwLock<HashMap<String, TaskDependency>>,
    completed: RwLock<HashMap<String, bool>>,
}

impl SharedTaskList {
    pub fn new() -> Self {
        Self {
            tasks: RwLock::new(HashMap::new()),
            completed: RwLock::new(HashMap::new()),
        }
    }

    pub async fn add_task(&self, task_id: String, depends_on: Vec<String>) {
        let mut tasks = self.tasks.write().await;
        tasks.insert(
            task_id.clone(),
            TaskDependency {
                task_id,
                depends_on,
            },
        );
    }

    pub async fn mark_completed(&self, task_id: &str) {
        let mut completed = self.completed.write().await;
        completed.insert(task_id.to_string(), true);
    }

    pub async fn is_completed(&self, task_id: &str) -> bool {
        let completed = self.completed.read().await;
        completed.get(task_id).copied().unwrap_or(false)
    }

    pub async fn can_start(&self, task_id: &str) -> bool {
        let tasks = self.tasks.read().await;
        if let Some(dep) = tasks.get(task_id) {
            for dep_id in &dep.depends_on {
                if !self.is_completed(dep_id).await {
                    return false;
                }
            }
        }
        true
    }

    pub async fn get_pending_tasks(&self) -> Vec<String> {
        let tasks = self.tasks.read().await;
        let completed = self.completed.read().await;
        tasks
            .iter()
            .filter(|(id, _)| !completed.contains_key(*id))
            .map(|(id, _)| id.clone())
            .collect()
    }
}

impl Default for SharedTaskList {
    fn default() -> Self {
        Self::new()
    }
}

pub struct IdleNotifier {
    listeners: RwLock<HashMap<String, broadcast::Sender<()>>>,
}

impl IdleNotifier {
    pub fn new() -> Self {
        Self {
            listeners: RwLock::new(HashMap::new()),
        }
    }

    pub async fn register(&self, agent_name: String) -> broadcast::Receiver<()> {
        let (tx, rx) = broadcast::channel(1);
        let mut listeners = self.listeners.write().await;
        listeners.insert(agent_name, tx);
        rx
    }

    pub async fn notify_idle(&self, agent_name: &str) {
        let listeners = self.listeners.read().await;
        if let Some(tx) = listeners.get(agent_name) {
            let _ = tx.send(());
        }
    }
}

impl Default for IdleNotifier {
    fn default() -> Self {
        Self::new()
    }
}

pub struct GracefulShutdown {
    shutdown_tx: broadcast::Sender<TeamShutdownSignal>,
    teams: Arc<TeamManager>,
}

#[derive(Debug, Clone)]
pub struct TeamShutdownSignal {
    pub team_name: String,
    pub reason: String,
}

impl GracefulShutdown {
    pub fn new(teams: Arc<TeamManager>) -> (Self, broadcast::Receiver<TeamShutdownSignal>) {
        let (shutdown_tx, rx) = broadcast::channel(1);
        (
            Self {
                shutdown_tx,
                teams,
            },
            rx,
        )
    }

    pub async fn shutdown_all(&self) -> Result<(), TeamError> {
        let team_names = self.teams.list_teams().await;
        for name in team_names {
            self.teams.shutdown_team(&name).await?;
        }
        Ok(())
    }

    pub async fn shutdown_team(&self, team_name: &str) -> Result<(), TeamError> {
        let signal = TeamShutdownSignal {
            team_name: team_name.to_string(),
            reason: "graceful shutdown".to_string(),
        };
        let _ = self.shutdown_tx.send(signal);
        self.teams.shutdown_team(team_name).await
    }
}

pub struct TeamCreateTool {
    manager: Arc<TeamManager>,
    base_dir: PathBuf,
}

impl TeamCreateTool {
    pub fn new(manager: Arc<TeamManager>, base_dir: PathBuf) -> Self {
        Self { manager, base_dir }
    }
}

#[async_trait::async_trait]
impl crate::tool::Tool for TeamCreateTool {
    fn name(&self) -> &str {
        "team_create"
    }

    fn description(&self) -> &str {
        "Create a team of agents for collaborative task execution"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Unique name for the team"
                },
                "agents": {
                    "type": "array",
                    "description": "List of agents to add to the team",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {
                                "type": "string",
                                "description": "Agent name"
                            },
                            "instructions": {
                                "type": "string",
                                "description": "Instructions for the agent"
                            },
                            "capabilities": {
                                "type": "array",
                                "description": "List of capabilities",
                                "items": { "type": "string" }
                            }
                        },
                        "required": ["name", "instructions"]
                    }
                }
            },
            "required": ["name", "agents"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let name = input["name"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'name' parameter".to_string()))?
            .to_string();

        let agents_json = input["agents"]
            .as_array()
            .ok_or_else(|| ToolError::Execution("missing 'agents' parameter".to_string()))?;

        let agents: Vec<TeamAgentConfig> = agents_json
            .iter()
            .map(|a| {
                Ok(TeamAgentConfig {
                    name: a["name"]
                        .as_str()
                        .ok_or_else(|| ToolError::Execution("missing agent 'name'".to_string()))?
                        .to_string(),
                    instructions: a["instructions"]
                        .as_str()
                        .ok_or_else(|| ToolError::Execution("missing agent 'instructions'".to_string()))?
                        .to_string(),
                    capabilities: a["capabilities"]
                        .as_array()
                        .map(|c| c.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                        .unwrap_or_default(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let config = TeamConfig {
            name: name.clone(),
            agents,
            base_dir: self.base_dir.clone(),
        };

        self.manager
            .create_team(config)
            .await
            .map_err(|e| ToolError::Execution(format!("failed to create team: {}", e)))?;

        Ok(format!("team '{}' created successfully", name))
    }
}

pub struct SendMessageTool {
    manager: Arc<TeamManager>,
}

impl SendMessageTool {
    pub fn new(manager: Arc<TeamManager>) -> Self {
        Self { manager }
    }
}

#[async_trait::async_trait]
impl crate::tool::Tool for SendMessageTool {
    fn name(&self) -> &str {
        "send_message"
    }

    fn description(&self) -> &str {
        "Send a message to an agent in a team"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "team": {
                    "type": "string",
                    "description": "Name of the team"
                },
                "to": {
                    "type": "string",
                    "description": "Recipient agent name"
                },
                "task": {
                    "type": "string",
                    "description": "Task description"
                },
                "from": {
                    "type": "string",
                    "description": "Sender agent name (default: coordinator)"
                }
            },
            "required": ["team", "to", "task"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let team_name = input["team"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'team' parameter".to_string()))?
            .to_string();

        let to = input["to"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'to' parameter".to_string()))?
            .to_string();

        let task = input["task"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'task' parameter".to_string()))?
            .to_string();

        let from = input["from"]
            .as_str()
            .unwrap_or("coordinator")
            .to_string();

        let message = TeamMessage::new(from, to.clone(), task);

        self.manager
            .send_message(&team_name, &message)
            .await
            .map_err(|e| ToolError::Execution(format!("failed to send message: {}", e)))?;

        Ok(format!(
            "message sent to '{}' in team '{}'",
            to, team_name
        ))
    }
}

pub struct ListMessagesTool {
    manager: Arc<TeamManager>,
}

impl ListMessagesTool {
    pub fn new(manager: Arc<TeamManager>) -> Self {
        Self { manager }
    }
}

#[async_trait::async_trait]
impl crate::tool::Tool for ListMessagesTool {
    fn name(&self) -> &str {
        "list_messages"
    }

    fn description(&self) -> &str {
        "List pending messages for an agent in a team"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "team": {
                    "type": "string",
                    "description": "Name of the team"
                },
                "agent": {
                    "type": "string",
                    "description": "Agent name to check messages for"
                }
            },
            "required": ["team", "agent"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let team_name = input["team"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'team' parameter".to_string()))?
            .to_string();

        let agent_name = input["agent"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'agent' parameter".to_string()))?
            .to_string();

        let messages = self
            .manager
            .deliver_messages(&team_name, &agent_name)
            .await
            .map_err(|e| ToolError::Execution(format!("failed to list messages: {}", e)))?;

        if messages.is_empty() {
            return Ok("no pending messages".to_string());
        }

        let list: Vec<String> = messages
            .iter()
            .map(|m| format!("{}: {} (from {})", m.to, m.task, m.from))
            .collect();

        Ok(list.join("\n"))
    }
}

pub struct TeamStatusTool {
    manager: Arc<TeamManager>,
}

impl TeamStatusTool {
    pub fn new(manager: Arc<TeamManager>) -> Self {
        Self { manager }
    }
}

#[async_trait::async_trait]
impl crate::tool::Tool for TeamStatusTool {
    fn name(&self) -> &str {
        "team_status"
    }

    fn description(&self) -> &str {
        "Get status of a team"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "team": {
                    "type": "string",
                    "description": "Name of the team"
                }
            },
            "required": ["team"]
        })
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let team_name = input["team"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'team' parameter".to_string()))?
            .to_string();

        let status = self
            .manager
            .get_team_status(&team_name)
            .await
            .map_err(|e| ToolError::Execution(format!("failed to get team status: {}", e)))?;

        Ok(format!(
            "Team: {}\nAgents: {}\nPending: {}\nCompleted: {}",
            status.team_name,
            status.agents.join(", "),
            status.pending_tasks,
            status.completed_tasks
        ))
    }
}

pub struct ListTeamsTool {
    manager: Arc<TeamManager>,
}

impl ListTeamsTool {
    pub fn new(manager: Arc<TeamManager>) -> Self {
        Self { manager }
    }
}

#[async_trait::async_trait]
impl crate::tool::Tool for ListTeamsTool {
    fn name(&self) -> &str {
        "list_teams"
    }

    fn description(&self) -> &str {
        "List all teams"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _input: serde_json::Value) -> Result<String, ToolError> {
        let teams = self.manager.list_teams().await;
        if teams.is_empty() {
            return Ok("no teams".to_string());
        }
        Ok(teams.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test(flavor = "current_thread")]
    async fn test_team_manager_creation() {
        let tmp = tempdir().unwrap();
        let manager = Arc::new(TeamManager::new());

        let config = TeamConfig {
            name: "test-team".to_string(),
            agents: vec![TeamAgentConfig {
                name: "worker".to_string(),
                instructions: "do work".to_string(),
                capabilities: vec!["coding".to_string()],
            }],
            base_dir: tmp.path().to_path_buf(),
        };

        let team = manager.create_team(config).await.unwrap();
        assert_eq!(team.name(), "test-team");
        assert_eq!(team.agent_names(), vec!["worker"]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_get_nonexistent_team() {
        let manager = Arc::new(TeamManager::new());
        assert!(manager.get_team("nonexistent").await.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_shared_task_list() {
        let task_list = Arc::new(SharedTaskList::new());

        task_list.add_task("task1".to_string(), vec![]).await;
        task_list.add_task("task2".to_string(), vec!["task1".to_string()]).await;

        assert!(task_list.can_start("task1").await);
        assert!(!task_list.can_start("task2").await);

        task_list.mark_completed("task1").await;
        assert!(task_list.can_start("task2").await);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_idle_notifier() {
        let notifier = Arc::new(IdleNotifier::new());
        let rx = notifier.register("agent1".to_string()).await;
        notifier.notify_idle("agent1").await;
        assert!(rx.recv().await.is_ok());
    }
}
