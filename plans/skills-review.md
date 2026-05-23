# Skills Module Architecture Review

## Verification Results

### Claims (table format: Claim | Status | Evidence)

| Claim | Status | Evidence |
|-------|--------|----------|
| `Skill` struct has `name`, `description`, `version`, `tags`, `body`, `source` fields | VERIFIED | `src/skills/mod.rs:7-15` matches exactly |
| `SkillIndex` has `skills: Vec<Skill>` (not HashMap) | VERIFIED | `src/skills/mod.rs:26-27` - correct Vec, not HashMap |
| `SkillIndex::new()` exists | VERIFIED | `src/skills/mod.rs:37` |
| `SkillIndex::load(&mut self, project_dir: &str) -> Result<(), AppError>` | VERIFIED | `src/skills/mod.rs:41` |
| `SkillIndex::get(&self, name: &str) -> Option<&Skill>` | VERIFIED | `src/skills/mod.rs:85` |
| `SkillIndex::list(&self) -> &[Skill]` | VERIFIED | `src/skills/mod.rs:89` |
| `SkillIndex::find_matching(&self, query: &str) -> Vec<&Skill>` | VERIFIED | `src/skills/mod.rs:93` (not `search()` as older docs claimed) |
| `SkillIndex::build_system_prompt(&self) -> String` | VERIFIED | `src/skills/mod.rs:107` |
| `SkillIndex::activate(&self, name: &str) -> Option<String>` | VERIFIED | `src/skills/mod.rs:123` |
| Skills loaded from global `~/.config/codegg/skills/` | VERIFIED | `src/skills/mod.rs:44-46` via `dirs::config_dir()` |
| Skills loaded from project `.codegg/skills/` | VERIFIED | `src/skills/mod.rs:48-50` |
| Direct `.md` files loaded as skills | VERIFIED | `src/skills/mod.rs:75-78` |
| Directories containing `SKILL.md` loaded as skills | VERIFIED | `src/skills/mod.rs:65-72` |
| `SkillTool` (`src/tool/skill.rs`) handles runtime skill loading | VERIFIED | `src/tool/skill.rs:11-65` exists |
| Skills use async file I/O (`tokio::fs`) | VERIFIED | `src/skills/mod.rs:3` - correctly using tokio |
| `SkillTool` returns JSON with name, description, body, resources | VERIFIED | `src/tool/skill.rs:55-60` |
| `build_system_prompt()` returns empty string when no skills | VERIFIED | `src/skills/mod.rs:108-110` |
| `find_matching()` searches name, description, tags | VERIFIED | `src/skills/mod.rs:94-104` |
| Global skills loaded before project skills | VERIFIED | `src/skills/mod.rs:52-54` - config_dir chain then local_dir |

## Bugs Found

### Medium

**1. SkillTool reloads all skills on every execution (`src/tool/skill.rs:44-47`)**

```rust
let mut loaded = crate::skills::SkillIndex::new();
if let Err(e) = loaded.load(&project_dir).await {
    return Err(ToolError::Execution(format!("failed to load skills: {e}")));
}
```

Every call to `/skill:<name>` re-parses all skill files from disk. For a development workflow where skills are frequently accessed, this is wasteful. Should cache the SkillIndex or provide a way to reuse it.

**2. `list_skill_resources` silently ignores errors (`src/tool/skill.rs:86-94`)**

```rust
if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
    while let Ok(Some(entry)) = entries.next_entry().await {
```

Uses `if let Ok(...)` pattern which silently drops all errors. If directory reading fails, an empty vector is returned without any logging. Makes debugging difficult.

**3. `parse_frontmatter` handles only `---` delimiter (`src/skills/mod.rs:157-168`)**

```rust
fn parse_frontmatter(content: &str) -> Option<(String, String)> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return None;
    }
```

Only handles the `---` opening delimiter. Most YAML frontmatter also ends with `---`, but this implementation assumes content between first `---` and second `---`. If there's no closing `---`, the entire file becomes the body with frontmatter as part of it. Consider more robust parsing or at least validate the format.

### Low

**4. No validation of skill name format**

The `get()` and `activate()` methods use raw user input to find skills. If someone creates a skill with a name containing special characters (e.g., `../`, newlines, control characters), it could cause issues. No sanitization of skill names is performed.

**5. `load()` clears skills before loading (`src/skills/mod.rs:42`)**

```rust
pub async fn load(&mut self, project_dir: &str) -> Result<(), AppError> {
    self.skills.clear();
```

This is correct behavior for reload, but there's no way to merge/add skills after initial load. If global skills are loaded first, then project skills, project skills could override global ones with same name. However, the current implementation doesn't handle duplicate names (same skill loaded twice from both locations).

**6. No error context in `parse_skill_file` failures**

When `parse_skill_file` fails (e.g., YAML parse error), the error doesn't include the file path. This makes debugging which skill file is problematic difficult.

## Improvement Suggestions

### Performance

1. **Cache SkillIndex in SkillTool**: Instead of reloading all skills on every `/skill:` invocation, maintain a cached SkillIndex. The tool could accept a shared `Arc<SkillIndex>` or use a static/cache mechanism.

2. **Lazy loading of skill bodies**: If skill bodies can be large, consider loading only metadata initially and loading body content on demand via `activate()`.

3. **Consider using `WalkDir` for recursive loading**: The current implementation only loads one level deep (doesn't recurse into subdirectories). If nested skill directories are needed in the future, consider `walkdir` crate.

### Correctness

1. **Add file path to error messages**: When parsing fails in `parse_skill_file`, include the path:

   ```rust
   Err(AppError::Other(format!("failed to parse skill file {}: {}", path.display(), e)))
   ```

2. **Log when skills are loaded**: Add tracing at `load()` to log number of skills loaded from each location for debugging.

3. **Handle missing frontmatter gracefully**: When a `.md` file has no frontmatter, it still gets loaded with empty name/description (using file stem as name). Document this behavior or require frontmatter.

4. **Validate skill names**: Add validation to reject skill names with problematic characters.

### Maintainability

1. **Add integration tests**: Test loading skills from actual directories with various file structures. Current tests only cover empty directories and basic struct creation.

2. **Document the activation flow**: The relationship between CLI `--session skill:git`, `skills.activate()`, and `app.prompt_state.prompt.set_text()` is unclear. Add comments explaining this.

3. **Consider SkillIndex caching at module level**: The `SkillTool` creates a new `SkillIndex` every time. A module-level cached index or a singleton pattern could simplify this.

4. **Add example skills to repository**: Having actual skill examples in `.codegg/skills/` would help developers understand the format.

## Priority Actions (top 5 items to fix)

1. **[High]** Add path to error messages in `parse_skill_file` - helps debugging malformed skill files
2. **[High]** Cache SkillIndex in SkillTool to avoid reloading skills on every execution
3. **[Medium]** Add tracing/logging for skill loading to aid debugging
4. **[Medium]** Add integration tests for skill loading from real directory structures
5. **[Low]** Validate skill names to reject problematic characters