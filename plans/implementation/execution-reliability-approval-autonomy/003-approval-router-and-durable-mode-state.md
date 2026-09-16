# Execution Reliability, Approval, and Autonomy M003 — ApprovalRouter and Durable Mode State

Status: implemented

Repository baseline: `18365458f881f6ac4524c9ea05224b69923faa4f`

Source roadmap:

- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#4.2-explicit-ownership`
- `plans/000-long-term-specification.md#8-deployment-profiles-and-authentication`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs:

- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`

Primary class: authorization invariant / capability

## 1. Objective

Create one production `ApprovalRouter` that resolves deterministic permission/security escalations according to an explicit `ApprovalMode`, persist the user's last approval/sandbox preference in daemon-owned state, and make existing “always allow/deny” decisions genuinely durable on production construction paths.

This milestone establishes Interactive and Yolo routing semantics plus the Automatic mode placeholder/contract; the model reviewer itself lands in M006 after sandbox wiring.

## 2. Why this milestone is ready

- current PermissionChecker/SecurityService already provide deterministic allow/deny/ask classification;
- current AgentLoop permission path and PermissionRegistry identify all human approval seams;
- long-term LocalOwner/principal model exists;
- SQLite/session stores and config paths provide established persistence patterns;
- ADR-0004 defines the authorization boundary.

No sandbox implementation change is required until M005.

## 3. Current implementation evidence

- `PermissionLevel` is Deny/Ask/Allow; `Ask` is the natural deterministic Escalate result.
- read-only/safe-mutating tools short-circuit allow; non-destructive shell and ordinary in-workspace mutation already reduce prompt fatigue.
- `src/agent/tool_batch.rs::check_tool_permission()` directly registers/publishes/waits for human approval in several branches, including security/sensitive-path escalation.
- `PermissionDecisionReceipt` records source/policy revision.
- `PermissionStore` can atomically persist optional HMAC-signed decisions, but production `PermissionChecker::new()` callers inspected at the baseline pass `store_path=None` (`agent_loop_factory`, `main`, `exec`, `worker`).
- `ToolExecutionContext.permission_mode` is currently populated as `None` even though decision receipt fields exist.
- built-in review/debug/docs modes are permission-rule envelopes and should remain distinct from user approval routing mode.

## 4. Invariants that must not regress

- explicit/hard Deny is never turned into Escalate/Yolo Allow;
- caller/session/agent/project authority ceiling is evaluated before routing;
- security-sensitive classification may Escalate or Deny but cannot be bypassed accidentally by a second direct call path;
- Interactive behavior remains functionally compatible;
- Yolo auto-allows only Escalate, not Deny;
- Automatic does not perform model review yet; until M006 it must safely defer to human or be feature-gated unavailable, never fail open;
- approval mode and sandbox profile are separate state fields;
- production “always” decisions survive restart and remain scoped as today;
- frontend state is a projection, not authority.

## 5. Scope

### In scope

- `ApprovalMode::{Interactive, Automatic, Yolo}` domain/config/protocol shape;
- normalized `ApprovalRequest`/`ApprovalDecision` and one router;
- immutable effective execution-policy snapshot at turn/tool-batch boundary;
- migration of direct AgentLoop human-wait branches through router;
- canonical PermissionStore path wiring and persistence diagnostics;
- principal-scoped `RuntimePreferenceStore` foundation for approval/sandbox preference;
- core protocol get/set/effective mode state;
- receipt/audit source propagation;
- tests/docs.

### Explicitly out of scope

- Automatic reviewer implementation (M006);
- production sandbox policy wiring (M005);
- selected-model preference (M004 consumes the store contract);
- granular capability-approval redesign (M007 polish);
- changing underlying permission rule syntax;
- team policy administration UI.

## 6. Required production changes

### Core/domain

Define:

```text
ApprovalMode = Interactive | Automatic | Yolo
ApprovalRequest = normalized action/tool/path/args-summary + escalation reasons + effect metadata + policy revision
ApprovalDecision = Allow | Deny | DeferUser (+ bounded reason/source)
ExecutionPolicySnapshot = approval mode + sandbox preference + principal/session/agent ceiling + policy revision + reviewer config identity
```

Keep raw sensitive args out of durable preference/audit payloads unless existing redaction policy already permits them.

### ApprovalRouter

Create one service invoked after deterministic PermissionChecker/SecurityService evaluation:

- `Allow` -> immediate receipt;
- `Deny` -> immediate denied outcome;
- `Escalate` + Interactive -> existing PermissionRegistry human request;
- `Escalate` + Yolo -> Allow receipt with source `yolo`, provided no hard ceiling/deny violation;
- `Escalate` + Automatic before M006 -> `DeferUser`/Interactive fallback under an explicit rollout flag, never auto-allow.

Consolidate duplicate permission-pending wait logic from sensitive/security/general Ask branches behind this service while preserving reason metadata.

### Runtime preferences

Add daemon-owned principal-scoped bounded store, preferably in `codegg-core` existing SQLite ownership, with revision/timestamp and fields reserved for M004:

```text
principal_id
approval_mode?
sandbox_profile?
last_provider_connection_id?
last_model_id?
revision
updated_at
```

No secrets. Local personal mode resolves explicit LocalOwner principal. Project/admin ceiling remains separate.

Persist approval/sandbox preference on successful user mode changes. Resolve precedence per ADR-0004.

### PermissionStore persistence

Define one canonical per-user permissions path through config path helpers and pass it to production PermissionChecker construction. Preserve atomic temp+rename and optional HMAC semantics. Do not give subagents independent global decision stores; scope/parent policy must remain clear.

Handle write failure visibly: in-memory decision may apply for current run but UI/log reports it was not persisted.

### Protocol and frontends

Add frontend-neutral requests/responses/events for get/set effective approval mode/policy snapshot as needed. Do not edit TUI manifest as security authority. Older clients can ignore new fields.

### Runtime/concurrency

Turn/accepted tool batch receives a fixed snapshot/policy revision. Mode changes from another frontend apply on the next safe turn/evaluation boundary and cannot retroactively bless a pending action.

### Security and authorization

Setting Yolo/Automatic requires the caller's normal session/settings authority. Mode change cannot exceed project/admin policy. Child loop receives an effective ceiling no broader than parent; M003 may wire the data path even before full UI.

### Documentation/static guards

Update permission architecture and add a targeted guard/test that production permission escalation goes through ApprovalRouter rather than directly registering PermissionPending from new call sites.

## 7. Ordered work packages

### Work package A — Types and preference store

Land ApprovalMode, ExecutionPolicySnapshot and principal-scoped RuntimePreferenceStore with migration/CAS/bounds.

### Work package B — Router and AgentLoop convergence

Normalize current permission/security branches into one route and preserve human request UX for Interactive.

### Work package C — Durable permission decisions

Wire canonical PermissionStore path through agent loop/main/exec/subagent construction with correct scope and persistence-failure diagnostics.

### Work package D — Protocol/effective state

Expose get/set/effective mode and receipt/audit source; keep frontend-neutral.

## 8. Failure, cancellation, restart, and contention semantics

- RuntimePreferenceStore write failure does not silently claim persistence; current explicit turn override may remain effective and diagnostic is shown.
- PermissionStore load corruption/tamper retains existing conservative ignore behavior.
- Human approval timeout remains deny/defer according to existing policy; router records timeout distinctly from explicit user deny.
- pending Interactive request is not changed to Yolo if the user toggles mode concurrently; it uses the captured snapshot.
- restart reloads last preference and permission decisions; pending human requests are not resurrected as approvals.
- stale preference revision update returns conflict/reload rather than last-write-wins between frontends.

## 9. Compatibility and migration

- existing PermissionConfig/rules unchanged;
- `Ask` maps to router Escalate;
- existing built-in modes remain independent;
- old permission JSON remains readable;
- preference table is additive/empty on upgrade;
- `with_exec_mode()` remains compatibility until later cleanup and must be documented as separate from Yolo.

## 10. Required tests

### Focused unit tests

- router matrix for Allow/Deny/Escalate × Interactive/Yolo/Automatic-placeholder;
- hard deny always wins;
- preference precedence/CAS/bounds;
- PermissionStore path/write/read/signature behavior.

### Integration tests

- existing safe tool remains no-prompt;
- destructive Ask in Interactive emits one human request;
- same Escalate in Yolo allows with Yolo receipt;
- security Deny remains denied in Yolo;
- Automatic placeholder defers safely;
- all current sensitive/security/general Ask call sites use router.

### Restart and recovery tests

- approval/sandbox preference survives daemon restart;
- AlwaysAllow/AlwaysDeny survives production checker reconstruction;
- corrupt preference/permission records fail conservatively.

### Contention and cancellation tests

- two frontends update preference with revisions;
- mode change races pending approval;
- cancellation unregisters pending request.

### Security and negative tests

- child cannot select broader mode than parent ceiling;
- TUI manifest cannot override daemon preference;
- Yolo cannot override explicit deny/sensitive hard-deny.

### Migration and compatibility tests

- pre-preference DB opens cleanly;
- existing permissions JSON remains valid;
- existing Interactive flow remains compatible.

## 11. Required verification commands

```bash
cargo test --test permission
cargo test -p codegg-core -- runtime_preference
cargo test --test agent_loop_harness -- permission
python3 scripts/check_core_boundary.py
python3 scripts/check_execution_ownership.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/verify.sh quick
```

## 12. Documentation updates

- `architecture/permission.md`
- `architecture/security.md` approval-vs-security boundary
- `architecture/session.md` runtime preference ownership if relevant
- core protocol documentation.

## 13. Acceptance criteria

- one ApprovalRouter owns all escalation resolution;
- Interactive preserves existing behavior;
- Yolo auto-allows only Escalate within authority ceiling;
- Automatic is safely non-operational/defer until M006, not fail-open;
- production Always decisions survive restart;
- last approval/sandbox preference is daemon-owned, durable and frontend-neutral;
- tool receipts identify decision source/policy revision;
- concurrent mode changes cannot alter an already captured pending authorization.

## 14. Stop conditions

Stop if implementation would add a second authorization engine, move security authority into TUI state, let Yolo bypass explicit deny, or require reviewer behavior before M005/M006 safety foundations are available.

## 15. Closure evidence required

- router call-site inventory before/after;
- approval-mode decision matrix;
- PermissionStore production restart evidence;
- RuntimePreferenceStore schema/CAS tests;
- protocol compatibility evidence;
- hard-deny/Yolo and concurrency regression results;
- actual verification commands and residual limitations.

## 16. Handoff notes

Keep this milestone mostly deterministic. Automatic should exist as a typed state but should defer until M006; this lets the authorization and persistence architecture land without coupling correctness to a model reviewer.
