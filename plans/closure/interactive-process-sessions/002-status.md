# Interactive Process Sessions Milestone 002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/interactive-process-sessions/002-bounded-attach-resume-protocol.md`

Source subsystem roadmap:

- `plans/subsystems/interactive-process-sessions-roadmap.md#M002--bounded-attachresume-protocol-and-authority`

Repository baseline reviewed: `312064eaf54272787860ae42f9d4266f82576a0a`

Implementation commits or pull requests:

- `d8520cc3` — feat(interactive): M002 bounded attach/resume protocol and attachment authority
- This closure commit — `plans: close interactive-process M002, unblock M003 to ready` (closure record, registry/roadmap/plan status updates)

## 1. Executive finding

M002 is complete as a capability: a client can create and reattach to a
live local PTY within bounds, resume output from a sequence cursor or
receive a typed resync, and cannot control another process by supplying
IDs; disconnect, lag, shutdown, and restart cleanly release transient
resources with typed answers. The work is additive only: a versioned
protocol module, ten daemon operations delegating to the M001 engine, a
transport-derived attachment registry, and an explicit authorization
seam. No TUI, no remote transport, no model-tool change. M003 may
proceed — its sole hard dependency (M002) is satisfied.

## 2. Requirement-to-evidence matrix

| Requirement (plan §) | Evidence | Result | Notes |
|---|---|---|---|
| Handle metadata, operation DTOs, chunk/sequence bounds, capability negotiation (§7A) | `crates/codegg-protocol/src/interactive_process.rs`: `INTERACTIVE_PROCESS_CAPABILITY`, v1/`MIN` v1, `negotiate()`, create/metadata/chunk/resync DTOs, `validate()` on every wire field | pass | 9 protocol unit tests |
| Daemon handlers + attachment ownership over M001 only (§7B) | `src/interactive_process_attach.rs::InteractiveProcessProtocol`; `CoreDaemon::run_interactive_request` arms; M001 `spawn/read/snapshot/terminate/remove` are the only engine calls | pass | No second engine; no `shell_session` touch |
| Output cursor/resume/resync + bounded queue/backpressure (§7C) | `resume`/`attach` serve the shared M001 ring on demand; no per-attachment queue or task; `HistoryExpired`/`CursorAhead`/`HandleGone` typed resyncs; 64 KiB default / 256 KiB cap reads | pass | Saturation fixture: 4 KiB ring, ~30 KiB output |
| Connection close/EOF/panic cleanup + multi-client policy (§7D) | `handle_disconnect` drops attachments only; re-attach idempotent per (client, handle); per-client 16 / per-process 16 / daemon 256 caps | pass | Disconnect-survival + two-client fixtures |
| Authorization seam, negative tests, docs (§7E) | `InteractiveAuthority::{local,remote,with_terminate_capability}`; spoofed-ID + remote-terminate-denied fixtures; arch rows + rustdoc | pass | 9 registry/authority unit tests |
| Create/list/attach/detach/input/resize/terminate (§10) | Full lifecycle fixture over real `cat` through the protocol | pass | `attach_resume_full_lifecycle_over_cat` |
| Spoofed owner/process ID (§10) | Foreign attachment IDs answer `interactive_attachment_gone` (same code as unknown); forged handles answer `interactive_handle_gone`; DTOs carry no identity field (asserted structurally) | pass | `spoofed_attachment_ids_match_unknown_ids` |
| Two-client policy (§10) | Independent cursors; A detach leaves B attached; B terminates | pass | `two_clients_share_a_process_with_independent_cursors` |
| Disconnect without accidental kill (§10) | `handle_disconnect` → session still `Running`; re-attach reads earlier output | pass | `disconnect_drops_attachments_without_killing` |
| Explicit kill (§10) | Terminate → `Terminated` + `InteractiveProcessExited` event; post-mortem resume; remove frees handle | pass | Lifecycle + daemon event fixtures |
| Output sequence/lag/resync (§10) | Gap chunk + `HistoryExpired` with base/next cursors; `CursorAhead`; live-cursor incremental resume | pass | `saturation_reports_typed_resync_with_cursors` |
| Queue saturation (§10) | Attachment caps (unit) + oversized chunk/input/size rejected before side effects | pass | `attachment_bounds_are_enforced`, `chunk_and_input_bounds_are_rejected_before_side_effects` |
| Writer failure (§10) | Input/resize after exit → `interactive_not_running`; terminate on exited converges | pass | `writer_failure_after_exit_is_typed` |
| Daemon shutdown/restart gone-handle (§10) | `shutdown()` → `interactive_shutting_down`; fresh instance → old handle `interactive_handle_gone`; surviving attachment of a removed handle → `HandleGone` resync | pass | `shutdown_rejects_new_spawns_with_typed_error`, `restart_invalidates_ephemeral_handles`, `removed_handles_resume_as_typed_gone` |
| Unknown capability compatibility (§10) | Disjoint ranges → `supported:false`; legacy `session_create` JSON decodes; unknown optional fields ignored | pass | `unknown_capability_degrades_without_blocking`, legacy fixture |
| Invalid create never consumes admission (§8) | Empty argv rejected pre-admission; session count unchanged | pass | `invalid_create_never_consumes_admission_or_handles` |
| Remote terminate seam (§6) | Remote transport denied without the semantic capability; granted via `with_terminate_capability` with no wire change | pass | `remote_terminate_requires_the_semantic_capability` |
| Daemon dispatch end-to-end (§6) | `CoreDaemon` arms: capabilities/create/attach/input/resume/terminate/remove + unknown-workspace rejection + cross-client denial + exit event | pass | 3 daemon dispatch fixtures + workspace/caps fixture |

## 3. Production implementation evidence

- **Protocol** (`crates/codegg-protocol/src/interactive_process.rs`, new;
  exported from `lib.rs`): capability id `interactive_process.v1`,
  protocol v1 (`MIN` v1) with intersection negotiation, create request
  validation (argv/env/cwd/id bounds mirroring M001), process metadata
  (no bytes, no secrets), base64 output chunks with `from_seq`/`next_seq`/
  `gap`, `InteractiveResync` with `HistoryExpired`/`CursorAhead`/
  `HandleGone`/`VersionMismatch`, wire-error enum. Ten `CoreRequest`
  variants + eleven `CoreResponse` variants + one `CoreEvent`
  (`InteractiveProcessExited`, bytes-free) in `core.rs`, all additive
  with `#[serde(default)]` on new optional fields.
- **Attachment authority** (`src/interactive_process_attach.rs`, new):
  `InteractiveAttachmentRegistry` (attachment id → handle + owning
  transport `client_id`; idempotent re-attach; indistinguishable
  unknown/foreign errors; `handle_disconnect` releases attachments only);
  `InteractiveAuthority` (local full / remote restricted + pluggable
  terminate capability); `InteractiveProcessProtocol` handler family
  taking `client_id` on every method and resolving handles server-side
  from caller-owned attachments. No background task per attachment; no
  per-attachment queue.
- **M001 additives** (`src/interactive_process.rs`): `InteractiveHandle::parse`
  (shape-checked rebuild; unknown-but-well-formed handles resolve to
  `UnknownHandle` at lookup, never alias) and `snapshot_all` (sorted,
  bounded by caller) for list support. No behavior change to existing paths
  (M001 suite 11/11 still green).
- **Daemon wiring** (`src/core/daemon.rs`): always-present
  `interactive_processes` field built over `JobScheduler::admission()`
  (one process-slot accounting for durable + interactive work);
  `interactive_authority_for` mapping (`ClientRegistry` principal →
  local/remote, never payload-derived); `run_interactive_request`
  dispatching all ten operations on a fresh task (see §10 stack note);
  `interactive_execution_context` resolving registered workspaces only;
  `InteractiveProcessExited` published on terminate. Daemon-wide
  authorization classifies the family `Global` with no semantic
  capability (same shape as projection resume/ack): per-process authority
  is enforced by the attachment seam, terminate/remove additionally
  require the local-owner transport (or the plugged capability).
- **Core additives** (`codegg-core`): `operation_descriptor` arms +
  representative requests for all ten operations (matrix stays in
  lockstep; `operation_matrix_*` tests green); `SafePublicationClass::Safe`
  for the bytes-free exit event.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-protocol interactive_process --no-fail-fast
cargo test -p codegg --lib interactive_process --no-fail-fast
cargo test --test interactive_process_attach_resume --no-fail-fast
cargo test --test interactive_process_sessions --no-fail-fast
cargo test -p codegg-core operation_matrix --no-fail-fast
cargo test -p codegg-core projection_replay --no-fail-fast
RUST_MIN_STACK=16777216 cargo test -p codegg --lib core::daemon --no-fail-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
bash scripts/verify.sh quick
bash scripts/check-core-boundary.sh
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_daemon_cwd_usage.py
(+ sandbox, discovery, catalog, tool-broker, tui-authority, identity-path,
git-pattern, websocket, PWD-inference, projection ×4, provider ×2 guards,
`generate_builtin_agents.py --check`)
cargo test --release --test interactive_process_attach_resume interactive_daemon_worker_stack_probe
cargo test --release --test interactive_process_attach_resume daemon_attach_input_resume
```

Substitution note: the plan's `cargo test --workspace <name>` lines use
crate names that do not exist in this workspace (`interactive_process`,
`transport` are modules of the root crate, not workspace members). The
commands above are the narrowest equivalents. All are local-execution
truth (no CI run claimed).

### Results

| Command | Result |
|---|---|
| `cargo test -p codegg-protocol interactive_process` | pass — 9/9 |
| `cargo test -p codegg --lib interactive_process` | pass — 22/22 (9 attach + 13 M001 unit) |
| `cargo test --test interactive_process_attach_resume` | pass — 19/19 (real PTY fixtures, ~5 s) |
| `cargo test --test interactive_process_sessions` (M001 regression) | pass — 11/11 (~24 s) |
| `cargo test -p codegg-core operation_matrix` | pass |
| `cargo test -p codegg-core projection_replay` | pass — 55/55 |
| `RUST_MIN_STACK=16777216 cargo test -p codegg --lib core::daemon` | pass — 31/31 (see §10 stack note) |
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass (3 initial findings fixed: redundant field init, boxed large `Err`, MSRV `is_none_or`; none suppressed) |
| `scripts/verify.sh quick` | pass (fmt, agent assets, core boundary, sandbox, execution ownership, full `--all-targets` check) |
| All static guards listed above | pass |
| release `interactive_daemon_worker_stack_probe` (default 2 MiB workers) | pass |
| release `daemon_attach_input_resume_and_cross_client_denial` on a temporary 2 MiB worker runtime | pass (production-faithful stack check; runtime size reverted to the 16 MiB debug-test default afterwards) |

## 5. Invariant review

| Plan §4 invariant | Evidence it remains true |
|---|---|
| Request payload cannot claim ownership/capabilities | DTOs have no `client_id`/`principal`/`role`/`capability` fields (structural test); daemon passes `trusted_client_id` alongside every call; `ClientRegistry` principal is handshake-bound and immutable |
| Output/input queues bounded | M001 ring + 64 KiB default / 256 KiB cap reads + 32 KiB input cap, all enforced before side effects; attachment counts 16/16/256 |
| One process engine owner | Only M001 `InteractiveProcessService` spawns; attach layer delegates; manifest classifies the new file `interactive` with the delegation reason |
| Attach is not human Session attachment | Handles are UUIDs; `ClientRegistry::attach_session` untouched; arch section states the separation |
| Disconnect does not kill unless explicit policy | `handle_disconnect` touches only the registry; survival fixture asserts `Running` + re-attach reads |
| Terminate requires authority | Attachment ownership + `can_terminate` (local transport or plugged capability); negative fixtures for foreign IDs and remote transport |
| Secret environment never returned | Env values never logged (stripped summaries only); snapshots/metadata carry executable name only; chunks carry PTY bytes the client produced/observed |
| Lag/resync is typed | `gap` flag + `InteractiveResync` with reason and both cursors; never silent shifting (saturation fixture) |

## 6. Failure and recovery review

- **Spawn failure**: engine error → secret-free `interactive_upstream`/`interactive_invalid_request`; no handle minted; permit released inside M001; invalid DTOs rejected before admission (slot counts asserted).
- **Attach failure**: unknown/malformed handle → `interactive_handle_gone` with no attachment minted and no side effect.
- **Input/resize failure**: unknown/foreign attachment → `interactive_attachment_gone`; oversized/non-base64 input and bad sizes rejected before any PTY write; post-exit writes → `interactive_not_running`.
- **Resume failure modes**: pre-retention → `HistoryExpired` (+ snapshot); ahead → `CursorAhead`; removed/restarted handle → `HandleGone` (no snapshot). All typed, all without shifting history.
- **Disconnect races**: close drops attachments; in-flight resume either served from the ring or answers `HandleGone`; re-attach is idempotent.
- **Daemon restart**: handles and attachments are memory-only; a fresh instance answers old handles with the typed gone response (restart fixture); daemon generation is not conflated with durable job generations.
- **Shutdown**: `shutdown()` terminates sessions boundedly and rejects new spawns with `interactive_shutting_down`.
- **Daemon dispatch isolation**: the interactive family runs on a fresh task (`tokio::spawn` + await at the dispatch boundary) so PTY handler frames never nest inside the near-limit dispatch match (see §10).

## 7. Migration and compatibility review

- Additive only: one new protocol module, ten request + eleven response + one event variant (all with defaults on new optionals), one new root module, two small M001 additives, daemon field + arms, authorization/matrix rows, one safe-publication arm, manifest entry, three doc sections.
- No schema migration, no config change, no Job/store/run format change. `terminal` tool and `shell_session` metadata untouched. Legacy `session_create` JSON still decodes; unknown capabilities degrade to `supported:false`.
- Rollback: delete the protocol module + variants, the attach module + daemon arms/field, and the manifest/docs rows; M001 stands alone as before.

## 8. Security review

- **Authorization**: every mutating operation resolves the handle server-side from a caller-owned attachment; terminate/remove additionally gate on transport-derived authority. The daemon-wide gate admits authenticated callers; the attachment seam fails closed per process. Team-role project scoping is the documented future plug through the same authority context (no wire change); until then remote principals observe/attach/drive only processes they attached to and cannot terminate.
- **Spoofing/probing**: unknown and foreign attachment IDs share one code; forged handles are typed gone; error strings never echo handles, owners, paths, or env.
- **Denial-of-service bounds**: attachment caps, chunk/input/size caps, shared-ring reads (no per-attachment queues/tasks), idempotent re-attach (no attachment fork bombs), base64 validated before allocation beyond the cap.
- **Transport trust**: `principal_for` bindings are immutable per connection (existing registry test); unregistered transports resolve to local (existing personal-local model), never to a payload claim.

## 9. Documentation and operations

- `docs/execution-ownership.toml`: new `interactive` site entry for `src/interactive_process_attach.rs` (guard passes).
- `architecture/process-tool-execution-ownership.md`: adapter-table row + "Attach ownership and resync (M002)" section.
- `architecture/scheduler.md`: shared-admission-controller paragraph under ephemeral interactive admission.
- `architecture/core.md`: module-table row.
- Module rustdoc: ownership/authority/bounds diagrams in both new modules.
- Operator diagnostics: metadata snapshots (no secrets), per-client/per-process/daemon attachment counts, `last_from_seq` per attachment, typed wire codes in §6.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Debug-build dispatch stack depth: `handle_request_with_client` is documented near its limit, and M002's daemon flows need ~3 MiB in unoptimized test threads (2 MiB default overflows). Pre-existing: baseline daemon tests (`project_catalog_protocol_lists…`, `durable_schedule_tui_…`) overflow identically on the clean tree in this environment. | Test-harness only in the evidence gathered: daemon tests run on the repo-precedent 16 MiB runtime (`projection_transport_real.rs` pattern); production-faithful release checks pass on default 2 MiB workers (capabilities/list probe + full attach/input/terminate flow). | No code change. If a future milestone adds heavy dispatch arms, prefer the spawned-task boundary used here and keep the 16 MiB test runtime. Re-check if MSRV/debug frame budgets change materially. |
| low | Linux-host PTY fixture evidence not executed locally (this host is macOS arm64), same as M001 §10. | Small: `openpty`/line-discipline behavior on Linux unexercised until CI runs the new suites there. | CI `verify` on Linux runs `interactive_process_attach_resume` + `interactive_process_sessions`; no code change expected. Not a closure blocker: the Unix implementation is shared and macOS-evidence is complete. |
| — | No other findings. | — | — |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone M002 closed; M003 (TUI terminal integration and legacy
disposition) is unblocked — its sole hard dependency (M002) is
satisfied — and moves to ready. No subsystem roadmap revision required
beyond the milestone status table.

## 12. Registry updates

- `plans/registry.md`: remove the M002 row from dependency-ready plans;
  add M003 as dependency-ready; remove the M003 row from blocked work;
  advance the subsystem row to "M003 ready"; update execution-order item
  5 (M002 closed, M003 may proceed).
- `plans/subsystems/interactive-process-sessions-roadmap.md`: M002 →
  closed with closure link; M003 blocked → ready.
- `plans/implementation/interactive-process-sessions/002-bounded-attach-resume-protocol.md`:
  status → implemented.
- `plans/implementation/interactive-process-sessions/003-tui-terminal-integration-and-legacy-disposition.md`:
  status → ready for handoff.
