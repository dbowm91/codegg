# Skills Module Review

## Summary

Reviewed `architecture/skills.md`, `.opencode/skills/skills/SKILL.md`, `src/skills/mod.rs`, `src/tool/skill.rs`, and related integration points. The documentation is accurate and the implementation matches.

## Verified Items

### SkillIndex Structure
- **Correct**: Uses `Vec<Skill>` (not `HashMap` as older docs incorrectly showed)
- **Correct**: `SkillIndex::new()` creates empty Vec
- **Correct**: `load()`, `get()`, `list()`, `find_matching()`, `build_system_prompt()`, `activate()` all implemented as documented

### Skill Struct
- **Correct**: All fields match (`name`, `description`, `version`, `tags`, `body`, `source`)
- **Source file at `src/skills/mod.rs:7-15`**

### Skill Loading
- **Correct**: Loads from global `~/.config/codegg/skills/` and project `.codegg/skills/`
- **Correct**: Recursive loading: `.md` files and directories with `SKILL.md`
- **Correct**: Uses `tokio::fs` for async I/O (previously blocking, fixed)
- **Code**: `src/skills/mod.rs:41-83`

### Skill Activation
- **Correct**: `activate()` returns `Option<String>` with skill body
- **Correct**: `SkillTool` provides runtime loading at `src/tool/skill.rs:14-64`
- **Correct**: `list_skill_resources()` function at `src/tool/skill.rs:67-98`

### Integration Points
- **Correct**: `main.rs:839-846` loads skills at startup and activates from CLI `--session` flag
- **Correct**: `prompt.rs:100-126` `assemble_system_prompt()` accepts `skills: &[String]` parameter
- **Correct**: Tests exist at `tests/skills.rs` (6 unit tests)

## Discrepancies Found

### None - Documentation Matches Implementation

All claims in `architecture/skills.md` and the SKILL.md are accurate. The "older docs incorrectly showed HashMap" note in the arch doc correctly reflects this was fixed.

## Bugs or Issues

### No Bugs Found

The implementation is correct and matches documentation.

## Minor Observations

1. **Skill Frontmatter Parsing** (`src/skills/mod.rs:139-143`): The fallback to `file_stem()` for name when `name` is missing in frontmatter is sensible but could lead to unexpected names if multiple skills in a directory share the same filename stem.

2. **Error Handling in `list_skill_resources`** (`src/tool/skill.rs:86-95`): Uses `unwrap_or()` on `read_dir` results which silently ignores permission errors. This is minor since it returns an empty `Vec` in error cases.

## Recommendations

### Documentation
- Both `architecture/skills.md` and `.opencode/skills/skills/SKILL.md` are accurate and well-maintained. No changes needed.

### Code
- Consider adding a test for loading skills from a directory structure to verify the `load_dir` logic works correctly with nested skill directories.
- Consider making `parse_frontmatter` more lenient with leading whitespace before `---`, since `trim_start()` only handles leading whitespace but the frontmatter delimiter must be at the start after optional whitespace.

## File References

| File | Line(s) | Description |
|------|---------|-------------|
| `src/skills/mod.rs` | 7-15 | `Skill` struct definition |
| `src/skills/mod.rs` | 26-28 | `SkillIndex` struct |
| `src/skills/mod.rs` | 37-39 | `SkillIndex::new()` |
| `src/skills/mod.rs` | 41-57 | `SkillIndex::load()` |
| `src/skills/mod.rs` | 59-83 | `load_dir()` recursive loading |
| `src/skills/mod.rs` | 85-87 | `get()` |
| `src/skills/mod.rs` | 93-105 | `find_matching()` |
| `src/skills/mod.rs` | 107-121 | `build_system_prompt()` |
| `src/skills/mod.rs` | 123-125 | `activate()` |
| `src/skills/mod.rs` | 128-155 | `parse_skill_file()` |
| `src/skills/mod.rs` | 157-169 | `parse_frontmatter()` |
| `src/tool/skill.rs` | 11-65 | `SkillTool` implementation |
| `src/tool/skill.rs` | 67-98 | `list_skill_resources()` |
| `src/main.rs` | 839-846 | Skills loading at startup |
| `tests/skills.rs` | 1-60 | Unit tests |
