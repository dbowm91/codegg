# Agent Workstream Blackboard M006 — Closure Status

Status: blocked — ExistingManaged launch/fork is implemented, but NewManaged checkout materialization remains unavailable

Source implementation plan: `plans/implementation/agent-workstream-blackboard/006-session-worktree-launch.md`
Source subsystem roadmap: `plans/subsystems/agent-workstream-blackboard-roadmap.md#M006--isolated-sessionworktree-fork-and-launch-ux-active`
Repository baseline reviewed: `3ee041db42e2f7cf4be681bd343b527f0881f92d`

Implementation commits:

- `097cba5d` — session/worktree launch and workspace build-admission implementation
- `ab86c789` — stable reservation recovery test
- `239034b7` — provider deferral compatibility and launch-response fixes
- `ed065038` — linked-worktree daemon fixture
- `3ee041db` — fixture repository-lineage correction

## 1. Executive finding

M006 cannot close. Session creation and history fork into an already-authorized
managed linked worktree are implemented with idempotent daemon requests,
workspace binding and stale-replay checks. NewManaged requests reserve a
recoverable identity but cannot materialize the checkout: the current Git
backend rejects `worktree add` because the operation requires
`CleanSmudgeFilter` containment and no contained backend is available. The
request fails closed and retains its reservation. This per-operation Git
materializer gap is distinct from M005's checkout-wide shared-writer gate;
M006 does not otherwise require M005 or shared-write execution.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Explicit launch/fork intent, placement and idempotency key | Protocol request/response types and daemon handlers in `src/core/daemon_sessions.rs` | pass | NewManaged and ExistingManaged are distinct typed choices. |
| Authenticated session launch and fork into an existing managed worktree | `src/core/daemon.rs::tests::managed_session_launch_and_fork_are_idempotent_and_bound` | pass locally | Creates real linked Git worktrees, checks path/workspace binding and duplicate request replay. |
| Durable NewManaged reservation and recoverable failure | `crates/codegg-core/src/worktree_service.rs::tests::stable_reservation_retries_same_checkout_and_retains_backend_failure` | pass in existing suite | The current backend denial leaves the reservation for retry/attention. |
| Materialize and launch/fork in a newly allocated checkout | `WorktreeService::create_reserved` -> `codegg_core::worktree::create_worktree_at` -> canonical Git policy | blocked | `worktree add` requires `CleanSmudgeFilter` containment; no qualifying backend is implemented. |
| CLI and TUI affordances; old fork semantics preserved | `src/main.rs`, TUI command dispatch and updated `docs/cli.md` / `architecture/session.md` | implemented | Existing --fork and /fork behavior remains unchanged. |
| Platform-specific linked `.git` and Windows path-length qualification | Existing general worktree tests plus this hosted run | partial | M006-specific Windows qualification was not run. |

## 3. Production implementation evidence

The daemon owns session launch/fork orchestration, stable reservation identity,
workspace registration/binding and session creation. CLI and TUI issue typed
daemon requests and display the concrete session/worktree result. Existing
managed linked worktrees can be selected without creating a second checkout.
NewManaged uses the same durable WorktreeService saga; it does not report a
session as isolated when materialization failed.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
git diff --check
cargo check -p codegg --tests --locked
bash scripts/verify.sh quick
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
RUSTC=/Users/davidbowman/.rustup/toolchains/1.89-aarch64-apple-darwin/bin/rustc \
RUSTDOC=/Users/davidbowman/.rustup/toolchains/1.89-aarch64-apple-darwin/bin/rustdoc \
rustup run 1.89-aarch64-apple-darwin cargo test --target aarch64-apple-darwin \
  -p codegg --lib \
  core::daemon::tests::managed_session_launch_and_fork_are_idempotent_and_bound \
  --locked
```

### Results

- Formatting, diff check, workspace/all-target check, quick verification and
  the two focused static guards passed locally.
- The focused M006 daemon test passed locally on the pinned Rust 1.89 aarch64
  toolchain after the fixture was given a deterministic, non-network Git
  remote. The earlier hosted run `38866058427` failed this test because the
  source fixture had no repository lineage; 1,080 tests had passed before
  fail-fast stopped that run.
- Exact-head hosted CI run `38868243512` passed its guards and workspace Clippy,
  then stopped at `tui::command::tests::built_in_command_count_matches_release_docs`
  (165 runtime commands vs. stale expected/documented count 163). This is an
  unrelated count assertion exposed by the two M006 commands; the assertion
  and command-count docs are corrected locally, and the next exact-head hosted
  run is pending. Do not claim green hosted CI until it passes.

## 5. Invariant review

- Session requests never turn a filesystem path into a managed worktree ID.
- Existing worktree authorization is resolved by the daemon and remains scoped
  to the caller's project/workspace binding.
- Duplicate operation keys return the same result; reusing a key for different
  intent is rejected.
- A failed NewManaged materialization does not create a session in the source
  checkout and does not clean a retained checkout.
- Shared checkout execution remains unavailable under ADR-0002/ADR-0032.

## 6. Failure and recovery review

The daemon saga persists the request/reservation before materialization and
retains uncertain or failed state. Stable retries resolve the same worktree ID
and path. Existing managed worktree launch/fork replay was qualified by the
focused daemon test. The unavailable contained Git backend is a hard stop for
NewManaged; no attempt is made to bypass it with a direct shell, weakened Git
policy or cleanup.

## 7. Migration and compatibility review

The change uses the existing durable worktree and session storage contracts;
no new database migration was introduced by M006. Existing session fork
semantics and the legacy CLI/TUI entry points remain compatible. The typed
request response carries concrete session/workspace/worktree identity.

## 8. Security review

Project/principal binding is checked in the daemon. Existing managed worktree
selection does not transfer ownership or allow selection by path. Git process
policy remains unchanged and fails closed for `CleanSmudgeFilter`. The test
fixture performs test-owned Git operations only; it grants no production
authority.

## 9. Documentation and operations

Updated `architecture/session.md`, `architecture/authorization.md`,
`docs/cli.md`, the CLI and TUI surface, and this implementation plan's status.
M005 records why this per-operation materializer limitation is distinct from
checkout-wide writer containment.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | NewManaged cannot create a checkout without a contained Git backend | The promised new-session/new-fork-in-new-checkout path is unavailable; reservation is retained and the request fails closed. | Implement and qualify a contained isolated-checkout materializer, then run NewManaged launch/fork, replay, restart, dirty-state, linked `.git`, and platform path tests. |
| medium | M006-specific Windows path-length evidence is absent | Cross-platform launch support is not established. | Add hosted/native Windows evidence before claiming Windows support. |

## 11. Roadmap disposition

M006 is **blocked** for full closure. ExistingManaged launch/fork is usable
and isolated from M005 shared-write execution, but the NewManaged capability
is incomplete until the named per-operation Git containment gap is resolved.
Do not mark M005 closed or enabled as a workaround.

## 12. Registry updates

- Keep M006 blocked on the contained NewManaged materializer. This is not a
  dependency on M005's shared-writer qualification.
- M008 broad qualification remains blocked on M006 and M007 closure; its
  authorized S0/S1 pilot is separately recorded at
  `plans/closure/agent-workstream-blackboard/008-pilot-results.md`.
- M005 remains active and shared mutation remains unavailable. No plan became
  newly ready from this M006 review.
