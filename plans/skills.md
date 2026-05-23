# Skills Architecture Review

## Architecture Document
- Path: architecture/skills.md

## Source Code Location
- src/skills/

## Verification Summary
Pass

## Verified Claims (table format)
| Claim | Status | Notes |
|-------|--------|-------|
| Skill struct with name, description, version, tags, body, source | Pass | Exact match |
| SkillIndex uses Vec<Skill> (not HashMap) | Pass | Corrected in doc ("older docs incorrectly showed HashMap") |
| SkillIndex methods: new(), load(), get(), list(), find_matching(), build_system_prompt(), activate() | Pass | All methods present and match signatures |
| Skill file format with YAML frontmatter | Pass | markdown with --- delimiters |
| Global skills: ~/.config/codegg/skills/ | Pass | Uses dirs::config_dir() |
| Project skills: .codegg/skills/ | Pass | Relative to project_dir |
| Loading: direct .md files + directories with SKILL.md | Pass | Both patterns implemented |
| activate() returns Option<String> with skill body | Pass | Returns s.body.clone() |
| SkillTool at src/tool/skill.rs handles runtime loading | Pass | Present and functional |
| build_system_prompt() generates available skills list | Pass | Returns empty string if no skills |
| assemble_system_prompt() accepts skill names (not bodies) | Pass | Confirmed in prompt.rs |

## Issues Found

### Missing Documentation
1. **list_skill_resources() undocumented**: The skill.rs module has an async `list_skill_resources()` function that scans the skill directory for additional resource files (excluding SKILL.md). This is called by SkillTool::execute() but never mentioned in architecture or skill docs.

2. **SkillTool return includes resources field**: The SkillTool returns JSON with `name`, `description`, `body`, AND `resources` (array of resource filenames). Architecture doc only mentions "name, description, body, and resources" but doesn't explain how resources are discovered.

3. **Fallback name from filename**: When frontmatter lacks a `name` field, `parse_skill_file()` falls back to using `path.file_stem()` (filename without extension). This behavior is not documented.

4. **SkillFrontmatter private struct**: The `SkillFrontmatter` struct with its optional fields is internal implementation detail not documented.

5. **Empty directories ignored**: Directories without SKILL.md are silently skipped (only directories WITH SKILL.md are loaded). This is not documented.

### Improvement Opportunities
1. **Could document parse_frontmatter()**: The manual YAML/frontmatter parsing via `parse_frontmatter()` could be noted as a design decision (uses basic string parsing rather than a full YAML parser for frontmatter only).

2. **SKILL.md naming convention**: Directory-based skills require the file to be named exactly `SKILL.md` (case-sensitive). This convention should be documented.

3. **No deduplication**: If the same skill exists in both global and project directories, both copies are loaded. No dedup or override mechanism exists. This could be documented as a known limitation or future improvement.

### Bugs
None found - implementation is correct and matches documentation.

## Recommendations
1. Add documentation for `list_skill_resources()` function and the `resources` field in SkillTool output
2. Document the fallback behavior when `name` is missing from frontmatter
3. Consider adding a note about directory-only skills requirement (must have SKILL.md)
4. Document the lack of deduplication when same skill exists in global and project locations
