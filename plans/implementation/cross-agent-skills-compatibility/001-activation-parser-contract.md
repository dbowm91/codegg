# Cross-Agent Skill Compatibility Milestone 001 — Truthful activation and portable validation contract

Status: ready for handoff
Repository baseline: `578b62bd9580382e00fb19e096fc1794e1f5618b` (2026-10-09; rebase/re-audit at execution)

Source roadmap: `plans/subsystems/cross-agent-skills-compatibility-roadmap.md` — M001.
Long-term requirements: `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`, `plans/001-terminology-and-domain-model.md`, `plans/003-planning-process.md`.
Applicable ADRs: none for the existing asset and skill authority; stop for an ADR if execution/trust/public protocol ownership changes.
Primary class: invariant

## 1. Objective

Truthful activation and portable validation contract. Deliver one bounded, independently qualified contract without a second skill registry.

## 2. Why this milestone is ready

Hard dependencies Runtime Assets M001–M004 are closed. Model-facing skill tool and snapshot pinning already exist; this pass requires no external or downstream change.

## 3. Current implementation evidence

`src/skills/registry.rs::build_system_prompt` and legacy `src/skills/mod.rs::SkillIndex::build_system_prompt` tell models to use `/skill:<name>`. Actual tool is `src/tool/skill.rs` (name argument); `docs/agents-skills.md` states skills load via model rather than slash command. `architecture/skills.md` and `.opencode/skills/skills/SKILL.md` repeat the contradictory slash invocation. `src/skills/parser.rs` has portable vs native branches with permissive `normalize_name` and opaque unknown top-level fields.

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

**Core/agent:** update the two skill-list prompt sites and `src/agent/asset_snapshot.rs` integration assertions to instruct activation through `skill` tool with `{"name":"..."}`; no fictional TUI `/skill` command. Keep lazy body loading and digest pinning intact. **Parser:** introduce conformance fixtures/validation for the Agent Skills portable subset, distinguishing normative name syntax/directory relationship from existing native compatibility; do not silently reject previously valid Codegg content. Source-verification sections should point to real command and capability entry points. **Operator:** reconcile `architecture/skills.md`, `docs/agents-skills.md`, `AGENTS.md` and relevant `.opencode/skills/{skills,upgrade}/SKILL.md` without claiming a non-existent command.

**Storage and migration:** prefer no migration; if a persisted enum/DTO changes, inspect existing serialization and use additive/versioned behavior with backward-compatible decoding before landing.

**Protocol:** reuse existing asset snapshot/refresh and read-only diagnostic surfaces; if a new public protocol or authority boundary is unavoidable, stop for an ADR.

**Runtime/concurrency:** preserve refresh coalescing and pinning. Expensive scans must be bounded and happen at refresh, never during palette keystrokes or each model token.

**Security/authorization:** metadata, `allowed-tools`, vendor execution hints and resource scripts are data only. Diagnostics must not contain credentials or sensitive file bodies.

## 7. Ordered work packages

**A — Activation contract:** inventory every agent/user-facing `/skill:` reference; distinguish literal legacy/session syntax from false TUI/model instructions, update only the latter. Prove prompt construction surfaces and generated tool declarations agree. **B — Portable validation baseline:** fixture valid minimal portable package, invalid name/directory, native fallback, empty description, unknown metadata and plugin contributions; specify which failures are errors vs warnings without breaking existing installations. **C — Regression/docs:** add tests for legacy facade, pinned runtime prompt, actual `skill` activation and denial of tool permission escalation; synchronize docs and change-triggered guards.

## 8. Failure, cancellation, restart, and contention semantics

- Missing optional source dirs are expected absence; permission errors and malformed existing candidates get bounded diagnostics without aborting unrelated candidates.
- Invalid/failed refresh retains last valid published generation; a cancelled or superseded refresh cannot publish a partial candidate.
- Multiple concurrent refreshes coalesce under the existing coordinator; unchanged content and alias layouts produce stable digests, provenance and sorted output.
- Restart reconstructs equivalent effective assets from the same project, home/config roots and options; currently active turns remain pinned until completion.

## 9. Compatibility and migration

Retain native direct Markdown compatibility, source ordering previously promised, user-specified `skills.paths`, existing project assets and plugin contributions. Document any deliberate source precedence or strict-vs-compat parser change with fixture evidence. Do not force users to move files.

## 10. Required tests

**Focused:** prompt contains correct tool name and required argument; no `/skill:<name>` guidance; body not eagerly copied into system prompt; correct tool activation from pinned assets. **Negative:** `allowed-tools` cannot grant permission, malformed vendor metadata inert, native skill remains readable, direct .md support untouched. **Regression:** plugin namespacing and tool invocation still work.

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

- `architecture/skills.md`; `docs/agents-skills.md`; `.opencode/skills/skills/SKILL.md`; relevant `AGENTS.md` index and upgrade guide.

## 13. Acceptance criteria

- Both model prompt paths document the actual available `skill` tool; runtime tests prove activation and pinned body digest.
- No unsupported slash-command capability is advertised, and existing native packages still load.
- Conformance boundary is documented and backed by fixtures.

## 14. Stop conditions

Stop if changing the skill tool API is proposed without an accepted protocol/authority decision, or stricter parsing would silently remove existing user skills.

## 15. Closure evidence required

Write `plans/closure/cross-agent-skills-compatibility/001-status.md` with implementation commits, requirement-to-test/guard matrix, actual commands and outcomes, adversarial fixtures, security/authorization and cross-platform findings, scope deviations, docs changes, remaining findings and registry/roadmap status. Do not mark closed based on compilation alone.

## 16. Handoff notes

Reinspect current files before edits; preserve unrelated user changes. Use closed Runtime Assets M001–M004 and the existing `AssetRegistry` as authority; this is an incremental successor. Do not close the next plan automatically without satisfying its dependency gate.
