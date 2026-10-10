# Cross-Agent Skill Compatibility M002 — Closure Status

Status: closed

Source plan: `plans/implementation/cross-agent-skills-compatibility/002-source-roots-and-deduplication.md`

Source roadmap: `plans/subsystems/cross-agent-skills-compatibility-roadmap.md`

Repository baseline reviewed: M001 closure at `plans/closure/cross-agent-skills-compatibility/001-status.md`.

Implementation commits: `d0164e95`, `8ef173bf`, `b1e03ce0`, `08b7daa7`.

## 1. Finding

M002 is closed. One `AssetRegistry` now recognizes the qualified project and user-global skill roots, applies explicit project-scope/source precedence, and deduplicates canonical roots while retaining alias provenance.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Cross-agent project roots | `discovers_cursor_gemini_copilot_and_home_global_roots_once`; registry integration coverage | pass |
| User-home and XDG/OpenCode roots | home-root fixtures and `default_global_discovery_roots` | pass |
| Codex and personal Copilot roots | `.codex/skills`, `~/.codex/skills`, and `~/.copilot/skills` fixtures | pass |
| Canonical source/file dedup with visible alias provenance | symlink alias fixture; one effective candidate and alias summary | pass |
| Stable precedence without enum renumbering | additive `SourceKind` variants and source precedence tests | pass |

## 3. Implementation evidence

Supported project roots include CodeGG, Agent Skills, OpenCode, Claude, Cursor, Gemini CLI, GitHub Copilot, and Codex. Global discovery uses the explicit config and home roots with a fixed vendor table; OpenCode's XDG home location is included. Roots are canonicalized/deduplicated before parsing. Home discovery does not recurse.

## 4. Verification executed

- `rtk cargo test -p codegg --lib skills::` — 49 passed.
- `rtk cargo test --test skills_registry` — 24 passed.
- `rtk scripts/verify.sh quick` — passed.
- `rtk cargo clippy --workspace --all-targets --locked -- -D warnings` — passed.

The first source-root unit attempt used the same skill name at project and global scope, correctly selected the project candidate, and failed its fixture expectation. The fixture was renamed to isolate the global-root assertion; the final skills suite passed.

## 5. Conflict and symlink fixtures

Tests prove nearer project scope wins, sibling projects are excluded, physical root aliases are parsed once, aliases are observable, and global roots are tested independently from project candidates. Existing shadowed-name behavior remains covered by `skills_registry`.

## 6. Security and authorization

Only fixed vendor paths beneath explicit home/config roots are considered. No arbitrary home scan, process-CWD lookup, descendant repository walk, network access, or vendor execution was added. File aliases and symlink escapes remain contained by the existing parser/resource boundaries.

## 7. Serialization and migration

New `SourceKind` variants are additive and existing variants keep their identities. New DTO fields (`scope_rank`, `alias_paths`) have serde defaults. No storage migration or public protocol DTO change was required.

## 8. Platform findings

Linux temporary-directory and symlink fixtures passed. Windows/macOS runtime qualification was not available locally. Config/home discovery uses `dirs` and vendor paths through `PathBuf`; no platform-specific hosted result is claimed.

## 9. Documentation

Updated source/precedence tables and global-root guidance in `architecture/skills.md`, `docs/agents-skills.md`, and `.opencode/skills/skills/SKILL.md`.

## 10. Deviations and remaining findings

The supported catalog is limited to documented, tested roots; Cline, Pi, Roo, Factory, Windsurf, and Devin remain disabled pending maintained evidence. Alias paths are retained for inspection but physical paths are not shown in `/skills`. No unresolved M002 correctness finding remains.

## 11. Dependency and registry disposition

M002 closed after M001 and unblocked M003. M003–M005 have since closed sequentially. Registry and roadmap record the completed line as closed.

## 12. Final disposition

M002 acceptance criteria are met. Closure accepted.
