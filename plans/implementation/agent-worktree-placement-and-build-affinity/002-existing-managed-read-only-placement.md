# Agent Worktree Placement and Build Affinity A002 — Existing Managed Checkout for Read-Only Delegated Runs

Status: ready for handoff
Repository baseline: `d4241f08dcfede0534df3ea08c83d762fa1c55f7` (`plan/worktree-affinity-cwd-build-admission-20261027`; verify/rebase exact head before edits)

Source roadmap:
- `plans/subsystems/agent-worktree-placement-and-build-affinity-corrective-addendum.md` §7 A002

Long-term requirements:
- `plans/000-long-term-specification.md` (workspace/security, agent runs, and tool execution)
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md` (delegated agents, worktrees and concurrency)

Applicable accepted ADRs: ADR-0002, ADR-0032, ADR-0033, ADR-0047, ADR-0048; ADR-0055 only for preserving the session's relative cwd boundary.
Primary class: **capability** (read-only inspection, not shared mutation).

## 1. Objective
Allow `task.spawn` and bounded `task.spawn_many` children to request `placement=existing_managed_read_only` plus an exact `worktree_id`, so a **read-only** delegated run executes against one already-created, available **managed** Git worktree without creating a new checkout or gaining mutation authority. The named checkout is reserved exclusively for this child's inspection for the duration of its attempt. Concurrent inspectors of the same checkout are deliberately **not** enabled in A002; the existing exclusive lease contract is the safe baseline.

## 2. Why this milestone is ready
A001 is strictly closed on the branch: a read-only `isolated_new` child obtains its own production-contained checkout and explicit isolation fails closed. The existing `WorktreeService` supports `get`, `refresh`, run-owned generation-fenced `acquire_lease` and `release_lease`. `SessionLaunchService` already validates managed existing checkout identity, state and ownership for **sessions**, but its session launch contract must not be reused as a delegated-run lease shortcut. No new concurrent-writer grant is sought.

## 3. Current implementation evidence
- `crates/codegg-core/src/run_placement.rs`: `Auto`, `IsolatedNew`, `SerializedShared { group_id }`; no existing-worktree delegated variant. The enum is serde-tagged `kind`, and old payloads default to `Auto`.
- `src/tool/task.rs`: a string placement field, group-level fallback and child overrides; `parse_run_placement` currently accepts no worktree ID. It constructs durable run/job payloads and a request fingerprint. Placement and target checkout must be included in duplicate/idempotency equivalence checks.
- `src/scheduler/executors.rs`: run placement occurs during `Preparing`; fresh isolated and group-shared leases have intentionally different owners/lifetimes. New reuse must be a **third explicit arm**, not an alias of `SerializedShared` or `Auto`.
- `crates/codegg-core/src/worktree_service.rs`: `Ready` versus `InUse` state, worktree health and lease generation; `acquire_lease` also admits `Reserved`, so an A002 caller must explicitly require verified `Ready` and usable health **before and after** the atomic claim.
- `src/agent/worker.rs::requires_isolation` is a placement heuristic, **not** a complete read-only proof: `is_read_only_agent` and `read_only_blocked_tools` enumerate different tool lists, and indirect process/plugin/child-spawn surfaces must be examined before granting access.
- `src/core/session_launch.rs` validates existing worktree `managed`, `project_id`, `repository_id`, `owner_run_id` and `Ready`, but a session can be associated without holding a synthetic worktree lease. A002 must investigate active-session/process ownership **in addition** to the worktree row before promising no concurrent writers.

## 4. Invariants that must not regress
1. A model-supplied `worktree_id` is an identifier, **never** an authorization token or path. The daemon resolves and verifies it against the child's immutable project/repository/workspace/principal/parent scope. If a linked checkout has a distinct workspace record, authorize that mapping through daemon catalog state rather than assuming string equality.
2. Read-only means an enforceable effective **allowlist** of read/inspection operations, not the absence of a few named mutating tools. Disallow shell/terminal/managed processes, `test`, `verify`, `python_script`, dynamic plugins/MCP/tool-program effects, mutation-capable Git, LSP apply/formatter, work orders, delegation/spawn, persistent services, and unknown tool aliases unless each is proven non-mutating with enforcement.
3. A002 acquires a **run-owned exclusive lease**, prevents concurrent managed writers, and refuses an occupied, active-session-owned, dirty/conflicted/missing/orphaned/unmanaged/foreign checkout when its safety cannot be established. No session lease is borrowed; no group writer permit is enabled. Existing external same-user OS writers are outside a blanket containment promise: this plan must state the boundary precisely.
4. Never create, remove, archive, `git checkout`, reset, auto-integrate or commit the selected worktree. Release **only** the acquired lease generation when the child attempt terminates; retain the target checkout and its contents.
5. Effective cwd, allowed paths, LSP/Git read roots and all tool paths resolve under the selected checkout while security ceilings and scratch remain separately scoped. No process-global `chdir`. In-flight turns and parent/sibling runs are never retargeted.
6. `Auto`, `IsolatedNew`, `SerializedShared`, WorkOrder `shared/serialized` fail-closed semantics, and the existing session-launch flow remain unchanged.

## 5. Scope
**In:** a typed `RunPlacement::ExistingManagedReadOnly { worktree_id }` (exact naming may change if warranted by existing API contracts), canonical model-facing `placement` + `worktree_id` fields, per-child `spawn_many` selection, durable payload/fingerprint, daemon-side authorization/claim, effective read-only tool enforcement, lifecycle/projection, tests and docs.
**Out:** simultaneous multiple readers of one lease, writable existing-worktree runs, arbitrary unmanaged checkout paths, changing session checkout bindings, interactive compiler/shell on this target, new Git backend, repository-cache work (A004), or permission elevation.

## 6. Required production changes
### Core/domain
Add an additive typed placement variant using `WorktreeId`. Validate presence and shape of ID only for this mode; reject unsupported placement/field combinations. Update `as_str`, `is_shared`, serde round trips and stable idempotency/request fingerprint (placement kind + target ID + effective denied/read-only capability ceiling).

### Storage/migration
Prefer the existing serde-defaulted `JobPayload::SubagentRun.placement` and `AgentRun.worktree_id`; no schema migration unless necessary for a durable, exact one-run lease link or recovery. If added, make additive/migrate and preserve historical `Auto` decoding. Never store a model-supplied absolute path as checkout identity.

### Protocol and DTOs
Expose `placement: "existing_managed_read_only"` and per-child `worktree_id` in task descriptions/schema. A group-level placement with no unambiguous per-child ID is refused; do not silently redirect children to the same target. Projection carries the selected worktree ID and read-only mode (authorized viewers only), with distinct typed refusal codes.

### Runtime and concurrency
At `Preparing`, resolve the named worktree via service; refresh/validate Git registration, canonical path, same-repository identity, managed flag, `Ready` and safe health; establish that no active session, persistent process, group or other job has live write ownership under existing daemon authorities. Atomically claim exclusive lease against concurrent requests, re-read/validate post-claim, then attach exact worktree to run before launching provider. Fail closed on any uncertainty. Ensure `allowed_paths` and child relative cwd are recomputed/revalidated beneath new checkout, not copied as a stale parent absolute path. Use existing attempt, cancellation, restart reconciliation and terminal release paths; **do not** clean up the selected checkout.

### Frontend/operator surfaces
Model response and delegated-run status must state the effective existing checkout/branch and that it is read-only. Distinguish `existing_managed_read_only` from `serialized_shared` and isolated-new allocation. Do not offer selection as an interactive free-form filesystem path.

### Security and authorization
Implement a fail-closed *actual effective tool surface* check at scheduling and a guard at tool construction/execution, independent of `requires_isolation` heuristic. Unknown/custom tools should deny by default. Inspect plugin, MCP, LSP, Git refresh/index, external process, child spawning and hidden write paths; document allowed inspection tools. A read-only label cannot substitute for enforced permissions or native filesystem containment. Require same authorized principal/project/repository context, not just a valid ID. Preserve writer-fence and checkout-reservation checks.

### Documentation/static guards
Update `architecture/worktree.md`, `architecture/writer_permit.md`, `docs/tools.md`, `architecture/agent.md` and the tool-placement contract matrix/guards. Add a targeted guard that rejects exposing the mode through an unvetted mutating/process tool profile.

## 7. Ordered work packages
**A — Red authorization and scheduler trajectory:** fixture with two managed worktrees; target `Ready`/clean; attempt an existing-ID read-only delegated spawn. Prove baseline unsupported and later selected checkout's `read` sees different content than parent. Include an active writer/occupied negative at the start.

**B — Typed intent and durable identity:** enum, schema, per-child/group placement handling, request fingerprint, same-scope validation, immutable target association. Prove missing/wrong ID, duplicate delivery same ID, and same idempotency key **different** ID refuse rather than coalesce.

**C — Exclusive inspection lease:** verify and claim under repository lock, attach before provider, use the checkout as effective root, terminal/cancel/restart lease release without cleanup; do not mistake a store read failure for `Ready`.

**D — Effective tool allowlist and security:** prove basic read/search succeeds, while every unreviewed write/process/indirect effect fails at admission and execution. Explicitly prohibit dynamic tool loading that could widen the run. Revalidate selected path and worktree lease before each authorized effect as applicable; do not infer authority from a path.

**E — Group and UX parity:** `spawn_many` with two different existing checkout IDs works independently; same-ID race only one lease winner; mixed `auto`/existing/isolated behavior and group-level fallback documented. Add bounded worktree/branch read-only disposition to projections, and operator guidance.

## 8. Failure, cancellation, restart and contention semantics
Atomic lease acquisition is single-winner; losing request is a typed `existing_worktree_in_use` refusal and never falls back to the parent. Missing/dirty/foreign/unmanaged targets have distinct errors. Generation mismatch prevents releasing another run's newer lease. A run that fails after acquisition but before attachment releases exactly its own generation; crash/restart reconciles based on durable run/lease state and avoids premature release of live attempts. Cancellation must stop the child before lease release; stale continuations cannot renew by path alone. If target disappears or changes Git lineage, stop the child and quarantine/refuse rather than retarget. A group cancel does not delete borrowed checkout files.

## 9. Compatibility/migration
Additive `RunPlacement` and task schema; preexisting jobs deserialize `Auto`. No default placement change. Older clients that cannot emit this mode continue safely. No auto-upgrade of unmanaged worktrees or previously leased run. Stop if cross-version deserialization cannot be safely handled additively.

## 10. Required tests
- **Focused:** typed serde/default/invalid-ID/placement combinations; read-only allowlist includes indirect effects and malicious aliases; idempotency fingerprint includes checkout ID and effective capability ceiling.
- **Integration:** live `JobSubmissionService → JobScheduler → SubagentJobExecutor → WorktreeService` with two distinguishable managed worktrees; provider reads selected file; no third worktree allocated; parent untouched.
- **Restart:** pending/active/terminal child, attempt reconciliation, durable lease reattachment, canceled-before-start and terminal release; no checkout cleanup.
- **Contention:** two runs same checkout, single winner, different-checkout parallelism; group member cancellation; stale generation and late continuation.
- **Security:** wrong project/repository/workspace/principal, foreign root/ID, symlink escape, dirty/held/orphaned/missing, active session, process/grant and mutation attempts, unsupported platform fail-closed, default child environment/scratch. Observe authorized projection only.
- **Compatibility:** old serialized `Auto`/shared/isolated payloads and existing session launch behavior unchanged.

## 11. Required verification commands
```bash
cargo test -p codegg-core --lib run_placement
cargo test -p codegg --lib requires_isolation
# Identify the exact scheduler and agent-loop family members before execution:
cargo test --test agent_loop -- --list
cargo test --test scheduler_jobs -- --list
cargo test --test agent_loop -- existing_managed_read_only
cargo test --test scheduler_jobs -- existing_managed_read_only
python3 scripts/check_writer_disposition.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_daemon_cwd_usage.py
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
```
If an example filter selects zero tests, correct it; it is **not** positive evidence. Reconcile actual guard names with `scripts/verify.sh` before running. The Linux contained creator and negative-host matrix should be run in the existing hosted lane; no platform claim without evidence.

## 12. Documentation updates
Explain read-only existing placement, target-ID lookup, checkout exclusivity and refusal cases; state that it does **not** allow multiple writers or simultaneous read-only inspections on one checkout. Update canonical developer and user docs plus tool schema consistently.

## 13. Acceptance criteria
A named, previously created and unoccupied managed checkout is selected by ID, read through the real child tools, and retained unchanged after completion; its worktree lease returns to `Ready`. A busy/foreign/dirty/missing/unmanaged ID yields a typed refusal. Both daemon and child enforce a read-only tool allowlist. A concurrent second selector cannot acquire the same checkout. No new worktree allocation, parent mutation, sandbox expansion or regression of existing placements.

## 14. Stop conditions
Stop rather than improvise if the existing lease is inadequate to exclude active session writers or external process writes; a read-only allowlist cannot be made complete; ownership cannot be established from durable daemon authority; cross-root identity is ambiguous; a proposed implementation requires process-capable sharing or relaxation of ADR-0047; or native containment evidence is unavailable for a claimed host guarantee. In such cases record a bounded corrective/ADR rather than enabling the profile.

## 15. Closure evidence required
Baseline failing test; exact branch SHA, produced placement/lease records, authorization path and effective tool inventory; real daemon/scheduler/provider-child read trace; negative/fault tests and restart/lease disposition; platform-labeled hosted qualification; quick/Clippy results; guarded writer capability matrix; explicit non-claims for external writers and concurrent shared checkout.

## 16. Handoff notes
A001/A003 closure records are on this branch; main may not contain their **strict** follow-up evidence until merged. Rebase only after reconciling the two. The current `acquire_lease` accepts `Reserved`: A002 MUST explicitly refuse that state and must not rely solely on a precheck separated by a race. Use `AgentRun.worktree_id` for projection and continuation; never steal the lease of a session's live run. The smallest safe implementation is *one read-only child, one unoccupied managed checkout, one run-owned lease*.
