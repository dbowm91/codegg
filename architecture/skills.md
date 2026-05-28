# Skills Module

The `skills` module provides specialized capabilities activated via `/skill:` commands.

## Overview

**Location**: `src/skills/mod.rs`

**Key Responsibilities**:
- Skill loading from markdown files with YAML frontmatter
- Skill activation via `/skill:<name>` commands
- System prompt augmentation with skill content

The `.skills/` directory at the repository root is a repo-maintained copy of agent-facing skill documentation. It mirrors the skill files that the runtime loader reads from `~/.config/codegg/skills/` (global) and `.codegg/skills/` (project). The runtime `SkillIndex::load()` loads from the global and project directories, not from `.skills/`. The `.skills/` directory exists for version-controlled documentation and is not directly consumed by the runtime skill loader.

## Key Types

### Skill

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub version: Option<String>,    // not in older docs
    pub tags: Vec<String>,
    pub body: String,
    pub source: PathBuf,            // not in older docs
}
```

### SkillIndex

```rust
pub struct SkillIndex {
    skills: Vec<Skill>,
}

impl Default for SkillIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillIndex {
    pub fn new() -> Self {
        Self { skills: Vec::new() }
    }
    pub async fn load(&mut self, project_dir: &str) -> Result<(), AppError>;
    pub fn get(&self, name: &str) -> Option<&Skill>;
    pub fn list(&self) -> &[Skill];
    pub fn find_matching(&self, query: &str) -> Vec<&Skill>;
    pub fn build_system_prompt(&self) -> String;
    pub fn activate(&self, name: &str) -> Option<String>;
}

### SkillIndex Methods

- `get(name)` - Retrieves a skill by exact name match (case-sensitive)
- `list()` - Returns all skills as a slice
- `find_matching(query)` - Case-insensitive partial match across name, description, and tags
- `activate(name)` - Retrieves skill body by exact name match (case-sensitive)

### SkillFrontmatter

Internal struct for parsing YAML frontmatter:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SkillFrontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
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

You have access to advanced git operations. Use these commands:

## Branches
- `git branch -a` - List all branches
- `git checkout -b <name>` - Create and switch to branch
```

## Skill Loading

Skills are loaded from two locations:
- **Global**: `~/.config/codegg/skills/` (via `dirs::config_dir()`)
  - On macOS: `~/Library/Application Support/codegg/skills/`
  - On Linux: `~/.config/codegg/skills/`
- **Project**: `.codegg/skills/` (in project directory)

Loading is recursive:
- Direct `.md` files are loaded as skills
- Directories containing `SKILL.md` are loaded as skills

## Activation

User activates skill via `/skill:` command:

```
/skill:git
```

The `SkillTool` (`src/tool/skill.rs`) handles runtime skill loading:

```rust
// Execute with /skill:<name>
let result = skill_tool.execute(json!({"name": "git"})).await;
// Returns JSON with name, description, body, and resources (list of resource file names in skill directory)
```

## Usage in Agent

```rust
// In main.rs - load at startup
let mut skills = SkillIndex::new();
skills.load(&project_dir).await?;

// Activate from CLI flag
if let Some(skill_body) = skills.activate(skill_name) {
    app.prompt_state.prompt.set_text(skill_body);
}
```

The `assemble_system_prompt()` in `src/agent/prompt.rs` accepts skill names but skill bodies are injected separately via prompt modification.

## See Also

- [tool.md](tool.md) - `/skill:` tool
- `.skills/skills/SKILL.md` - Detailed skill system guide
