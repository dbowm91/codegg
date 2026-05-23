# Skills Module Architecture Review

## Verified Claims

### Skill Struct
The `Skill` struct in `src/skills/mod.rs:7-15` matches architecture documentation exactly:
- `name: String` ✅
- `description: String` ✅
- `version: Option<String>` ✅
- `tags: Vec<String>` ✅
- `body: String` ✅
- `source: PathBuf` ✅

### SkillIndex Struct
`src/skills/mod.rs:26-28` correctly uses `Vec<Skill>` (not HashMap as older docs incorrectly showed). ✅

### SkillIndex Methods
All methods match the architecture doc:

| Method | Location | Verified |
|--------|----------|----------|
| `new()` | mod.rs:37 | ✅ |
| `async fn load(&mut self, project_dir: &str)` | mod.rs:41 | ✅ |
| `get(&self, name: &str) -> Option<&Skill>` | mod.rs:85 | ✅ |
| `list(&self) -> &[Skill]` | mod.rs:89 | ✅ |
| `find_matching(&self, query: &str) -> Vec<&Skill>` | mod.rs:93 | ✅ |
| `build_system_prompt(&self) -> String` | mod.rs:107 | ✅ |
| `activate(&self, name: &str) -> Option<String>` | mod.rs:123 | ✅ |

### Skill File Format
YAML frontmatter parsing matches `SkillFrontmatter` struct at mod.rs:18-24. ✅

### Skill Loading Locations
- **Global**: `~/.config/codegg/skills/` via `dirs::config_dir()` (mod.rs:44-46) ✅
- **Project**: `.codegg/skills/` (mod.rs:49) ✅

### Recursive Loading Logic
Direct `.md` files and directories containing `SKILL.md` are loaded correctly (mod.rs:65-79). ✅

### Async File I/O
Uses `tokio::fs` for all file operations (mod.rs:60, 128-129), not blocking `std::fs`. ✅

### Default Implementation
`SkillIndex` implements `Default` via `impl Default for SkillIndex` (mod.rs:30-34) which calls `new()`. ✅

### SkillTool Integration
`src/tool/skill.rs` correctly loads skills at runtime via `SkillIndex::new()` and `load()`. ✅

### Main.rs Integration
`src/main.rs:839-846` shows correct pattern: create SkillIndex, load, then activate. ✅

## Bugs/Discrepancies Found

### 1. SkillTool `resources` Field Not Documented
**Priority**: low

**Location**: `src/tool/skill.rs:53` and `architecture/skills.md:92`

The architecture doc says SkillTool "Returns JSON with name, description, body, and resources" but doesn't explain what `resources` contains. The implementation at `skill.rs:67-98` (via `list_skill_resources`) returns other files in the skill directory (excluding SKILL.md itself). This is useful metadata but undocumented.

**Current doc says**:
```
// Returns JSON with name, description, body, and resources
```

**Should clarify**: `resources` is an array of filenames (strings) listing companion files in the skill directory.

### 2. `find_matching` Search Scope Not Explicitly Documented
**Priority**: low

**Location**: `src/skills/mod.rs:93-105` vs `architecture/skills.md:42`

The architecture doc shows the method signature but doesn't document that matching is case-insensitive and searches across `name`, `description`, AND `tags`. Implementation is correct:
```rust
s.name.to_lowercase().contains(&query_lower)
    || s.description.to_lowercase().contains(&query_lower)
    || s.tags.iter().any(|t| t.to_lowercase().contains(&query_lower))
```

### 3. `load_dir` is Private But Not Noted
**Priority**: low

**Location**: `src/skills/mod.rs:59`

The `load_dir` method is `async fn` but not marked `pub`. Only `load` is public. Architecture doc correctly shows only `load` as public.

## Improvement Suggestions

### 1. Document `list_skill_resources` Function
**Priority**: low

**Location**: `src/tool/skill.rs:67-98`

This helper function enumerates companion files in a skill directory. It should be documented somewhere (SKILL.md skill or architecture doc) since it provides the `resources` field in SkillTool output.

### 2. Add `SkillFrontmatter` to Architecture Doc
**Priority**: low

**Location**: `src/skills/mod.rs:18-24`

The internal `SkillFrontmatter` struct is an implementation detail but documenting it would help anyone extending the skill system understand how YAML frontmatter maps to `Skill`.

### 3. Document `parse_frontmatter` Function
**Priority**: low

**Location**: `src/skills/mod.rs:157-169`

This private function handles `---` delimited frontmatter parsing. It's a standalone function (not a method). Documenting its existence and behavior would help future maintainers.

### 4. Add Note About Empty Frontmatter Fallback
**Priority**: low

**Location**: `src/skills/mod.rs:139-143`

If frontmatter lacks a `name` field, the skill name falls back to the file stem (filename without extension). This graceful fallback isn't documented.

### 5. SkillTool Description Slightly Inaccurate
**Priority**: very low

**Location**: `src/tool/skill.rs:20`

The description says "Returns the skill content and list of resource files" but the actual output is JSON with `name`, `description`, `body`, `resources` - not a raw skill content return. Minor wording issue.

## Summary

**No bugs found.** The implementation matches the architecture documentation accurately. The discrepancies are all minor documentation omissions rather than actual bugs. The skills module is well-implemented with:
- Correct type definitions
- Proper async file I/O
- Accurate method signatures
- Correct integration points

The module correctly uses `Vec<Skill>` (not HashMap), uses async file I/O, and the SkillTool properly returns the `resources` array even though this isn't highlighted in the architecture doc.