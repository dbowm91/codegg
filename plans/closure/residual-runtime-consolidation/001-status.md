# Residual Runtime Consolidation Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/residual-runtime-consolidation/001-retire-stranded-coordination-and-metadata-surfaces.md`

Source subsystem roadmap:

- `plans/subsystems/residual-runtime-consolidation-roadmap.md#M001--retire-stranded-coordination-and-metadata-surfaces`

Repository baseline reviewed: `47953a2caa84646dfb61e501f2d3cb4b5424b7d4`

Implementation commits or pull requests:

- Implementation, closure record, and registry updates land in a single commit titled "plans: close residual-runtime M001, retire stranded team/shell-session surfaces" (this record's own commit; locate with `git log --oneline --grep="close residual-runtime M001"`) — retire stranded team/shell-session surfaces, reconcile docs, close M001

## 1. Executive finding

M001 is complete. The stranded filesystem-team coordination surface
(`src/agent/team.rs`, `src/agent/teams.rs`, `src/tool/teams.rs`) and the
metadata-only `src/shell_session/` surface (plus its public re-export,
architecture doc, and skill) have been deleted after an explicit
supported-consumer census proved they had no production, protocol,
config, persisted-data, or downstream consumers. Documentation now points
collaboration at the canonical durable agent-run/control, principal,
project-channel, and projection owners, and interactive processes at the
dedicated interactive-process subsystem. No canonical architecture
changed and no new coordination or process owner was introduced.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Work package A: supported-consumer census over module declarations/imports, tests, config, serde strings, protocol DTOs, docs, examples, public re-exports | §3 census table; `rg` sweeps over `src/`, `tests/`, `crates/`, `examples/`, protocol sources (§4) | pass | `codegg-core::team` (TeamStore/principals) is the canonical authorization owner and is explicitly out of scope; untouched |
| Work package B: delete undeclared/dead team files only after census proves safety; preserve durable run-control behavior | `git rm` of `src/agent/team.rs`, `src/agent/teams.rs`, `src/tool/teams.rs`; removal of `pub mod team` from `src/agent/mod.rs` | pass | `agent/teams.rs` and `tool/teams.rs` were never declared by their parent modules (dead files); `agent/team.rs` had exactly one consumer (the dead `agent/teams.rs`) |
| Work package C: dispose `shell_session` (remove, or retain with demonstrated consumer + truthful contract) | `git rm` of `src/shell_session/{mod,session}.rs`; removal of `pub mod shell_session` from `src/lib.rs` | pass | Removed: in-memory only, zero production consumers, no persistence, no wire DTO, no config keys |
| Work package D: documentation truth pass; narrow guard only if simpler than compile/search evidence | Edits to `architecture/{agent,core,overview,tool}.md`, `architecture/process-tool-execution-ownership.md`, `AGENTS.md`, `human-shell` + `architecture-review` skills; deletion of `architecture/shell_session.md` and the `shell_session` skill | pass | No new static guard: removal is enforced by compilation (any re-added `pub mod` reference fails) plus recorded search evidence; adding a CI lane would violate the registry verification policy |
| Acceptance: no stranded undeclared team implementation remains | `ls` confirms files gone; `rg` over `src/ tests/ crates/` for `crate::agent::team`, `crate::agent::teams`, `crate::tool::teams`, `crate::shell_session`, `team_create`, `TeamTools`, `create_team_handles` returns zero matches | pass | §4 |
| Acceptance: `shell_session` gone or justified with consumer + contract | Gone; census shows no consumer (§3) | pass | — |
| Acceptance: docs point collaboration to canonical owners and processes to the PTY workstream | `tool.md` and `process-tool-execution-ownership.md` retain bounded historical notes naming the removal; `overview.md`/`core.md`/`agent.md` ownership tables corrected | pass | Historical closure records untouched |

## 3. Production implementation evidence

### Consumer census (keep/remove table)

| Surface | Declared? | Consumers found | Disposition |
|---|---|---|---|
| `src/agent/team.rs` — file-backed `Team`/`TeamMessage`/`AgentRole` inbox/outbox | Yes (`pub mod team` in `src/agent/mod.rs`) | Exactly one: `src/agent/teams.rs` (itself stranded). No refs in `src/` production code, `tests/`, `crates/` (incl. `codegg-protocol`), `examples/`, config keys, or serde/persisted names. `crates/codegg-core/src/collaboration.rs::send_message` and `src/core/daemon.rs` hits are the unrelated canonical collaboration/daemon paths | **Removed** |
| `src/agent/teams.rs` — `TeamManager`, `SharedTaskList`, `IdleNotifier`, `GracefulShutdown`, `TeamCreateTool`/`SendMessageTool`/`ListMessagesTool`/`TeamStatusTool`/`ListTeamsTool` | No (absent from `src/agent/mod.rs`; file never compiled) | Exactly one: `src/tool/teams.rs` (itself stranded) | **Removed (dead file)** |
| `src/tool/teams.rs` — `TeamTools::register_all`, `create_team_handles` | No (absent from `src/tool/mod.rs`; file never compiled) | Zero. `ToolRegistry::with_options()` never registers `team_create`/`send_message`/`list_messages`/`team_status`/`list_teams` | **Removed (dead file)** |
| `src/shell_session/` — `ShellSession`/`CreateShellSession`/`ShellResize` DTOs + `ShellManager` in-memory CRUD | Yes (`pub mod shell_session` in `src/lib.rs`) | Zero production consumers (only its own unit tests, docs, and skill). No protocol DTO in `codegg-protocol`, no config keys, no persistence (ephemeral `HashMap`, lost on restart) | **Removed** (`mod.rs` + `session.rs`, module dir gone) |
| `codegg-core::team` — `TeamStore`, principals, memberships, capabilities | Yes (canonical) | Pervasive (authorization, transport auth) | **Retained** — out of scope per plan §5 (principal/team authorization) |

Persisted-data check: the file-backed team wrote to `<base>/.opencode/team/{inbox,outbox}` at runtime only; no migration, schema table, or stored-run format references it. `shell_session` never touched disk. No storage or migration work required; none performed.

### Landed changes

- Deleted: `src/agent/team.rs`, `src/agent/teams.rs`, `src/tool/teams.rs`, `src/shell_session/mod.rs`, `src/shell_session/session.rs`.
- `src/agent/mod.rs`: removed `pub mod team;` (module-root declaration surface only; no other edits).
- `src/lib.rs`: removed `pub mod shell_session;`.
- Deleted: `architecture/shell_session.md`, `.opencode/skills/shell_session/SKILL.md` (`.agents/skills` is a symlink to `.opencode/skills`, so both views resolve).
- Corrected: `architecture/agent.md` (dropped team rows from module table), `architecture/overview.md` (Agent row, TUI-layer table, nav index, directory layout), `architecture/core.md` (root-side module rows), `architecture/tool.md` (run-control authority note now names the M001 removal), `architecture/process-tool-execution-ownership.md` (shell-session disposition in past tense), `AGENTS.md` (skills index row), `.opencode/skills/human-shell/SKILL.md` and `architecture-review/SKILL.md` (dropped shell-session references).
- No new runtime owner, lock, task, protocol variant, config key, or compatibility shim introduced.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg --lib team
cargo test -p codegg --lib shell_session
cargo test -p codegg --lib agent::
cargo test -p codegg --lib shell::
cargo test -p codegg --lib tool::
cargo test -p codegg --lib managed_process
cargo test -p codegg --lib interactive_process
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Focused `team`/`shell` filters from the plan were adapted: the plan's
`cargo test --workspace team` / `--workspace shell` syntax does not match
this repo's test layout, so the equivalent focused filters
(`-p codegg --lib team`, `--lib shell_session`, plus the canonical-owner
suites for agent run-control, human shell, tool registry, managed
process, and interactive process) were run instead and recorded here.

### Results

| Command | Result |
|---|---|
| `cargo test -p codegg --lib team` | pass — 0 matched, 4405 filtered (removed surface has no tests left) |
| `cargo test -p codegg --lib shell_session` | pass — 0 matched, 4405 filtered (removed surface has no tests left) |
| `cargo test -p codegg --lib agent::` | pass — 308 passed, 0 failed |
| `cargo test -p codegg --lib shell::` | pass — 379 passed, 0 failed |
| `cargo test -p codegg --lib tool::` | pass — 546 passed, 0 failed |
| `cargo test -p codegg --lib managed_process` | pass — 13 passed, 0 failed |
| `cargo test -p codegg --lib interactive_process` | pass — 22 passed, 0 failed |
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass — zero warnings |
| `scripts/verify.sh quick` | pass (builtin-agents check, core-boundary, sandbox contract, execution-ownership, workspace check incl. `cargo ckroot` equivalent) |
| Negative file check (`ls` of the five removed paths) | pass — all absent |
| Negative import sweep (`rg` for `crate::agent::team(s)`, `crate::tool::teams`, `crate::shell_session`, `team_create`, `TeamTools`, `create_team_handles` over `src/ tests/ crates/`) | pass — zero matches |

All verification is local; no `CI / verify` hosted evidence is claimed.

## 5. Invariant review

| Plan invariant | Evidence it remains true |
|---|---|
| Durable agent-run/task/control remains the only agent coordination authority | `codegg_core::agent_run_control` untouched; `src/agent/run_control.rs` bridge untouched; `agent::` suite (308 tests) passes; `tool.md` now states run control is the only authority |
| Human collaboration must not be implemented by reviving filesystem inbox/outbox state | The inbox/outbox implementation is deleted, not deprecated; negative sweep proves no revival path remains in-tree |
| `Session` remains a human interaction domain object, not a terminal process | Untouched; interactive-process M001–M003 closures already establish the PTY/session separation and still pass (22 tests) |
| Process execution continues through existing managed-process/scheduler owners | `managed_process` (13 tests) and scheduler guards (`check_execution_ownership.py`, `check_scheduler_bypass.py` via `verify.sh quick`) pass |
| Supported serialized/config/public compatibility is not removed without evidence and migration notes | Census found no wire DTO, config key, migration, or persisted name for any removed surface (§3); pre-1.0 internal dead files deleted per plan §9; `codegg-core::team` public API untouched |

## 6. Failure and recovery review

This milestone changes no production failure, cancellation, restart, or
contention behavior (plan §8). There is no live task, persisted recovery
state, or durable queue behind any removed surface:

- The file-backed team performed synchronous `fs::` inbox reads/writes with no daemon recovery, generation, or replay — nothing to drain or migrate.
- `ShellManager` was an ephemeral in-memory map with no restart path — nothing survives a restart to reconcile.
- No locks, broadcast channels, background tasks, or scheduler permits were owned by the removed modules (the `TeamManager` broadcast/`RwLock` state lived in the never-compiled dead file).
- The stop condition (discovered live task or persisted recovery behavior) never triggered.

## 7. Migration and compatibility review

- Schema migrations: none (no tables referenced the removed types; `STORAGE_LAYOUT_VERSION` unchanged).
- Wire protocol: unchanged (`codegg-protocol` has no `TeamMessage`/`ShellSession` DTO; sweep confirms).
- Configuration: no keys added, removed, or renamed.
- Stored runs: historical-name readers intentionally retained elsewhere (e.g. `multiedit` history) are unaffected; no stored-run format referenced the removed team/shell-session types.
- Rollback: re-creating the deleted files would fail compilation until the `pub mod` declarations are also restored, which the negative sweep and review would catch; no runtime rollback path is needed for a deletion with zero consumers.

## 8. Security review

- No execution or collaboration authority widened: five model-tool registrations (`team_create`, `send_message`, `list_messages`, `team_status`, `list_teams`) never existed in the live registry, so the model tool surface is byte-identical before/after.
- No new filesystem, network, or process-spawn site: deletions only; `check_execution_ownership.py` passes.
- No secret, credential, or redaction boundary touched; no authorization seam changed (`codegg-core::team`, permission registries, daemon gates untouched).
- No denial-of-service surface change: the removed `fs::read_dir` inbox scans and unbounded in-memory map are gone, strictly reducing latent resource exposure.

## 9. Documentation and operations

- Updated: `architecture/agent.md`, `architecture/core.md`, `architecture/overview.md`, `architecture/tool.md`, `architecture/process-tool-execution-ownership.md`, `AGENTS.md` (skills index), `.opencode/skills/human-shell/SKILL.md`, `.opencode/skills/architecture-review/SKILL.md`.
- Removed: `architecture/shell_session.md`, `.opencode/skills/shell_session/SKILL.md`.
- Historical closure records untouched (no rewrite of predecessor conclusions).
- No new operator diagnostics, static guards, or recovery instructions required. The deliberate non-addition of a guard is recorded: reintroduction is caught by compilation plus the §4 negative sweep, and a new CI lane would violate the registry verification policy.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None | — | — |

No critical/high/medium/low findings remain. The plan's handoff note
("do not resurrect `agent/teams.rs` or `tool/teams.rs` for their
useful-looking APIs") is honored: nothing from the deleted files was
reintroduced elsewhere; future team behavior stays with its dedicated
roadmaps.

## 11. Roadmap disposition

Milestone closed and next dependency may proceed. M001 closure unblocks
nothing new: per the roadmap dependency graph, M003's hard dependency is
M002 (not M001), and M002 was already `ready` and independent. M002
remains `ready`; M003 remains `blocked` on M002.

## 12. Registry updates

- `plans/registry.md`: residual-roadmap row `M001/M002 ready` → `M001 closed, M002 ready`; M001 row removed from dependency-ready table with its closure recorded in the control-points table; execution-order gate 1 updated to reflect M001 closed; blocked-work rows unchanged (M003 still blocked on M002; no plan listed M001 as a hard/interface dependency, so the unblock audit moves nothing to `ready`).
- `plans/subsystems/residual-runtime-consolidation-roadmap.md`: M001 row `ready` → `closed` with closure-record link.
- `plans/implementation/residual-runtime-consolidation/001-retire-stranded-coordination-and-metadata-surfaces.md`: status `ready for handoff` → `implemented` (closed via this record).
