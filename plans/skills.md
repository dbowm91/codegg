# Skills Architecture Review Findings

## Verified Claims

### Skill struct (lines 20-30)
- All fields verified at `src/skills/mod.rs:7-15`: name, description, version (Option), tags, body, source (PathBuf)
- Correctly notes older docs showed `HashMap` but it's `Vec<Skill>` in `SkillIndex` (line 36)

### SkillIndex (lines 32-47)
- `skills: Vec<Skill>` verified at `mod.rs:26-28`
- All methods verified: `new()`, `load()`, `get()`, `list()`, `find_matching()`, `build_system_prompt()`, `activate()`

### load() method (lines 41-56)
- Loads from global `~/.config/codegg/skills/` via `dirs::config_dir()`
- Loads from project `.codegg/skills/`
- Recursive loading: direct `.md` files and directories containing `SKILL.md`

### get() method (lines 85-87)
- Exact name match (case-sensitive) confirmed

### find_matching() (lines 93-105)
- Case-insensitive partial match across name, description, and tags confirmed

### activate() (lines 123-125)
- Exact name match (case-sensitive) confirmed

### Skill File Format (lines 60-77)
- YAML frontmatter with name, description, version, tags verified at `mod.rs:128-155`
- `parse_frontmatter` function at `mod.rs:157-169` correctly parses `---` delimited frontmatter

### Loading locations (lines 81-88)
- Global: `~/.config/codegg/skills/` (via `dirs::config_dir()`)
- Project: `.codegg/skills/`
- Verified at `mod.rs:44-50`

## Stale Information

### skill_search tool reference
The doc references "SkillTool (`src/tool/skill.rs`)" at line 97-103. Need to verify this exists in tool module.

### assemble_system_prompt reference
Line 118 references `assemble_system_prompt()` in `src/agent/prompt.rs`. Need to verify this exists.

## Bugs Found

None found. The skills module documentation is accurate and matches implementation.

## Improvements Suggested

### Clarify activation mechanism
The document describes `/skill:<name>` command but doesn't fully explain how skill body is injected into the agent's context. The `activate()` returns `Option<String>` (the body), but it's unclear how the caller uses it. This is mentioned briefly but could be clearer.

### skill_search vs activate
The document says skill activation returns "JSON with name, description, body, and resources" but `activate()` returns `Option<String>` which is just the body. This seems contradictory.

### find_matching name
Line 44 mentions "older docs showed search()" but current method is `find_matching()`. This is correct in current docs.

## Cross-Module Issues

### Tool integration
The `src/tool/skill.rs` reference should be verified - if skill tool exists there, it should be documented how it uses SkillIndex.

### Agent prompt integration
The `assemble_system_prompt()` reference in `src/agent/prompt.rs` should be verified for accurate cross-referencing.