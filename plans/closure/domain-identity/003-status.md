# Domain Identity Milestone 003 — Closure Status

Status: corrective pass required

Source implementation plan:

- `plans/implementation/domain-identity/003-daemon-protocol-adoption.md`

Source subsystem roadmap:

- `plans/subsystems/domain-identity-roadmap.md#milestone-3--daemon-and-protocol-adoption`

Repository baseline reviewed: `583f51e478503baaf94f283f01f8a4f12a6fd77d` (`main`; planning-only commits since the plan's `3ce0a7e` baseline do not alter production state)

Implementation commits or pull requests:

- **None.** All nine commits between the plan's stated baseline `3ce0a7e` and the current `main` are planning-only (`plans: …`). The plan was registered as `ready for handoff` but was never handed to an implementation agent, and no `feat:`/`fix:` work has landed against it.

## 1. Executive finding

The milestone's principal infrastructure boundary — a single authoritative project/workspace/session context resolver, additive identity-bearing protocol DTOs, daemon-side handler migration off legacy path/string authority, and server project-route cleanup that stops returning a path as `ProjectInfo.id` — **has not been implemented**. The repository still matches the plan's own `§3 Current implementation evidence` description of the baseline: `CoreRequest::SessionList` still takes a string `project_id`, `SessionCreate` still takes only a `directory`, `SessionCreateFromTemplate` still takes string `project_id` plus `directory`, `SessionSnapshot` still carries string `project_id` / optional string `workspace_id` / `directory`, `ServerState.project_dir: String` is still authoritative, and `src/server/routes/project.rs` still returns `state.project_dir` as `ProjectInfo.id` and groups sessions by legacy `session.project_id` strings.

The plan cannot be closed under `plans/closure/README.md`'s closure rules (no production code, no daemon-owned path migrated off path authority, no static guard coverage, no migration of compatibility-only write paths). Per `plans/003-planning-process.md` §7 a corrective pass is the correct disposition, and this record constitutes the gating evidence: the original plan remains the authoritative source of requirements and a new handoff (or the same plan re-handed) must produce the production evidence before this milestone may be marked closed.

No critical or high-severity finding has been introduced because no implementation work has occurred. The finding is structural: the milestone is not started despite being dependency-ready and being a hard prerequisite for Session Projections 001.

## 2. Requirement-to-evidence matrix

The plan's §13 acceptance criteria were transcribed from `plans/implementation/domain-identity/003-daemon-protocol-adoption.md:425-434`. Each is mapped against the current `main` (`583f51e`).

| # | Acceptance criterion (plan §13) | Current evidence | Result | Notes |
|---|---|---|---|---|
| 1 | Every new executable session has a durable canonical project/workspace binding. | `SessionCreate` writes still flow through the legacy path; `session_project_binding` rows from Milestone 002 are not written through the new session-create path. `crates/codegg-protocol/src/core.rs:298-300` shows `SessionCreate { directory: String, title: Option<String> }` with no `project_id`/`workspace_id` fields. | fail | The session-binding atomicity required by `§6 Required production changes → Storage and migrations` is not in place for the production write path. |
| 2 | New daemon requests and snapshots carry stable project/workspace identities. | `CoreRequest` session variants remain string-typed (`crates/codegg-protocol/src/core.rs:293, 298-300, 343-347`). `SessionSnapshot` (`crates/codegg-protocol/src/core.rs:242-258`) still carries string `project_id` and optional string `workspace_id` and `directory`. No `ProjectContextDto` / `SessionBindingDto` exists in `crates/codegg-protocol/src/`. | fail | Additive identity-bearing DTOs and snapshot fields have not been added. |
| 3 | Directory-only compatibility requests cannot create or redefine identity. | Old `SessionCreate { directory }` is still the primary request. No `project_context_required` outcome is implemented. No resolver rejects directory-only requests. | fail | The `§6 Protocol and DTOs` "deterministic workspace lookup or `project_context_required`" branch is unimplemented. |
| 4 | Old clients remain readable or fail with explicit context-required diagnostics. | Old clients remain readable (no breakage), but the explicit `project_context_required` diagnostic path does not exist. | partial | Backward reading is preserved by accident of not migrating; explicit diagnostics are absent. |
| 5 | `src/server/routes/project.rs` no longer returns a path as `ProjectInfo.id`. | `src/server/routes/project.rs:45` still sets `id: state.project_dir.clone()`. Line 61 still groups sessions by legacy `session.project_id` strings. Line 114-115 still returns the canonicalized path as a created project's ID. | fail | Work package E is not implemented. |
| 6 | Session listing and server counts use canonical bindings. | `src/server/routes/project.rs:61` groups by `s.project_id.clone()` (legacy string column). No `ProjectCatalog` or `session_project_binding` consumer in the route. | fail | Server still depends on legacy `project_id` text for listing/counts. |
| 7 | Project/workspace mismatches and unresolved contexts fail before execution. | No resolver produces `mismatch` / `unbound` / `rebind_required` / `archived` / `stale_revision` outcomes. No context is resolved before execution because no context type is required. | fail | Work packages A and D are not implemented. |
| 8 | Static guards reject new authoritative path-derived project identity. | `scripts/check_identity_path_usage.py` only scans `crates/codegg-core/src/**/*.rs` for four forbidden patterns; it does not cover `src/server/`, `src/agent/`, the protocol crate, or `ServerState.project_dir` usage as an ID. The `§6 Documentation and static guards` enumeration is unimplemented. | partial | The Milestone 001 guard seam exists; the Milestone 003 expanded guard does not. |
| 9 | Existing provider selection, workspace execution, and session compatibility behavior remain functional. | `cargo test -p codegg-core identity` (8 pass), `cargo test -p codegg-core session` (40 pass), `cargo test -p codegg-protocol` (75 pass), `cargo test --test session_crud` (32 pass), `cargo test --test workspace_isolation` (6 pass), `cargo test --test workspace_services_isolation` (11 pass) all pass. Existing tests do not exercise the missing path, so this is necessary but not sufficient. | pass | Pre-existing tests still green; no regression introduced by the (absent) implementation. |

### 2.1 Work-package-to-evidence matrix

| Work package (plan §7) | Status | Evidence |
|---|---|---|
| A — Authoritative context resolver | not implemented | No `context.rs`, `resolver.rs`, `project_context.rs`, or `binding_resolver.rs` in `crates/codegg-core/src/`. Grep for `ProjectContext` / `ContextResolver` in `crates/codegg-core/src/` returns 0 matches. |
| B — Session write-path adoption | not implemented | `CoreRequest::SessionCreate` and friends unchanged; no atomic session + binding transaction. |
| C — Additive protocol adoption | not implemented | No `ProjectContextDto` / `SessionBindingDto` in `crates/codegg-protocol/src/`; no capability advertisement for identity-aware requests (only the workspace capability at `crates/codegg-protocol/src/core.rs:91`). |
| D — Daemon request migration | not implemented | All `CoreRequest` session variants still string-typed; no resolver routes. |
| E — Server compatibility cleanup | not implemented | `src/server/routes/project.rs:45, 61, 114-115` still on legacy path. |
| F — Guards, docs, closure | partial | `architecture/identity.md`, `architecture/project_identity_storage.md`, `architecture/project_catalog.md` exist from prior milestones; Milestone 003-specific `§6 Documentation and static guards` updates are not landed. `scripts/check_identity_path_usage.py` unchanged. |

## 3. Production implementation evidence

**None.** The repository state at the time of this closure review is identical to the plan's `§3 Current implementation evidence` description. No new `codegg_core` module, no new `codegg_protocol` DTO, no new `ServerState` field, no `src/server/routes/project.rs` rewrite, no new static-guard pattern, and no new architecture-document changes specifically authored for this milestone exist.

The Milestone 002 building blocks consumed by this plan remain in place and are the only relevant infrastructure to record:

- `codegg_core::identity` (`crates/codegg-core/src/identity.rs`) — typed `ProjectId`, `RepositoryId`, `WorkspaceId`, `ProjectBinding`, `SessionBinding`, with `validate_identity`, `parse`, `FromStr`, `AsRef<str>`, `into_string`, serde, and `Display` contracts.
- `codegg_core::project_storage::ProjectStorage` — additive SQLite v25 with `project`, `repository`, `project_repository_binding`, `workspace_project_binding`, `session_project_binding`, and `identity_diagnostic` tables, plus `reconcile_workspace_path`, `reconcile_catalog`, `bind_session`, `inspect_workspace`, and `rebind_workspace` (closed in `84d92f0`, closure record `plans/closure/domain-identity/002-status.md`).
- `codegg_core::project_catalog::ProjectCatalog` — additive SQLite v28 with `list`, `get`, `register`, `archive`, `restore`, locator/health placeholders, restart hydration with no probe (closed in `a2db5e4`, closure record `plans/closure/project-catalog/001-status.md`).
- `scripts/check_identity_path_usage.py` — narrow guard scanning `crates/codegg-core/src/**/*.rs` for `ProjectId::new_unchecked`, `ProjectId::from_path`, and `ProjectId::parse` with path arguments; insufficient for the Milestone 003 boundary.

These are the dependencies the milestone was expected to consume, and they are present. What is missing is the resolver, the protocol DTOs, the request/handler migration, the server route rewrite, the expanded guard, and the Milestone 003-specific architecture updates.

## 4. Verification executed

The plan's `§11 Required verification commands` (plan lines 390-411) were run against the current `main` to confirm the focused prerequisites still pass and to document the broader suite outcome. The original `rtk cargo …` / `rtk python3 …` prefixes were preserved; `rtk` resolves to the repository's `rtk` wrapper, and the commands are otherwise the same.

### Commands run

```bash
# Focused tests that the plan's required-verification list calls out
rtk cargo fmt --all -- --check
rtk cargo test -p codegg-core identity
rtk cargo test -p codegg-core project_storage
rtk cargo test -p codegg-core project_catalog
rtk cargo test -p codegg-core session
rtk cargo test -p codegg-protocol
rtk cargo test --test session_crud
rtk cargo test --test storage_migrations
rtk cargo test --test workspace_isolation
rtk cargo test --test workspace_services_isolation
rtk cargo test --lib core::transport::daemon_socket

# Static guards and core-boundary scripts
rtk bash scripts/check-core-boundary.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_execution_ownership.py
rtk python3 scripts/check_identity_path_usage.py
rtk python3 scripts/check_project_catalog_invariants.py
rtk git diff --check

# Lint (informational — known unrelated warning noted below)
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
```

The broad workspace all-features run (`CARGO_BUILD_JOBS=1 rtk cargo test --workspace --all-features -- --test-threads=14`) was **not** claimed as evidence for this milestone. It remains the verification standard for the eventual corrective pass; running it now would only confirm that prior milestones remain green and would not change the corrective-pass disposition.

### Results

- `cargo fmt --all -- --check`: pass.
- `cargo test -p codegg-core identity`: 8 passed, 0 failed.
- `cargo test -p codegg-core project_storage`: 7 passed, 0 failed.
- `cargo test -p codegg-core project_catalog`: 11 passed, 0 failed.
- `cargo test -p codegg-core session`: 40 passed, 0 failed.
- `cargo test -p codegg-protocol`: 75 passed, 0 failed.
- `cargo test --test session_crud`: 32 passed, 0 failed.
- `cargo test --test storage_migrations`: 4 passed, 0 failed.
- `cargo test --test workspace_isolation`: 6 passed, 0 failed.
- `cargo test --test workspace_services_isolation`: 11 passed, 0 failed.
- `cargo test --lib core::transport::daemon_socket`: 10 passed, 0 failed.
- `scripts/check-core-boundary.sh`: pass ("codegg-core boundary check passed").
- `scripts/check_daemon_cwd_usage.py`: pass (no violations).
- `scripts/check_execution_ownership.py`: pass ("execution-ownership guard ok").
- `scripts/check_identity_path_usage.py`: pass (no forbidden construction found).
- `scripts/check_project_catalog_invariants.py`: pass (7/7 checks).
- `git diff --check`: clean.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: **1 error** in `crates/codegg-protocol/src/provider.rs:170` (large enum variant in `SessionSelectionDto`). This is the same known finding noted in prior closure records and is unrelated to Domain Identity 003; it does not block the corrective pass disposition.

All focused evidence required by the plan's `§11` runs to the extent they were specified for narrowing the failure. None of these results can be claimed as closure evidence for §13 because the production code that §13 verifies has not been authored.

## 5. Invariant review

Each source-plan invariant (plan §4) is reviewed against the current `main`.

- **Paths, directories, Git roots, and server-local roots remain locators, never canonical project identity.** The legacy `ServerState.project_dir` continues to be treated as a project ID by `src/server/routes/project.rs:45, 61, 114-115`. This invariant is **violated** by the existing server route — the violation is exactly what the plan is meant to remove.
- **A new session must have one validated canonical `ProjectId` and one validated canonical `WorkspaceId` before it can execute.** No new code enforces this; `SessionCreate` still takes only `directory`. **Violated.**
- **Project and workspace membership must be checked against durable binding stores, not inferred from textual similarity.** The durable stores exist; nothing in the request/handler path consults them. **Violated by absence**, not by introduction of a new path.
- **Existing legacy rows must remain loadable; compatibility fields may be projected but may not override a valid canonical binding.** Legacy rows are loadable; nothing overrides anything because nothing reads the canonical binding. **Trivially preserved** for the absence of work, but the affirmative half of the invariant (canonical authority) is not exercised.
- **Ambiguous or unresolved rows must fail actionably and remain inspectable.** No new code path produces such outcomes. **Trivially preserved.**
- **Identity parsing does not grant authorization.** The Milestone 001 parser remains; no new code grants authority. **Preserved.**
- **Protocol additions must be additive until explicit compatibility-removal criteria are accepted.** No protocol additions were made. **Preserved.**
- **No daemon-owned path may use process-global cwd to establish project context.** `check_daemon_cwd_usage.py` still passes. **Preserved.**

The first three invariants are the ones the milestone was meant to enforce. They remain in the violation state the plan's `§3` already documents.

## 6. Failure and recovery review

This section is intentionally short. The plan's `§8` failure semantics assume the existence of a resolver and atomic write paths. Because none of those exist:

- Invalid IDs would still fail at parse time (Milestone 001) but no new typed-error outcomes are produced.
- Mismatched, unbound, rebind-required, archived, and stale-revision outcomes are not defined in any new location.
- Cancellation, restart, and contention semantics for the new atomic write path are not applicable because the new path is absent.
- Daemon restart currently relies on Milestone 002 storage; it continues to function, but it does not exercise the new authority layer.

No new failure mode has been introduced. The corrective pass must re-examine these properties once the resolver and atomic write path are landed.

## 7. Migration and compatibility review

- v25 binding tables and v28 catalog tables are present (closed in prior milestones). The plan required an additive migration only when a concrete missing constraint/index/compatibility-state field is required; no such field has been identified because the resolver is absent.
- Historical tables and fields are intact: `crates/codegg-protocol/src/core.rs:242-258` and `src/server/state.rs:13-20` confirm the legacy surface is unchanged.
- The new protocol fields required by `§6 Protocol and DTOs` are not present; old JSON fixtures continue to decode, and no new fixtures exist.
- The `project_dir` field of `ServerState` remains. The plan explicitly permits it to remain temporarily as a compatibility locator. The plan does **not** permit treating it as a project ID; that is the violation noted in §5.
- No protocol-version bump is appropriate; no additive fields exist.

## 8. Security review

- Identity parsing bounds (Milestone 001) are unchanged. 128-byte limit, ASCII alphanumeric plus `-`/`_` only, rejects empty/path-like/control/whitespace input.
- `ServerState.project_dir` continues to be used as an ID in `src/server/routes/project.rs:45, 61, 114-115`. The plan calls this out as the security-sensitive legacy path; no remediation is in place.
- No new untrusted-ID surfaces were introduced; no new authorization seams were introduced.
- The path-identity static guard is unchanged; the planned expansion is not landed.

## 9. Documentation and operations

- `architecture/identity.md`, `architecture/project_identity_storage.md`, `architecture/project_catalog.md`, `architecture/session.md`, `architecture/protocol.md`, `architecture/core.md`, `architecture/server.md` (or its current equivalent), and `architecture/workspace.md` are **not updated** with the Milestone 003-specific content the plan's `§6 Documentation and static guards` enumerates (canonical resolver ownership, context type, additive DTOs, capability advertisement, compatibility-only field marking, server route transitional behavior).
- No new operator command or recovery procedure is required by this corrective pass.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| critical | Milestone 003's principal infrastructure boundary (resolver, DTOs, daemon migration, server route cleanup, expanded static guard) is not implemented. | Daemon and server still treat a path as a project ID; old directory-only requests can still define session/identity behavior; canonical context is not authoritative for any new executable session. | Corrective implementation pass under `plans/implementation/domain-identity/003-…` (same plan, re-handed) or a new corrective plan that inherits this record. |
| medium | `src/server/routes/project.rs` continues to return `state.project_dir` as `ProjectInfo.id` and to group sessions by legacy `session.project_id` strings. | Server counts, listing, and `ProjectInfo.id` remain path-derived; the `ServerState.project_dir` field remains a hidden identity authority. | Resolved by the corrective pass (work package E). |
| medium | `scripts/check_identity_path_usage.py` covers only `crates/codegg-core/src/**/*.rs`. The planned daemon/protocol/server boundary is not enforced. | Path-derived identity can be re-introduced outside the core identity module without guard failure. | Resolved by the corrective pass (work package F). |
| low | Pre-existing clippy finding at `crates/codegg-protocol/src/provider.rs:170` (large enum variant on `SessionSelectionDto`). | Unrelated lint noise. | Out of scope for this record; track separately. |

No new high-severity security or contention finding has been introduced because no new code has landed. The critical finding is that **the milestone's entire required-production-change set is absent**; that absence is the corrective pass.

## 11. Roadmap disposition

**Corrective pass required** (per `plans/003-planning-process.md` §7 and `plans/closure/README.md`'s closure rules). A corrective implementation plan is required to perform the work described in the original plan:

- the canonical `ProjectId`/`WorkspaceId` context value;
- the authoritative resolver with `not_found` / `unbound` / `rebind_required` / `archived` / `mismatch` / `stale_revision` outcomes;
- additive protocol DTOs (`ProjectContextDto`, `SessionBindingDto`, capability advertisement);
- the migration of `CoreRequest::SessionList`, `SessionCreate`, `SessionCreateFromTemplate`, `SessionAttach`, `SessionLoad`, fork/import, and the session/daemon snapshot DTOs;
- the rewrite of `src/server/routes/project.rs` to return stable `ProjectId` and label locators separately;
- the expansion of `scripts/check_identity_path_usage.py` to cover the daemon/protocol/server boundary and reject `ServerState.project_dir` as an ID value;
- the `§6 Documentation and static guards` architecture updates.

The original plan `plans/implementation/domain-identity/003-daemon-protocol-adoption.md` remains the authoritative source of requirements for the corrective pass. The implementation agent re-handed the plan must, per §7 of the planning process:

- reference this closure record in the corrective plan or re-handoff notes;
- include regression tests and at least one new static-guard negative fixture that would have failed under the present state (e.g., a fixture that calls `ProjectId::parse` from a `PathBuf` or that uses `state.project_dir` as a `ProjectId` in a daemon handler);
- avoid reopening already-closed Milestones 001–002 or the Project Catalog 001 storage foundation.

The next dependency that this milestone gates is **Session Projections Milestone 001** (`plans/implementation/session-projections/001-projection-contracts.md`), whose `§2 Why this milestone is blocked` hard-dependency statement is: "Domain Identity daemon/protocol adoption must provide stable project/session/workspace identity." Because this closure record does not close Milestone 003, Session Projections 001 **remains blocked**. No new plan is unblocked by this record.

TUI Project Sessions Milestone 001 and Runtime Assets Milestone 002 are not direct dependents of Domain Identity 003:

- `plans/implementation/tui-project-sessions/001-project-aware-state.md` §2 hard-dependencies cite "Runtime Assets refresh interfaces" and "Project Catalog protocol/server milestone" — not Domain Identity 003.
- `plans/implementation/runtime-assets/002-explicit-context-agent-instruction-resolution.md` §16 handoff notes say "Domain Identity 003 may land in parallel. Consume the stable project/workspace storage interface and avoid depending on unfinished protocol details."

Both of those plans remain unaffected by this closure record and should be tracked by their own explicit dependencies.

## 12. Registry updates

- Move Domain Identity Milestone 003 from the `Dependency-ready implementation plans` table in `plans/registry.md` to the `Active closure work` table with a status of `corrective pass required` and a link to this record.
- Update `plans/subsystems/domain-identity-roadmap.md` Milestone 3 row in the `§12 Milestone status` table from `ready` to `corrective pass required`, link this closure record, and keep Milestone 4's `not started` row (its `Hard on Milestones 1–3` dependency remains unsatisfied).
- Keep the `Blocked work` table entry for **Session Projections 001** unchanged. Its blocker is still "Close Domain Identity 003, Project Catalog 004, and Multi-Project TUI 001." This record confirms the Domain Identity 003 leg of that conjunction.
- Do **not** mark Domain Identity 003 closed in any table, do **not** archive the implementation plan, and do **not** modify the canonical long-term documents.
- The next step in the planning lifecycle is to create or re-hand a corrective implementation plan under `plans/implementation/domain-identity/` (this may reuse the same plan if it is re-handed with the new evidence requirements, or a new corrective plan may be filed alongside it referencing this closure record).
