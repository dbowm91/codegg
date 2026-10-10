# Repository Initialization Milestone 001 — Bounded Evidence and AGENTS.md Candidate Draft

Status: ready for handoff
Repository baseline: `578b62bd9580382e00fb19e096fc1794e1f5618b` (2026-10-09; re-audit at execution)
Source roadmap: `plans/subsystems/repository-initialization-roadmap.md#7-milestones`
Long-term requirements: `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`; `#27-security-requirements`; `#29-system-invariants`; `plans/001-terminology-and-domain-model.md`; `plans/003-planning-process.md`.
Applicable ADRs: existing Runtime Assets and explicit-project contracts; this read-only milestone needs no new public protocol.
Primary class: infrastructure

## 1. Objective

Implement a bounded, passive repo analyzer and inert AGENTS.md candidate generator for the **explicitly selected workspace root**, reusable by a later /init command. This milestone reads evidence and returns a candidate only: no slash command, file write, Git init, tool execution, provider call or network lookup.

## 2. Why ready

Runtime Assets M001–M004, the explicit project identity contract, `ProjectInstructionResolver`, and immutable `ProjectAssetSnapshot` are closed. This milestone is independent of new cross-agent skills discovery. The contract adds no new execution authority or protocol.

## 3. Current implementation evidence

`src/agent/instructions.rs` reads root/ancestor AGENTS.md and local instruction files from `AssetContext`, and `src/agent/asset_refresh.rs` owns refresh. `src/tui/command.rs` has no /init command. The `crates/codegg-document` and daemon document paths may be reusable later but do not confer write authority for M001.

## 4. Invariants

- Only the selected ProjectId/workspace context determines the target, never ambient CWD or the previous TUI tab.
- Never read private global instructions, ~/.claude, ~/.codex personal data, credentials, .env, .git/config, secret stores, environment dumps or sibling projects; protect even when present in fixture repos.
- Treat README/manifests/CI/scripts as **untrusted data**: never execute discovered commands or obey embedded injection instructions.
- Distinguish observed manifest facts from inferred advice or unknowns; never claim tests passed without execution evidence.
- No writes, migrations, permission expansion, agent loop or skill activation.

## 5. Scope

In scope: read-only bounded inspection and proposal generation for AGENTS.md; handle both absent and already present target. Out of scope: /init dispatch, model-assisted drafting, Git init, project scaffolding, external rule-file edits, auto-tests, packages or global instruction copy.

## 6. Required production changes

**Core/domain.** Add one reusable source-aware `ProjectBootstrapEvidence` / `InstructionDraft`-equivalent module co-located with project instructions. Inputs: authoritative workspace/project identity, exact root, scan budget. Output: target relative `AGENTS.md`, create/update/no-op operation, exact observed target digest or explicit absent marker, candidate Markdown, bounded evidence items (relative source path, observed/inferred/unknown classification) and diagnostics. No raw secret excerpts. Preserve user-authored content.

**Detection.** Read a small documented allowlist: root README, Cargo.toml, pyproject.toml, package.json, go.mod, known test/CI manifests and local AGENTS/project instructions, with bounded path depth/count/bytes/total/time. Respect ignores; avoid vendor/cache/build/output trees. Manifest script existence may be stated as an *available script*, not a verified successful command. An empty repo gets a minimal honest starter rather than invented commands. Monorepo target is the selected workspace root's AGENTS.md, never an ancestor unless the user explicitly selected it.

**Candidate.** Short useful sections: project facts with sources, verified-vs-unverified build/test invocations, conventions inferred only when defensible, review TODOs. On existing AGENTS.md propose a conservative section merge/diff, preserve unknown sections/line endings, and produce an idempotent no-op on unchanged evidence. Include digest/identity ready for downstream CAS.

**Storage/protocol.** Inert in-memory values only, no database or CoreRequest change; do not replace existing runtime instruction resolver/refresh.

**Security.** Canonical root bounds, reject external symlinks and binary/oversized inputs, capped diagnostics, deterministic stable scan ordering; hostile README content remains literal evidence and cannot direct scanning or copy sensitive files.

## 7. Ordered work packages

A. Define read-only typed proposal/evidence/skip DTO and bounded source/path budgets with fixture root injection.
B. Implement manifest, README, local instruction observation for empty/Rust/Python/JS/Go/mixed and nested workspaces; annotate source and confidence.
C. Implement conservative Markdown draft, exact original target digest, preservation and no-op/idempotence.
D. Add adversarial and privacy tests, documentation and focused verification. Do not register /init before M002.

## 8. Failure, cancellation, restart and contention

Read failure yields an explicit skip/unknown rather than a fabricated result. If a file changes during scan, mark candidate stale or require re-analysis. Cancellation drops the in-memory proposal; concurrent scans never mutate assets. Restart needs no recovery or replay, and no old generation is changed.

## 9. Compatibility and migration

No existing AGENTS.md, CLAUDE.md, .agents skills or project instructions are modified; no config/schema migration. Proposed data is not published as effective runtime instructions until a later authorized file write and refresh.

## 10. Required tests

- Unit: bounded scan ordering, manifest fact classification, stable digest, deterministic candidate output, no-op, merge preservation, CRLF handling.
- Integration: empty repository, several language manifests, existing human-edited AGENTS, selected nested workspace, Git worktree .git file, independent projects.
- Security: secret decoys in .env and home global instructions, hostile README text, path/symlink escapes, binary/oversized files, permission errors; no shell side effects.
- Qualification: no incorrect assertion of tests performed or provider/model connection needed.

## 11. Required verification commands

~~~bash
cargo test --lib agent::instructions
cargo test --lib bootstrap
cargo fmt --all -- --check
scripts/verify.sh quick
cargo clippy --workspace --all-targets --all-features -- -D warnings
~~~

Select actual new test target/filter names at implementation; record exact results and broader runs as needed.

## 12. Documentation updates

Document draft semantics in the existing instructions architecture doc and a focused `docs/repository-init.md` draft/usage note. Do **not** call /init implemented before M002.

## 13. Acceptance criteria

The no-write candidate is deterministic, concise, source-attributed, useful without a provider, non-destructive for existing instructions, and provably excludes private/global data. Its target digest and root identity make safe downstream publication possible.

## 14. Stop conditions

Stop if implementation requires cwd inference, a hidden global prompt, uncontrolled model execution, persistent storage or any write/protocol/authorization change; obtain explicit design/ADR rather than improvising.

## 15. Closure evidence required

Write `plans/closure/repository-initialization/001-status.md` with commits, observed-vs-inferred samples, no-write/privacy/hostile-file fixtures, tests/commands actually run, deviations and residual findings; update roadmap and registry and only then unblock M002.

## 16. Handoff notes

A provider-independent, deterministic default is required. M002 may add opt-in model-assisted enhancement only with isolated allowlisted evidence and the same privacy/approval contract. Preserve unrelated user work.
