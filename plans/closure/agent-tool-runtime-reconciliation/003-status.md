# Agent Tool Runtime Reconciliation M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/agent-tool-runtime-reconciliation/003-descendant-tool-authority-ceilings.md`

Source subsystem roadmap:

- `plans/subsystems/agent-tool-runtime-reconciliation-roadmap.md#6-dependencies-and-milestones`

Repository baseline reviewed: `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`
(plus accepted M001/M002 closures on this branch, HEAD `832205c7`;
no drift beyond them — rebaseline verify is `cargo check` +
focused suites green before code change).

Hard dependency: M002 accepted closure
`plans/closure/agent-tool-runtime-reconciliation/002-status.md`
(verified before code change; descendant suite covers native, plugin,
and MCP identities per plan §2).

Implementation commits or pull requests:

- Pending commit on `planning/agent-tool-runtime-reconciliation` —
  M003 descendant ceilings (same push as M004; files below; SHA to be
  recorded at commit time)
  - `src/agent/parent_authority.rs` (new) — `ParentToolAuthoritySnapshot`
    (mint/validate/check_tool/narrow/encode/decode,
    `check_generation_fresh`), `ParentDenyReason`, 7 focused tests
  - `src/agent/mod.rs` — `parent_authority` module
  - `src/agent/tool_surface.rs` — `AgentCapabilitySet`
    Serialize/Deserialize, `resolve_with_parent_authority`,
    `from_registry_with_parent_authority`, parent-bound fingerprint
    (root fingerprints byte-stable)
  - `src/agent/loop.rs` — `parent_tool_authority` field +
    setter/getter
  - `src/agent/tool_batch.rs` — `mint_delegation_ceiling` (live
    re-resolution at delegation), exec-ctx threading for `task`,
    broker `allowed_tools` + `parent_tool_authority` forwarding
  - `src/tool/backend.rs` — `ToolExecutionContext.parent_tool_authority`
  - `src/tool/broker.rs` — `BrokerInvocationContext.parent_tool_authority`
    + inner-exec-ctx forwarding (broker rebuilds the inner context)
  - `src/tool/task.rs` — `parent_tool_authority` field + setter +
    `require_delegation_authority` (fail-closed) at durable-submit and
    spawner-send launch points (queue-only path needs none)
  - `src/agent/worker.rs` — `SubAgentRequest.parent_authority`,
    snapshot validation (integrity/expiry/session/generation),
    registry exact-name pre-filter, surface under ceiling, narrowed
    snapshot on loop + nested task tool, cross-workspace durable-lineage
    binding, `task` withheld when absent from ceiling
  - `crates/codegg-core/src/jobs/mod.rs` — additive
    `parent_authority: Option<String>` on `Subagent`/`SubagentRun`
    (no migration; legacy payloads decode as `None`)
  - `src/scheduler/executors.rs`, `src/job_dispatcher.rs` — decode
    fail-closed (corrupt → job/dispatch failure; absent → system
    delegation)
  - `src/tool/task.rs` payloads, `src/scheduler/submission.rs`,
    `src/core/daemon.rs`, `src/agent/snapshot_capture.rs`,
    `crates/codegg-core/src/jobs/schedule.rs`,
    `src/work_plan_eggplan.rs` — additive field threading
    (`None` = explicit root/system delegation)
  - `src/agent/specialized_runtime.rs`, `src/agent/snapshot_capture.rs`
    — system-delegation marking
  - `tests/agent_tool_m003_descendant.rs` (new) — 20 tests (matrix below)
  - `tests/subagent.rs`, broker/program/palette/audit test suites —
    additive `None` fields only, no behavior change
  - `scripts/check_descendant_tool_ceiling.py` (new) — static guard
  - `architecture/agent-tool-surface.md`, `architecture/tool_broker.md`
    — ceiling/broker-admission story reconciled

## 1. Executive finding

M003 is complete as an invariant boundary. A subagent's eligible tool
names **and** typed capabilities are a proven subset of its parent
turn's effective eligible names/effects end to end: the same
host-attested ceiling governs model discovery (child registry
pre-filter), provider definitions (child surface → definitions),
definition cache identity (filtered defs key it; child fingerprint
binds the parent snapshot fingerprint), and broker invocation
(`allowed_tools` manifest + forwarded snapshot into the inner
execution context). Exact-name denies and capability denies enforce
independently; forged/tampered/stale/cross-scope snapshots fail before
any provider request or side effect; nested delegation only narrows
(`narrow_for_child` monotonic, ancestry bound transitive); durable and
scheduler paths carry the ceiling as additive host-persisted JSON with
fail-closed decode; the model-driven `task` path fails closed when no
snapshot is available. No new ADR: broker, permission, sandbox,
AgentRun, and provider-wire ownership are unchanged; no new database
table or migration (payload JSON is forward-compatible).

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A — Typed immutable carrier (run/turn/workspace/principal scope, eligible names, caps, per-tool effect envelope, surface fp, MCP/plugin generations, policy rev, expiry); host-only mint at decision boundary; never from model args; carry through task/scheduler/durable/compat paths on existing identity contexts; smallest durable change, no new table | `parent_authority.rs` (mint/validate/check/narrow/encode/decode); `require_delegation_authority` (ctx-first, registration fallback); payload `Option<String>` + session/workspace binding; AgentRun audit → no migration needed (payload JSON additive, `authority_digest` untouched) | pass | Principal binding enforced via session scope (no separate principal threads through subagent loops today; documented §10). Per-tool effect envelope = `max_effect_class` wire spelling checked per call. |
| A — Forged/stale/missing ceiling fails closed, never `None` unrestricted | Tamper/expiry/session/generation validation in worker + `require_*` at both launch points; `MissingAuthority`/`InvalidParentReceipt` reasons; queue-only path exempt (nothing runs) | pass | Explicit root/system `None` is a distinct permitted kind with existing denied/filtered behavior, not a default. |
| B — Snapshot in `SubAgentRequest` + child `TurnRuntime`/`AgentLoop`; all production child surface sites intersect names AND caps; only roots omit; aliases canonicalized first, collisions fail; parent generation/policy in cache identity; new unseen wrappers default-deny | `SubAgentRequest.parent_authority`; `from_registry_with_parent_authority` (+ `resolve_with_parent_authority`); worker pre-filter + surface + loop install; child fp binds parent fp; `can_delegate` requires ceiling-`task` | pass | Root `turn_runtime`/`request_preparation` `None` paths unchanged (only roots omit). |
| C — Broker admission per child tool: envelope, exact name, contract effect, workspace/principal, expiry/revocation/generation, permission receipt + broker scope/policy; extend context minimally; never trust child `parent_id`; post-launch injections denied; nested re-intersects immediate parent; approvals only via canonical policy | `allowed_tools` manifest bind on child loops; `parent_tool_authority` forwarded broker→inner ctx; `check_tool` name+effect; cross-workspace durable-lineage binding; narrowed snapshot for grandchildren; approval/permission paths untouched | pass | Revocation narrows future calls via live re-mint at delegation + per-call broker grant/policy-revision check (canonical current-policy check, unchanged). |
| D — Update worker/turn_runtime/request_preparation/surface/task/scheduler/broker only as needed; preserve fast paths, depth, worktree, delegation counts, restart; bounded deny reasons with fingerprints, no sensitive payloads; docs + static guard vs `None` ceilings | Minimal diffs listed above; deny reasons bounded (`ParentCeiling`, `StaleGeneration`, `ForbiddenEffect`, `InvalidParentReceipt`, `MissingAuthority` + free-text scope only); docs + `check_descendant_tool_ceiling.py` | pass | Turn runtime + request preparation need no changes (child registry pre-filter makes definitions/cache agree). |
| Tests — root/plan/general/verifier/nested/durable/compat matrix incl. read-vs-git_query, plugin pair, MCP scoping, per-capability denials + tool_search negatives + manual dispatch, sandbox hold, alias resurrection, forgery/session/expiry/effect, restart/cancel/revocation, immediate+deferred+non-deferral | `agent_tool_m003_descendant` 20/20: exact-name trio, capability independence + 10-family matrix, alias/collision/nesting, fp binding, forgery matrix, generation matrix, payload roundtrip + legacy, broker manifest, task fail-closed + host-ctx launch, worker e2e (write withheld, task withheld), forged-snapshot e2e (Failed pre-request), concurrent siblings isolation | pass | Sandbox hold via unchanged `resolve_child_sandbox` + M005 suites green; durable restart via payload roundtrip + scheduler suites green; cancellation via existing suites; revocation via live re-mint design + per-call grant check (no new test — recorded §10). Deferred/non-deferral catalogs share the ceiling path (surface-level; wire behavior is M004). |
| Acceptance — real parent→child→broker trace, same host-attested ceiling everywhere, independent denies, forged fails pre-execution, nested/durable/restart tested, no unrestricted `None` child, sandbox/worktree/approval green | Worker e2e ×3 + broker manifest + task launch pair + guard + full suite greens below | pass | Hand-crafted in-turn tool calls to unregistered names park on approval-ask (pre-existing permission behavior, §10) — the ceiling withholds advertisement/registry/broker regardless. |

## 3. Production implementation evidence

What changed (design §5 target + plan §4):

- Mint is delegation-time live re-resolution (`mint_delegation_ceiling`:
  live registry + current-agent role denies + plan mode + spawner bit +
  live MCP generation + plugin activation + `config:{hash}` policy rev +
  24 h expiry), so revocation narrows the next child immediately; the
  snapshot itself is immutable once minted.
- Worker validates (integrity → expiry → session → MCP freshness),
  pre-filters the child registry by exact name, resolves the surface
  under the ceiling (names + caps + alias-first + collision-fail +
  parent-bound fp), narrows for broker/nesting, binds cross-workspace
  roots to verified durable lineage, and withholds `task` when absent.
- Broker: child loops bind `allowed_tools` (manifest denies
  post-launch injections); `parent_tool_authority` forwards through
  the rebuilt inner execution context (this was load-bearing:
  `agent_loop_harness` caught the drop).
- TaskTool: `require_delegation_authority` (ctx-first freshness,
  registration fallback for nesting) at durable-submit and
  spawner-send; encoded snapshot attached to request + both payload
  variants. Queue-only (no spawner/submission) proceeds without — it
  launches nothing.
- Scheduler executors + legacy dispatcher decode fail-closed; all
  other producers mark explicit system delegation (`None`).

What did not change (explicit non-goals honored): no model tool
rename; `task` args compatible (no new input field); no session,
worktree, sandbox, approval, AgentRun, provider-wire, or storage API
change; no new table/migration; `authority_digest` semantics
untouched; no polling; no second broker/permission system.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib -- agent::parent_authority agent::tool_surface agent::tool_batch agent::request_preparation tool::broker tool::tool_search tool::mcp_adapter agent::tool_descriptor_snapshot
cargo test -p codegg --test agent_tool_m003_descendant
cargo test -p codegg --test agent_tool_m002_discovery
cargo test -p codegg --test subagent
cargo test -p codegg --test tool_surface_minimization
cargo test -p codegg --test tool_broker_integration
cargo test -p codegg --test scheduler_authority_matrix
cargo test -p codegg --test agent_loop_harness
cargo test -p codegg --test search_backend_eggsearch
cargo test -p codegg --test fake_eggsearch_mcp
cargo test -p codegg --test mcp
cargo test -p codegg --test scheduler_submission_idempotency
cargo test -p codegg --test scheduler_protocol_consistency
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/check_descendant_tool_ceiling.py
python3 scripts/check_mcp_broker_adapter.py
python3 scripts/check_tool_descriptor_snapshot.py
bash scripts/verify.sh quick
```

### Results

- `cargo fmt --all -- --check`: exit 0.
- New `parent_authority` lib tests: 7/7 pass (roundtrip, tamper,
  expiry/workspace, name+effect independence, narrowing
  monotonicity, generation matrix, encode/decode).
- Focused lib sweep (surface/batch/preparation/broker/search/
  adapter/snapshot): 87/87 pass.
- `agent_tool_m003_descendant`: 20/20 pass (surface trio + matrix,
  aliases/nesting/fp, forgery/transport/broker/launch, worker e2e ×3
  incl. concurrent siblings with per-child ceiling attribution).
- `agent_tool_m002_discovery` 12/12, `subagent` 22/22,
  `tool_surface_minimization` 13/13, `tool_broker_integration` 25/25,
  `scheduler_authority_matrix` 13/13, `agent_loop_harness` 52/52,
  `search_backend_eggsearch` 9/9, `fake_eggsearch_mcp` 28/28, `mcp`
  26/26, `scheduler_submission_idempotency` 11/11,
  `scheduler_protocol_consistency` 13/13 — all pass.
- `cargo clippy --workspace --all-targets -- -D warnings`: exit 0
  (two findings fixed during work: collapsible match, unused mut).
- All three guards: `OK`, exit 0.
- `verify.sh quick`: exit 0 (fmt, agent schema, core/client/desktop
  boundaries, sandbox, execution-ownership, TUI authority,
  http-route-disposition, audit-coverage, scheduler-bypass,
  config-merge-coverage, no-hardcoded-models, provider
  wire/catalog/resilience + OpenAI endpoint + Eggwork routing,
  `cargo check --workspace --all-targets --locked`).

Not run (recorded, not implied): full workspace nextest sweep,
`server,plugins,lsp-test-support` feature tests beyond quick, live
providers/models, real remote MCP servers, native macOS/Windows,
hosted CI. Plan §8 literal selectors
(`--workspace tool_surface/subagent/agent_run/tool_broker/tool_search`)
do not exist as package names; the replacement (lib filters + named
integration targets above, incl. scheduler suites for the durable
axis) is recorded per plan §8.

## 5. Invariant review

- Child names/effects ⊆ parent effective (minus host-internal
  non-model `task`-withholding mechanics): holds — pre-filter +
  surface intersection + broker manifest + `check_tool`, all
  fail-closed; no widening via role/alias/provider-quirk/
  plugin/MCP/search/asset/refresh/nesting (aliases canonicalized,
  collisions fail, unseen defaults deny, fp binds generations).
- Backend availability + user/project approval still additional:
  holds — `has_functional_spawner`, `MissingBackend` omissions,
  permission/approval/sandbox paths untouched (harness + approval
  suites green).
- Stale/forged/missing → fail closed, never `None`: holds — worker
  validation + launch requirements; system-`None` is explicit and
  retains denied/filtered behavior.
- Delegation never implies unauthorized child mutation: holds —
  exact-name + capability + max-effect envelope enforced at surface
  and invocation; sandbox/worktree narrowing independent
  (capability ≠ sandbox; M005 suites green).
- No model field forges ceiling; no credentials/URLs/raw policy to
  agents: holds — no input field exists (guard-enforced); snapshot
  carries names/caps/fingerprints/revisions only.

## 6. Failure and recovery review

- Duplicate delivery: snapshot is read-only input, never a replay
  token; scheduler idempotency/delegation keys unchanged.
- Cancellation: worker validation is synchronous pre-loop; broker
  cancellation/deadline paths unchanged; e2e harness cancellation
  suites green.
- Restart: ceiling re-resolves from host-persisted payload
  (durable) or fails closed when absent/corrupt (legacy payloads run
  as before — recorded risk, §10); no auto-reissue of permissions;
  run-lineage binding verified for cross-workspace roots.
- Stale generation/lease: `StaleGeneration` fail-closed at child
  build; in-flight adapter pins fail closed pre-transport (M001);
  expiry bounds every snapshot (24 h).
- Contention: mint uses `try_read` for MCP (never blocks); no
  locks held across awaits beyond existing patterns; concurrent
  siblings test proves per-child isolation.
- Malformed input: non-registry names → omission/NotFound; corrupt
  transport → fail-closed decode; unknown effect spellings →
  `ForbiddenEffect`; empty names → `InvalidDefinition` (unchanged).
- Bounded diagnostics: deny reasons are codes + fingerprints only.

## 7. Migration and compatibility review

No storage migration (payload JSON additive with `serde(default)`;
`authority_digest` untouched), no protocol/DTO break (all additions
`Option` + additive), no config key change, no model tool rename.
`TaskTool` args compatible. Legacy scheduler payloads and direct
`execute()` queue-only behavior preserved. Rollback is a plain
revert with no data migration. Cross-crate surface: one additive
optional field on two `JobPayload` variants (codegg-core) + additive
struct fields in the root crate + ~40 test-literal `None` fills.

## 8. Security review

- Authorization: exact-name + capability + effect-envelope at
  surface, registry, broker manifest, and invocation (`check_tool`
  matrix green); forged snapshot e2e fails pre-request.
- Secret handling: snapshot/telemetry carry names, caps, fps,
  revisions — no credentials, tokens, endpoints, args, or outputs.
- Path validation: workspace scope bound at delegation + worker
  (prefix containment or verified durable lineage); path allowlists
  and `**` deny preserved.
- Privilege boundaries: remote annotations never lower category
  (M001 preserved); plugin hooks still confined (M002 guard green);
  `task` withheld without ceiling membership.
- DoS bounds: mint is one in-memory resolve per `task` call (no
  network, no locks across awaits); broker bounds unchanged.
- Audit: pre/post hooks + receipts + provenance unchanged; ceiling
  decisions emit bounded debug lines (eligible/omission counts +
  fingerprints).

## 9. Documentation and operations

- Updated: `architecture/agent-tool-surface.md` (ceiling story +
  carriage + guard), `architecture/tool_broker.md` (21-field context
  + manifest admission note).
- New guard: `scripts/check_descendant_tool_ceiling.py` (run
  manually; wiring into `verify.sh`/CI deferred as a low follow-up
  like the M001/M002 guards).
- Operator diagnostics: worker debug lines (registry narrowing,
  ceiling resolution fps); delegation debug via existing broker
  tracing; deny reasons are bounded codes safe for logs.
- Recovery: stale/expired/mismatched → typed refusal naming the
  reason class; next delegation re-mints live. No manual steps.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Hand-crafted in-turn calls to unregistered names park on approval-ask with no responder in headless tests (pre-existing permission behavior) | Forged calls still cannot execute (no registry entry, no broker path); UX treats hallucinated tools as approval items | Consider fast-fail for registry-absent tools before approval as separate polish; explicitly out of M003 scope. |
| low | No separate principal thread through subagent loops; binding is session/workspace scope | Cross-principal confusion would need session confusion first (permission receipts remain per-call) | A future identity milestone may thread `origin_principal` into the snapshot's principal scope; validation already accepts the shape. |
| low | `plugin_activation_rev` is empty when no plugin service backs the mint; policy rev uses the `config:{hash}` scheme only | Drift is still caught by exact-name filtering + worker MCP check; activation changes without name changes are invisible to the ceiling | Revisit if plugin hot-activation without tool changes needs generation teeth; M002 cache already keys activation. |
| low | Legacy durable payloads (pre-ceiling) resume as system delegations | Old resumed runs predate the invariant; new submissions always carry it | Note in runbook; no migration (deliberate smallest-change decision per §4A audit). |
| low | Queue-only `execute()` without spawner/submission needs no authority | Nothing runs; consistent with fail-closed-at-launch | None. |
| low | New guard not yet wired into `verify.sh`/CI (like M001/M002 guards) | Manual runs only | Wire all three guards into `verify.sh quick` + CI as one follow-up. |
| low | Live-provider/remote-MCP/native-OS/hosted-CI evidence not run | Deterministic closure only | Hosted `CI / verify` + Desktop E2E on the closure revision before merge. |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone M003 closed. Successor state:

- M004 (`004-provider-deferral-wire-qualification.md`, hard M002,
  soft M003) → `ready` (both satisfied now: hard M002 by its closure,
  soft M003 by this closure; rebaseline before code change).

No other registered plan is unblocked by this closure.

## 12. Registry updates

- `plans/registry.md`: move M003 `ready` → `closed` (link this
  closure); M004 stays `ready` (hard M002 + soft M003 both satisfied).
- `plans/subsystems/agent-tool-runtime-reconciliation-roadmap.md`:
  header `M001–M002 closed; M003–M004 ready` → `M001–M003 closed;
  M004 ready`; milestone table M003 `ready` → `closed`.
- `plans/implementation/agent-tool-runtime-reconciliation/003-descendant-tool-authority-ceilings.md`:
  `ready for handoff` → `implemented; closed` with closure link.
- `plans/implementation/agent-tool-runtime-reconciliation/004-provider-deferral-wire-qualification.md`:
  keep `ready for handoff` (rebaseline before code change; note soft
  M003 now satisfied).
