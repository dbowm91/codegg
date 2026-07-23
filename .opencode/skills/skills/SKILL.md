---
name: skills
description: Skills module for specialized capabilities activated via /skill: commands
version: 1.1.0
tags:
  - skills
  - module
  - loading
  - activation
---

# Skills Module Guide

This skill covers the skills system in codegg for loading and activating specialized capabilities.

## Overview

The `skills` module (`src/skills/mod.rs`) provides:
- Skill loading from markdown files with YAML frontmatter
- Skill activation via `/skill:<name>` commands
- System prompt augmentation with skill content

This repository also keeps the agent-facing maintenance copy of those skill docs in `.skills/`. Keep that directory aligned with the runtime behavior documented here.

## Key Types

### Skill

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub tags: Vec<String>,
    pub body: String,
    pub source: PathBuf,
}
```

### SkillIndex

```rust
pub struct SkillIndex {
    skills: Vec<Skill>,
}

impl SkillIndex {
    pub fn new() -> Self;
    pub async fn load(&mut self, project_dir: &str) -> Result<(), AppError>;
    pub fn get(&self, name: &str) -> Option<&Skill>;
    pub fn list(&self) -> &[Skill];
    pub fn find_matching(&self, query: &str) -> Vec<&Skill>;
    pub fn build_system_prompt(&self) -> String;
    pub fn activate(&self, name: &str) -> Option<String>;
}
```

## Skill File Format

Skills are stored as markdown files with YAML frontmatter:

```markdown
---
name: git
description: Advanced git operations
version: 1.0.0
tags: [vcs, git]
---

# Git Skill

You have access to advanced git operations...

## Branches
- `git branch -a` - List all branches
```

## Skill Loading

Skills are loaded from two locations:
- **Global**: `~/.config/codegg/skills/`
- **Project**: `.opencode/skills/` (in project directory; legacy `.codegg/skills/` remains a compatibility path)
- **Workspace mirror**: `.agents/skills` (symlink to `.opencode/skills/` for workspace-root convenience)

Loading is done recursively:
- Direct `.md` files are loaded as skills
- Directories containing `SKILL.md` are loaded as skills (directory name becomes skill name)

## Usage in Code

### Loading Skills (main.rs)

```rust
let mut skills = SkillIndex::new();
skills.load(&project_dir).await?;
```

### Activating a Skill

```rust
// From CLI --session flag
if let Some(skill_body) = skills.activate(skill_name) {
    app.prompt_state.prompt.set_text(skill_body);
}
```

### SkillTool for Runtime Loading

The `SkillTool` (`src/tool/skill.rs`) provides runtime skill loading:

```rust
// Execute with /skill:<name>
let result = skill_tool.execute(json!({"name": "git"})).await;
// Returns JSON with name, description, body, and resources
```

### Resource Enumeration

The `list_skill_resources()` function (`src/tool/skill.rs:67`) scans a skill's directory for additional resource files:

```rust
async fn list_skill_resources(skill_path: &Path) -> Vec<String>
```

**Behavior:**
- If `skill_path` is a file, uses its parent directory
- If `skill_path` is a directory, uses it directly
- Returns empty `Vec` if path is not a valid directory
- Excludes `SKILL.md` from results
- Returns file/directory names (not full paths)

**Used by:** `SkillTool::execute()` to include resource list in tool output

## Integration Points

| Location | Usage |
|----------|-------|
| `main.rs:930` | Creates and loads `SkillIndex` at startup |
| `src/tool/skill.rs` | Provides `/skill:` tool for runtime loading |
| `src/agent/prompt.rs` | `assemble_system_prompt()` accepts `skills: &[String]` parameter |

## Key Methods

### `load(project_dir)`

Async method that loads skills from both global and project directories. Clears existing skills before loading.

### `get(name)`

Returns `Option<&Skill>` for exact name match.

### `find_matching(query)`

Searches name, description, and tags for fuzzy matching. Returns `Vec<&Skill>`.

### `activate(name)`

Returns `Option<String>` containing the skill body for a given name.

### `build_system_prompt()`

Generates a markdown listing of all available skills. Returns empty string if no skills loaded.

## Skills vs System Prompts

- **Skills**: Loaded on-demand via `/skill:` command, contain specialized instructions
- **System Prompts**: Agent-level instructions baked into `Agent.system_prompt`
- **Instructions**: Global instructions from `config.instructions` applied to all agents

## Adding New Skills

1. Create `~/.config/codegg/skills/<name>/SKILL.md` or `.opencode/skills/<name>.md`
2. Use YAML frontmatter with `name`, `description`, and optional `version` and `tags`
3. Add skill body content after frontmatter

Example:
```markdown
---
name: docker
description: Docker and container operations
version: 1.0.0
tags: [containers, devops]
---

# Docker Skill

You have access to Docker commands for container management...
```
