# Desktop Frontend/IDE Foundation Post-Closure Corrective C002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation-post-closure-corrective/002-feature-gated-compile-break-and-undrained-publisher.md`

Source corrective addendum:

- `plans/subsystems/desktop-frontend-ide-foundation-post-closure-corrective-addendum.md`

Predecessor corrective in this track (closed, not edited):

- `plans/implementation/desktop-frontend-ide-foundation-post-closure-corrective/001-m004-prompt-composition-error-boundary-and-hosted-closure.md`
- `plans/closure/desktop-frontend-ide-foundation-post-closure-corrective/001-status.md`

Records this corrective discharges (accepted and immutable, none edited):

- `plans/closure/desktop-frontend-ide-foundation/009-status.md` §3 — the sentence
  stating that `publish_changed_diagnostics` "polls at
  `DIAGNOSTICS_POLL_INTERVAL = 750ms`". It has zero callers and the constant has
  zero users. §4 and §5 of that record are accurate and are not disturbed.

Repository baseline reviewed: `533be594` (both defects reproduced on a clean tree)

Implementation commits:

- `8e5fcc76` — C002 plan and registry rows.
- `6280b1b8` — C002 source fix: `fold_session_snapshot_bundle` extracted, the
  `LspDiagnostics` arm added, and a falsification-proven test.

Merge: `d2ab3d9a` (PR `#102`)

## 1. Executive finding

A `--features server` compile break that M006-B introduced is repaired, and the
second defect is recorded rather than papered over: the diagnostics publisher
M006-B delivered is **not driven**, so `009-status.md`'s claim that it polls at
750 ms is false. Both defects trace to one cause — M006-B was closed on
`verify.sh quick` and a default-feature clippy, neither of which compiles the
feature-gated surface, and the closure was written from intended design rather
than from the wiring.

The break was a hard failure, not a warning:
`cargo check -p codegg --features server --locked` exited non-zero with
`error[E0004]`. The publish path being undriven is invisible to correctness,
because the pull is the correctness authority and
`LspDiagnosticsSubscribe` ships the authoritative set in its own response — but
it means `CoreEvent::LspDiagnosticsUpdated` is never emitted in production, which
is a materially different statement from the one the closure made.

No LSP behavior, authorization decision, protocol variant, or DTO changed. The
source diff is one file.

## 2. Requirement-to-evidence matrix

| Requirement (plan §4) | Evidence | Result |
|---|---|---|
| `cargo check -p codegg --features server --locked` exits zero | exit 0 after the fix; before it, `error[E0004]` at `src/server/ws.rs:2984` | pass |
| The feature-gated sweep compiles and runs | `cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci` compiled and began executing tests | pass (partial locally, see §4) |
| A project-scoped bundle is never folded into a session snapshot | `fold_session_snapshot_bundle` returns the empty session snapshot for `LspDiagnostics`, matching `ProjectionConsumer::single_snapshot` | pass |
| The rule is proven, not just compiled | `a_session_subscription_never_adopts_project_scoped_diagnostics` asserts fold equality with an empty session list; **deliberately breaking the arm fails the test** | pass |
| `One` and `BoundedSessionList` arms unchanged | unchanged in the diff | pass |
| `publish_changed_diagnostics` and `DIAGNOSTICS_POLL_INTERVAL` still defined and still unused | both still present; no call site was added | pass |
| `009-status.md` byte-identical on this branch | absent from both commits | pass |
| No LSP behavior, authorization, protocol, or DTO change | diff is `src/server/ws.rs` plus `plans/` | pass |
| fmt clean | `cargo fmt --all -- --check` clean | pass |
| clippy clean | `cargo clippy --workspace --all-targets --locked -- -D warnings` clean | pass |
| `verify.sh quick` green | exit 0, all guards green | pass |
| Projection guards green | `check_projection_publication_seam.sh`, `check_projection_disclosure.sh`, `check_websocket_bounds.py` all green | pass |
| Hosted `CI / verify` green | run `37411992115`, 24/24 steps, sweep 12287/12287 in 399.769s | pass |

## 3. Production implementation evidence

One extracted function and one match arm, in `src/server/ws.rs`.

The fold was previously an inline `match` inside `handle_projection_subscribe`.
It is now `fold_session_snapshot_bundle(bundle, descriptor)`, so the rule is a
named function with a doc comment and a direct unit test rather than an
anonymous match arm. The `One` and `BoundedSessionList` arms are byte-identical
to before; only the call site changed.

The new arm returns `SessionProjectionSnapshot::empty(...)` for
`ProjectionSnapshotBundle::LspDiagnostics`, sharing the same closure the
`BoundedSessionList` arm uses for an empty list. That is the correct answer
because this is a **session subscription** and the bundle is **project-scoped**.
`ProjectionConsumer::single_snapshot` already states the rule and already refuses
this bundle for the same reason; this brings the WebSocket surface in line.

The arm is handled explicitly rather than with a catch-all, and the comment
records that it should be unreachable in practice — the daemon builds this bundle
only in the `LspDiagnosticsSubscribe` response
(`src/core/daemon_lsp.rs:721`) and routes the event to
`ProjectionStreamKind::Project` only. Handling it explicitly means the rule is
enforced by the compiler and documented at the fold site, rather than left to
accident.

Security note: diagnostics carry source content and are gated at `file.read`.
Adopting them into a session-scoped WebSocket snapshot would be a disclosure.
The arm prevents it, and the test pins the prevention.

Root cause, verified rather than inferred: M006-B added a third
`ProjectionSnapshotBundle` variant and updated three of the five fold sites. The
fourth is behind `--features server`. `verify.sh full` does contain the
feature-gated step — `cargo nextest run -p codegg --locked --features
server,plugins,lsp-test-support --profile ci` — but M006-B was never run through
`verify.sh full`, so the step that would have caught this was never reached.

## 4. Verification executed

### Commands run

```bash
# the failing command, on clean main at 533be594
cargo check -p codegg --features server --locked

# grep evidence for the publisher finding
grep -rn "publish_changed_diagnostics" src/ crates/ tests/ --include=*.rs
grep -rn "DIAGNOSTICS_POLL_INTERVAL" src/ crates/ tests/ --include=*.rs

# after the fix
cargo check -p codegg --features server --locked
cargo test --locked --features server --lib \
  a_session_subscription_never_adopts_project_scoped_diagnostics
# falsification: the arm was deliberately changed to synthesize a non-empty
#   snapshot; the test failed; the arm was restored
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci
bash    scripts/check_projection_publication_seam.sh
bash    scripts/check_projection_disclosure.sh
python3 scripts/check_websocket_bounds.py
scripts/verify.sh quick
scripts/verify.sh full
```

Hosted:

```bash
gh run view 37411992115     # CI / verify, PR #102
```

### Results

Local:

- Before the fix, `cargo check -p codegg --features server --locked` failed:
  `error[E0004]: non-exhaustive patterns: ProjectionSnapshotBundle::LspDiagnostics
  { .. } not covered`, `--> src/server/ws.rs:2984:34`,
  `--> crates/codegg-protocol/src/projection/replay.rs:140:1`.
- After the fix, the same command exits 0.
- Grep evidence for the publisher finding, quoted verbatim in §3 of the plan:
  `publish_changed_diagnostics` appears only at its own definition
  (`src/core/daemon_lsp.rs:496`), and `DIAGNOSTICS_POLL_INTERVAL` only at its own
  definition (`:481`). There is no `tokio::spawn` in `src/core/daemon_lsp.rs`.
- The new test passes. Its falsifiability was demonstrated rather than assumed:
  the `LspDiagnostics` arm was temporarily rewritten to synthesize a non-empty
  snapshot (`event_seq = 99`, `generated_at_ms = project_id.len()`), the test
  failed with the assertion `a project-scoped diagnostics bundle must fold to the
  same empty session snapshot as an empty session list` and a full left/right
  diff, and the arm was restored and re-verified. The temporary edit was never
  committed; the restored file was confirmed to contain the original arm.
- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean.
- The three projection guards: all green. Note `check_projection_disclosure.sh`
  is a shell script; running it under `python3` was a mistake in this session and
  is recorded because it produced a confusing `SyntaxError` that looked like a
  guard failure.
- `scripts/verify.sh quick`: exit 0.

Hosted (PR `#102`, head `6280b1b8`) — run **`37411992115`**, verify job
**success**, **24 of 24 steps green**, no failed step:

- `Workspace tests`: **`12287` tests run, `12287` passed, `7` skipped**, in
  399.769s.

### What the hosted run does not cover, stated plainly

**The new test did not execute on hosted CI.** It lives behind
`--features server`, and CI runs clippy and nextest without features. The grep of
run `37411992115` for the test name returns nothing. Its evidence is therefore
local `--features server` runs only.

This is the same gap that allowed the compile break to reach `main`, and closing
it would fix both halves at once: a CI job or step that compiles
`--features server` would have caught the break, and it would also give the new
test hosted coverage. It is filed in §10 rather than taken here, because adding a
CI job changes the bounded job's cost profile and is a decision for the owner.

## 5. Invariant review

- `ProjectionSnapshotBundle::LspDiagnostics` is never adopted by a session-scoped
  surface. `src/server/ws.rs` now matches the rule
  `ProjectionConsumer::single_snapshot` already states, and the test pins it
  with a claim that can actually fail.
- `publish_changed_diagnostics` and `DIAGNOSTICS_POLL_INTERVAL` are still defined
  and still unused. Neither was deleted: they are the delivered half of
  ADR-0012's decision, and removing them would silently withdraw an accepted ADR.
- No LSP behavior changed. No authorization decision changed. No protocol variant
  or DTO changed. `ADR-0012` is unmodified and remains controlling.
- No runtime surface was added: one match arm, no spawn, no thread, no timer,
  no scheduler interaction, no `ExecutionContext` entry, no
  `docs/execution-ownership.toml` entry. The guard that would require one,
  `check_execution_ownership.py`, matches none of its spawn/dispatch patterns
  and is unaffected.
- No migration, no schema change, no `STORAGE_LAYOUT_VERSION` change.
- No TUI surface change.
- `plans/closure/desktop-frontend-ide-foundation/009-status.md` is byte-identical
  on this branch. The falsification is recorded here and in the registry, never by
  editing the record it corrects.
- The source diff is one file, four hunks: the extracted function, the call site,
  the removed now-unused local imports, and the test.

## 6. Failure and recovery review

The one behavioral change is what a session-scoped WebSocket subscriber receives
if it is ever handed a diagnostics bundle: previously a compile error, now the
empty session snapshot. That is the same answer the surface already gave for an
empty `BoundedSessionList`, and it is the safe direction. The alternative —
folding project diagnostics into a session snapshot — would be a disclosure.

No state, process, or persistence surface is involved, so there is nothing else
to review. The temporary falsification edit was made against a file copy, applied,
tested, and reverted, with the restored file verified to contain the original arm
before the commit.

## 7. Migration and compatibility review

None. No schema, storage, protocol, configuration, or public API change, and no
migration. The change is one match arm in a feature-gated handler plus a test.
MSRV 1.89 is preserved. The arm is additive to a `match` that was previously
non-exhaustive under this feature set, so no existing arm's behavior changes.

Extracting `fold_session_snapshot_bundle` is a refactor with no signature
exposed outside the module; it is private and used at exactly one call site plus
one test.

## 8. Security review

The corrective reinforces, and does not weaken, a security property.

Diagnostics carry source content and are gated at `file.read` precisely because
they disclose file content to an authorized reader. A **session-scoped**
WebSocket subscription is not that authorization: it is a narrower observation
surface. Folding a project-scoped diagnostics bundle into it would hand
project-wide diagnostic content — messages, paths, and code spans — to a
subscriber authorized only for its own session. The new arm prevents exactly that,
and the test's bundle deliberately carries a `"top secret constant"` message so
the intent is legible to a future reader.

No new capability is granted, no authorization is relaxed, no input path widens,
and no secret-handling surface is touched.

## 9. Documentation and operations

- `plans/implementation/desktop-frontend-ide-foundation-post-closure-corrective/002-feature-gated-compile-break-and-undrained-publisher.md`
  — this milestone's plan.
- This closure record.
- `plans/registry.md` — roadmap row updated to C001/C002 closed, implementation-plan
  row added, closure row added.
- The in-tree doc comment on `fold_session_snapshot_bundle`, stating the rule and
  why the arm is expected to be unreachable.
- No `architecture/` change. `architecture/lsp.md` describes the pull authority
  and the resync contract, both accurate and unchanged. It does **not** claim the
  push path is running, so no architecture document is falsified.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | CI never compiles `--features server`: `cargo clippy --workspace --all-targets --locked` and `cargo nextest run --workspace --locked` run with default features. This is the gap that let the `E0004` break reach `main`, and it is also why this corrective's new test has no hosted coverage. | A feature-gated surface can be broken on `main` without any gate noticing, and its tests can run nowhere but a contributor's laptop. | Add a CI step compiling and testing `--features server,plugins,lsp-test-support`, or make the existing `verify.sh full` feature step a required check. Deliberately not taken here: it changes the bounded CI job's cost profile and is the owner's call. **This is the single highest-value follow-up from this milestone.** |
| medium | The diagnostics publisher is delivered but undriven. `publish_changed_diagnostics` has zero callers; `CoreEvent::LspDiagnosticsUpdated` is never emitted in production. | No consumer is affected today: the pull is the correctness authority and `LspDiagnosticsSubscribe` ships the authoritative set in its response, and the TUI has no daemon event pump at all. The cost is that ADR-0012's push half is a capability, not a behavior. | Owner's decision, taken and recorded: the driver is **not** built here. It should be built by whichever milestone first consumes the stream, or explicitly deferred. Until then, no document may describe the push path as running. |
| medium | `codegg::scheduler_cancellation::cancel_running_job_terminates_process_and_releases_permit` fails **deterministically**, 3/3 in isolation on this branch and 2/2 on clean `main` with this branch's changes stashed. `tests/scheduler_cancellation.rs:440`. | A pre-existing failure on `main`, unrelated to LSP, the projection surface, or this corrective. | Its own corrective. Not absorbed here. |
| low | `codegg::agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication` times out at 120 s under the concurrent `ci` sweep on this machine while passing 3/3 in isolation in 0.00 s. It is a `Notify` rendezvous (`started.notified().await`), so it is load-sensitive. | Truncates local sweeps, which is why local full-sweep evidence in this milestone is partial. Hosted CI passed 12287/12287 twice on adjacent heads. | Filed. Distinct defect, distinct subsystem. |
| low | `scripts/check_projection_transport_lifecycle.py` fails on `main` with `daemon_socket.rs: raw forwarder is spawned without an owned handle` (verified pre-existing at `cde7dbfe`). | A guard outside the CI quick subset. | Unrelated; noted so it is not mistaken for fallout. |

None of these are regressions introduced by this corrective, and the two medium
findings about CI and the publisher are consequences of the M006-B process error
rather than of the code changed here.

Honest statement of what this milestone does not achieve:

- It does not make the diagnostics push path work. It makes the record accurate
  about that.
- It does not give the new test hosted coverage. Closing the CI gap in the first
  row would do that, and nothing else will.
- It does not fix the two pre-existing test failures, and it does not make a full
  local `verify.sh full` pass on this machine.

## 11. Roadmap disposition

C002 is closed. The `--features server` surface compiles again, and the one rule
that governs how a project-scoped bundle crosses into a session-scoped surface is
now stated, implemented, and pinned by a test that can fail.

This corrective reopens no M006 scope. M006-B's protocol surface, authorization
decisions, and DTOs are untouched. M006-C remains unblocked exactly as the M006-B
closure left it — this corrective makes that closure *accurate*, which is a
different kind of fix and does not move the roadmap.

The owner-decided disposition of the undriven publisher stands: the push path is
delivered-but-undriven, the driver is not built here, and ADR-0012 is unmodified.

## 12. Registry updates

- `plans/registry.md` roadmap/status table: the desktop-frontend post-closure
  corrective row records C001 and C002 closed, with the compile break and the
  undriven publisher described.
- `plans/registry.md` implementation-plan table: add C002, closed, pointing at the
  plan and this closure.
- `plans/registry.md` closure table: add C002, closed, with both the local
  `--features server` evidence and hosted run `37411992115`.
- `plans/registry.md` desktop-frontend gate paragraph: additively record that
  `009-status.md` §3's poller claim is falsified, that the publisher is
  delivered-but-undriven, and that CI does not compile `--features server`.
