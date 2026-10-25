# Desktop Frontend and IDE Foundation Milestone M007-A — Desktop Build and Hosted Gate

Status: ready for handoff

Repository baseline: `c4fa30aeba439894b048768d4829dea0cfa0b84b`

Source roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-007--desktop-gui-parity-line`
- M007 decomposition table: `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#m007-decomposition`
- the pause/resume record: `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` header

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles` — the daemon remains the
  single owner; a frontend is a client connection, not a second authority.
- `plans/000-long-term-specification.md#44-frontends-render-projections` — frontends render
  frontend-neutral snapshots; they do not own durable state.
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`
  — the canonical session view is what every frontend consumes.
- `plans/002-long-term-roadmap.md` cross-phase execution rule 3 — typed protocol and storage
  representations before frontend-only state.

Applicable ADRs and architecture:

- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md` (accepted) — the desktop
  is a separate client over the shared client crate. **This milestone is bound by it.**
- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md` (accepted) — the
  document foundation a later milestone consumes.
- `architecture/desktop.md` — the desktop's own architecture page, including §8 (renderer
  security boundary) and "The hosted qualification is paused (2026-10-23)".
- `architecture/client.md` — the shared client crate's boundary.
- `architecture/overview.md` — the counted surfaces this milestone changes.
- `architecture/process-tool-execution-ownership.md` — only if a process-launch surface moves.

Predecessor closures (hard dependencies, all satisfied):

- `plans/closure/desktop-frontend-ide-foundation/003-status.md` — M003, the Tauri shell and the
  secure bridge, closed. Its boundary is this milestone's to preserve.
- `plans/closure/desktop-frontend-ide-foundation/004-status.md` and
  `plans/closure/desktop-frontend-ide-foundation-post-closure-corrective/001-status.md` — M004,
  the session/control-plane slice, strictly closed. Its live-turn trajectory is the lane this
  milestone must make green again.
- `plans/closure/desktop-frontend-ide-foundation/005-status.md` — M006-A, the TUI editor
  buffer, closed.

Primary class: infrastructure

> This milestone delivers **no user-visible capability**. It restores the ability to *prove*
> that the desktop tree works. Per the planning governance, infrastructure must not be
> represented as a completed capability until a consumer path exists — the consumer paths are
> M007-B, M007-C, M007-D and M007-E, which are registered and depend on this milestone.

## 1. Objective

One bounded outcome: **every change to the desktop tree is compiled, linted and tested by an
automated gate that runs on the pull request, and the paused hosted qualification is green
again with its triggers restored.**

Concretely, four things:

1. the built-app visible-window lane that has been red on `main` since before this line paused
   is green, with the cause understood and recorded;
2. the hosted qualification's `push` and `pull_request` triggers are restored, exactly as the
   workflow's own header documents, and the workflow no longer says "(paused)";
3. the desktop Rust host is linted with `clippy -D warnings` — it has never been — and the
   renderer and the host have **one** documented local entry point that runs the same checks;
4. a static guard fails closed if the desktop tree ever falls out of an automated gate again.

## 2. Why this milestone is ready

Closed hard dependencies:

- M001…M006-E are all closed or strictly closed; the desktop foundation, the shared client
  crate, the document foundation and the native LSP read surface are settled interfaces.
- ADR-0010 is accepted and pins the desktop as a separate client over `codegg-client`.
- The `desktop-e2e` workflow already contains every check this milestone needs. It is not
  missing machinery; it is missing **triggers** and it is **red**.

Stable interface dependencies:

- `apps/desktop/src-tauri/Cargo.toml:6-9` is its own Cargo workspace with `rust-version = "1.90"`
  and its own `rust-toolchain.toml:2-4` pinning `channel = "1.90.0"` with the `rustfmt` and
  `clippy` components. The gate therefore has a pinned toolchain to invoke.
- `apps/desktop/package.json:8-12` already defines `typecheck`, `test`, `bindings:check` and
  `build`, and `apps/desktop/e2e/run-e2e.sh` is the canonical built-app entry point.
- `scripts/check-desktop-boundary.sh` is the existing renderer boundary guard and is already
  wired into `scripts/verify.sh quick`.

No ADR is required. This milestone changes no protocol, no storage, no authorization semantics,
no ownership boundary and no product behaviour. It restores a gate.

## 3. Current implementation evidence

At the baseline:

- **The desktop tree is compiled by no automated gate.** `Cargo.toml:6` reads
  `exclude = ["crates/eggwork-test-node", "apps/desktop/src-tauri"]`, and
  `apps/desktop/src-tauri/Cargo.toml` declares its own `[workspace]` with `members = ["."]`.
  The root workspace's `cargo check` in `scripts/verify.sh quick` therefore never compiles
  `apps/desktop/src-tauri/src/**`. `scripts/check-desktop-boundary.sh:5-7` asserts this separation
  is real — `scripts/check-desktop-boundary.sh:5-7` fails if `cargo metadata` on the root manifest
  reports a `tauri`, `tauri-build`
  or `codegg-desktop` package — so the separation is **deliberate and enforced**, and this
  milestone must not undo it.
- **The hosted visible-window qualification is manual-dispatch only.** `.github/workflows/desktop-e2e.yml:52-53`
  reads `on:` with `workflow_dispatch:` and nothing else, and its name at `:1` still carries
  the "(paused)" marker. Its own header documents the restoration at
  `.github/workflows/desktop-e2e.yml:36-50`
  procedure: *"To restore the qualification when development resumes: re-add the `push` and
  `pull_request` triggers with the path filters preserved below, and drop the '(paused)' from
  this workflow's name. Nothing else in this file changes."* The path filters are preserved in
  that header as a comment block, beginning at `.github/workflows/desktop-e2e.yml:36`.
- **The lane is red for a known reason.** `main`'s most recent run, `39679968444` at
  `53d9fc9fd`, fails with
  `element ("[data-testid="permission-list"]") still not existing after 120000ms` at
  `apps/desktop/e2e/specs/m004-session.e2e.ts:169:5`, and the nine runs behind it are
  consecutive failures. `architecture/desktop.md` §"The hosted qualification is paused
  (2026-10-23)" records the same fact.
- **The failing assertion is a render-conditional.** `apps/desktop/src/App.tsx:401` renders the
  permission list only when the projection carries pending permissions — the element is
  inside a `projection.pendingPermissions.length && …` arm — and the e2e spec waits for that
  element to exist before answering the permission. The desktop's Rust presenter does derive
  the field: `apps/desktop/src-tauri/src/present.rs:107-108` builds `pending_permissions` from
  `turn.pending_permissions`, iterating it at `:283-288` under the
  `MAX_PENDING_PERMISSIONS` bound declared at `:53`. So the failure is either
  the permission never reaching the desktop's projection view, or the render condition never
  becoming true for that turn. **The cause is not yet established and this milestone must
  establish it.** Do not assume either branch.
- **The desktop host's Rust code is never linted.** The workflow's "Desktop host checks
  (production configuration)" step (`.github/workflows/desktop-e2e.yml:142-146`) runs
  `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
  --locked --lib` and `cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check`.
  There is **no** `clippy` step: `grep -c clippy .github/workflows/desktop-e2e.yml` is `0`. The
  root workspace's `cargo clippy --workspace --all-targets --locked -- -D warnings` does not
  reach this package. The pinned toolchain already carries the `clippy` component
  (`apps/desktop/src-tauri/rust-toolchain.toml:2-4`), so the missing step is a missing *call*.
- **The desktop host has 73 unit tests across five files** —
  `apps/desktop/src-tauri/src/lib.rs` (19), `route.rs` (24), `present.rs` (12),
  `projection.rs` (10) and `lifecycle.rs` (8) — and no integration-test target. The host
  checks run `--lib`, which is the right scope for them.
- **The renderer has 18 component tests** in `apps/desktop/src/App.test.tsx`, and the
  built-app suite has **14** tests across three spec files
  (`apps/desktop/e2e/specs/m003-autostart.e2e.ts` 2,
  `m003-lifecycle.e2e.ts` 5, `m004-session.e2e.ts` 7).
- **The renderer checks are complete but unreachable.** The workflow's "Desktop renderer
  checks" step (`.github/workflows/desktop-e2e.yml:124-132`) runs `npm ci`, `npm run typecheck`,
  `npm test`, `npm run bindings:check` and
  `npm run build` with `working-directory: apps/desktop`. `npm run bindings:check` is
  `node scripts/check-bindings.mjs`, which proves the Rust DTO fields and the TypeScript
  mirror agree — a guard that currently runs only inside a workflow that never triggers.
- **The boundary guard already runs on every change to the repository.**
  `scripts/verify.sh:68-72` runs `./scripts/check-desktop-boundary.sh` and
  `./scripts/check-client-boundary.sh` as part of the quick guard sweep. What is **not**
  guarded is the gate itself: nothing asserts that the desktop tree is inside an automated
  gate, so the lane can be paused again and the repository will not notice.

## 4. Invariants that must not regress

- **The desktop stays a separate Cargo workspace.** `scripts/check-desktop-boundary.sh` fails
  if the root `cargo metadata` reports `tauri`, `tauri-build` or `codegg-desktop`. Do **not**
  make the desktop a root workspace member; that guard exists to keep the root workspace's
  Rust 1.89 MSRV and dependency graph free of the Tauri toolchain.
- **The root workspace's Rust MSRV stays 1.89** — the same guard asserts it at
  `scripts/check-desktop-boundary.sh:8-11`.
- **The renderer gains no machine authority.** The guard rejects `shell`, `filesystem`, `fs:`,
  `http:default`, `process:`, `updater`, `clipboard` and `global-shortcut` —
  `scripts/check-desktop-boundary.sh:16-21` — in
  `apps/desktop/src-tauri/capabilities/main.json`, rejects `unsafe-eval` and any remote origin
  in `apps/desktop/src-tauri/tauri.conf.json` (`scripts/check-desktop-boundary.sh:12-15`), and rejects a generic machine-authority bridge
  (`core_request`, `read_file`, `invoke.*shell`) anywhere in `apps/desktop/src` or
  `apps/desktop/src-tauri/src` (`scripts/check-desktop-boundary.sh:22-25`). This milestone must not widen that set, and it must not
  "fix" the gate by relaxing the guard.
- **The hosted lane's path filters stay as they are.** They are the recorded set of paths that
  can change the desktop's behaviour, and they were preserved deliberately through the pause.
- **The production Tauri configuration stays production.** The `desktop-e2e` feature — the only
  configuration that compiles and registers the embedded WebDriver server — is enabled only by
  the e2e path. The host checks step runs with the production configuration deliberately.
- **No protocol, storage, migration or product-behaviour change.** This milestone repairs a
  gate. If fixing the red lane appears to require a product change, that is a stop condition,
  not an invitation.

## 5. Scope

### In scope

- Diagnosing the `permission-list` failure, fixing its cause, and restoring the lane to green.
- Restoring the hosted workflow's `push` and `pull_request` triggers with the preserved path
  filters, and dropping "(paused)" from the workflow's name.
- Adding the missing `clippy -D warnings` step for the desktop Rust host.
- A single documented local entry point that runs the same checks the hosted lane runs.
- A new static guard proving the desktop tree is inside an automated gate, with a
  `--self-test`, wired into `scripts/verify.sh quick` and `.github/workflows/ci.yml`.
- `architecture/desktop.md`'s pause section, reconciled to the restored state.

### Out of scope

- Any change to the renderer's UI, layout, commands or keymap — M007-B, M007-C and M007-D.
- Any editor or file-tree work — M007-E.
- Any new `CoreRequest` variant, protocol change, storage change or migration.
- Any new renderer capability, Tauri plugin, or relaxation of
  `scripts/check-desktop-boundary.sh`.
- Any change to the TUI, the daemon's behaviour, or the shared client crate's semantics.
- Moving the desktop into the root Cargo workspace.
- Fixing the desktop's missing coverage of *behaviour* beyond what the lane already asserts —
  M007-B through M007-E own the new coverage.
- Windows qualification for the desktop. M002's Windows transport evidence remains outstanding
  and is not discharged here; this milestone does not make a Windows claim.

## 6. Required production changes

### CI and workflow

- `.github/workflows/desktop-e2e.yml` — restore the `push` and `pull_request` triggers with the
  path filters the file's own header preserves; rename the workflow to drop "(paused)"; add a
  `clippy` step for the desktop host using the pinned `1.90.0` toolchain; keep every existing
  step and its order.
- `.github/workflows/ci.yml` — add the new desktop-gate guard as a named step, in the same
  position the other guard steps occupy.

### Scripts

- `scripts/check_desktop_gate_wiring.py` — **new**, with `--self-test`. Proves that the desktop
  tree is covered by an automated gate: the workflow is not manual-dispatch-only, the path
  filters cover every path under `apps/desktop/` plus the shared-client and protocol paths the
  desktop consumes, and the desktop host has a lint step. The guard must fail closed.
- `apps/desktop/scripts/verify-desktop.sh` — **new**. One local entry point running the same
  checks the hosted lane runs, in the same order, so a developer's local run and the hosted run
  are the same gate. It must not require a display for the renderer and host checks; the
  built-app trajectory may require one and must say so.
- `scripts/verify.sh` — add the new guard to `run_guards()` with its `--self-test`.

### Product code

- The red lane's cause. Where the cause is in the renderer or the host, the fix is the
  smallest one that restores the documented behaviour. **Where the cause is the e2e assertion
  itself, the assertion may be replaced — but only by an assertion that proves the same
  behaviour.** Both the old and the new assertion, and the reason, go in the closure record.

### Documentation

- `architecture/desktop.md` — the pause section becomes a resumed-and-restored section, with
  the lane's run identifiers and the cause of the failure. The pause's history stays.
- `architecture/overview.md` — update any counted surface this milestone changes (the guard
  count, the CI guard count, the workflow count).
- `AGENTS.md` — the guard counts, if they change.

## 7. Ordered work packages

### WP-A — Establish the failure's cause

*Intent:* know what is actually broken before changing anything.

*Required changes:* none to production code. Reproduce the `permission-list` failure
locally — `CODEGG_DAEMON_EXECUTABLE=<path> apps/desktop/e2e/run-e2e.sh` against an isolated
phase home — and determine which of these is true: the pending permission never reaches the
desktop's projection view; it reaches the view but the render condition never becomes true
for that turn; or the e2e fixture's turn does not raise a permission at all in the current
daemon. Record the finding, with the evidence, before writing the fix. The three candidate
causes have different owners and only one is a desktop defect.

*Acceptance evidence:* a written diagnosis naming the cause, the evidence for it, and whether
the cause is in the desktop tree, in the shared client, or in the daemon.

### WP-B — Fix the cause and restore the lane

*Intent:* the lane is green for the right reason.

*Required changes:* the smallest change that restores the documented behaviour. If the fix is
in the renderer, the renderer's component tests must cover it. If the fix is in the host, the
host's unit tests must cover it. If the fix is in the e2e fixture or the assertion, the
replacement assertion must prove the same behaviour and the closure record must show the old
and the new assertion side by side.

*Acceptance evidence:* the built-app trajectory passes locally; the specific failing
assertion passes for the reason the diagnosis found; a regression test covers the cause.

### WP-C — Restore the hosted triggers

*Intent:* the qualification runs on the pull request again.

*Required changes:* re-add `push` and `pull_request` to `.github/workflows/desktop-e2e.yml`
with the path filters its header preserves, unchanged; drop "(paused)" from the workflow's
name; keep `workflow_dispatch`; keep the `ubuntu-24.04` runner, the Node 24 pin, the Rust
1.90.0 pin, the Tauri Linux dependencies, the boundary-guard step, the daemon build, the
renderer checks, the daemon staging, the host checks, the built-app trajectory and the log
upload. Change nothing else in that file.

*Acceptance evidence:* a hosted run on the pull-request head, green, with the `push` and
`pull_request` triggers visible in the workflow's own metadata.

### WP-D — Lint the desktop host

*Intent:* the host's Rust code is held to the same lint bar as the rest of the repository.

*Required changes:* add a `clippy` step to the host-checks step of the hosted workflow, using
the pinned toolchain — `rustup run 1.90.0 cargo clippy --manifest-path
apps/desktop/src-tauri/Cargo.toml --all-targets --locked -- -D warnings`. Expect findings:
this code has never been linted at `-D warnings`, and the fix may be several small changes.
**Do not** add `#[allow]` attributes to silence a finding without a written reason; fix the
finding, or record the one that cannot be fixed here and its severity.

*Acceptance evidence:* the step is green on the pull-request head, or every remaining
finding is recorded with a severity and an owner in the closure record.

### WP-E — One local entry point

*Intent:* the gate is runnable before the push, so "builds on the maintainer machine" is never
the closure evidence.

*Required changes:* `apps/desktop/scripts/verify-desktop.sh` running, in order: the boundary
guard, the renderer checks (`npm ci`, `npm run typecheck`, `npm test`, `npm run bindings:check`,
`npm run build`), the host checks (`cargo test --manifest-path
apps/desktop/src-tauri/Cargo.toml --locked --lib`, `cargo fmt --manifest-path
apps/desktop/src-tauri/Cargo.toml -- --check`, and the `clippy` call), and — only when a
display is available — the built-app trajectory. It must use the pinned toolchain and must
print which steps it skipped and why. The script must be referenced from the workflow, so the
hosted lane and the local entry point cannot drift.

*Acceptance evidence:* the script exists, is executable, runs the same step list as the
workflow, and its output names the skipped steps when a display is absent.

### WP-F — Guard the gate

*Intent:* the desktop tree cannot silently fall out of an automated gate again.

*Required changes:* `scripts/check_desktop_gate_wiring.py` with a `--self-test`, wired into
`scripts/verify.sh quick` and `.github/workflows/ci.yml`. The guard proves at least: the
workflow file exists and is not manual-dispatch-only; its `push` and `pull_request` triggers
carry path filters that include `apps/desktop/**` and the shared-client, protocol and daemon
paths the desktop consumes; the desktop host has a `clippy` step; and the local entry point
exists and is referenced by the workflow. The `--self-test` injects each defect class — a
dispatch-only trigger, a path filter that omits `apps/desktop/**`, a removed `clippy` step, a
removed local entry point — and proves the guard rejects each.

*Acceptance evidence:* the guard passes on the branch, its `--self-test` rejects every
injected defect class, and it is named as a step in both `scripts/verify.sh quick` and
`.github/workflows/ci.yml`.

### WP-G — Reconcile the documentation

*Intent:* the recorded pause and the actual state agree.

*Required changes:* `architecture/desktop.md`'s pause section becomes a restored section that
keeps the pause's history and adds the restoration, the lane's run identifiers, and the
failure's cause. `architecture/overview.md` and `AGENTS.md` carry the changed counts. The
roadmap's header and the registry's desktop row move from "paused" to "resumed 2026-10-24,
M007 line open" — **those two are the parent change and the implementing agent must not edit
them**; instead the closure record's §11 and §12 own the roadmap and registry updates.

*Acceptance evidence:* no document still says the hosted qualification is paused once the
lane is restored, and the pause's history is still recorded.

## 8. Failure, cancellation, restart, contention semantics

- **The hosted lane may be slow.** The built-app trajectory builds the whole app and runs
  under `xvfb`. A timeout on the hosted runner is a lane failure, not a pass; the run must
  report it.
- **A red lane must never be recorded as green.** If the lane cannot be made green because
  the cause is in the daemon or the shared client, that is a **stop condition**, and the
  milestone is `conditionally closed` at best with the condition named — not closed.
- **The local entry point may skip the built-app trajectory** when no display is available.
  It must say so explicitly, and the closure record must not claim the built-app trajectory
  ran locally if it did not.
- **The gate must fail closed.** If the workflow file is missing, or the triggers are
  dispatch-only, or the path filters no longer cover the desktop tree, the guard fails. A
  guard that passes when the gate is absent is worse than no guard.
- **Contention.** The hosted workflow uses `concurrency: group: desktop-e2e-${{ github.ref }}`
  with `cancel-in-progress: true`. Restoring the `push` trigger means a push to `main`
  supersedes the pull-request run for the same ref; the closure evidence must cite the run
  that actually concluded, not the one that was cancelled.

## 9. Compatibility and migration

- **None.** No protocol, storage, migration, or product-behaviour change is permitted. The
  e2e fixture's daemon surface is unchanged; the daemon binary the lane bundles is this
  revision's, as before.
- The only migration is operational: a workflow's triggers, a workflow's name, one added CI
  step, one added script, and possibly small renderer or host fixes for the lane's cause.
- If the lane's cause turns out to require a daemon behaviour change, that is a different
  milestone and a stop condition here.

## 10. Required tests

### Focused

- The regression test for the lane's cause, in the layer the cause lives: a renderer
  component test if the renderer, a host unit test if the host, or a fixture-level assertion
  if the fixture.
- A host unit test for any host change.
- A renderer component test for any renderer change.

### Integration

- The built-app trajectory, locally and hosted. This is the milestone's primary integration
  evidence and it is not optional.
- The 14 built-app tests across the three spec files must all pass, not just the one that was
  failing.

### Restart

- The lane's existing lifecycle and autostart specs already cover renderer reload, visible
  reconnect and native close. They must keep passing; this milestone adds no new restart
  scenario unless the diagnosis demands one.

### Contention

- None new. The hosted concurrency group already cancels superseded runs.

### Security

- `scripts/check-desktop-boundary.sh` must pass unchanged.
- The guard added in WP-F must be proven to fail closed (WP-F's acceptance).
- The e2e capability file must still be generated only for feature builds; the production
  `main` capability must remain the only committed capability.

### Migration

- None required. Assert instead that the root workspace's `cargo check` and the desktop
  workspace's `cargo test` remain independent and that the root MSRV is still 1.89.

## 11. Required verification commands

```bash
# the desktop boundary guards (already in quick)
./scripts/check-desktop-boundary.sh
./scripts/check-client-boundary.sh

# the new gate guard and its self-test
python3 scripts/check_desktop_gate_wiring.py
python3 scripts/check_desktop_gate_wiring.py --self-test

# the local entry point this milestone adds
apps/desktop/scripts/verify-desktop.sh

# the renderer, directly
cd apps/desktop && npm ci && npm run typecheck && npm test && npm run bindings:check && npm run build

# the desktop host, directly (its own workspace, its own pinned toolchain)
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --lib
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets --locked -- -D warnings

# the root workspace must be unaffected
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

Do not claim commands that were not actually run. If the built-app trajectory cannot run in
the local environment because no display is available, the hosted run is the evidence and the
closure record must say which steps ran locally and which only ran hosted.

## 12. Documentation updates

- `architecture/desktop.md` — replace "The hosted qualification is paused (2026-10-23)" with a
  restored section that keeps the pause's history and adds the restoration, the lane's run
  identifiers, and the failure's cause. Keep the deferred-surfaces list accurate: the editor
  and terminal remain unimplemented until M007-E.
- `architecture/overview.md` — update the counted surfaces this milestone changes. Read the
  file's counted numbers before editing; they include integration-test files, guard scripts and
  CI guard scripts.
- `AGENTS.md` — the guard counts, if they change. `AGENTS.md` states the guard list lives in
  `scripts/verify.sh` and must not be duplicated there.
- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` — **the parent change owns this
  edit.** The implementing agent must not edit the roadmap; the closure record's §11 owns the
  roadmap disposition.
- `plans/registry.md` — **the parent change owns this edit.** The closure record's §12 owns the
  registry updates.
- Do **not** edit `plans/000-long-term-specification.md`,
  `plans/001-terminology-and-domain-model.md`, `plans/002-long-term-roadmap.md`, ADR-0010,
  ADR-0011, ADR-0012, or any existing closure record.

## 13. Acceptance criteria

1. The built-app visible-window qualification is green on the pull-request head, and every one
   of its 14 tests passes — not only the one that was failing.
2. The cause of the `permission-list` failure is recorded, with the evidence, and the fix
   matches that cause. If the assertion itself changed, the closure record shows the old and
   the new assertion and why.
3. The hosted workflow runs on `push` and `pull_request` with the path filters its header
   preserved, and the workflow's name no longer says "(paused)".
4. The desktop Rust host is linted with `clippy --all-targets -- -D warnings` in the hosted
   lane, and that step is green — or every remaining finding is recorded with a severity and
   an owner.
5. One local entry point runs the same checks the hosted lane runs, and the hosted workflow
   references it so the two cannot drift.
6. A guard with a `--self-test` fails closed when the desktop tree falls out of an automated
   gate, and that guard runs in `scripts/verify.sh quick` **and** in `.github/workflows/ci.yml`.
7. `scripts/check-desktop-boundary.sh` and `scripts/check-client-boundary.sh` pass unchanged,
   and the desktop remains a separate Cargo workspace with the root MSRV at 1.89.
8. No protocol, storage, migration, or product-behaviour change landed.
9. The renderer's product behaviour and the TUI's behaviour are unchanged by this milestone.
10. `architecture/desktop.md` no longer describes the hosted qualification as paused, and the
    pause's history is still recorded.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- the lane's cause turns out to be in the daemon or the shared client crate rather than in
  the desktop tree — that is a different milestone's work;
- the lane cannot be made green without a product-behaviour change, a protocol change, or a
  relaxation of `scripts/check-desktop-boundary.sh`;
- restoring the triggers appears to require the desktop to join the root Cargo workspace —
  the boundary guard forbids it, and the separation is deliberate;
- the desktop host's `clippy` findings appear to require changing `codegg-client`'s semantics
  or the daemon's behaviour;
- the guard in WP-F cannot be made to fail closed — a guard that passes when the gate is
  absent must not ship;
- the local entry point would have to run with different checks than the hosted lane, because
  that is the drift the milestone exists to prevent;
- the `desktop-e2e` feature appears to need to be enabled in a production configuration;
- fixing the lane appears to pull in the renderer's layout, the command surface or the
  editor — M007-B, M007-D and M007-E own those;
- the milestone would expand into another subsystem roadmap.

## 15. Closure evidence required

The closure record at `plans/closure/desktop-frontend-ide-foundation/011-status.md` must
contain:

- implementation commits, and confirmation that no protocol, storage, migration, or
  product-behaviour change landed;
- the diagnosis of the `permission-list` failure, with its evidence, and the fix that
  followed from it;
- if the e2e assertion changed, the old and the new assertion side by side and the reason;
- a requirement-to-evidence matrix mapping each §13 acceptance criterion to a named test,
  guard, or command outcome;
- the full command list from §11 with actual outcomes, including any command that failed and
  its classification, and an explicit statement of which steps ran locally and which only ran
  hosted;
- the hosted run identifiers for the restored qualification on the pull-request head, with
  their conclusions, and the `clippy` step's outcome;
- the `--self-test` evidence for `scripts/check_desktop_gate_wiring.py`, showing that every
  injected defect class was rejected;
- confirmation that `scripts/check-desktop-boundary.sh` and
  `scripts/check-client-boundary.sh` pass unchanged and that the desktop is still a separate
  Cargo workspace;
- the documentation updates actually made, and an explicit statement if `README.md` or
  `architecture/overview.md` was deliberately not changed;
- unresolved findings classified by severity, with any medium or high finding keeping M007-A
  open;
- a recommendation of closed, conditionally closed, corrective pass required, or blocked.

Per `plans/003-planning-process.md` §2.5, a commit message asserting closure is not closure
evidence. A build that compiles is not closure.

## 16. Handoff notes

- **Authority order:** ADR-0010 and the M003/M004 closure records outrank this plan. Where
  repository evidence conflicts with this plan, preserve the canonical invariant, record the
  discrepancy, and make the smallest coherent adjustment.
- **Do not edit** the roadmap, the registry, `architecture/desktop.md`'s renderer-security
  section, `scripts/check-desktop-boundary.sh`, or any closure record. The roadmap and registry
  updates belong to this milestone's closure record.
- **The desktop is a separate Cargo workspace.** Invoke cargo against it with
  `--manifest-path apps/desktop/src-tauri/Cargo.toml`. Do not add it to the root workspace.
- **Environment:** the root workspace is Rust 1.89; the desktop workspace pins 1.90.0 through
  `apps/desktop/src-tauri/rust-toolchain.toml` and declares `rust-version = "1.90"`. Use the
  pinned toolchain for the desktop and the root toolchain for the root workspace. The
  desktop's own `Cargo.lock` is committed and must be updated with `--locked`-compatible
  changes only.
- **Node and the desktop tree:** `npm ci` is the install step; the lockfile is
  `apps/desktop/package-lock.json`. The desktop's Node dependency tree is confined to
  `apps/desktop`.
- **The built-app trajectory needs a display.** On Linux that is `xvfb-run -a`; on macOS it
  is a visible window. The local entry point must report a skipped built-app trajectory
  rather than passing it silently.
- **Preserve unrelated user changes.** If the working tree is not clean, do not revert
  anything you did not author.
- **No `--all-features`** on the root workspace; it pulls in real-LSP-server tests. Broad
  sweeps use `scripts/verify.sh full`, which pins `--features server,plugins,lsp-test-support`.
