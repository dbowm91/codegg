# Desktop Frontend and IDE Foundation Milestone 018 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation/018-the-daemon-s-published-projection-surfaces.md`

Source subsystem roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`

Source closure of the predecessor:

- `plans/closure/desktop-frontend-ide-foundation/017-status.md`

Applicable ADR:

- **ADR-0053** — `plans/adrs/ADR-0053-daemon-published-projection-surfaces.md`,
  accepted 2026-10-25 on the owner's direction. The daemon publishes the
  session's **selected model**, its **shell command runs** and its **tool
  contracts**, and it publishes the **real** session summary on a session
  update.

Repository baseline reviewed: `02030facc` — `origin/main` with M007-D and
its corrective pass (plan 017) landed, and the owner's decisions on plan 017
recorded.

Implementation commits on this milestone's branch:

- `4f387f7e0` — ADR-0053, plan 018, the **M007-J** registration, and the
  roadmap's M007 decomposition row.
- `2ed792570` — the protocol foundation: the two projection DTOs, the two
  projection events with their reducer arms and bounds, the serde-defaulted
  session summary on `CoreEvent::SessionUpdated`, and the daemon's event
  vocabulary for the model's selection, the agent's selection, the shell
  run's lifecycle and the session's tool contracts.

## 1. Executive finding

The daemon now publishes the three surfaces ADR-0053 named, and it publishes
the **real** session summary. The desktop performs **five** of the six
declared frontend actions instead of three; `show_terminal` remains
uncarried with the desktop's own honest notice, and **M007-J** is
registered as its own milestone.

Three things changed underneath, and they are the substance of the
milestone:

1. **The daemon's registry became the authority for the run list.** The
   human-shell service gained `list_for_session`
   (`src/shell/daemon.rs:458`), and the TUI's `/shell-list` reads the
   daemon's published list (`src/tui/commands/shell.rs:1221`) instead of the
   TUI's local mirror. The local store keeps the **output bytes** it
   renders; it is no longer a second copy of the *list*.
2. **The session's own registry became the authority for the contracts.**
   The turn runtime records the contract summary of the registry the turn
   actually runs against, after the plugin tools are registered
   (`src/agent/turn_runtime.rs:678`), and the daemon publishes it
   (`src/agent/turn_runtime.rs:683`). The daemon never builds a second
   registry, so no second LSP connection can open.
3. **The stub summary is withdrawn.** A session update now carries the
   session's own title, status, selected model, selected agent and token
   counts (`src/core/session_summary.rs:16`), the daemon fills it at its
   event bridge (`src/core/daemon_bootstrap.rs:170`), and an update that
   carries no summary publishes **nothing** rather than replacing the
   projection's primary session with an empty one
   (`crates/codegg-core/src/projection_replay/publication.rs:399`).

The mechanism is the one plan 018 chose inside ADR-0053's latitude: all
three surfaces ride the **canonical session projection**. The desktop's
action dispatch is synchronous and holds only what the renderer already has,
so a surface that arrived by a separate daemon read would have had to be
fetched before the action ran — a second mechanism, and an action reporting
a value the renderer did not itself receive.

## 2. Requirement-to-evidence matrix

| # | Plan 018 §7 criterion | Evidence |
|---|---|---|
| 1 | The daemon's projection snapshot carries the session's **selected model** and **selected agent**, taken from the session's own runtime | `src/core/session_summary.rs:16` builds the summary from `SessionRuntime`; `src/core/daemon_refresh.rs:89` writes it onto `snapshot.primary_session` |
| 2 | `ProjectionEvent::ModelSelected` and `ProjectionEvent::AgentSelected` have producers, and a change of selection is visible without a resync | `src/core/daemon_sessions.rs:187` publishes `CoreEvent::ModelSelected`; `src/core/daemon_turns.rs:1673` publishes `CoreEvent::AgentSelected`; the publication maps them at `crates/codegg-core/src/projection_replay/publication.rs:418`; the reducer arms are at `crates/codegg-protocol/src/projection/reducer.rs:1172` |
| 3 | A session update publishes the session's **real** summary, and a subscriber keeps its title, status, model and token counts across an unrelated update | `crates/codegg-core/src/projection_replay/publication.rs:399` publishes the carried summary and publishes nothing when none is carried; `crates/codegg-protocol/src/projection/reducer.rs:347` still replaces `primary_session`, but now with a real summary |
| 4 | The snapshot carries the session's **shell command runs**, the list is the daemon's own registry, and the publication carries the listing and not the output bytes | `src/shell/daemon.rs:458` reads the daemon's own registry; `src/shell/daemon.rs:536` builds the projection's view and carries no chunks; `crates/codegg-protocol/src/projection/snapshot.rs:66` and `:277` hold the bounded collection |
| 5 | The TUI's `/shell-list` answers from the daemon's published list | `src/tui/commands/shell.rs:1221` reads `active_snapshot()`; the rows are built at `src/tui/commands/shell.rs:1266`; the `[promoted]` marker is this frontend's own annotation, overlaid by run id at `src/tui/commands/shell.rs:1251` |
| 6 | The snapshot carries the session's **tool contracts**, taken from the session's own registry, and a plugin-contributed tool is named when it is really registered | `src/agent/turn_runtime.rs:302` derives the summary from the registry; `src/agent/turn_runtime.rs:678` records it **after** the plugin tools are registered at `src/agent/turn_runtime.rs:668` |
| 7 | The TUI's `/tool-contracts` answers from the published contracts, with its rendering unchanged | `src/tui/commands/diagnostics.rs:350` reads `active_snapshot()`; the four rendered fields and the three caller-policy spellings are the daemon's own |
| 8 | The desktop's `show_tool_contracts` and `show_shell_runs` are **performed**, and `show_status` names the model | `apps/desktop/src/commands/frontendActions.ts:136` and `:159` render the two surfaces; `apps/desktop/src/commands/frontendActions.ts:105` names the model; the presenter maps the three at `apps/desktop/src-tauri/src/present.rs:521` |
| 9 | The desktop's `UNCARRIED_ACTIONS` holds **one** entry, `show_terminal`, with the desktop's own reason | `apps/desktop/src/commands/frontendActions.ts:67` |
| 10 | The bridge's field list matches on both sides, and the bindings guard is green | `apps/desktop/src-tauri/src/bridge.rs:821` and `apps/desktop/src/bridge-types.ts:305`; `apps/desktop/scripts/check-bindings.mjs:55` pins the field list |
| 11 | The comment budget is green with no manifest edit | `python3 scripts/check_comment_budget.py` → `comment budget guard passed`; `docs/comment-budget.toml` is unchanged |
| 12 | `scripts/verify.sh quick` is green | recorded in §4 |

## 3. What was delivered

### 3.1 The protocol foundation (`2ed792570`)

- Two bounded projection DTOs: `ShellRunProjection`
  (`crates/codegg-protocol/src/projection/dto.rs:1024`) and
  `ToolContractProjection`
  (`crates/codegg-protocol/src/projection/dto.rs:1052`).
- Two projection events with their reducer arms: `ShellRunUpserted`
  (`crates/codegg-protocol/src/projection/event.rs:386`,
  `crates/codegg-protocol/src/projection/reducer.rs:1184`) and
  `ToolContractsUpdated`
  (`crates/codegg-protocol/src/projection/event.rs:391`,
  `crates/codegg-protocol/src/projection/reducer.rs:1188`).
- Two bounded collections on the snapshot
  (`crates/codegg-protocol/src/projection/snapshot.rs:66`, `:69`) with their
  helpers (`:277`, `:290`) and their bounds
  (`crates/codegg-protocol/src/projection/limits.rs:54`, `:57`).
- `CoreEvent::SessionUpdated` gains a serde-defaulted summary field
  (`crates/codegg-protocol/src/core.rs:4694`), and the daemon's event
  vocabulary gains `ModelSelected` (`:4710`), `AgentSelected`,
  `HumanShellRunUpserted` (`:4720`) and `ToolContractsUpdated` (`:4725`).
- `HumanShellRunMetadataDto` gains `started_at_ms` and `completed_at_ms`
  (`crates/codegg-protocol/src/human_shell.rs:63`).

Every added field is serde-defaulted and every added collection is bounded,
so a client that predates the change decodes it as absent. The projection
protocol version is **not** bumped
(`crates/codegg-protocol/src/projection/caps.rs:22`).

### 3.2 The real session summary

- `src/core/session_summary.rs:16` — the summary builder. The title and
  the timestamps come from the durable record; the status, the selection and
  the counters come from the runtime.
- `src/core/daemon_bootstrap.rs:170` — the daemon's event bridge fills the
  summary on every `SessionUpdated` it forwards.
- `src/core/daemon_refresh.rs:89` — the snapshot builder writes the same
  summary onto `primary_session`, so a connect and a resync agree with the
  event path.
- `crates/codegg-core/src/projection_replay/publication.rs:399` — the stub
  summary is withdrawn. An update that carries no summary publishes
  **nothing**, so the projection keeps the summary it already holds.

### 3.3 The daemon owns the session's shell command runs

- `src/shell/daemon.rs:458` — `list_for_session` reads the daemon's own
  registry, keeps the runs the session owns, and returns the metadata
  only. The output chunks stay behind the daemon's own read.
- `src/shell/daemon.rs:536` — the projection's view of one run. The status
  and origin strings are the enums' serde `snake_case` names, so they are
  the same strings the TUI already spells.
- `src/shell/daemon.rs:93` — the lifecycle publisher. It is installed with
  `human_shell_run_publisher` (`src/core/daemon_human_shell.rs:367`) and
  wired at `src/core/daemon_construct.rs:913` and `:987`. A panicking publisher cannot
  fail a run.
- The run record now carries a **completion timestamp**
  (`crates/codegg-protocol/src/human_shell.rs:63`), stamped when the run
  reaches a terminal status, so a listing can show how long a run took.

### 3.4 The daemon publishes the session's tool contracts

- `src/core/session_tool_contracts.rs:16` — the daemon-owned,
  session-keyed **contract summary**. It holds a summary, never a registry
  and never a tool instance.
- `src/agent/turn_runtime.rs:302` — the summary is derived from the
  registry the turn actually runs against, through the broker's own
  catalogue.
- `src/agent/turn_runtime.rs:678` — the summary is recorded and published
  **after** the plugin tools are registered, so a plugin-contributed tool is
  named when it is really available.
- `src/core/daemon_refresh.rs:118` — the daemon publishes the session's
  contracts on the snapshot.

### 3.5 The frontends read the same daemon-owned state

- The TUI's `/shell-list` (`src/tui/commands/shell.rs:1221`) and
  `/tool-contracts` (`src/tui/commands/diagnostics.rs:350`) read the
  projection. Neither builds a local registry or keeps a second copy of the
  list.
- The desktop's presenter maps the three surfaces
  (`apps/desktop/src-tauri/src/present.rs:521`), the bridge carries them
  (`apps/desktop/src-tauri/src/bridge.rs:821`), and the action module
  renders them (`apps/desktop/src/commands/frontendActions.ts:136`, `:159`,
  `:105`).

## 4. Verification executed

Every command below was run in this worktree on this branch, and the
results are quoted as observed.

### 4.1 Compile and format

| Command | Result |
|---|---|
| `cargo ck` | clean, no errors |
| `cargo fmt --check` | clean |
| `rustup run 1.99.0 cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0, no warnings |
| `rustup run 1.90.0 cargo check --all-targets` (in `apps/desktop/src-tauri`) | clean |
| `rustup run 1.90.0 cargo clippy --all-targets -- -D warnings` (in `apps/desktop/src-tauri`) | clean, no warnings |

**The root clippy must run at CI's toolchain, not at the MSRV's.** CI's
`verify` lane installs `dtolnay/rust-toolchain@stable`, which was **1.99.0**
when this milestone's lanes ran; the repository's MSRV is **1.89** and the
local default toolchain is 1.89. `needless_borrows_for_generic_args` fires
on 1.99 and not on 1.89, so the MSRV toolchain **accepts** code CI
**rejects** — `scripts/verify.sh quick` does not run clippy at all, and
`scripts/verify.sh full` runs it at whatever the default toolchain is
(`scripts/verify.sh:603`). The
first run of this milestone's `verify` lane failed on exactly that lint
(`src/shell/daemon.rs:1187`, `serde_json::to_value(&origin)` against a
`Copy` `HumanShellOriginDto`); the fix is one word, and the branch is
clippy-clean at **1.99.0** over `--workspace --all-targets --locked`. A
milestone that wants its `verify` lane to pass on the first attempt should
run the root clippy at 1.99.0 itself, and treat the local 1.89 clippy as
the MSRV floor rather than as the gate.

### 4.2 The scoped test runs

| Command | Result |
|---|---|
| `cargo test -p codegg-protocol -p codegg-core` | every target `ok`, **0 failed** |
| `cargo test --test core_daemon --test projection_replay --test session_family --test frontend_parity --locked` | `510 passed`, `6 passed`, `63 passed`, `97 passed`; **0 failed** |
| `cargo test -p codegg --lib shell::daemon::tests` | `10 passed; 0 failed` |
| `cargo test -p codegg --lib -- tui::` | `2789 passed; 0 failed` |
| `cargo test -p codegg --lib -- tui::shell_dispatch_tests` | `31 passed; 0 failed` |
| `cargo test -p codegg --lib core::session_tool_contracts` | `4 passed; 0 failed` |
| `npm test` (in `apps/desktop`) | `31 passed` test files, `241 passed` tests |
| `npm run typecheck` (in `apps/desktop`) | clean |
| `rustup run 1.90.0 cargo test` (in `apps/desktop/src-tauri`) | `92 passed; 0 failed; 1 ignored`, plus `14 passed` and `5 passed` |

### 4.3 The guards

| Guard | Result |
|---|---|
| `python3 scripts/check_comment_budget.py` | `comment budget guard passed` |
| `python3 scripts/check_test_assertion_quality.py` | `all checks pass` |
| `python3 scripts/check_test_duplication.py` | `all checks pass` |
| `python3 scripts/check_adr_integrity.py` | `ADR integrity fixtures passed; active ADR IDs are unique and local paths resolve` |
| `python3 scripts/check_frontend_action_set.py` | `frontend-action set: clean` |
| `python3 scripts/check_command_catalog_ownership.py` | `command catalog ownership: clean` |
| `bash scripts/check-desktop-boundary.sh` | `desktop dependency and authority boundaries verified` |
| `node apps/desktop/scripts/check-bindings.mjs` (from `apps/desktop`) | `desktop bridge DTO fields match` |
| `python3 scripts/check_daemon_cwd_usage.py` | `cwd usage check passed` |
| `bash scripts/check-core-boundary.sh` | `codegg-core boundary check passed` |
| `python3 scripts/check_tool_broker_boundary.py` | `OK: No direct tool execution calls outside the broker boundary` |
| `python3 scripts/check_writer_disposition.py` | `writer-disposition check ok` |
| `scripts/verify.sh quick` | recorded in §4.4 |

### 4.4 The canonical sweep

`scripts/verify.sh quick` — rustfmt, the two generated-asset checks, the
71 guard scripts and the workspace check — is green on this branch. The
run ended with:

```
==> CARGO_BUILD_JOBS=2 cargo check --workspace --all-targets --locked
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 29.48s
==> Quick verification passed.
```

and the harness lab's own suite ran `Ran 329 tests ... OK (skipped=1)`.
The full log is not reproduced here.

### 4.5 The desktop's own gate

The desktop is a separate Cargo workspace and the root check does not
compile it. The desktop's lane is therefore its own `clippy`, its own
`cargo test`, its own TypeScript build and its own bindings guard, all of
which are recorded in §4.1, §4.2 and §4.3.

### 4.6 Hosted

The lanes that qualify this milestone are `verify`, `e2e`,
`restricted-userns-fails-closed` and `process-ipc`; the whole-workspace
suite runs in `verify`.

The hosted runs this milestone has produced, in order:

| Run | Head | Workflow | Conclusion |
|---|---|---|---|
| `40219661610` | `9e9404a2e` | CI / verify | **failure** — the **Workspace Clippy** step rejected `src/shell/daemon.rs:1187` under clippy 1.99; every step before it was green |
| `40219661652` | `9e9404a2e` | Desktop E2E | **success** — 18 green steps, the built-app WebDriver trajectory |
| `40219658631` | `9e9404a2e` | Linux M004 process and IPC qualification | **success** — `process-ipc` and `restricted-userns-fails-closed` both green |
| `40221224433` | `006c27c03` | CI / verify | **success** — 135 green steps and one skipped (the Eggwork live fixture): the guard steps, formatting, workspace clippy and the workspace test sweep |
| `40221224520` | `006c27c03` | Desktop E2E | **success** — 18 green steps |
| `40221224456` | `006c27c03` | Linux M004 process and IPC qualification | **success** — `process-ipc` (18 green steps) and `restricted-userns-fails-closed` (13 green steps) both green |

`9e9404a2e` is this milestone's merge head and `006c27c03` is its clippy
corrective; the two differ by **one word in one test**
(`src/shell/daemon.rs:1187`), so the qualification of `006c27c03` is the
qualification of the milestone's implementation.

The one failure is recorded because it is the evidence for the toolchain
note in §4.1. The lint fires on clippy 1.99 and not on the MSRV's 1.89, so
the local `scripts/verify.sh quick` and `scripts/verify.sh full` runs were
**green on a tree CI rejected** — the defect was reachable only by running
the root clippy at CI's toolchain.

This closure's own head's hosted run is recorded on its own pull-request
head, in the same way closure 017 §4.2 records its own: the run ids belong
to the pull request, not to this document, and naming them here would name
a head this commit is not.

## 5. Invariant review

Five invariants held, and each is checked by a named test rather than by
inspection.

1. **The daemon's registry is the only authority for the run list.** The
   TUI's local store keeps the **output bytes**; it is not consulted for
   the list. `list_for_session_returns_the_session_runs_newest_first_without_output`
   asserts that the listed metadata carries no output bytes, and that
   another session's run is not in the list.
2. **The publication carries a listing, not the bytes.** The same test
   asserts the listed metadata's serialised form carries no bytes or
   chunks key.
3. **A panicking publisher cannot fail a run.**
   `a_panicking_publisher_cannot_fail_a_run` asserts the run still reaches
   `Exited` and still appears in the listing.
4. **The contracts come from the session's own registry.**
   `the_recorded_summary_names_the_registrys_own_tools` asserts that a
   probe tool in the session's registry is named in the recorded summary,
   with the caller policy the TUI renders.
5. **An absent record is not an empty session.**
   `an_unrecorded_session_has_no_contracts` asserts that a session with no
   recorded contracts returns an empty list rather than a fabricated
   default.

Two further invariants are structural rather than behavioural:

- **No second registry.** The daemon never builds a `ToolRegistry` to
  publish from. The summary is derived from the registry the turn runs
  against (`src/agent/turn_runtime.rs:302`), so no duplicate plugin
  adapter and no second LSP connection can exist.
- **No second list.** The projection's `shell_runs` collection is filled
  from the daemon's registry (`src/core/daemon_refresh.rs:115`) and from
  the daemon's own lifecycle events. There is no other copy.

## 6. Failure, cancellation, restart, and contention semantics

- **A run that never starts.** The service's `FailedToStart` transition
  stamps the completion time and publishes the terminal status, so the run
  appears in the listing as `failed` rather than as a run that never ended.
- **A run whose handle is lost.** The service's terminal `Lost` transition
  does the same (`src/shell/daemon.rs:366`).
- **A publisher that panics.** Contained by `catch_unwind` and logged; the
  run's lifecycle does not depend on it.
- **A run outliving the daemon.** The registry is ephemeral by design: runs
  and handles disappear on restart and no command is replayed. The
  projection's `shell_runs` collection is therefore empty after a restart
  until the next run, which is honest — the daemon no longer owns those
  runs.
- **A session with no runtime.** `session_summary` yields a summary from
  the durable record alone. An absent record is not an empty session.
- **A summary that cannot be assembled.** The daemon's event bridge fills
  the summary only when the runtime and the record are reachable; when the
  record read fails the bridge logs and publishes an update with no
  summary, and the publication path then publishes **nothing**. The
  projection keeps what it holds. This is the failure mode the stub
  summary used to cause, inverted.
- **Contention on the contract store.** The store is an
  `Arc<parking_lot::RwLock<HashMap<…>>>`. A write replaces the session's
  list; a read returns a clone. A turn that writes concurrently with a
  snapshot read sees one list or the other, never a partial one.
- **Contention on the run registry.** The service's own
  `tokio::sync::Mutex<RegistryInner>` serialises the registry, and the
  listing is taken under that lock.

## 7. Migration and compatibility review

- **No new `CoreRequest` variant.** The core protocol is unchanged in
  shape.
- **No protocol version bump.** The projection protocol version stays at
  `1` (`crates/codegg-protocol/src/projection/caps.rs:22`); the new fields
  are additive and serde-defaulted.
- **Old clients.** A client that predates the change decodes the new
  fields as absent. `durable_run_projection_replays_and_old_snapshots_default_new_fields`
  covers the equivalent for the snapshot's defaulted collections.
- **Old daemons.** A newer frontend reading an older daemon's projection
  sees empty `shell_runs` and `tool_contracts` and a `None` model, and says
  so. The desktop's `show_tool_contracts` and `show_shell_runs` report
  "none published" rather than inventing a list.
- **The stored data.** No migration. The new timestamps are runtime facts
  on an ephemeral registry; the contract summary is a runtime fact held in
  memory; the session summary is assembled, not stored.
- **The command catalog.** Untouched. No form, row or effect classification
  moved, so no asserted count in `architecture/command.md` moves.

## 8. Security review

- **The publication carries no secret.** A tool contract carries a name, a
  caller policy, an effect class, an idempotency class and an
  implementation id. It does **not** carry the contract's input or output
  schema, and it carries no credential. The schemas were left out
  deliberately: the projection publishes the policy a frontend renders, not
  the tool's input grammar, and the projection is bounded by lines rather
  than by bytes.
- **The run listing carries no bytes.** The run's output stays behind the
  daemon's own bounded read (`src/shell/daemon.rs:389`), which the
  session's scope authorises. The listing carries the command, the status,
  the origin and the cwd — all of which the daemon already held.
- **The session summary carries nothing new.** Its fields are the session's
  title, its status, its selection, its counters and its timestamps, all of
  which the daemon already published on `CoreResponse::SnapshotSession`.
- **The safe-publication gate classifies the new events.**
  `CoreEvent::ModelSelected`, `AgentSelected`, `HumanShellRunUpserted` and
  `ToolContractsUpdated` are all classified `Safe`
  (`crates/codegg-core/src/projection_replay/safe_publication.rs`), and each
  is required to carry a non-empty session id before it is projected. A
  run with no session is announced with an empty session id and is
  therefore dropped by the gate rather than globalised.
- **The registry's scope.** `list_for_session` returns only the runs the
  named session owns. A session cannot list another session's runs, and a
  run with no session is in no session's list.

## 9. Documentation and operations

- **ADR-0053** — `plans/adrs/ADR-0053-daemon-published-projection-surfaces.md`,
  accepted 2026-10-25.
- **Plan 018** — `plans/implementation/desktop-frontend-ide-foundation/018-the-daemon-s-published-projection-surfaces.md`.
- **This closure** — `plans/closure/desktop-frontend-ide-foundation/018-status.md`.
- **The registry and the roadmap** — §11 and §12.
- **The temporal anchoring.** ADR-0053's Context and plan 018's line
  anchors describe the state as of **2026-10-25**, before the
  implementation. Both say so in place. This closure carries the current
  anchors.
- **The comment budget.** The corpus ceiling and every per-file ceiling
  were **not** edited. Every new comment was paid for by compressing
  pre-existing prose in the same file; several files ended with fewer
  comment lines than they began with.

## 10. Unresolved findings

Two findings stay unresolved. Neither is a defect in the delivered work;
both are recorded with their reason.

### 10.1 The desktop's terminal surface is not carried

`show_terminal` remains in the desktop's `UNCARRIED_ACTIONS`
(`apps/desktop/src/commands/frontendActions.ts:67`) with the desktop's own
reason. The daemon already serves `CoreRequest::InteractiveProcessList`
(`src/core/daemon.rs:4779`) and the metadata carries a handle, a state and
the PTY geometry, so **no ADR and no daemon work** is needed — the gap is
that the desktop has no terminal surface at all. The owner's direction of
2026-10-25 made that its own milestone, **M007-J**, which is registered in
the roadmap's M007 decomposition and in the registry.

### 10.2 The TUI's promotion marker is a frontend-local annotation

The TUI's `/shell-list` has always marked a run whose output it promoted
into its own conversation. The daemon's published list does not carry that
marker, and the desktop's `show_shell_runs` does not show one either — so
the two frontends agree about the run list.

The marker is deliberately **not** published. It records that *this*
frontend promoted that output into *this* frontend's conversation; putting
it in the daemon's projection would make one shared list carry a
per-frontend annotation, which is the "one list, two annotations" defect
this milestone closes.

The TUI keeps the marker by overlaying its own annotation onto the
daemon's rows, matched by the daemon's run id
(`src/tui/commands/shell.rs:1251`). To make the join faithful the daemon
binding is now kept alive past a run's terminal event
(`src/tui/commands/shell.rs:297`) — a run is promoted *after* it exits — and
`/shell-kill` was guarded so that it still cancels only a **live** run
(`src/tui/commands/shell.rs:1159`). A row whose local entry the store has
evicted carries no marker, rather than a guessed one
(`src/tui/commands/shell.rs:1813`).

### 10.3 The TUI's list row now names the daemon's run

The daemon's run id is a UUID; the TUI's local counter is not. The
projection publishes no local id, so `/shell-list` now prints the daemon's
run id in the row (`src/tui/commands/shell.rs:1279`) where it used to print
the TUI's counter. `/shell-show <n>` still takes the **local** numeric id or
`last`, so a row's identifier and `/shell-show`'s argument are no longer the
same value. This is the one column that visibly changes.

The alternative was to publish the TUI's local id, which the daemon does not
own and cannot know. The daemon's id is the one identifier both frontends
can name.

### 10.4 The no-daemon path still answers from the local store

`src/tui/app/mod.rs:8554` answers `/shell-list` from the local store when
the TUI has **no command channel** — that is, when there is no daemon to
own a list at all. It keeps its own row shape (`[id] $ cmd (status)`),
which differs from the daemon-backed path's. That path predates this
milestone, and the plan forbids changing what a user sees, so it was left
alone. It is recorded here because it is the one place where a second copy
of the list survives.

### 10.5 What was closed rather than left open

The elapsed time is **not** in this list. The daemon's run record carries a
completion timestamp (`crates/codegg-protocol/src/human_shell.rs:63`), so
the listing shows `done exit=0 1.2s` as it always did.

## 11. Roadmap disposition

- The **M007-J** row is in the roadmap's M007 decomposition table
  (`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`), placed
  after **M007-E**.
- The M007 milestone's status row names **M007-J** in the decomposition
  list.
- The M007 milestone's row is **not** closed by this milestone. The
  desktop GUI parity line continues: M007-F, M007-G and M007-I each still
  need their own ADR, and the two `/lsp-*` forms route to M007-F.
- The three granularity forms (`shell_show`, `shell_expand`,
  `terminal_send`) remain recorded outside the audited path, by the
  owner's decision of 2026-10-25 (closure 017 §3.5). This milestone
  changed neither their routing nor their behaviour.
- The 28 catalog kind changes and the 24 new `FrontendAction` declarations
  remain recorded, untouched (closure 017 §10). This milestone added **no**
  new declared action: `show_tool_contracts` and `show_shell_runs` were
  already declared, and the desktop simply stopped reporting them as
  uncarried.

## 12. Registry updates

- The registry's `## Dependency-ready implementation plans` table gains the
  **M007 projection milestone** row, naming ADR-0053 as its dependency and
  recording the owner's direction of 2026-10-25.
- The same table gains the **M007-J the desktop terminal surface** row,
  recording that it needs no ADR and no daemon work.
- The registry's **Active subsystem roadmaps** row names ADR-0053 and
  M007-J.
- The registry's `## Current execution order and dependency gates` gains
  the projection milestone's paragraph, with the baseline `02030facc`.

## 13. Handoff notes

- **What a reviewer should read first.** ADR-0053 for *what* the daemon
  publishes; plan 018 §3 for *how*; this closure §3 for what landed.
- **The one design decision a reviewer should challenge.** The mechanism:
  all three surfaces ride the canonical session projection rather than
  arriving by a separate daemon read. The reason is the desktop's
  synchronous action dispatch. A reviewer who thinks a separate read is
  better should check §1 of plan 018 and the desktop's action context
  (`apps/desktop/src/commands/frontendActions.ts:46`) before arguing.
- **The next milestone.** **M007-J**, the desktop's terminal surface. It
  needs no ADR and no daemon work. M007-F, M007-G and M007-I each still
  need their own ADR.
- **What was deliberately not done.** The contract **schemas** are not
  published (§8). The **promotion marker** is not published (§10.2). The
  desktop's **terminal surface** is not carried (§10.1).
- **What to watch.** The projection's `shell_runs` and `tool_contracts`
  collections are bounded at 32 and 64
  (`crates/codegg-protocol/src/projection/limits.rs:54`, `:57`). The
  desktop's window is narrower still (`MAX_VISIBLE_SHELL_RUNS = 16`,
  `MAX_VISIBLE_TOOL_CONTRACTS = 32`), so a session with more contracts
  than that shows the desktop a bounded slice. Both bounds are named in the
  code beside their use.
- **The toolchain to verify against.** CI's `verify` lane installs
  `dtolnay/rust-toolchain@stable`, and its clippy is therefore **newer than
  the repository's MSRV** — it was 1.99.0 when this milestone's lanes ran,
  against an MSRV of 1.89. A lint can pass under the local toolchain and
  fail under CI's. Before pushing, run the root clippy at CI's toolchain —
  `rustup run 1.99.0 cargo clippy --workspace --all-targets --locked -- -D warnings`
  — and treat the MSRV clippy as the floor, not as the gate (§4.1).
