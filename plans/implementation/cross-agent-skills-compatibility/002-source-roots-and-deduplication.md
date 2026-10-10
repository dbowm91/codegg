# Cross-Agent Skill Compatibility Milestone 002 — Home/project source roots and physical package deduplication

Status: blocked (hard prerequisite: previous milestone closure)
Repository baseline: `578b62bd9580382e00fb19e096fc1794e1f5618b` (2026-10-09; rebase/re-audit at execution)

Source roadmap: `plans/subsystems/cross-agent-skills-compatibility-roadmap.md` — M002.
Long-term requirements: `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`, `plans/001-terminology-and-domain-model.md`, `plans/003-planning-process.md`.
Applicable ADRs: none for the existing asset and skill authority; stop for an ADR if execution/trust/public protocol ownership changes.
Primary class: capability

## 1. Objective

Home/project source roots and physical package deduplication. Deliver one bounded, independently qualified contract without a second skill registry.

## 2. Why this milestone is planned, not yet ready

Hard dependency: accepted closure of M001. Existing `AssetRegistry` read-only sources, explicit workspace context and immutable snapshot are stable interfaces. The data-only source table may be designed earlier, but no execution until M001 is closed.

## 3. Current implementation evidence

`src/skills/registry.rs::resolve_source_roots` appends `codegg/skills`, `agents/skills`, `opencode/skills`, `claude/skills` to every provided global root, and `src/agent/asset_context.rs::default_global_discovery_root` returns only `dirs::config_dir()`. This misses typical home locations; `.agents/skills` is a symlink to `.opencode/skills` in codegg and is currently scanned twice. `SourceKind` is a public serializable enum with ordinal precedence; do not casually renumber variants.

## 4. Invariants that must not regress

- Resolve assets only from explicit project/workspace identity, not ambient process CWD.
- Skill discovery, metadata parsing and resource inventory never execute scripts or grant tool permissions.
- Preserve project-owned vs user-global precedence, configuration support, plugin namespacing, transactional publication and immutable turn snapshots.
- Foreign harness directories remain read-only. Fail closed on traversal, oversized/corrupt assets and unsafe symlink boundaries.
- No duplicate scheduler, privilege gate, global scanner or live network dependency.

## 5. Scope

**In scope:** the production work and tests explicitly listed below.

**Out of scope:** installing or editing external skills; vendor hook/command execution, provider feature work, broad new CI gate, duplicate asset snapshots, arbitrary descendant scanning, and changes to closed runtime-asset closure records.

## 6. Required production changes

**Source catalog:** compose explicit, bounded platform-aware candidate roots from injected `HOME`/user-profile, XDG config and platform config contexts; preserve existing legacy locations as compatibility inputs. Add native locations (project vs global): Codegg; Codex `.agents/skills`, optional documented legacy `.codex/skills`; Claude `.claude/skills`; OpenCode `.opencode/skills`, `~/.config/opencode/skills`; Cursor `.cursor/skills`; Gemini CLI `.gemini/skills`; GitHub Copilot `.github/skills`. Cover Cline, Pi, Roo and optional Factory/Devin/Windsurf only after official docs confirm path and scope; distinguish `skills-architect` mode-qualified subdirectories from universally active packages. **Identity:** deduplicate canonical roots and identical physical `SKILL.md` targets before parsing; preserve a deterministic chosen source label and display discoverable alias locations without fake shadows. **Precedence:** add stable precedence for new vendors and project vs global scope; avoid transient `HashMap`/equal-rank ordering; add tests for serialized historical values and configured roots. **Platform/security:** honor absent roots silently and unreadable existing roots diagnostically; cap candidate root count and prevent arbitrary parent traversal.

**Storage and migration:** prefer no migration; if a persisted enum/DTO changes, inspect existing serialization and use additive/versioned behavior with backward-compatible decoding before landing.

**Protocol:** reuse existing asset snapshot/refresh and read-only diagnostic surfaces; if a new public protocol or authority boundary is unavoidable, stop for an ADR.

**Runtime/concurrency:** preserve refresh coalescing and pinning. Expensive scans must be bounded and happen at refresh, never during palette keystrokes or each model token.

**Security/authorization:** metadata, `allowed-tools`, vendor execution hints and resource scripts are data only. Diagnostics must not contain credentials or sensitive file bodies.

## 7. Ordered work packages

**A — Path contract:** document source→scope→path matrix and fixture root builder with injectable home/config/OS; verify correct macOS OpenCode XDG discovery rather than erroneously only macOS application support. **B — Registry integration:** extend `SourceKind` or use a stable vendor descriptor without breaking serialization; adapt `resolve_source_roots`, all `SourceKind` classifications, snapshot summaries, collision/promotion checks and plugin sources. **C — Dedup and priority:** single canonical package across alias directories, stable shadow winner for same-name distinct packages, explicit `skills.paths` as directories not global parent. **D — Qualification:** two concurrent workspaces, restart, Linux/macOS/Windows simulated roots, permission denied/missing paths, symlink aliases and foreign-target escape.

## 8. Failure, cancellation, restart, and contention semantics

- Missing optional source dirs are expected absence; permission errors and malformed existing candidates get bounded diagnostics without aborting unrelated candidates.
- Invalid/failed refresh retains last valid published generation; a cancelled or superseded refresh cannot publish a partial candidate.
- Multiple concurrent refreshes coalesce under the existing coordinator; unchanged content and alias layouts produce stable digests, provenance and sorted output.
- Restart reconstructs equivalent effective assets from the same project, home/config roots and options; currently active turns remain pinned until completion.

## 9. Compatibility and migration

Retain native direct Markdown compatibility, source ordering previously promised, user-specified `skills.paths`, existing project assets and plugin contributions. Document any deliberate source precedence or strict-vs-compat parser change with fixture evidence. Do not force users to move files.

## 10. Required tests

**Fixture matrix:** Claude `~/.claude/skills`, Codex `~/.agents/skills`, OpenCode `~/.config/opencode/skills` on macOS, conventional XDG Linux and config fallback, Cursor/Gemini/Copilot project dirs, Windows profile/config variations. **Negative:** global vs project collision; same-name different packages; symlink aliases parsed once; unauthorized external link rejected; missing and unreadable roots; configurable source disable. **Regression:** historical Codegg/global/config/plugin precedence and serialized enum/DTO expectations, promotion collision check, no process-PWD leakage.

## 11. Required verification commands

~~~bash
# Select actual existing test target names from the repository at implementation time
cargo test --lib skills::
cargo test --lib agent::asset_
cargo fmt --all -- --check
scripts/verify.sh quick
cargo clippy --workspace --all-targets --all-features -- -D warnings
~~~

Use focused integration test targets where applicable. Run broader tests if the changes affect protocol/daemon/TUI. Record actual commands and exit codes; do not present unrun tests as passing.

## 12. Documentation updates

- `architecture/skills.md`; `docs/agents-skills.md`; compatibility source table and source ordering docs. Reconcile stale global root examples in agent docs.

## 13. Acceptance criteria

- Ordinary valid skills in the core supported vendor locations appear automatically with source provenance.
- One physical skill visible via multiple symlink aliases is parsed once, yet genuinely different same-name skills show deterministic shadow reports.
- No foreign source is written or executed; current Codegg installations and explicitly configured roots retain behavior.

## 14. Stop conditions

Stop if the source catalog requires unsafe recursive home scanning, undocumented vendor directories are being enabled unconditionally, historical serializations would break, or the needed symlink trust choice expands read authority without review.

## 15. Closure evidence required

Write `plans/closure/cross-agent-skills-compatibility/002-status.md` with implementation commits, requirement-to-test/guard matrix, actual commands and outcomes, adversarial fixtures, security/authorization and cross-platform findings, scope deviations, docs changes, remaining findings and registry/roadmap status. Do not mark closed based on compilation alone.

## 16. Handoff notes

Reinspect current files before edits; preserve unrelated user changes. Use closed Runtime Assets M001–M004 and the existing `AssetRegistry` as authority; this is an incremental successor. Do not close the next plan automatically without satisfying its dependency gate.
