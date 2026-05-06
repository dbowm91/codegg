use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use thiserror::Error;
use tracing::{info, warn};

#[derive(Error, Debug)]
pub enum TeamError {
    #[error("team not found: {0}")]
    NotFound(String),

    #[error("agent not found in team: {0}")]
    AgentNotFound(String),

    #[error("invalid team config: {0}")]
    Invalid(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("message delivery failed: {0}")]
    DeliveryFailed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRole {
    pub name: String,
    pub instructions: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMessage {
    pub from: String,
    pub to: String,
    pub task: String,
    pub context: Vec<Message>,
    pub status: MessageStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MessageStatus {
    Pending,
    Delivered,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamStatus {
    pub team_name: String,
    pub agents: Vec<String>,
    pub pending_tasks: usize,
    pub completed_tasks: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub time_created: i64,
    pub time_updated: i64,
    pub data: MessageData,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MessageData {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(rename = "messageID")]
    #[serde(default)]
    pub message_id: String,
    #[serde(default)]
    pub parts: Vec<PartInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartInfo {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
    #[serde(flatten)]
    pub data: PartData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PartData {
    Text {
        text: String,
    },
    Reasoning {
        reasoning: String,
    },
    ToolCall {
        id: String,
        name: String,
        input: serde_json::Value,
        output: Option<String>,
    },
}

pub struct Team {
    name: String,
    agents: Vec<AgentRole>,
    inbox_dir: PathBuf,
    outbox_dir: PathBuf,
    status_file: PathBuf,
}

impl Team {
    pub fn new(name: String, agents: Vec<AgentRole>, base_dir: PathBuf) -> Self {
        let inbox_dir = base_dir.join(&name).join("inbox");
        let outbox_dir = base_dir.join(&name).join("outbox");
        let status_file = base_dir.join(&name).join("status.json");

        let team = Self {
            name,
            agents,
            inbox_dir,
            outbox_dir,
            status_file,
        };

        if let Err(e) = team.ensure_dirs() {
            warn!("failed to create team directories: {}", e);
        }

        team
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn agents(&self) -> &[AgentRole] {
        &self.agents
    }

    pub fn agent_names(&self) -> Vec<&str> {
        self.agents.iter().map(|a| a.name.as_str()).collect()
    }

    pub fn get_agent(&self, name: &str) -> Option<&AgentRole> {
        self.agents.iter().find(|a| a.name == name)
    }

    pub fn inbox_path(&self, agent_name: &str) -> PathBuf {
        self.inbox_dir.join(agent_name)
    }

    pub fn outbox_path(&self, agent_name: &str) -> PathBuf {
        self.outbox_dir.join(agent_name)
    }

    fn ensure_dirs(&self) -> Result<(), TeamError> {
        for agent in &self.agents {
            fs::create_dir_all(self.inbox_dir.join(&agent.name))?;
            fs::create_dir_all(self.outbox_dir.join(&agent.name))?;
        }
        Ok(())
    }

    pub fn send_message(&self, message: &TeamMessage) -> Result<(), TeamError> {
        if !self.agents.iter().any(|a| a.name == message.to) {
            return Err(TeamError::AgentNotFound(message.to.clone()));
        }

        let inbox = self.inbox_dir.join(&message.to);
        let filename = format!(
            "{}_{}.json",
            message.timestamp(),
            sanitize_filename(&message.from)
        );
        let filepath = inbox.join(&filename);

        let json = serde_json::to_string_pretty(message)?;
        fs::write(&filepath, json)?;

        info!(
            "team message sent from {} to {} via {}",
            message.from,
            message.to,
            filepath.display()
        );

        Ok(())
    }

    pub fn deliver_messages(&self, agent_name: &str) -> Result<Vec<TeamMessage>, TeamError> {
        if !self.agents.iter().any(|a| a.name == agent_name) {
            return Err(TeamError::AgentNotFound(agent_name.to_string()));
        }

        let inbox = self.inbox_dir.join(agent_name);
        let mut messages = Vec::new();

        if !inbox.is_dir() {
            return Ok(messages);
        }

        for entry in fs::read_dir(&inbox)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            match fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str::<TeamMessage>(&content) {
                    Ok(mut msg) => {
                        msg.status = MessageStatus::Delivered;
                        let updated = serde_json::to_string_pretty(&msg)?;
                        fs::write(&path, updated)?;
                        messages.push(msg);
                    }
                    Err(e) => {
                        warn!("failed to parse message {}: {}", path.display(), e);
                    }
                },
                Err(e) => {
                    warn!("failed to read message file {}: {}", path.display(), e);
                }
            }
        }

        messages.sort_by_key(|a| a.timestamp());

        Ok(messages)
    }

    pub fn mark_completed(&self, agent_name: &str, message: &TeamMessage) -> Result<(), TeamError> {
        let inbox = self.inbox_dir.join(agent_name);
        let filename = format!(
            "{}_{}.json",
            message.timestamp(),
            sanitize_filename(&message.from)
        );
        let filepath = inbox.join(&filename);

        if filepath.exists() {
            let mut msg = message.clone();
            msg.status = MessageStatus::Completed;
            let json = serde_json::to_string_pretty(&msg)?;
            fs::write(&filepath, json)?;
        }

        Ok(())
    }

    pub fn get_status(&self) -> Result<TeamStatus, TeamError> {
        let mut pending = 0;
        let mut completed = 0;

        for agent in &self.agents {
            let inbox = self.inbox_dir.join(&agent.name);
            if inbox.is_dir() {
                if let Ok(entries) = fs::read_dir(&inbox) {
                    pending += entries.count();
                }
            }

            let outbox = self.outbox_dir.join(&agent.name);
            if outbox.is_dir() {
                if let Ok(entries) = fs::read_dir(&outbox) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().and_then(|e| e.to_str()) == Some("json") {
                            if let Ok(content) = fs::read_to_string(&path) {
                                if let Ok(msg) = serde_json::from_str::<TeamMessage>(&content) {
                                    if msg.status == MessageStatus::Completed {
                                        completed += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(TeamStatus {
            team_name: self.name.clone(),
            agents: self.agents.iter().map(|a| a.name.clone()).collect(),
            pending_tasks: pending,
            completed_tasks: completed,
        })
    }

    pub fn save_status(&self) -> Result<(), TeamError> {
        let status = self.get_status()?;
        let json = serde_json::to_string_pretty(&status)?;
        fs::write(&self.status_file, json)?;
        Ok(())
    }

    pub fn load_status(&self) -> Result<TeamStatus, TeamError> {
        let content = fs::read_to_string(&self.status_file)?;
        let status: TeamStatus = serde_json::from_str(&content)?;
        Ok(status)
    }
}

impl TeamMessage {
    pub fn new(from: String, to: String, task: String) -> Self {
        Self {
            from,
            to,
            task,
            context: Vec::new(),
            status: MessageStatus::Pending,
        }
    }

    pub fn with_context(mut self, context: Vec<Message>) -> Self {
        self.context = context;
        self
    }

    pub fn timestamp(&self) -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        format!("{}", duration.as_millis())
    }
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("agent-1"), "agent-1");
        assert_eq!(sanitize_filename("agent.1"), "agent_1");
        assert_eq!(sanitize_filename("agent@1"), "agent_1");
    }

    #[test]
    fn test_team_creation() {
        let tmp = tempdir().unwrap();
        let agents = vec![
            AgentRole {
                name: "planner".to_string(),
                instructions: "Plan tasks".to_string(),
                capabilities: vec!["planning".to_string()],
            },
            AgentRole {
                name: "builder".to_string(),
                instructions: "Build things".to_string(),
                capabilities: vec!["coding".to_string()],
            },
        ];

        let team = Team::new("test-team".to_string(), agents, tmp.path().to_path_buf());
        assert_eq!(team.name(), "test-team");
        assert_eq!(team.agent_names(), vec!["planner", "builder"]);
    }

    #[test]
    fn test_team_message_new() {
        let msg = TeamMessage::new("a".to_string(), "b".to_string(), "test task".to_string());
        assert_eq!(msg.from, "a");
        assert_eq!(msg.to, "b");
        assert_eq!(msg.task, "test task");
        assert_eq!(msg.status, MessageStatus::Pending);
        assert!(msg.context.is_empty());
    }

    #[test]
    fn test_team_message_with_context() {
        let msg = TeamMessage::new("a".to_string(), "b".to_string(), "test".to_string())
            .with_context(vec![Message {
                id: "1".to_string(),
                session_id: "s1".to_string(),
                time_created: 0,
                time_updated: 0,
                data: MessageData::default(),
            }]);

        assert_eq!(msg.context.len(), 1);
    }

    #[test]
    fn test_send_and_deliver_message() {
        let tmp = tempdir().unwrap();
        let agents = vec![AgentRole {
            name: "worker".to_string(),
            instructions: "Work".to_string(),
            capabilities: vec![],
        }];

        let team = Team::new("test-team".to_string(), agents, tmp.path().to_path_buf());

        let msg = TeamMessage::new(
            "coordinator".to_string(),
            "worker".to_string(),
            "do work".to_string(),
        );
        team.send_message(&msg).unwrap();

        let delivered = team.deliver_messages("worker").unwrap();
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].task, "do work");
        assert_eq!(delivered[0].status, MessageStatus::Delivered);
    }

    #[test]
    fn test_send_to_unknown_agent() {
        let tmp = tempdir().unwrap();
        let agents = vec![AgentRole {
            name: "worker".to_string(),
            instructions: "Work".to_string(),
            capabilities: vec![],
        }];

        let team = Team::new("test-team".to_string(), agents, tmp.path().to_path_buf());

        let msg = TeamMessage::new(
            "coordinator".to_string(),
            "unknown".to_string(),
            "task".to_string(),
        );
        assert!(team.send_message(&msg).is_err());
    }

    #[test]
    fn test_team_status() {
        let tmp = tempdir().unwrap();
        let agents = vec![
            AgentRole {
                name: "a".to_string(),
                instructions: "".to_string(),
                capabilities: vec![],
            },
            AgentRole {
                name: "b".to_string(),
                instructions: "".to_string(),
                capabilities: vec![],
            },
        ];

        let team = Team::new("test-team".to_string(), agents, tmp.path().to_path_buf());

        let status = team.get_status().unwrap();
        assert_eq!(status.team_name, "test-team");
        assert_eq!(status.agents.len(), 2);
    }

    #[test]
    fn test_message_timestamp() {
        let msg1 = TeamMessage::new("a".to_string(), "b".to_string(), "task".to_string());
        let msg2 = TeamMessage::new("a".to_string(), "b".to_string(), "task".to_string());
        assert!(msg1.timestamp() <= msg2.timestamp());
    }
}
