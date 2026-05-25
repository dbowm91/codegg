# Skills Module Architecture Review

**Status**: VERIFIED ACCURATE (no changes needed since May 25 2026)

**Review Date**: 2026-05-25
**Reviewed By**: Architecture Review Agent

## Summary

Verified `architecture/skills.md` against actual implementation in `src/skills/mod.rs` and related files. The documentation is accurate and matches the implementation. The May 25 2026 commit (6513f38 - "docs: refresh core and TUI separation guidance") did not modify skills.md content itself (only timestamps and metadata).

## What Was Verified

### 1. Key Types

#### Skill struct (`src/skills/mod.rs:7-15`)
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
**Status**: ✅ ACCURATE - All fields match exactly.

#### SkillIndex struct (`src/skills/mod.rs:26-28`)
```rust
pub struct SkillIndex {
    skills: Vec<Skill>,  // not HashMap as older docs incorrectly showed
}
```
**Status**: ✅ ACCURATE - Uses `Vec<Skill>` as documented.

### 2. SkillIndex Methods

| Method | Signature | Location | Status |
|--------|-----------|----------|--------|
| `new()` | `pub fn new() -> Self` | mod.rs:37 | ✅ ACCURATE |
| `load()` | `pub async fn load(&mut self, project_dir: &str) -> Result<(), AppError>` | mod.rs:41 | ✅ ACCURATE |
| `get()` | `pub fn get(&self, name: &str) -> Option<&Skill>` | mod.rs:85 | ✅ ACCURATE |
| `list()` | `pub fn list(&self) -> &[Skill]` | mod.rs:89 | ✅ ACCURATE |
| `find_matching()` | `pub fn find_matching(&self, query: &str) -> Vec<&Skill>` | mod.rs:93 | ✅ ACCURATE |
| `build_system_prompt()` | `pub fn build_system_prompt(&self) -> String` | mod.rs:107 | ✅ ACCURATE |
| `activate()` | `pub fn activate(&self, name: &str) -> Option<String>` | mod.rs:123 | ✅ ACCURATE |

### 3. Skill Loading Locations

| Location | Documentation | Implementation | Status |
|----------|---------------|----------------|--------|
| Global | `~/.config/codegg/skills/` | `dirs::config_dir().join("codegg").join("skills")` | ✅ ACCURATE |
| Project | `.codegg/skills/` | `project_dir.join(".codegg").join("skills")` | ✅ ACCURATE |

### 4. Recursive Loading Behavior

- Direct `.md` files → loaded as skills ✅
- Directories containing `SKILL.md` → loaded as skills (directory name becomes skill name) ✅

**Implementation** (`mod.rs:59-83`): Both cases handled correctly.

### 5. SkillTool (`src/tool/skill.rs`)

- `SkillTool` struct at line 11 ✅
- `execute(input: serde_json::Value)` at line 36 ✅
- Returns JSON with `name`, `description`, `body`, `resources` ✅
- `list_skill_resources()` function at line 67 ✅

### 6. list_skill_resources() Behavior

| Behavior | Documented | Implementation | Status |
|----------|------------|-----------------|--------|
| File path handling | Uses parent directory | `skill_path.parent()` if not dir | ✅ ACCURATE |
| Directory path handling | Uses directly | `skill_path` if is_dir() | ✅ ACCURATE |
| Invalid path | Returns empty Vec | `!tokio::fs::metadata(&dir).is_dir()` check | ✅ ACCURATE |
| Excludes SKILL.md | Yes | `path.file_name() != Some("SKILL.md")` | ✅ ACCURATE |
| Returns names only | Yes | `path.file_name().and_then(n => n.to_str())` | ✅ ACCURATE |

### 7. assemble_system_prompt() Integration

**Location**: `src/agent/prompt.rs:100-143`
**Signature**: `pub fn assemble_system_prompt(agent: &Agent, config: &Config, tools: &[String], skills: &[String], custom_instructions: Option<&str>) -> String`

**Status**: ✅ ACCURATE - Function accepts `skills: &[String]` parameter and handles it at lines 123-126.

### 8. .skills Directory

The repository maintains `.skills/` as a symlink to `.opencode/skills/` for agent-facing maintenance copies.

**Status**: ✅ ACCURATE - Symlink exists at repository root.

## Discrepancies Found

**None** - All documented types, functions, signatures, and behaviors match the implementation exactly.

## Code Quality Observations

### 1. Minor: Undocumented Private Helper

The `parse_frontmatter()` function at `mod.rs:157-169` is private but not documented. It's a helper for parsing skill files and works correctly:
- Handles content without leading `---`
- Returns `None` if no frontmatter found
- Properly strips frontmatter delimiters

### 2. Minor: SkillFrontmatter is Private

`SkillFrontmatter` struct at `mod.rs:17-24` is private and not documented. This is fine - it's an internal implementation detail for YAML deserialization.

## Recommendations

### Documentation

1. **Consider documenting private helpers** - `parse_frontmatter()` and `SkillFrontmatter` could be mentioned in architecture doc as internal types, but this is optional.

### Code

1. **Consider adding tests** - The skills module has no unit tests. Adding tests for `parse_frontmatter()`, `find_matching()`, and `build_system_prompt()` would improve reliability.

2. **Consider Error Context** - `load_dir()` could benefit from including the directory path in error messages for easier debugging.

## Conclusion

The `architecture/skills.md` document is **accurate and up-to-date**. All documented types, functions, methods, and behaviors match the actual implementation in `src/skills/mod.rs` and `src/tool/skill.rs`. No discrepancies were found that require documentation fixes or code changes.