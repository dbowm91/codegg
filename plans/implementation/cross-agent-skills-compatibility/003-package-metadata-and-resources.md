# Cross-Agent Skill Compatibility Milestone 003 — Vendor metadata compatibility and bounded nested resources

Status: implemented; closed (`plans/closure/cross-agent-skills-compatibility/003-status.md`)
Repository baseline: `578b62bd9580382e00fb19e096fc1794e1f5618b` (2026-10-09; rebase/re-audit at execution)

Source roadmap: `plans/subsystems/cross-agent-skills-compatibility-roadmap.md` — M003.
Long-term requirements: `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`, `plans/001-terminology-and-domain-model.md`, `plans/003-planning-process.md`.
Applicable ADRs: none for the existing asset and skill authority; stop for an ADR if execution/trust/public protocol ownership changes.
Primary class: infrastructure

## 1. Objective

Vendor metadata compatibility and bounded nested resources. Deliver one bounded, independently qualified contract without a second skill registry.

## 2. Why this milestone is planned, not yet ready

Hard dependency: M002 accepted closure. The expanded source descriptors supply vendor context and canonical package identities; `ResourceHandle` supplies existing bounded containment checks.

## 3. Current implementation evidence

`src/skills/parser.rs::PortableFrontmatter` requires name/description (non-Codegg sources); some Claude Code skills legitimately omit `name` and use directory identity. Serde ignores nonportable top-level extension fields. `inventory_resources()` currently enumerates immediate regular files, skipping `references/`, `assets/`, `scripts/`. `src/skills/resource.rs::ResourceHandle` already validates relative nested paths on read, rejects `..` and backslash separators, disallows external symlinks, and enforces byte bounds.

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

**Portable parser:** admit missing name only for documented vendor(s), deriving a validated directory basename; keep required descriptions and strict normalized valid-name constraints for standard packages while preserving explicit native compatibility. Preserve unknown extension metadata inertly with size budgets and diagnostic tags: unsupported command substitution, agent forks, disabled invocation, tools/mode hints do not execute or grant rights. **Resources:** index nested files under `references/`, `assets/`, `scripts/` with bounded max depth, max enumerated entries, max resources, aggregate metadata budget and per-file bounds; ensure visible inventory and actual `ResourceHandle` paths agree. **Symlink:** reject escape and cycles by default; allow external package symlinks only if approved in the explicit trusted source-root policy, with canonical package containment on all resource reads; never automatically loosen for arbitrary repositories. **Digest:** document whether resource-content changes affect refresh fingerprint; ensure changes to referenced files cannot invisibly stale a pinned generation, or explicitly constrain resource reads to validated generation guarantees.

**Storage and migration:** prefer no migration; if a persisted enum/DTO changes, inspect existing serialization and use additive/versioned behavior with backward-compatible decoding before landing.

**Protocol:** reuse existing asset snapshot/refresh and read-only diagnostic surfaces; if a new public protocol or authority boundary is unavoidable, stop for an ADR.

**Runtime/concurrency:** preserve refresh coalescing and pinning. Expensive scans must be bounded and happen at refresh, never during palette keystrokes or each model token.

**Security/authorization:** metadata, `allowed-tools`, vendor execution hints and resource scripts are data only. Diagnostics must not contain credentials or sensitive file bodies.

## 7. Ordered work packages

**A — Vendor conformance:** fixtures for Claude directory-derived names vs malformed generic portable packages; preserve legal portable `metadata` and additional fields as inert data. **B — Nested inventory:** recursive bounded traversal with deterministic order, no following external symlink/file descriptor escapes, stable relative descriptors; test script exposure as data only. **C — Digest/refresh:** decide and implement the minimum correct resource version behavior, testing active-turn pinning and changed-resource detection. **D — Negative qualification:** race/replacement, symlink chains/cycles, unicode/confusable names, over-depth/resources, oversized payload, permission denied nested paths, malicious README/tool hints.

## 8. Failure, cancellation, restart, and contention semantics

- Missing optional source dirs are expected absence; permission errors and malformed existing candidates get bounded diagnostics without aborting unrelated candidates.
- Invalid/failed refresh retains last valid published generation; a cancelled or superseded refresh cannot publish a partial candidate.
- Multiple concurrent refreshes coalesce under the existing coordinator; unchanged content and alias layouts produce stable digests, provenance and sorted output.
- Restart reconstructs equivalent effective assets from the same project, home/config roots and options; currently active turns remain pinned until completion.

## 9. Compatibility and migration

Retain native direct Markdown compatibility, source ordering previously promised, user-specified `skills.paths`, existing project assets and plugin contributions. Document any deliberate source precedence or strict-vs-compat parser change with fixture evidence. Do not force users to move files.

## 10. Required tests

**Focused:** nested `references/API.md` and `assets/template.json` visible/readable through safe handles; Claude name fallback only on supported vendor roots; unsupported vendor metadata survives introspection but cannot invoke tools. **Security:** external symlink, traversal, symlink swap, partial reads, size/count/depth caps, failed refresh, resource content generation. **Regression:** legacy `SkillIndex`, plugin-contributed packages, frozen tool permissions and checksum stability.

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

- `architecture/skills.md`, `docs/agents-skills.md`, `architecture/security.md` if the containment contract changes, and sample fixture docs.

## 13. Acceptance criteria

- Realistic portable skills with nested reference assets load and expose their intended bounded data.
- Every file read is subject to canonical containment and byte limits.
- Vendor-specific unsupported instructions are diagnosed as inert rather than silently honored; no skill content triggers a command.

## 14. Stop conditions

Stop if resource pinning cannot maintain immutable in-flight semantics, a TOCTOU gap requires an unowned filesystem authority, or granting vendor execution semantics would be needed for a skill to be labeled compatible.

## 15. Closure evidence required

Write `plans/closure/cross-agent-skills-compatibility/003-status.md` with implementation commits, requirement-to-test/guard matrix, actual commands and outcomes, adversarial fixtures, security/authorization and cross-platform findings, scope deviations, docs changes, remaining findings and registry/roadmap status. Do not mark closed based on compilation alone.

## 16. Handoff notes

Reinspect current files before edits; preserve unrelated user changes. Use closed Runtime Assets M001–M004 and the existing `AssetRegistry` as authority; this is an incremental successor. Do not close the next plan automatically without satisfying its dependency gate.
