# Execution Reliability, Approval, and Autonomy M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/execution-reliability-approval-autonomy/003-approval-router-and-durable-mode-state.md`

Source subsystem roadmap:

- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md#7-milestones`

Repository baseline reviewed: `efc67ae1`

Implementation commits or pull requests:

- `79cac319` — execution-reliability M003: ApprovalRouter and durable mode state

## 1. Executive finding

M003 is complete. One production `ApprovalRouter` now resolves all
deterministic permission/security escalations against an explicit
`ApprovalMode` (`Interactive` / `Automatic` / `Yolo`) and an immutable
`ExecutionPolicySnapshot` captured at the turn/accepted-tool-batch
boundary. `Interactive` preserves the existing human-request UX,
`Yolo` auto-allows only `Escalate` within the resolved ceiling (never
`Deny`), and `Automatic` is a typed placeholder that safely defers to
the human until M006 (never fail-open, even with rollout enabled).
Production `Always` decisions use one canonical per-user store path and
survive restart; approval/sandbox preference is daemon-owned,
principal-scoped, durable (`runtime_preferences`, v58), and
frontend-neutral via new core protocol. Receipts identify decision
source/policy revision, and concurrent mode changes cannot retroactively
bless a pending action.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| ApprovalMode domain/config/protocol shape (WP-A/D) | `codegg-core/src/approval.rs`: `ApprovalMode::{Interactive,Automatic,Yolo}` with `parse/as_str/rank`; `codegg-protocol/src/core.rs`: `ApprovalModeDto`, `SandboxProfileDto`, `RuntimePreferenceDto`, `ExecutionPolicySnapshotDto`; `CoreRequest::ApprovalPreferenceGet/ApprovalModeSet/SandboxProfileSet/ExecutionPolicyGet` + `CoreResponse::ApprovalPreference/ExecutionPolicy`; authorization `Global` gates (principal server-side) | pass | Closed enum per ADR-0004; unknown wire values fail closed; approval/sandbox separate |
| Normalized ApprovalRequest/Decision + one router (WP-B) | `src/permission/approval.rs`: `ApprovalRequest` (bounded tool/path/summary/reasons/effect/revision), `ApprovalDecision::{Allow,Deny,DeferUser}`, `DeterministicVerdict::{Allow,Deny,Escalate}`, `ApprovalRouter::{decide_deterministic,route_escalation,request_human_approval}` with `HumanApprovalOutcome{persist,allow}`; `scripts/check_approval_router.py` single-owner guard | pass | Raw sensitive args excluded; summaries bounded 512, reasons 8×512 |
| Immutable execution-policy snapshot at turn/batch (WP-B) | `ExecutionPolicySnapshot::capture()` (mode+sandbox+principal/session/agent+revision+reviewer, `captured_at_ms`); `AgentLoop::capture_execution_snapshot()`; `execute_tool_calls_impl` captures `batch_snapshot` once and threads it through `check_tool_permission_with_snapshot` + `build_tool_execution_context`; `narrow_for_child` ceiling | pass | Pending Interactive request uses captured snapshot, never live re-read |
| AgentLoop convergence, human UX preserved (WP-B) | `src/agent/tool_batch.rs`: deterministic normalize (Deny wins, sensitive/security escalate, workspace-file-mutation allow, else allow) then router branch (Yolo allow, Automatic/Interactive human fallback); `resolve_escalation_via_human` + `apply_human_outcome` preserve sensitive/security/deny/timeout messages; `persist_always_choice` with `user_choice_unpersisted` diagnostic | pass | 3 direct `register_with_session` sites removed; 1 remains in router |
| Canonical PermissionStore wiring + diagnostics (WP-C) | `canonical_permission_store_path()` (env `CODEGG_PERMISSIONS_PATH` or `default_store_path()`); `agent_loop_factory`, `main`, `exec` use canonical path; `worker` subagents ephemeral with ceiling comment; `always_allow/always_deny` return `bool`, `store_path/is_persistent` helpers; `ToolExecutionContext.permission_mode` populated from snapshot (never `None` on loop path) | pass | Atomic temp+rename + HMAC preserved; `with_exec_mode` retained as headless adapter distinct from Yolo |
| RuntimePreferenceStore foundation (WP-A) | `codegg-core/src/approval.rs`: `RuntimePreferenceStore{get,set_approval_mode,set_sandbox_profile,set_model_preference}` with revision CAS, bounds (principal 256, identity 512), secret-free; `RUNTIME_PREFERENCE_SCHEMA_STATEMENTS`; migration v58 + `STORAGE_LAYOUT_VERSION=58`; `resolve_effective_mode` precedence; `child_mode/sandbox_allowed` | pass | M004 model fields reserved and tested; corrupt rows degrade to defaults |
| Core protocol get/set/effective state (WP-D) | `daemon_ops.rs` approval arms (principal from `authority.principal_id()`, pool-less defaults, `preference_conflict`/`invalid_approval_mode`/`preference_unavailable` codes); `daemon_family.rs` Ops routing; `daemon_turns.rs` resolves persisted preference into `TurnRunInput{approval_mode,sandbox_profile}` before `run_turn` | pass | Older clients ignore new variants; new optional DTO fields have defaults |
| Receipt/audit source propagation (WP-D) | `source::{permission_evaluation,workspace_file_mutation,user_choice,user_choice_unpersisted,yolo,automatic_defer,security_deny,permission_deny,sensitive_deny,timeout_deny}`; `accepted_permission_receipt(source)` threaded into `ToolExecutionContext` + broker grant (mode-matched) | pass | Timeout distinct from explicit deny; `Always` persist failure surfaces as `user_choice_unpersisted` |
| Tests/docs (WP-A-D) | `tests/approval_router.rs` (16), `approval.rs` unit (5 router + 10 core), `permission.rs` (44), harness permission (5); `architecture/permission.md`, `security.md`, `session.md` M003 sections; guard script | pass | See §4 |

## 3. Production implementation evidence

Landed ownership and behavior:

```text
ApprovalMode / SandboxProfile / ExecutionPolicySnapshot (codegg-core/src/approval.rs)
  parse/as_str/rank; resolve_effective_mode(explicit > ceiling > persisted > default)
  snapshot.capture(mode, sandbox, principal/session/agent, revision, reviewer)
  narrow_for_child enforces child <= parent (Interactive<Automatic<Yolo;
    ReadOnly<WorkspaceWrite<FullHost)

RuntimePreferenceStore (codegg-core/src/approval.rs + schema v58)
  runtime_preferences(principal_id PK, approval_mode, sandbox_profile,
    last_provider_connection_id, last_model_id, revision, updated_at)
  CAS: expected_revision mismatch -> Conflict (reload, never last-write-wins)
  revision 0 = absent; next = max(current+1, 1); updated_at = now_millis
  corrupt mode strings -> None (Interactive/WorkspaceWrite fallback)

ApprovalRouter (src/permission/approval.rs)
  new(snapshot).with_automatic_rollout(false default)
  decide_deterministic: Allow -> Allow; Deny -> Deny; Escalate -> route
  route_escalation: Interactive -> DeferUser; Yolo -> Allow(yolo);
    Automatic -> DeferUser(automatic_defer) regardless of rollout
  request_human_approval: register_with_session -> publish PermissionPending
    -> 300s timeout -> unregister; AllowOnce/AlwaysAllow/DenyOnce/AlwaysDeny/timeout
    mapped to HumanApprovalOutcome{decision, persist, allow}

AgentLoop (loop.rs / coordinator.rs / tool_batch.rs / factory / turn_runtime)
  services.approval_mode/sandbox_profile (Interactive/WorkspaceWrite defaults)
  set_approval_mode/set_sandbox_profile/capture_execution_snapshot
  check_tool_permission -> capture + check_tool_permission_with_snapshot
  normalize: Deny(security/permission) wins; sensitive/security Ask escalate;
    workspace-file-mutation allow; else allow
  route: Yolo allow(yolo); Automatic human fallback(automatic_defer);
    Interactive human(user_choice); Always persist with unpersisted diagnostic
  execute_tool_calls_impl captures batch_snapshot once; build_tool_execution_context
    sets permission_mode = snapshot mode (broker grant/caller mode-matched)
  factory TurnRunInput/AgentLoopBuildInput carry approval/sandbox;
    daemon_turns resolves persisted preference per turn (restart reload)

PermissionStore durability (permission/mod.rs)
  canonical_permission_store_path() -> env override or ~/.config/codegg/permissions.json
  factory/main/exec use canonical; worker subagents ephemeral (parent ceiling)
  always_allow/deny -> bool; is_persistent/store_path helpers

Protocol (protocol/core.rs + daemon_ops/family/turns + authorization/policy.rs)
  DTOs + 4 requests + 2 responses; Ops family; Global auth (principal server-side)
  daemon_turns TurnSubmit resolves preference before run_turn
```

Router call-site inventory before/after:

```text
Before (baseline efc67ae1):
  src/agent/tool_batch.rs:291 register_with_session (sensitive escalation)
  src/agent/tool_batch.rs:350 register_with_session (security escalation)
  src/agent/tool_batch.rs:421 register_with_session (general Ask)
After (79cac319):
  src/permission/approval.rs:266 register_with_session (single owner)
  src/agent/tool_batch.rs: no direct register (delegates via router)
Guard: scripts/check_approval_router.py passes.
```

Approval-mode decision matrix (router unit + integration):

```text
Allow x {Interactive,Automatic,Yolo} -> Allow (mode-independent)
Deny x {Interactive,Automatic,Yolo} -> Deny, never Yolo (hard deny wins)
Escalate x Interactive -> DeferUser/human wait (one PermissionPending)
Escalate x Yolo -> Allow(yolo) within ceiling
Escalate x Automatic -> DeferUser(automatic_defer), human fallback in loop; never Allow
```

## 4. Verification executed

### Commands run (local, this environment)

```bash
cargo test --test permission
cargo test -p codegg-core --lib approval
cargo test -p codegg-core -- runtime_preference
cargo test --test approval_router
cargo test --test agent_loop_harness -- permission
cargo test -p codegg-core --test continuation_checkpoint
cargo test -p codegg --lib permission::approval
python3 scripts/check_approval_router.py
./scripts/check-core-boundary.sh
python3 scripts/check_execution_ownership.py
python3 scripts/check_project_catalog_invariants.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
./scripts/verify.sh quick
```

Focused target `approval_router` is the new M003 harness; `permission`
and `agent_loop_harness -- permission` are the plan gates.

### Results

- `cargo test --test permission`: 44/44 pass (existing ruleset/store/bash/destructive coverage intact; Interactive compatible).
- `cargo test -p codegg-core --lib approval`: 10/10 pass (mode/sandbox parse, child ceiling, precedence, snapshot narrow, 5 preference store tests).
- `cargo test -p codegg-core -- runtime_preference`: 5/5 lib tests pass (round-trip/revise, CAS conflict, bounds, M004 reserved, corrupt degrade).
- `cargo test --test approval_router`: 16/16 pass — safe-tool no-prompt; Interactive one-request human UX; Yolo Escalate→Allow(yolo); security Deny→Denied in Yolo; Automatic defer (both rollout flags); hard-deny wins; child ceiling; Yolo≠deny; snapshot race; preference restart (file DB close/reopen); Always restart (checker reconstruct); corrupt JSON conservative; CAS conflict; pre-preference clean; protocol additive; Ask→Escalate mapping.
- `cargo test --test agent_loop_harness -- permission`: 5/5 pass (existing Interactive human flows preserved through router).
- `cargo test -p codegg --lib permission::approval`: 5/5 pass (router matrix, hard-deny, snapshot/child, bounds, store path).
- `cargo test -p codegg-core --test continuation_checkpoint`: 11/11 pass (v58 additive, layout 58, remigrate no-op).
- `check_approval_router.py`: pass (1 owner site).
- `check-core-boundary.sh`: pass (approval store/router respect core boundary; core has no root imports).
- `check_execution_ownership.py`: pass (no new spawn/scheduler surface; router is pure + bus wait).
- `check_project_catalog_invariants.py`: 7/7 pass (layout 58 tracks v58 wiring).
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass.
- `scripts/verify.sh quick`: pass (fmt, builtin-agents, core-boundary, sandbox, execution-ownership, tui-authority, workspace check).

PermissionStore production restart evidence: `always_decisions_survive_production_checker_reconstruction` writes via canonical temp path, asserts file exists + `is_persistent`, rebuilds checker from same path, asserts `Allow` (in `tests/approval_router.rs`).

RuntimePreferenceStore schema/CAS evidence: v58 `IF NOT EXISTS` + index; `runtime_preference_round_trips_and_revises` (rev 1→2, mode retained across sandbox write); `cas_conflicts_instead_of_last_write_wins`; `bounds_reject_oversize_principal`; `corrupt_mode_degrades_conservatively`.

Protocol compatibility evidence: `protocol_snapshot_is_additive_for_older_clients` (legacy JSON without optionals decodes; new DTO round-trips); new request/response variants are additive enum cases with `#[serde(default)]` optionals; authorization matrix extended with representative requests.

Hard-deny/Yolo and concurrency regression results: `hard_deny_always_wins`, `security_deny_remains_denied_in_yolo`, `yolo_cannot_override_explicit_deny` (store Deny wins), `mode_change_races_pending_approval_uses_captured_snapshot` (captured Interactive ≠ live Yolo), `preference_cas_conflicts…`, human timeout→`timeout_deny` distinct path in router.

## 5. Invariant review

| Plan invariant (§4) | Evidence |
|---|---|
| Explicit/hard Deny never becomes Escalate/Yolo Allow | Normalize returns `Deny` before router; `hard_deny_always_wins`, `security_deny…`, `yolo_cannot_override…`; Yolo arm reachable only for `Escalate` |
| Authority ceiling evaluated before routing | Deny normalization precedes `route_escalation`; child `narrow_for_child` enforced; subagents ephemeral + Interactive default (never broader) |
| Security-sensitive may Escalate/Deny, never bypassed by second path | Single `check_tool_permission_with_snapshot` owner; sensitive branch escalates even when perm `Allow`; guard proves no second register path |
| Interactive functionally compatible | 44 permission + 5 harness permission tests pass unchanged in behavior; one-request human UX test asserts single `PermissionPending` + unregister |
| Yolo auto-allows only Escalate, not Deny | Router `route_escalation` Yolo arm takes `&ApprovalRequest` only; `decide_deterministic` Deny path ignores mode |
| Automatic safely defers, never fail-open | `route_escalation` Automatic ignores rollout flag; integration asserts both flags defer; loop falls back to human, never Allow |
| Approval/sandbox separate | Distinct `ApprovalMode`/`SandboxProfile` types, services fields, store columns, protocol DTOs/requests; `set_approval_mode` never touches sandbox |
| Production Always survives restart, scoped as today | Canonical path wiring (factory/main/exec) + file restart test; session-scoped decisions preserved; subagent scope explicit |
| Frontend is projection, not authority | Protocol derives principal server-side; TUI manifest untouched; `ApprovalPreference`/`ExecutionPolicy` are daemon projections; docs state non-authority |

## 6. Failure and recovery review

- RuntimePreference write failure returns `Storage` error; daemon handler maps to `preference_write_failed` (no silent claim); current explicit turn override may remain effective and diagnostic is shown (handler error + tracing warn in turn resolve).
- PermissionStore load corruption/tamper retains conservative ignore (empty decisions → `Ask`, never `Allow`); covered by `corrupt_permission_json_fails_conservatively` plus existing HMAC tamper tests.
- Human approval timeout (300s) maps to `Deny{timeout_deny}` distinctly from explicit `DenyOnce` (`user_choice`); messages surface "approval timeout".
- Pending Interactive request uses captured snapshot: mode toggle mid-wait cannot convert it to Yolo (race test); batch snapshot fixed for all tools in batch.
- Restart reloads last preference (`approval_preference_survives_daemon_restart` file close/reopen) and `Always` decisions (checker reconstruct); pending human requests are not resurrected (oneshot registry is process-local, 310s TTL).
- Stale preference revision returns `preference_conflict` (CAS test); frontend must reload, never last-write-wins.
- Cancellation unregisters pending request (`unregister_scoped` after wait in router; harness `respond_scoped` atomicity unchanged).

## 7. Migration and compatibility review

- PermissionConfig/rules unchanged; `Ask` maps to router `Escalate` (mapping test).
- Built-in review/debug/docs modes untouched (permission-rule envelopes, distinct from approval mode).
- Old permission JSON readable; corrupt → conservative `Ask` (test).
- Preference table additive/empty on upgrade (`IF NOT EXISTS`, pre-preference clean test); v57→v58 chain + dispatch + definition agree (catalog guard 7/7).
- `with_exec_mode()` retained as headless compatibility, documented separate from Yolo; exec path now uses canonical store path but keeps `Allow` default semantics.
- Protocol additive: 4 requests + 2 responses, optional DTO fields defaulted; older clients ignore new variants/fields (legacy decode test).
- `STORAGE_LAYOUT_VERSION` 57→58 with highest migration 58 (continuation test updated).

## 8. Security review

- Yolo≠deny enforced at type/flow level (Deny never constructs `ApprovalRequest`); explicit store Deny wins even in Yolo (integration).
- Child cannot broaden parent (`child_mode/sandbox_allowed` + `narrow_for_child`; subagents ephemeral Interactive default; worker comment records ceiling).
- TUI manifest cannot override daemon preference: no TUI manifest write path added; protocol carries no principal field (server-side derivation); `check_tui_project_authority` still passes in quick.
- Sensitive hard-deny path: sensitive match escalates, but explicit Deny precedes it; Yolo cannot bypass sensitive Deny because Deny short-circuits.
- No secrets in preference/audit: store holds only mode/profile/connection/model IDs + revision/timestamp; `ApprovalRequest.args_summary` bounded 512 + NUL-stripped; router dialog for sensitive uses summaries, not raw commands; audit sources are bounded labels.
- Authorization: new ops are `Global` with no semantic capability (any authenticated caller reads/writes own principal only); mode change cannot exceed project/admin ceiling by construction (ceiling param threaded through `resolve_effective_mode`; daemon M005 will enforce project ceiling at policy wiring — M003 wires the data path).
- Reviewer placeholder cannot mutate: Automatic never constructs an Allow without human; no reviewer model call exists in M003.

## 9. Documentation and operations

Updated:

- `architecture/permission.md` — ApprovalRouter/store/snapshot ownership table + M003 invariant 7 (router contract, snapshot, canonical path, subagent scope, frontend projection).
- `architecture/security.md` — approval-vs-security boundary section (Deny precedence, Yolo/Automatic semantics, mode/sandbox orthogonality).
- `architecture/session.md` — `RuntimePreferenceStore` ownership (methods, CAS, restart precedence, protocol).
- Protocol DTO doc comments in `codegg-protocol/src/core.rs` (authority, additive compat).
- Guard: `scripts/check_approval_router.py` (single-owner enforcement).

Operator notes: watch `permission Always decision applied in-memory but NOT persisted` (warn, with tool) vs `approval preference read failed; using defaults` (warn in turn resolve) vs `preference_conflict` protocol error (stale frontend reload) vs `approval timeout` denial (distinct from user deny). Receipt sources (`yolo`, `user_choice`, `user_choice_unpersisted`, `automatic_defer`, `permission_evaluation`, `workspace_file_mutation`) distinguish deterministic/human/Yolo/defer in tool context.

No new CI lane: guard is a focused ownership lint run locally (plan §6 allowance); `verify.sh quick` lanes unchanged.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Low | Daemon `ExecutionPolicyGet` synthesizes `policy_revision` from preference revision only, not the full config hash the loop captures (`config:hash`) | Daemon effective snapshot and loop batch snapshot can carry different revision strings for the same turn; no authorization impact (revision is diagnostic), but M005/M008 should converge the revision source | M005 sandbox wiring or M008 qualification should unify `policy_revision` (prefer loop `permission_version()` content hash) |
| Low | `TurnRunInput` approval/sandbox threading covers daemon `TurnSubmit`; direct `AgentLoop::new` harnesses and `exec`/`main` standalone paths use `Interactive` default without reading the daemon store | Standalone/headless runs do not reload the daemon preference (by design: pool-less); daemon turns do. No fail-open (default is narrowest) | M007 user surfaces should document standalone vs daemon preference scope; no code change for M003 |
| — | No other open items | — | — |

No stop condition triggered (no second authorization engine, no TUI security authority, no Yolo bypass of deny, no reviewer behavior required).

## 11. Roadmap disposition

Milestone closed with two downstream unblocks:

- M003 (ApprovalRouter and durable mode state): hard dependency satisfied — close.
- M004 (selected-model/runtime-preference convergence): hard dependency was M003 RuntimePreferenceStore contract. Contract now stable (`RuntimePreferenceStore` + `set_model_preference` reserved + revision/CAS + protocol preference shape). **Unblock to `ready`.**
- M005 (production sandbox policy wiring): hard dependency was M003 execution-policy/ApprovalRouter contract. Contract now stable (immutable snapshot + router + sandbox profile preference + `narrow_for_child`). **Unblock to `ready`.**
- M006 remains **blocked** on M003+M005 (M005 not yet closed).
- M007 remains **blocked** on M003-M006 (M004-M006 not yet closed).
- M008 remains **blocked** on M001-M007 (M001+M002+M003 closed; still blocked on M004-M007).

## 12. Registry updates

- `plans/registry.md`: M003 `ready` → `closed` with closure link and implementation `79cac319`; subsystem row `M001+M002 closed; M003 ready` → `M001+M002+M003 closed; M004+M005 ready`; dependency-ready table M003 row → `closed`, M004/M005 rows `blocked` → `ready` with M003-contract notes; execution-order item 2 rewritten (M004/M005 ready, M006-M008 still blocked); M003 appended to recently-closed work; blocked-work M004/M005 rows updated.
- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md`: M003 section `ready` → `closed` with closure link; M004/M005 `blocked` → `ready`; status table updated.
- `plans/implementation/execution-reliability-approval-autonomy/003-approval-router-and-durable-mode-state.md`: `Status: ready for handoff` → `Status: implemented`.
- `plans/implementation/execution-reliability-approval-autonomy/004-selected-model-and-runtime-preference-convergence.md`: `Status: blocked` → `Status: ready for handoff`.
- `plans/implementation/execution-reliability-approval-autonomy/005-production-sandbox-policy-wiring.md`: `Status: blocked` → `Status: ready for handoff`.
