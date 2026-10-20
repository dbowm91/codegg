# TUI Live Work Visibility Corrective M003 — Closure Status

Status: **closed**
Date: 2026-10-19
Class: **invariant**
Source implementation plan: `plans/implementation/tui-live-work-visibility-corrective/003-session-owned-subagent-lifecycle.md`
Source subsystem roadmap: `plans/subsystems/tui-live-work-visibility-corrective-roadmap.md#7-milestones`
ADR gate: `plans/adrs/ADR-0043-session-owned-subagent-lifecycle-projection.md` — Option A explicitly accepted 2026-10-19
Implementation commit: `a735ffed21fce498a26b1f50aa7085456760ce9d`

## Finding

Subagent lifecycle identity now belongs to a bounded session projection as well
as the compatible per-turn tree. A dispatch has a stable `child_instance_id`,
immutable session and optional originating-turn identity, and optional durable
run/job links. Worker start, progress, completion, and failure preserve that
identity through the event, protocol, reducer, and snapshot path. The projection
can therefore retain a child after its parent turn exits without joining a
numeric task id to a durable UUID.

## Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Trusted identity is minted once and reaches every lifecycle event | `SubAgentWorker` admission and worker lifecycle tests; event adapter/DTO mapping tests | pass | Empty legacy requests receive one UUID before enqueue; producer paths preserve supplied identity. |
| Session child state reduces independently of an active turn | `subagent` reducer tests in `codegg-protocol`; session runtime snapshot test | pass | Idle-session start, progress, failed/completed terminal, same numeric task id across generations, and parent-turn replacement are covered. |
| Duplicate, delayed, and reordered events do not misaddress or revive a child | `child_generation_survives_turn_replacement_and_replay_is_deterministic`; reducer duplicate/terminal tests | pass | New events use exact generation identity; old-wire events remain turn-local. Terminal state is monotonic. |
| Projection size is bounded and terminal identity is retained safely | bounded session-subagent reducer tests and `MAX_PROJECTION_SESSION_SUBAGENTS` | pass | Terminal-first eviction and overflow accounting keep the snapshot bounded. |
| Replay and old-wire decoding remain compatible | protocol projection serde/replay tests | pass | Missing generation fields decode to the legacy value; deterministic replay is asserted. |
| Cold process state does not resurrect ephemeral live children | `session_projection_snapshot_includes_in_process_live_child_instances` | pass | The runtime registry is empty on cold start; live state appears only after current-process lifecycle observation. Durable job/run reconstruction remains store-owned. |
| Cross-session isolation and UI transport authority remain intact | focused projection transport isolation/lifecycle and TUI transport guards | pass | Child records are filtered by canonical session; no new write or direct-child steering authority was added. |

## Verification executed

| Command | Result |
|---|---|
| `rtk cargo test -p codegg-protocol --lib projection --locked` | pass (98) |
| `rtk cargo test -p codegg --lib subagent --locked` | pass (120) |
| `rtk cargo test -p codegg --lib session_projection_snapshot_includes_in_process_live_child_instances --locked` | pass (recorded with focused M003 run) |
| `rtk python3 scripts/check_projection_transport_isolation.py` | pass |
| `rtk python3 scripts/check_projection_transport_lifecycle.py` | pass |
| `rtk python3 scripts/check_tui_transport_authority.py` | pass |
| `rtk python3 scripts/check_scheduler_bypass.py` | pass |
| `rtk python3 scripts/check_execution_ownership.py` | pass |
| `rtk bash scripts/verify.sh quick` | pass (exact implementation tree) |
| `CARGO_BUILD_JOBS=2 rtk cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |

Local results only; no hosted CI or manual terminal receipt is claimed.

## Residuals and disposition

No durable child-instance table was added: ephemeral children are process-owned
and intentionally absent after daemon restart, while durable AgentRuns and Jobs
are recovered from their existing stores. A combined real worker-to-socket-to-
painted-TUI scenario, cancellation/restart operator trace, and manual terminal
receipt remain M006 acceptance work. Those limits do not invalidate M003's
identity and reducer contract; M006 records the remaining presentation proof
as conditional rather than claiming it passed.

## Unblock audit

M003 is closed after ADR-0043 Option A acceptance. This unblocked M004, whose
hard prerequisites M001 and M002 were already closed. M005 remained dependent
on M004; M006 remained sequenced behind M001–M005 and its real-terminal gate.
