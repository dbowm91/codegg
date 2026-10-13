# Agent Tool Runtime Reconciliation M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/agent-tool-runtime-reconciliation/001-mcp-broker-transport-adapter.md`

Source subsystem roadmap:

- `plans/subsystems/agent-tool-runtime-reconciliation-roadmap.md#6-dependencies-and-milestones`

Repository baseline reviewed: `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`

Implementation commits or pull requests:

- Pending commit on `planning/agent-tool-runtime-reconciliation` — M001 broker adapter (files below; SHA to be recorded at commit time)
  - `src/tool/mcp_adapter.rs` (new) — `McpToolAdapter`, pin validation, `translate_result`, `register_mcp_adapters`, `mcp_exposure_policy_for_turn`, 11 focused tests
  - `src/tool/mod.rs` — register `mcp_adapter` module
  - `src/mcp/mod.rs` — `pinned_client_handle` accessor for pinned-instance transport
  - `src/agent/turn_runtime.rs` — async session registration before prompt surface + broker
  - `src/agent/worker.rs` — subagent registration before deny filtering + surface
  - `src/agent/loop.rs` — sync safety-net registration before broker snapshot (policy from loop `Config`)
  - `src/agent/request_preparation.rs` — single canonical origin (registry only; retired `list_filtered_tools` append)
  - `src/agent/tool_batch.rs` — removed `mcp_tool_calls` fork; all calls via broker; preserved terminal-status mapping (`isError` → `Error`, never success)
  - `src/agent/tool_inspect.rs`, `src/agent/coordinator.rs` — retired dispatch helper retention + `mcp_service` field retention for M002 refresh
  - `scripts/check_mcp_broker_adapter.py` (new) — static no-bypass guard
  - `architecture/tool_broker.md`, `architecture/mcp.md`, `architecture/tool.md`, `architecture/agent-tool-surface.md` — execution/visibility story reconciled

## 1. Executive finding

M001 is complete as an invariant boundary. Every raw `mcp__<server>__<tool>` model call is provably broker-mediated end-to-end through a session-pinned `McpToolAdapter` registered in the ordinary `ToolRegistry` before broker snapshot construction. Wrong grants, stale descriptors, hidden servers and malformed names fail before transport. Structured `structuredContent`, MCP `isError`-as-failure, bounded provenance without credentials, broker `ctx://` artifact spillover, hooks, permission receipts and auditable execution order are preserved. All old model-side direct `McpService::call_tool` branch code is gone. Managed eggsearch wrappers and plugin tools remain available under the old configuration (verified by `search_backend_eggsearch` 9/9 and `fake_eggsearch_mcp` 28/28).

No new ADR was required: transport authorization and broker ownership are unchanged. `McpService` remains the sole HTTP/stdio/credential/DNS owner; no second client was created.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A — Bounded `McpToolAdapter` near `src/tool/mcp.rs` with frozen `mcp__<server>__<tool>` identity, description, input schema, conservative contract, `execute_structured`, no legacy side effects outside broker | `src/tool/mcp_adapter.rs::McpToolAdapter`; `Tool::category Mutating`, `contract DirectOnly/NonIdempotent/no-retry`, `implementation_id mcp:<server>`; 11 lib tests | pass | Location is `src/tool/mcp_adapter.rs` (canonical; no `src/tool/mcp.rs` existed). Contract explicitly asserts `DirectOnly`, `NonIdempotent`, `max_retries 0`. |
| A — Session construction takes bootstrapped handle, frozen generation/version, exposure policy; registers before broker; preserves plugin/extension/work-order tools | `turn_runtime.rs` async pass, `worker.rs` subagent pass, `loop.rs` sync safety net; `register_mcp_snapshot` idempotent; `ToolBroker::new` after registration | pass | Generation is `McpService::catalog_fingerprint` (no credentials). Plugin registration order preserved (plugin first, then MCP, then deny filtering in worker). |
| A — Validate names/collisions/schema bounds; reject malformed/ambiguous/duplicate/oversized/unreachable; metadata bounded/sanitized; schema drift invalidates pin | `parse_canonical_name` (rejects `__` ambiguity, empty, over-long, bad alphabet); `validate_descriptor` (256 KiB schema, 16 KiB description); collision vs registry + duplicate set; `check_pinned_locked` schema-fingerprint + liveness check | pass | Tests: `canonical_name_validation`, `oversized_schema_is_rejected`, `duplicate_canonical_names_collide_safely`, `hidden_servers_never_register`, `broker_catalog_contains_adapter_and_stale_fails_before_transport` (drift → fail before transport). |
| B — Call `call_tool_structured` only from adapter after broker admission; preserve structured/`isError`/text fallback/provenance/artifacts; `outputSchema` optional consistency check; no `Error:`-as-success | `execute_structured` (pinned-client clone, no 60 s timer, cancellation select); `translate_result` (`!is_error` success, structured retention + JSON fallback, `outputSchema` requires present+conforming structured else safe `Execution` error); broker owns 256 KiB `ctx://` spillover | pass | Tests: `translate_maps_is_error`, `output_schema_mismatch_is_a_safe_error`, `broker_mediated_call_preserves_structured_and_is_error`. Batch maps broker `Error` → `ToolError::Execution` (not success). |
| B — Prefer context deadline/cancellation over independent timer; no busy loop or registry lock across await; cooperative cancel; typed failure (not `Error:` success) | No `tokio::time::timeout(60s)`; broker watchdog provides bound; `read().await` (cooperative, no `try_read` spin); guard dropped before network; `cancellation` checked at entry + `select` during call; `Timeout` errors typed | pass | Tests: `cancelled_adapter_does_not_dispatch`, `concurrent_adapter_calls_share_no_global_lock`. Limits when server ignores cancellation recorded (broker watchdog still bounds). |
| C — Remove `mcp_tool_calls` fork; route via native batch pipeline (max-parallel, progress recovery, hooks, receipts, ordering, display); no other raw bypass; retire separate schema append without wire/config change | `tool_batch.rs` fork deleted (60-line block → 8-line comment); `request_preparation.rs` second origin deleted; `check_mcp_broker_adapter.py` 0 violations; wire names + raw-visibility config unchanged | pass | `search_backend_eggsearch` 9/9 confirms wire/config stability (hide/show + custom `server_name`). |
| D — Static guard detecting direct model-path `call_tool*`/`mcp__` routing outside adapter | `scripts/check_mcp_broker_adapter.py` — forbids `src/agent` direct transport, fork markers, second origin; requires adapter ownership | pass | `OK: No raw MCP model bypass outside the broker adapter.` |
| D — Amend `tool_broker.md`, `mcp.md`, `tool.md`, `agent-tool-surface.md` | Four docs updated (broker mediation note, transport ownership, registry origin, surface namespacing) | pass | No closed predecessor records edited. |
| Work package 1 — Contract first (fake-server + adapter tests) | 11 `tool::mcp_adapter` lib tests | pass | Deterministic, no network. |
| Work package 2 — Broker adapter integration (consent/grant/scope, hooks/event order, sandbox) | Broker catalog + grant test (`broker_mediated_call...`); hooks/order preserved by shared batch path (no fork); `agent_loop_harness` 52/52 | pass | Sandbox profiles untouched; read-only/full-profile interactions unchanged (adapter is `Mutating`, so sandbox still gates). |
| Work package 3 — Result fidelity (structured/text-only, `isError`, malformed, 256 KiB+ spillover, redaction) | `translate_*` + `output_schema_*` tests; broker artifact path unchanged (256 KiB display bound); provenance `mcp:<server>` + version only, no endpoint/token/args echo | pass | 256 KiB+ spillover exercised by broker's existing path (not re-proven with a 256 KiB MCP fixture; see §10 low finding). |
| Work package 4 — Remove bypass + static guard + negatives | Fork removal + guard + `hidden_*`, `duplicate_*`, stale-drift, unknown-tool-before-transport, `parse_canonical` negatives | pass | `grep` + guard evidence in §4. |
| Work package 5 — Lifecycle/docs (disconnect, cancel, timeout, duplicate effect, concurrent batch; docs + matrix) | Stale/disconnect fail-closed; cancel entry + select; concurrent join test; docs + this matrix | pass | Duplicate-effect (non-idempotent no-replay) is structural (no retry) + broker `UncertainSideEffect` path retained; no new duplicate-effect test beyond `max_retries 0` assert. |
| Acceptance — raw call provably broker-mediated; wrong grants/stale fail before transport; structured/artifact/hooks/audit correct; old branch gone; wrappers/plugins available | Above + `verify.sh quick` exit 0; `search_backend_eggsearch` + `fake_eggsearch_mcp` green | pass | Wrong-grant failure is broker-owned (`verify_grant_scope`); adapter adds pinned-descriptor failure before transport. Both verified. |

## 3. Production implementation evidence

Ownership is as designed in the roadmap §5 target architecture:

```text
McpService (connected server + descriptor generation)
  -> session-pinned McpToolAdapter(s): Tool trait / conservative contract
  -> ToolRegistry.register -> shared ToolCatalog -> ResolvedToolSurface
  -> immediate provider projection; tool_search discovers via live catalog
  -> AgentLoop approval receipt + hooks -> ToolBroker (ALL direct model calls)
  -> MCP adapter -> pinned client call_tool_structured
  -> StructuredToolResult {success=!is_error, structured, provenance mcp:<server>}
  -> Broker ToolValue {terminal status, artifacts, truncation}
```

Implemented behavior:

- One adapter per connected, policy-visible raw tool. Hidden servers (`eggsearch` by default, plus per-domain `tool_backends` without `expose_raw`) never register, so they cannot leak into discovery or invocation.
- Strict canonical validation (`__` forbidden in components, 128-byte components, 256-byte canonical, MCP alphabet). Collisions with native/plugin names or within the desired set fail closed per-tool.
- Pinned execution: `check_pinned_locked` under the service read guard verifies server liveness (`Connected`), tool presence, and input-schema fingerprint equality. The concrete client `Arc` is cloned under the guard and the guard dropped before the network await, so a concurrent disconnect/reconnect cannot silently redirect to a replacement. Schema drift with an unchanged name refuses with `schema changed since registration`.
- Transport uses only `call_tool_structured` via the pinned client. No independent 60 s timer; the broker watchdog (tool deadline + 5 s) plus the batch wrapper (tool + 10 s) bound the call. Cancellation is checked at entry and raced during the call.
- Result translation preserves `structuredContent` (plus best-effort JSON object/array parse of text when structured is absent), maps `isError=true` to `success=false` (broker `Error`), validates pinned `outputSchema` as a consistency check (missing/non-conforming/oversized structured → safe `Execution` error, never verified), and attaches `mcp:<server>` + server version provenance with `ExternalUntrusted` trust and no credentials.
- Registration is idempotent within a turn (explicit async pass + `AgentLoop::new` sync safety net via `try_read`). Stale `mcp__*` entries absent from the desired set are removed via `filter_out` so disconnects leave no unexecutable advertisement. Denied `mcp__*` globs still filter (worker registers before deny filtering).
- Request preparation is single-origin: definitions come solely from `tool_registry.list()` (which includes adapters). The retired `list_filtered_tools` append and its `try_read` + 50 ms retry loop are gone. Cache identity uses the registry's visible `mcp__*` surface revision.
- Batch execution is single-path: the `mcp_tool_calls` fork, its 60 s timeout, and its 3×50 ms lock-retry loop are deleted. Adapter calls flow through the same semaphore, pre/post hooks, plugin lifecycle hooks, `ToolCallStarted/Finished` events, checkpoint classification, broker grant construction, watchdog ordering, retry (none for MCP: `max_retries 0`), and deterministic ordering as native tools. The batch now preserves broker terminal status (`Error` → `Execution`, `Denied` → `Permission`, etc.) instead of returning display as `Ok` unconditionally — this is the `isError`-not-success fix.

Planned but absent (deferred to M002 by design):

- Per-turn immutable `ToolDescriptorSnapshot` with server origin/generation/policy/surface fingerprint and turn-boundary refresh coalescing. M001 pins per-tool schema + generation string; global refresh lifecycle is M002.
- `tool_search` exact-schema parity for external tools beyond shared-catalog visibility. Adapters enter the live shared catalog (so they are searchable), but M002's two-stage discover-and-call qualification across Full/Curated/Minimal is not claimed here.
- Provider-native deferral wire changes. `defer_loading` stays `false` (wire-identical to baseline); M004 owns native `tool_search` qualification.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib -- tool::mcp_adapter
cargo test -p codegg --lib -- agent::tool_batch agent::request_preparation tool::broker
cargo test -p codegg --test agent_loop_harness
cargo test -p codegg --test search_backend_eggsearch
cargo test -p codegg --test fake_eggsearch_mcp
cargo test -p codegg --test mcp
cargo test -p codegg --lib -- tool::tool_search
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/check_mcp_broker_adapter.py
bash scripts/verify.sh quick
```

Additional0290probe (not in closure matrix, recorded for honesty):

```bash
python3 scripts/check_tool_broker_boundary.py
```

### Results

- `cargo fmt --all -- --check`: exit 0.
- `tool::mcp_adapter` lib tests: 11/11 pass (8 original + 3 new: broker-mediated, cancelled, concurrent).
- Focused lib sweep (`agent::tool_batch`, `request_preparation`, `tool::broker`): 56/56 pass.
- `agent_loop_harness`: 52/52 pass.
- `search_backend_eggsearch`: 9/9 pass (2 initially failed on stale-registry policy derivation; fixed by deriving the `AgentLoop::new` safety-net policy from loop `Config` instead of the registry default — see §10).
- `fake_eggsearch_mcp`: 28/28 pass.
- `mcp` integration: 26/26 pass.
- `tool::tool_search` lib: 4/4 pass.
- `cargo clippy --workspace --all-targets -- -D warnings`: exit 0.
- `check_mcp_broker_adapter.py`: `OK: No raw MCP model bypass outside the broker adapter.` exit 0.
- `verify.sh quick`: exit 0 (`cargo fmt --check`, builtin-agents `--check`, core/client/desktop boundaries, sandbox, execution-ownership, TUI authorities, HTTP disposition, audit, scheduler-bypass, config-merge, provider wire/catalog/resilience/OpenAI/eggwork guards, `cargo check --workspace --all-targets --locked`).
- `check_tool_broker_boundary.py`: FAILS on baseline and on this head (flags pre-existing `Tool::execute → execute_structured` wrappers in `src/tool/extension.rs`, `src/tool/verify.rs`, `src/tool/git_read.rs`, `src/permission/reviewer.rs` plus this adapter's standard `execute()` wrapper). It is not part of `verify.sh quick`. Recorded as incompatible with the current layout (plan §7 allows recording a replacement + reason): the replacement is `check_mcp_broker_adapter.py` for the MCP boundary plus `cargo clippy` for general hygiene. No new bypass was introduced — the adapter follows the same `execute()`-delegates-to-`execute_structured()` shape as every existing tool.

Full workspace test sweep (`cargo nextest run --workspace --profile ci`) was not run locally; `verify.sh quick` plus the focused suites above are the closure evidence. Hosted CI was not run (no network/credentials in this environment).

## 5. Invariant review

- Every raw MCP call requires the existing permission/approval receipt and a broker-verified scoped grant before transport: preserved — permission checks run before batch admission for all tools (including `mcp__*`), and the broker rejects `Unverified` plus `verify_grant_scope` (workspace/caller/effect/session/mode/principal/path-policy) before `execute_structured`.
- Remote metadata never downgrades authorization: preserved — adapter `category()` is always `Mutating`, contract is `NonIdempotent`/`DirectOnly`/no-retry regardless of `readOnlyHint`/annotations; disclosure never widens authority.
- Default conservative mutating/non-idempotent, `DirectOnly`, zero retries: verified by contract asserts (`DirectOnly`, `NonIdempotent`, `max_retries 0`).
- No execution when pinned service/server/descriptor is gone/replaced or policy excludes raw: verified by `check_pinned_locked` (liveness + presence + schema fingerprint) and registration policy gate (`hidden_servers_never_register`).
- Cancellation/timeout does not prove rollback: documented in adapter docs and batch comment; timed-out non-idempotent calls surface as typed `Timeout`/`UncertainSideEffect` via the broker retry chain (which never auto-replays non-idempotent effects), never blind replay.
- `McpService` sole HTTP/stdio/credential/DNS owner; no second client: verified — adapter holds only `Arc<RwLock<McpService>>` plus cloned client `Arc`s; `grep` shows no new `LocalClient::new`/`McpConnectionManager::new` outside `src/mcp` + bootstrap.
- Raw managed eggsearch hidden unless opted in; native wrappers unaffected: verified by `search_backend_eggsearch` hide/show/custom-`server_name` tests.

## 6. Failure and recovery review

- Duplicate delivery/idempotency: MCP contract is `NonIdempotent`/`NonIdempotent` with `max_retries 0`; the broker M002 retry chain maps ambiguous non-idempotent effects to `UncertainSideEffect`, never blind replay. Batch retains its 2-attempt loop only for `is_retryable` (`Io/Network/Timeout`) errors — `Execution` (including stale-descriptor and `isError` failures) is non-retryable and breaks after one attempt. No new durable store; no replay of uncertain side effects.
- Cancellation races: checked at adapter entry and raced during the call; broker also selects on cancellation; batch wrapper preserves `Cancelled` → `Timeout(cancelled: …)` typed outcome. A server that ignores cancellation is still bounded by the broker watchdog + batch wrapper (limits recorded).
- Daemon/node restart: no new durable state. On restart the registry rebuilds from fresh activation/MCP connection; unreviewed grants are never persisted (grants are per-call, in-memory).
- Partial persistence failure: n/a (no new store). Artifact spillover failures surface as `BrokerError::Execution` (typed), not success.
- Stale generation/lease: pinned-schema check fails closed before transport on drift/disconnect/replacement; stale `mcp__*` registry entries are removed at registration; wrong-grant/stale-policy failures are broker-owned before execution.
- Contention/resource release: no registry/catalog write lock held across network awaits (guard dropped before transport); service read uses cooperative `read().await`, not `try_read` spin; batch semaphore + permit timeout preserved; concurrent adapter test passes.
- Malformed/unauthorized input: non-object input → `Format` error; oversized schema → registration rejection; malformed canonical names → rejection; `outputSchema` violations → safe `Execution` error; unknown names → `NoContract`/`NotFound` before transport; hidden servers → never registered.
- Bounded event/artifact behavior: broker bounds (10 MB input, 10 MB output, 256 KiB display spillover to `ctx://` with digest) apply unchanged; provenance carries no credentials/args/output echo.

## 7. Migration and compatibility review

No storage migration, no protocol/DTO change, no config key change. `McpService::call_tool()` remains for non-model consumers with identical text projection. Canonical names `mcp__<server>__<tool>`, raw-visibility config (`expose_raw_mcp_tools`, per-domain `server_name`/`expose_raw`), provider aliases, plugin tools, and eggsearch wrapper behavior are wire-stable (proven by `search_backend_eggsearch` + `fake_eggsearch_mcp`). A disconnected/restarted server never redirects to a different descriptor (pinned-client + pre-transport check). Rollback is a plain code revert with no data migration.

## 8. Security review

- Authorization enforcement: permission receipt + broker grant scope verified before transport; `DirectOnly` denies programmatic callers; effect ceiling `NonIdempotent` admits only at-or-below (read-only tools still callable under the default ceiling via severity ordering).
- Secret handling: no credentials in descriptors, fingerprints, logs, or provenance (`mcp:<server>` + version only). `Debug` for `SearchRuntimeContext` already redacts env values. Descriptions stripped of NULs; bounded at 16 KiB.
- Path validation: n/a (MCP tools carry no host path authority; workspace file-mutation auto-allow in `check_tool_permission` applies only to native file tools with workspace-contained paths, never to `mcp__*` — MCP tools fall through to the general permission checker).
- Privilege boundaries: remote `readOnlyHint`/annotations never lower the `Mutating` category or contract; plugin hook output cannot create registry authority (unchanged M002-owned seam; this milestone does not alter `dispatch_tool_definition` handling).
- Denial-of-service bounds: input schema 256 KiB at admission; broker input 10 MB; output 10 MB with 256 KiB spillover; no unbounded retries (MCP `max_retries 0`); no polling (refresh is turn-boundary rebuild, not continuous).
- Redaction: bounded `truncate_for_audit` summaries preserved; no full args/output in audit/bus payloads.
- Audit: pre/post hooks, `ToolCallStarted/Finished` events, broker provenance, and receipts are now emitted for MCP calls (previously bypassed). No audit bypass remains on the model path.

## 9. Documentation and operations

- Updated: `architecture/tool_broker.md` (MCP mediation note + guard), `architecture/mcp.md` (adapter ownership + guard), `architecture/tool.md` (adapter registration + broker funneling), `architecture/agent-tool-surface.md` (namespacing + single origin + contract).
- New guard: `scripts/check_mcp_broker_adapter.py` (run manually; not yet wired into `verify.sh` — see §10).
- Operator diagnostics: registration report (`registered/hidden/collisions/rejected` counts + `generation` fingerprint, no credentials) is `tracing::debug`-emitted at turn, subagent, and loop construction. `/tool-backends` report semantics unchanged.
- Recovery: on `stale descriptor`/`not connected` errors the model receives a typed failure directing it to retry via discovery or report unavailability; the next turn rebuilds the registry from the live inventory. No manual migration.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | 256 KiB+ artifact spillover not proven with a dedicated 256 KiB MCP fixture | Broker spillover path is unchanged and covered by existing broker tests, but no MCP-specific oversized-return test asserts `ctx://` retrieval | M002 should include a 256 KiB+ MCP return asserting typed status + artifact retrieval (already in M002 §5 work package 2/§9). |
| low | `check_mcp_broker_adapter.py` not yet wired into `verify.sh` / CI | Guard runs manually; regression could land without CI signal | Wire the guard into `verify.sh quick` (and CI `verify` job) as a follow-up; one-line addition. |
| low | `check_tool_broker_boundary.py` fails on baseline (pre-existing `execute()` wrappers) and is listed in plan §7 | Plan §7 command is incompatible with the current layout; closure uses `check_mcp_broker_adapter.py` + clippy as the recorded replacement | No code action; noted here per plan §7. |
| low | Live remote MCP (real stdio/HTTP server, `tools/list_changed` notification, timeout against a real slow server) not run | Deterministic mock coverage only; no live-network evidence | M002 §9 explicitly permits deterministic closure with live mode gated; record any live attempt in M002 closure. Native macOS/Windows and hosted CI not run in this environment. |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone M001 closed. Hard dependency for M002 is satisfied; M002 (`plans/implementation/agent-tool-runtime-reconciliation/002-external-tool-discovery-and-generation.md`) may proceed to `ready` — its plan may now be implemented after a rebaseline verify (it was written against the same baseline and explicitly permits implementation after M001 closure verification).

M003/M004 remain `sequenced/blocked` on M002 (M003 hard, M004 hard on M002 + soft on M003). No other registered plan is unblocked by this closure.

## 12. Registry updates

- `plans/registry.md`: move `M001 MCP broker transport adapter` from `ready` to `closed` (link this closure); move `M002 external tool discovery and generation` from `sequenced / blocked` to `ready` (blocker `M001 accepted closure` satisfied).
- `plans/subsystems/agent-tool-runtime-reconciliation-roadmap.md`: update header status to `active — M001 closed; M002 ready; M003–M004 sequenced` and milestone table (`M001 closed`, `M002 ready`).
- `plans/implementation/agent-tool-runtime-reconciliation/001-mcp-broker-transport-adapter.md`: status `ready for handoff` → `implemented; closed` with closure link.
- `plans/implementation/agent-tool-runtime-reconciliation/002-external-tool-discovery-and-generation.md`: status `sequenced; blocked until M001 closure` → `ready for handoff` (rebaseline before code change per its §2).
