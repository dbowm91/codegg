# Skills Module

The `skills` module provides specialized capabilities activated via commands.

## Overview

**Location**: `src/skills/`

**Key Responsibilities**:
- Skill loading from files
- Skill activation via `/skill:` commands
- System prompt augmentation with skills

## Key Types

### Skill

```rust
pub struct Skill {
    pub name: String,
    pub description: String,
    pub body: String,
    pub tags: Vec<String>,
}
```

### SkillIndex

```rust
pub struct SkillIndex {
    skills: RwLock<HashMap<String, Skill>>,
}

impl SkillIndex {
    pub fn load_from_dir(&self, path: &Path) -> Result<()>;
    pub fn get(&self, name: &str) -> Option<Skill>;
    pub fn search(&self, query: &str) -> Vec<Skill>;
}
```

## Skill File Format

Skills are stored as markdown files:

```markdown
---
name: git
description: Advanced git operations
tags: [vcs, git]
---

# Git Skill

You have access to advanced git operations. Use these commands:

## Branches
- `git branch -a` - List all branches
- `git checkout -b <name>` - Create and switch to branch

## Commits
- `git commit -am "message"` - Stage and commit
- `git rebase -i HEAD~n` - Interactive rebase

## Tips
- Always check `git status` before committing
```

Frontmatter contains metadata; body contains instructions.

## Skill Loading

Skills loaded from:
- `~/.config/codegg/skills/` (global)
- `.codegg/skills/` (project)

## Activation

User activates skill via command:

```
/skill:git
```

Skill instructions appended to system prompt.

## Usage in Agent

```rust
pub fn build_system_prompt(config: &Config, active_skills: &[String]) -> String {
    let mut prompt = config.agent.system_prompt.clone();

    let skill_index = SkillIndex::new();
    for skill_name in active_skills {
        if let Some(skill) = skill_index.get(skill_name) {
            prompt.push_str("\n\n");
            prompt.push_str(&skill.body);
        }
    }

    prompt
}
```

## See Also

- [tool.md](tool.md) - `/skill:` tool
