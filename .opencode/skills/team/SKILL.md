---
name: team
description: Multi-agent teams via file-based inbox communication
tags: [team, multi-agent, collaboration, inbox]
---

Use the `/skill:team` command to load context about multi-agent team collaboration.

## Overview

Multi-agent teams enable independent agents to collaborate via file-based inboxes. Each agent has its own inbox and outbox directory for message passing.

## File Structure

```
.opencode/team/
  {team_name}/
    inbox/
      {agent_name}/
        pending/
        delivered/
    outbox/
      {agent_name}/
        pending/
        delivered/
    status.json
```

## Message Format

```rust
pub struct TeamMessage {
    pub from: String,           // Sender agent name
    pub to: String,             // Recipient agent name
    pub task: String,           // Task description
    pub context: Vec<Message>,  // Conversation context
    pub status: MessageStatus,  // Pending, Delivered, Completed, Failed
}
```

## Key Structs

```rust
pub struct Team {
    name: String,
    agents: Vec<AgentRole>,
    inbox_dir: PathBuf,
}

pub struct AgentRole {
    name: String,
    instructions: String,
    capabilities: Vec<String>,
}
```

## Module

`src/agent/team.rs` contains:
- `Team` struct for team management
- `AgentRole` for agent definitions
- `TeamMessage` for inter-agent communication
- `MessageStatus` enum for tracking

## Usage

```rust
let team = Team::new("dev-team".to_string());
team.add_agent(AgentRole {
    name: "reviewer".to_string(),
    instructions: "Review code changes".to_string(),
    capabilities: vec!["read".to_string(), "glob".to_string()],
});

// Send message to agent
team.send_message("reviewer", "review", messages).await;

// Check inbox
let messages = team.deliver_messages("reviewer").await;
```

## Status Tracking

The `status.json` file tracks team-level status:
- Active agents
- Pending tasks
- Completed work