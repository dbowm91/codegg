# Residual Runtime Consolidation Milestone 001 — Retire Stranded Coordination and Metadata Surfaces

Status: ready for handoff

Repository baseline: `9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6`

Source roadmap:

- `plans/subsystems/residual-runtime-consolidation-roadmap.md#M001--retire-stranded-coordination-and-metadata-surfaces`

Long-term requirements:

- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#7-current-foundation-and-required-evolution`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs: none.

Primary class: polish

## 1. Objective

Remove or explicitly justify the stranded filesystem-team and metadata-only `shell_session` surfaces, then reconcile architecture/skill documentation so future implementation agents cannot mistake them for canonical collaboration or PTY owners.

## 2. Why this milestone is ready

The durable agent-run/control, worktree, scheduler, projection, and managed-process owners are already closed. No hard dependency remains. This work changes no canonical architecture.

## 3. Current implementation evidence

`src/agent/mod.rs` declares `team`; the older file-backed team implementation uses inbox/outbox-style coordination. Adjacent `agent/teams.rs` and `tool/teams.rs` are not declared by their parent modules. `src/shell_session/` exposes an in-memory `ShellManager` and metadata DTOs but owns no PTY/process. `src/lib.rs`, architecture docs, and a skill still advertise that surface. Prior maintainability closure deferred its disposition.

## 4. Invariants that must not regress

- Durable agent-run/task/control remains the only agent coordination authority.
- Human collaboration must not be implemented by reviving filesystem inbox/outbox state.
- `Session` remains a human interaction domain object, not a terminal process.
- Process execution continues through existing managed-process/scheduler owners.
- Supported serialized/config/public compatibility is not removed without evidence and migration notes.

## 5. Scope

### In scope

Consumer census; deletion of unreferenced `agent/team.rs`, `agent/teams.rs`, `tool/teams.rs` or equivalent stranded files; disposition/removal of `shell_session`; public re-export cleanup; test/docs/skill cleanup; narrow negative guards against accidental reintroduction when cheap.

### Explicitly out of scope

Principal/team authorization, presence/chat, PTY implementation, daemon decomposition, protocol redesign, new compatibility shims.

## 6. Required production changes

### Core/domain

Delete obsolete implementations with no supported consumers. If a real public consumer is found, retain only a thin compatibility adapter and record its canonical replacement/removal condition.

### Storage and migrations

None expected. Stop if persisted team/shell-session data is discovered.

### Protocol and DTOs

Do not alter wire DTOs unless a removed type is proven serialized; split migration work if so.

### Runtime and concurrency

No new runtime owner. Remove associated locks/tasks only with the dead surface.

### Frontend or operator surface

Remove obsolete documentation/help references only; no new UI.

### Security and authorization

Do not widen any execution or collaboration authority.

### Documentation and static guards

Reconcile `architecture/core.md`, `architecture/overview.md`, `architecture/shell_session.md`, relevant skills, and removed-team references with current canonical owners.

## 7. Ordered work packages

### Work package A — Supported-consumer census

Search Rust module declarations/imports, tests, config, serde strings, protocol DTOs, docs, examples, and public re-exports. Produce an explicit keep/remove table in the closure record.

### Work package B — Remove stranded team surface

Delete undeclared/dead team files and any obsolete declared compatibility module only after the census proves it safe. Preserve durable run-control/team-independent behavior.

### Work package C — Dispose `shell_session`

Remove metadata-only types/manager/export/docs if no supported consumer exists. If retained, document why and prevent it from being described as PTY execution.

### Work package D — Documentation truth pass

Correct stale ownership claims and add a narrow regression guard only if simpler than ordinary compile/search evidence.

## 8. Failure, cancellation, restart, and contention semantics

This milestone must not change production failure/cancellation/restart/contention behavior. Any discovered live task or persisted recovery behavior means the surface is not dead and is a stop condition.

## 9. Compatibility and migration

Pre-1.0 internal dead files may be deleted. Public exports require a repository/downstream evidence search. Persisted/serialized names require explicit migration and are out of scope unless trivially retained as aliases.

## 10. Required tests

Focused compile/tests for affected modules; negative compile/search evidence that removed modules are not registered; existing agent run-control, managed-process, shell, and tool-registry tests sufficient to prove no canonical owner regressed.

## 11. Required verification commands

```bash
cargo test --workspace team --no-fail-fast
cargo test --workspace shell --no-fail-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Adjust focused filters to actual surviving test names; record exactly what ran.

## 12. Documentation updates

Update/remove architecture and skill files whose subjects disappear; correct overview/core ownership tables; do not rewrite historical closure records.

## 13. Acceptance criteria

No stranded undeclared team implementation remains; `shell_session` is either gone or has a demonstrated supported consumer and truthful contract; future docs point collaboration to principal/project/run/projection owners and interactive processes to the dedicated PTY workstream.

## 14. Stop conditions

Stop if a durable schema/wire contract depends on a candidate deletion, a supported downstream consumer requires behavior beyond a thin adapter, or removal would change process/agent authority.

## 15. Closure evidence required

Consumer census, deleted/retained surface matrix, exact tests/commands/results, doc changes, compatibility notes, and confirmation that no new coordination/process owner was introduced.

## 16. Handoff notes

Preserve unrelated user changes. Do not resurrect `agent/teams.rs` or `tool/teams.rs` merely because they contain useful-looking APIs; future team behavior has dedicated roadmaps.
