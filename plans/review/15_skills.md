# Skills Module Architecture Review

## Date: 2026-05-25

## Verified Correct Items

| Item | Location | Status |
|------|----------|--------|
| Skill struct fields | `src/skills/mod.rs:7-15` | ✅ Correct |
| SkillIndex uses `Vec<Skill>` | `src/skills/mod.rs:26-28` | ✅ Correct (doc correctly notes older docs incorrectly showed HashMap) |
| `find_matching` method name | `src/skills/mod.rs:93-105` | ✅ Correct (doc correctly notes older docs showed `search()`) |
| Skill file format (markdown + YAML frontmatter) | `src/skills/mod.rs:128-155` | ✅ Correct |
| Global path `~/.config/codegg/skills/` | `src/skills/mod.rs:44-46` | ✅ Correct |
| Project path `.codegg/skills/` | `src/skills/mod.rs:48-50` | ✅ Correct |
| Directory loading (SKILL.md in directories) | `src/skills/mod.rs:65-72` | ✅ Correct |
| SkillTool in `src/tool/skill.rs` | `src/tool/skill.rs:11-65` | ✅ Correct |
| `list_skill_resources` function | `src/tool/skill.rs:67-98` | ✅ Correct |
| SkillIndex loading in main.rs | `src/main.rs:930` | ✅ Correct |
| `assemble_system_prompt` accepts skills parameter | `src/agent/prompt.rs:100-106` | ✅ Correct |
| `.skills/` repo maintenance copy exists | `.skills/` directory | ✅ Correct |

## Incorrect/Stale Items

### 1. Skill Loading Path (Line 75)

**Doc says:** `.codegg/skills/` (in project directory)

**Actual:** `.codegg/skills/` - Wait, let me re-check the code.

Looking at `src/skills/mod.rs:48-50`:
```rust
let project_dir = PathBuf::from(project_dir);
let local_skills = project_dir.join(".codegg").join("skills");
```

So the actual path is `<project_dir>/.codegg/skills/`. The architecture doc at line 75 says ".codegg/skills/ (in project directory)" which is correct.

**Verdict:** ✅ Correct

### 2. Missing `list_skill_resources` documentation

The architecture doc does NOT mention the `list_skill_resources` function at `src/tool/skill.rs:67-98`. This function:
- Scans a skill's directory for additional resource files
- Returns file/directory names (not full paths)
- Excludes `SKILL.md` from results
- Used by SkillTool to include resource list in tool output

**Need to add:** Documentation for `list_skill_resources` function behavior.

### 3. Missing `list()` and `get()` method documentation

The architecture doc mentions `get()` and `list()` exist but doesn't explicitly document them:
- `get(&self, name: &str) -> Option<&Skill>` - Returns exact name match
- `list(&self) -> &[Skill]` - Returns slice of all skills

These are documented in the SKILL.md skill but not in the architecture doc.

### 4. Missing Default implementation

`SkillIndex` implements `Default` trait (`src/skills/mod.rs:30-34`) which is not documented.

### 5. Missing `SkillFrontmatter` internal type

The internal `SkillFrontmatter` struct at `src/skills/mod.rs:17-24` is not documented. While this is an internal implementation detail, it explains how skill metadata is parsed.

## Bugs Found

None. The implementation is correct.

## Specific Line Numbers Needing Updates

### `architecture/skills.md`

| Line | Change |
|------|--------|
| 47-48 | Add `list(&self) -> &[Skill];` method |
| 47-48 | Add `get(&self, name: &str) -> Option<&Skill>;` method |
| 53 | Note that `find_matching` searches name, description, and tags |
| 63-69 | The skill file format example is correct but could be more complete |
| 89-95 | Add documentation for `list_skill_resources` function behavior |
| 110 | Add note that `assemble_system_prompt` accepts `skills: &[String]` parameter |
| 112-115 | Add See Also reference to `.opencode/skills/skills/SKILL.md` |

## Summary

The architecture document is **mostly accurate**. Key corrections needed:
1. Add `list()` and `get()` method signatures
2. Document `find_matching` behavior (searches name, description, tags)
3. Document `list_skill_resources` function behavior
4. Add `Default` trait implementation note
5. Reference the detailed SKILL.md skill guide in See Also
