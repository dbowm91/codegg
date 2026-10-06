# Desktop Frontend/IDE Foundation Post-Closure Corrective Milestone 002 — Feature-Gated Compile Break and Undrained Diagnostics Publisher

Status: planned

Repository baseline: `533be594` (`main`; M006-B implementation `3bee7011`, closure
`533be594`)

Source corrective addendum:

- `plans/subsystems/desktop-frontend-ide-foundation-post-closure-corrective-addendum.md`

Predecessor corrective in this track (closed, not edited):

- `plans/implementation/desktop-frontend-ide-foundation-post-closure-corrective/001-m004-prompt-composition-error-boundary-and-hosted-closure.md`
- `plans/closure/desktop-frontend-ide-foundation-post-closure-corrective/001-status.md`

Records this corrective addresses (accepted and immutable, none edited):

- `plans/closure/desktop-frontend-ide-foundation/009-status.md` §3, which states
  that `publish_changed_diagnostics` "polls at `DIAGNOSTICS_POLL_INTERVAL =
  750ms`". It has zero callers and the interval constant has zero users. §4 and
  §5 of that record are otherwise accurate and are not disturbed.
- `plans/implementation/desktop-frontend-ide-foundation/008-native-lsp-read-surface.md`
  — the M006-B plan.

Source subsystem roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`

Controlling ADR:

- `plans/adrs/ADR-0012-native-lsp-read-surface-delivery-and-authorization.md`

Long-term requirements:

- `architecture/lsp.md`
- `architecture/authorization.md`

Primary class: infrastructure

## 1. Objective

Repair a feature-gated compile break that M006-B introduced into the WebSocket
surface, and record truthfully that the diagnostics publisher M006-B delivered is
not driven, so that no accepted record claims a capability that no code path
exercises.

Both are one corrective because they share a single root cause: **M006-B was
closed on `scripts/verify.sh quick` and a default-feature clippy, neither of which
compiles the `--features server` surface, and the record was written from the
intended design rather than from the wiring.**

## 2. Why this milestone is ready

- Both defects are reproduced on a clean `main` tree at `533be594`, so neither
  depends on any unmerged work.
- The compile break is a hard failure: `cargo check -p codegg --features server`
  exits non-zero with `error[E0004]`. No interpretation is involved.
- The publisher finding is a two-line grep: `publish_changed_diagnostics` has no
  caller anywhere in `src/`, `crates/`, or `tests/`, and
  `DIAGNOSTICS_POLL_INTERVAL` has no user. There is no `tokio::spawn` in
  `src/core/daemon_lsp.rs` at all.
- The remedy for the break is unambiguous and has direct precedent in
  `ProjectionConsumer::single_snapshot`.
- The remedy for the publisher is a governance decision already taken by the
  owner: declare the capability delivered-but-undriven, do not build the driver in
  this corrective.

## 3. Current implementation evidence

### Defect 1 — the compile break

```
$ cargo check -p codegg --features server --locked
error[E0004]: non-exhaustive patterns:
  `ProjectionSnapshotBundle::LspDiagnostics { .. }` not covered
   --> src/server/ws.rs:2984:34
   --> crates/codegg-protocol/src/projection/replay.rs:140:1
error: could not compile `codegg` (lib) due to 1 previous error
```

M006-B added a third `ProjectionSnapshotBundle` variant. Three of the five
folding sites were updated; `src/server/ws.rs:2984` was not, because it is behind
`--features server` and the closure gate did not compile it.

The correct handling is not an arbitrary arm. `src/server/ws.rs:2984` folds a
snapshot bundle for a **session subscription**, and `LspDiagnostics` is a
**project-scoped** bundle. The headless consumer already states the rule:

```rust
// crates/codegg-protocol/src/projection/consumer.rs:710-716
// A headless session consumer cannot fold in a project-scoped
// diagnostics bundle; rejecting it is correct, not a limitation.
ProjectionSnapshotBundle::BoundedSessionList { .. }
| ProjectionSnapshotBundle::LspDiagnostics { .. } => {
    Err(HeadlessConsumerError::InvalidSnapshotBundle)
}
```

So the WebSocket session surface returns the empty session snapshot — the same
answer the `BoundedSessionList` arm already gives when it holds no sessions — and
a comment records that the arm should be unreachable in practice, because the
daemon produces this bundle only in the `LspDiagnosticsSubscribe` response and
routes the event to `ProjectionStreamKind::Project` only.

### Defect 2 — the publisher is not driven

```
$ grep -rn "publish_changed_diagnostics" src/ crates/ tests/ --include=*.rs
./src/core/daemon_lsp.rs:496:pub async fn publish_changed_diagnostics(

$ grep -rn "DIAGNOSTICS_POLL_INTERVAL" src/ crates/ tests/ --include=*.rs
./src/core/daemon_lsp.rs:481:pub const DIAGNOSTICS_POLL_INTERVAL: ...
```

Zero callers. Zero users. No `tokio::spawn` anywhere in `daemon_lsp.rs`.

What *is* delivered and live:

- `CoreRequest::LspDiagnosticsGet`, the pull authority, fully reachable.
- `CoreRequest::LspDiagnosticsSubscribe`, which returns the authoritative set in
  its response (`src/core/daemon_lsp.rs:721`) — so a subscriber starts from the
  resync authority without any push having occurred.
- `CoreEvent::LspDiagnosticsUpdated`, `SafePublicationClass::Safe`, routed to
  `ProjectionStreamKind::Project` only.
- `LspDiagnosticsStore`, `DiagnosticsTracker`, and the digest rule.
- `DiagnosticsReconciler` on the client.

What is **not** delivered: anything that invokes the publisher. The push path is
a delivered-but-undriven capability. `CoreEvent::LspDiagnosticsUpdated` is never
emitted in production.

This does not affect correctness, because the pull is the correctness authority by
design and `LspDiagnosticsSubscribe` ships the authoritative set in its own
response. It affects what a consumer can expect: with no driver, the stream is
silent forever, and `DiagnosticsReconciler` can only ever report `NoBaseline`.

### Why no consumer notices

The TUI has no daemon event pump at all: zero `ProjectionStreamEvent` references
under `src/tui/`, zero production calls to `CoreClient::subscribe()`, and
`TuiCommand::PresenceHint` has a dispatch arm with no producer anywhere in the
repository. So the absence of a driver is currently unobservable, which is exactly
why it survived a closed milestone.

## 4. Invariants that must not regress

- `ProjectionSnapshotBundle::LspDiagnostics` is a project-scoped bundle. No
  session-scoped surface may fold it into a session snapshot. This corrective
  makes `src/server/ws.rs` follow the rule `consumer.rs` already states.
- `cargo check -p codegg --features server --locked` must exit zero. Before this
  corrective it did not.
- `cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support`
  must compile and run. This is the gate step that was never reached.
- `publish_changed_diagnostics` and `DIAGNOSTICS_POLL_INTERVAL` remain defined
  and remain unused. Neither is deleted: they are the delivered half of ADR-0012's
  decision, and deleting them would silently withdraw an accepted ADR.
- `plans/closure/desktop-frontend-ide-foundation/009-status.md` is byte-identical
  on this branch. The falsification is recorded here, never by editing the record
  it corrects.
- No LSP behavior, authorization decision, protocol variant, or DTO changes.
- The `--features server` fold sites for `One` and `BoundedSessionList` are
  unchanged.

## 5. Scope

### In scope

- `src/server/ws.rs`, one match arm plus its explanatory comment.
- This plan, its closure record, and the `plans/registry.md` rows.
- A direct test asserting a session subscription never surfaces project
  diagnostics.

### Explicitly out of scope

- **Driving the publisher.** No poll loop, no background task, no timer, no
  lifecycle. The owner has decided the push path is delivered-but-undriven; the
  driver is named work, not work done here. Building it would add an always-on
  background timer whose only output is events nothing consumes.
- **Reopening or editing the M006-B closure or plan.** Both are accepted records.
- **Any change to ADR-0012.** Its delivery/authorization decision stands; this
  corrective records that half of it is undriven.
- **The pre-existing failures found while verifying.** See §10.
- **CI configuration.** CI runs clippy and nextest without `--features server`,
  which is why the break passed. Whether to close that gap is its own decision and
  is filed, not taken.
- **`verify.sh full` content.** It already has the feature-gated step; the
  process error was not running it, not its absence.

## 6. Required production changes

### Core/domain

None. No domain type changes.

### Storage and migrations

None. No schema, layout, or migration change.

### Protocol and DTOs

None. `ProjectionSnapshotBundle` already has the variant; this corrective only
adds the missing fold site.

### Runtime and concurrency

None. No spawn, no thread, no scheduler interaction. This is precisely why the
corrective is small: it adds no runtime surface.

### Frontend or operator surface

None. No TUI, command, or keybinding change.

### Security and authorization

None new. The security-relevant property is *preserved*: a session-scoped WebSocket
subscriber must not receive project-scoped diagnostics, because diagnostics carry
source content and are gated at `file.read`. The arm enforces that rather than
letting an accidental future bundling expose it.

### Documentation and static guards

- This plan and its closure record.
- `plans/registry.md`: roadmap row, implementation-plan row, closure row, and an
  additive amendment to the desktop-frontend gate paragraph recording that
  `009-status.md` §3 is falsified on the poller claim.
- No `architecture/` change: `architecture/lsp.md` describes the pull authority and
  the resync contract, both of which are accurate and unchanged.
- `scripts/check_projection_disclosure.sh` and `check_websocket_bounds.py` run as
  change-triggered verification.

## 7. Ordered work packages

### Work package A — Fold the variant correctly

In `src/server/ws.rs`, add a `ProjectionSnapshotBundle::LspDiagnostics { .. }` arm
to the match at the session-subscribe snapshot fold. It returns
`SessionProjectionSnapshot::empty(...)` with the same descriptor fields the
`BoundedSessionList` fallback uses, and carries a comment that states the rule
(a project-scoped bundle is not a session snapshot), cites the headless
consumer's precedent, and notes that the arm should be unreachable in practice.

### Work package B — Prove the rule, not just the compile

Add a test that folds a `LspDiagnostics` bundle through the session path and
asserts the result is an empty session snapshot with no diagnostic content. A
compile-only fix would satisfy the compiler and prove nothing about the rule; the
test is what makes the security property explicit.

### Work package C — Verify the gate that was missed

1. `cargo check -p codegg --features server --locked` — the failing command.
2. `cargo fmt --all -- --check`.
3. `cargo clippy --workspace --all-targets --locked -- -D warnings`.
4. `cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci`
   — the step that was never reached.
5. The projection guards named in §6.
6. `scripts/verify.sh quick`.

`scripts/verify.sh full` is the umbrella command for 1–6, but on this machine it
cannot complete for reasons unrelated to this change (§10), so each step is also
recorded individually.

### Work package D — Record the falsification additively

Add a closure record stating that `009-status.md` §3's poller claim is false,
with the grep evidence, and that the push path is delivered-but-undriven. Do not
edit `009-status.md`.

## 8. Failure and cancellation, restart, and contention semantics

Not applicable in the production sense: this corrective adds one match arm and no
runtime surface, so there is no new state, process, or persistence to reason
about.

The one behavioral change is what a session-scoped WebSocket subscriber receives if
it is ever handed a diagnostics bundle: previously a compile error, now an empty
session snapshot. That is the same answer the surface already gave when handed an
empty `BoundedSessionList`, and it is the safe direction — the alternative,
folding project diagnostics into a session snapshot, would be a disclosure.

## 9. Compatibility and migration

None. No schema, storage, protocol, configuration, or public API change, and no
migration. The change is confined to one `match` arm in a feature-gated handler
plus a test. MSRV 1.89 is preserved.

The arm is additive to a `match` that was previously non-exhaustive under this
feature set, so no existing arm's behavior changes.

## 10. Required tests

### Focused unit tests

One new test for Work package B: a project-scoped diagnostics bundle folded through
the session-subscribe path yields an empty session snapshot and carries no
diagnostic content.

### Integration tests

- The new test, in the same module as the handler it covers.
- `cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support`
  in full, which is the step that never ran for M006-B.

### Restart and recovery tests

Not applicable. No persistent state.

### Contention and cancellation tests

Not applicable. No concurrency introduced.

### Security and negative tests

The Work package B test is the negative test: it asserts the session surface does
not adopt project-scoped diagnostics.

### Migration and compatibility tests

Not applicable. No migration.

## 11. Required verification commands

```bash
# the failing command, before the fix
cargo check -p codegg --features server --locked

# after the fix
cargo check -p codegg --features server --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci

# change-triggered guards
bash    scripts/check_projection_publication_seam.sh
bash scripts/check_projection_disclosure.sh
python3 scripts/check_websocket_bounds.py

scripts/verify.sh quick
```

Hosted:

```bash
gh run view <run-id>
```

## 12. Documentation updates

- This plan.
- `plans/closure/desktop-frontend-ide-foundation-post-closure-corrective/002-status.md`
  — milestone number `002`, matching this plan.
- `plans/registry.md` — roadmap row, implementation-plan row, closure row, and an
  additively-worded amendment to the desktop-frontend gate paragraph recording
  that `009-status.md` §3's poller claim is falsified and naming the driver as
  outstanding work.
- The in-tree comment at the new arm, so the rule is visible at the fold site.

## 13. Acceptance criteria

- `cargo check -p codegg --features server --locked` exits zero.
- `cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support
  --profile ci` compiles and runs.
- The new test proves a session subscription cannot adopt project-scoped
  diagnostics.
- The `One` and `BoundedSessionList` arms are unchanged.
- `publish_changed_diagnostics` and `DIAGNOSTICS_POLL_INTERVAL` still exist and
  are still unused; neither is deleted.
- `plans/closure/desktop-frontend-ide-foundation/009-status.md` is byte-identical
  on this branch.
- fmt, clippy, and `scripts/verify.sh quick` are clean.
- The three projection guards are green.
- The closure records the falsification with its grep evidence.
- Hosted `CI / verify` is green on the fix head.

## 14. Stop conditions

Stop and re-plan if any of these hold:

- The fix requires changing any `One` or `BoundedSessionList` behavior. That would
  mean the variant does not belong at this fold site at all, which is a different
  finding.
- Fixing the break requires editing a `ProjectionSnapshotBundle` variant, a
  protocol type, or an ADR. That is a design change, not a corrective.
- The feature-gated sweep cannot be made to run on this machine for reasons
  unrelated to this change. Record the specific blocker honestly and rely on
  hosted CI, rather than claiming a gate that did not execute.
- Anyone proposes deleting `publish_changed_diagnostics` or
  `DIAGNOSTICS_POLL_INTERVAL` as "dead code". Deleting them would silently
  withdraw an accepted ADR's decision and is explicitly out of scope.

## 15. Closure evidence required

- The exact baseline and fix commits.
- The before/after output of `cargo check -p codegg --features server --locked`,
  with the exact `E0004` text quoted.
- The grep output proving zero callers and zero users, quoted verbatim.
- A requirement-to-evidence matrix, one row per §4 invariant.
- The new test's name and its assertion, plus the command that ran it.
- The feature-gated nextest result, or an explicit statement of why it could not
  complete locally and what hosted CI showed instead.
- `git diff --stat`, showing one source file and `plans/`.
- Proof that `009-status.md` is byte-identical on the branch.
- An explicit statement of the CI gap that let this reach `main`, filed as a
  finding rather than silently fixed.
- Honest reporting of the pre-existing failures that block a clean local sweep.

## 16. Handoff notes

- This corrective reopens no M006 scope. M006-B's protocol surface, authorization
  decisions, and DTOs are untouched and remain as delivered.
- It does not unblock anything that was blocked: M006-C was already unblocked by
  the M006-B closure and remains unblocked. This corrective makes that closure
  *accurate*, which is a different kind of fix.
- The two lessons worth carrying: a feature-gated surface needs a gate that
  compiles it, and a closure record written from intended design rather than from
  the wiring will state capabilities that do not run. Both are cheap to check and
  expensive to discover late.
- The CI gap in §10 is the systemic version of this defect and deserves its own
  decision.
