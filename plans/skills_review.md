# Skills Module Architecture Review

**Reviewed**: 2026-05-26
**Source**: `architecture/skills.md`
**Verified Against**: `src/skills/mod.rs`, `src/tool/skill.rs`, `src/agent/prompt.rs`

---

## Summary

**Status**: Generally accurate with minor discrepancies noted below.

---

## Findings

### 1. Skill Struct (Line 22-29) ✅ VERIFIED

```rust
// Documented:
pub struct Skill {
    pub name: String,
    pub description: String,
    pub version: Option<String>,    // not in older docs
    pub tags: Vec<String>,
    pub body: String,
    pub source: PathBuf,            // not in older docs
}

// Actual (src/skills/mod.rs:7-15):
pub struct Skill {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub tags: Vec<String>,
    pub body: String,
    pub source: PathBuf,
}
```

**Verdict**: Exact match. Field order and types are correct.

---

### 2. SkillIndex (Line 35-47) ✅ VERIFIED

```rust
// Documented:
pub struct SkillIndex {
    skills: Vec<Skill>,            // older docs incorrectly showed HashMap
}

// Actual (src/skills/mod.rs:26-28):
pub struct SkillIndex {
    skills: Vec<Skill>,
}
```

**Verdict**: Correct. Documentation correctly notes older docs showed HashMap.

---

### 3. SkillIndex Methods (Line 39-47) ✅ VERIFIED

| Method | Documented | Actual | Line Match |
|--------|-----------|--------|------------|
| `new()` | ✅ | ✅ | 37 |
| `load(&mut self, project_dir: &str)` | ✅ async | ✅ async | 41 |
| `get(&self, name: &str)` | ✅ | ✅ | 85 |
| `list(&self)` | ✅ | ✅ | 89 |
| `find_matching(&self, query: &str)` | ✅ | ✅ | 93 |
| `build_system_prompt(&self)` | ✅ | ✅ | 107 |
| `activate(&self, name: &str)` | ✅ | ✅ | 123 |

**Verdict**: All methods match. `find_matching` correctly documented (not `search()` as older docs claimed).

---

### 4. Method Behavior (Line 51-54) ✅ VERIFIED

- `get(name)` - Case-sensitive exact match ✅ (line 86: `s.name == name`)
- `list()` - Returns slice ✅ (line 89-90)
- `find_matching(query)` - Case-insensitive partial match on name, description, tags ✅ (lines 93-105)
- `activate(name)` - Case-sensitive exact match ✅ (line 124)

---

### 5. Skill Loading Locations (Line 81-83) ✅ VERIFIED

| Location | Documented | Actual | Line |
|----------|------------|--------|------|
| Global | `~/.config/codegg/skills/` | `dirs::config_dir().join("codegg").join("skills")` | 44-46 |
| Project | `.codegg/skills/` | `.codegg/skills/` | 49 |

**Verdict**: Correct.

---

### 6. Loading Behavior (Line 86-87) ✅ VERIFIED

- Direct `.md` files loaded as skills ✅ (lines 75-78)
- Directories containing `SKILL.md` loaded as skills ✅ (lines 65-71)

**Verdict**: Correct.

---

### 7. SkillTool (Line 97) ✅ VERIFIED

- Location: `src/tool/skill.rs` ✅
- Tool name: `skill` ✅ (line 15)
- Returns JSON with `name`, `description`, `body`, `resources` ✅ (lines 55-60)

**Verdict**: Correct. The tool also returns `resources` which is not mentioned in the doc but is an additional feature.

---

### 8. assemble_system_prompt (Line 118) ⚠️ PARTIALLY CORRECT

**Documented**: `assemble_system_prompt()` accepts skill names but skill bodies are injected separately via prompt modification.

**Actual** (src/agent/prompt.rs:100-143):
```rust
pub fn assemble_system_prompt(
    agent: &Agent,
    config: &Config,
    tools: &[String],
    skills: &[String],           // Takes skill NAMES only
    custom_instructions: Option<&str>,
) -> String {
    // ...
    if !skills.is_empty() {
        let skill_list = skills.join(", ");
        parts.push(format!("Available skills: {skill_list}"));  // Just lists names
    }
    // ...
}
```

**Verdict**: Correct. The function only receives skill names and lists them. It does NOT inject skill bodies. The actual skill body retrieval happens via `SkillTool` (the `/skill:` tool) at runtime.

---

### 9. .skills/ Directory (Line 14) ⚠️ CORRECT BUT INCOMPLETE

**Documented**: Repository keeps agent-facing skill docs in `.skills/` for maintenance.

**Actual**: `.skills/` directory exists with 44 skill subdirectories (agent-loop, caching, client, command, compaction, config, core, crypto, diff, e2e, error, event-bus, exec, hooks, ide, lsp, mcp, memory, mode, model-dialog, notifications, permission, plugin, provider, question-response, resilience, router, sandbox, security, server, session, shell_session, skills, snapshot, storage, subagent, team, testing, tool, tool-search, tts, tui, tui_input, tui-dialog-maintenance, tui-dialog-testing, upgrade, util, worktree).

**Verdict**: Correct, but count is 44 skill directories, not explicitly documented.

---

### 10. Skill File Format (Line 60-77) ✅ VERIFIED

Frontmatter parsing is confirmed at `src/skills/mod.rs:157-168` with YAML structure matching documented format.

---

### 11. Additional Findings

#### SkillIndex Default Implementation (Not documented)
The source includes `impl Default for SkillIndex` (lines 30-34) which is not mentioned in the documentation.

#### SkillFrontmatter Internal Type (Not documented)
The source has an internal `SkillFrontmatter` struct (lines 18-24) used for YAML parsing that is not documented. This is an implementation detail.

---

## Discrepancy Summary

| Item | Severity | Description |
|------|----------|-------------|
| `resources` field in tool output | Low | `SkillTool` returns `resources` array not mentioned in docs |
| Default trait impl | Low | `impl Default for SkillIndex` not documented |
| SkillFrontmatter type | Low | Internal parsing type not documented |

---

## Conclusion

The architecture document is **largely accurate**. All key types, methods, and behaviors are correctly documented. Minor discrepancies are related to implementation details not exposed in the public API (internal types, Default trait). The document correctly notes historical errors (HashMap vs Vec, `search()` vs `find_matching()`) that have since been corrected.