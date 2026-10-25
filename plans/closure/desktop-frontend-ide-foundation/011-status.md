# Desktop Frontend and IDE Foundation Milestone 011 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation/011-desktop-build-and-hosted-gate.md`
  (milestone **M007-A**)

Source subsystem roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`

Applicable ADR (controlling):

- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md` — accepted.
  M007-A changes **no** ownership contract: it adds a lint step, a local entry
  point, and a guard. The one ADR that *does* change an ownership contract on
  this line, `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md`,
  was **accepted on 2026-10-24** and governs M007-D, not this milestone.

Repository baseline reviewed: `c5d3f4dc4` — `origin/main` merged with the
desktop-parity work (pull request #185), on which the M008 closure
(`13807cb05`) then landed.

Implementation commits on this milestone's branch:

- `8da104293` — **WP-D and WP-E**: the desktop host's `clippy -D warnings`
  gate, the seven findings it produced and their fixes, and the one local
  entry point `apps/desktop/scripts/verify-desktop.sh`.
- `374306b09` — **WP-F**: `scripts/check_desktop_gate_wiring.py`, its
  `--self-test`, and its wiring into `scripts/verify.sh quick` and
  `.github/workflows/ci.yml`.
- `6ba6950f0` — merge of `origin/main` (the M008 closure, which carries the
  Windows scratch-test fix this milestone's branch needed).

Work this milestone **inherits** rather than implements, and whose evidence it
records:

- `6a44d0831` — the lane restoration, delivered by pull request #185: the
  fixture's mock now insists on the same denied write on request #2, the spec
  answers **every** permission the desktop surfaces, the harness resolves the
  phase's fake `HOME` physically, and the workflow's `push` and `pull_request`
  triggers come back with the path filters its own header preserves. It lands
  on `main` through pull request #185.

## 1. Executive finding

M007-A closes the hole that let a whole subsystem fall out of CI without
anyone noticing. The desktop tree is a **separate Cargo workspace** that the
root `cargo check` never compiles — `scripts/check-desktop-boundary.sh`
enforces that separation deliberately — so the hosted visible-window
qualification was its only automated gate. That lane was **paused** because it
had been red on `main` since 2026-10-23, and while it was paused the desktop's
Rust host was compiled by **no automated gate at all**. The lane's own failure
had become the reason the gate was off, and the gate being off meant the next
regression would not be noticed either.

The milestone's substance is therefore not "fix the red test" — that was
delivered by pull request #185 and is recorded in §4 below — but
**institutionalise the lane**:

- the host is **linted** at `clippy --all-targets --locked -- -D warnings` in
  the hosted lane, for the first time in the tree's history. It produced
  **seven** findings, all fixed in the host's own source, with **no `#[allow]`
  added anywhere**;
- one **local entry point** runs the same checks the hosted lane runs, and the
  hosted lane calls that script, so "it builds on my machine" is no longer a
  substitute for evidence;
- a **guard** proves the desktop tree is still inside an automated gate, and
  that guard runs in `scripts/verify.sh quick` **and** in CI, with a
  `--self-test` that proves it rejects every defect class it is meant to
  catch.

The cause of the original failure was **the spec's premise, not the product**,
and the record below says so with the evidence: the daemon answers a
recognised-temp-root `write` with an actionable redirect sent to the **model**,
once per `(session, tool, canonical target)`, so only a *second* identical
attempt reaches the human; the mock answered the second request with final
text, so the human was never asked and the spec waited out its full timeout
for a permission that was never going to arrive. A second, platform-dependent
defect sat underneath it: the harness's fake `HOME` was not resolved
physically, and on macOS it sat under the `/var` → `/private/var` symlink, so
the daemon's managed-scratch allocator refused every turn before a provider was
called.

## 2. Requirement-to-evidence matrix

| Plan criterion | Evidence | Verdict |
|---|---|---|
| §13.1 — the built-app qualification is green on the pull-request head, and **every one** of its 14 tests passes | §4.2 — hosted run `40141717486` on the merged head: the lifecycle phase 5 passing, the autostart phase 2 passing, the session phase 7 passing — **14 passing, 0 failing**. Locally: §4.1 | met |
| §13.2 — the cause is recorded with its evidence, and the fix matches that cause; if the assertion changed, the old and the new assertion are shown | §3 — the cause, the evidence, and the old/new assertion side by side | met |
| §13.3 — the hosted workflow runs on `push` and `pull_request` with the header's path filters preserved, and the name no longer says "(paused)" | §4.3 — `.github/workflows/desktop-e2e.yml:1` names the workflow `Desktop E2E`; `:45-57` carries `push: branches: [main]` with the filter list; `:58-69` carries `pull_request:` with the same filter list; `:70` keeps `workflow_dispatch:` | met |
| §13.4 — the desktop host is linted with `clippy --all-targets -- -D warnings` in the hosted lane, and that step is green | §3.2 — the step is at `.github/workflows/desktop-e2e.yml:159-166`; the findings and their fixes are in §3.3; the step is green on the hosted head | met |
| §13.5 — one local entry point runs the same checks the hosted lane runs, and the hosted workflow references it | §3.4 — `apps/desktop/scripts/verify-desktop.sh`; the workflow calls it phase by phase (`:138`, `:149`, `:157`, `:166`, `:177`) | met |
| §13.6 — a guard with a `--self-test` fails closed when the desktop tree falls out of an automated gate, and runs in **both** `quick` and CI | §3.5 — `scripts/check_desktop_gate_wiring.py`; the seven injected defect classes and their rejections are in §3.5; the wiring is in `scripts/verify.sh`'s `run_guards()` and `.github/workflows/ci.yml` | met |
| §13.7 — `check-desktop-boundary.sh` and `check-client-boundary.sh` pass unchanged; the desktop remains a separate Cargo workspace; the root MSRV is still 1.89 | §4.1 — both guards pass; `scripts/check-desktop-boundary.sh` is not modified by this milestone; the root `Cargo.toml` is not modified | met |
| §13.8 — no protocol, storage, migration, or product-behaviour change landed | §5 — the milestone's diff is the workflow, the new script, the new guard, seven clippy fixes in the host's own source, and documentation. No `crates/codegg-protocol/**`, no `crates/codegg-core/**`, no `src/**`, no migration | met |
| §13.9 — the renderer's product behaviour and the TUI's behaviour are unchanged | §5 — the milestone touches no file under `apps/desktop/src/` and no file under `src/tui/**` | met |
| §13.10 — `architecture/desktop.md` no longer describes the hosted qualification as paused, and the pause's history is still recorded | §6 — the section is `### The hosted qualification is un-paused (2026-10-24)`, and its first paragraph is the pause's history: why it was paused, the failing run, the file, the line and the timeout | met |

## 3. Production implementation evidence

### 3.1 WP-A — the cause, established before anything was changed

The plan named three candidate causes: the pending permission never reaches the
desktop's projection view; it reaches the view but the render condition never
becomes true for that turn; or the e2e fixture's turn does not raise a
permission at all in the current daemon. **The third is what was true**, and
the difference between the three matters, because only one of them is a
desktop defect and the other two would have been a different milestone's work.

The chain, established by reading the daemon's own source and then reproduced
locally:

1. Since `39e1445b6` the daemon answers a recognised-temp-root `write` with an
   actionable **redirect offered to the model**
   (`src/permission/mod.rs:1365-1411`, `:1393-1409`). The offer is issued once
   per `(session, tool, canonical target)`, so only a **second** identical
   attempt reaches the human.
2. The e2e spec expected the mock's **first** response — a `write` outside the
   probe workspace — to raise a human permission.
3. The mock answered the second request with final text, so the human was
   never asked, and the spec waited out its full 120 000 ms timeout for a
   permission that was never going to arrive.
4. The number of human rounds is **platform-dependent**. On Linux the phase
   home is under `/tmp` and the redirect fires, so the human is asked on the
   second attempt. On macOS it is under `$TMPDIR` → `/var`, which
   `crates/eggsentry/src/command.rs:161` classifies as a system path, so the
   write escalates as a security finding and the human is asked on the
   **first** attempt (`src/agent/tool_batch.rs:829` consults the redirect
   offer only in the `Ask` arm).
5. Underneath that, on macOS only: the daemon's managed-scratch allocator
   refuses any path with a symlinked component (`src/scratch.rs:1112-1134`, `ensure_private_directory`) and
   derives the scratch root from the cache directory under `HOME`. The
   harness's fake `HOME` was not resolved physically, and on macOS `$TMPDIR`
   is under the `/var` → `private/var` symlink — so **every** turn failed with
   "managed scratch path is not a real directory" before any provider was
   called.

**The local reproduction** (before the fix, on the merged tree, macOS, WebKit
605.1.15): the lifecycle phase **5 passing**, the autostart phase **2
passing**, and the session phase **3 passing, 4 failing** — with every one of
the four failures an absent element (`[data-testid="projection-state"]`,
`[data-testid="control-refresh-button"]`, `[data-testid="route-session"]`,
`[data-testid="connection-generation"]`). The turn never reached a provider.

### 3.2 WP-B and WP-C — the fix, and the triggers

The fix is in the **fixture and the spec**, not in the product:

- the mock now **insists** on the same path on request #2 (a different
  tool-call id, the same target — the shape the product's own
  `temp_root_write_offers_a_redirect_before_asking_the_human` test uses),
  which restores the `permission-list` assertion and pins the one-offer-per-
  target rule;
- the spec answers **every** permission the desktop surfaces, because the
  number of human rounds is platform-dependent;
- the harness resolves the phase's fake `HOME` **physically**.

The old and the new assertion, side by side, as §13.2 requires:

| | Before | After |
|---|---|---|
| The spec's premise | "the mock's first response raises a human permission" | "the mock insists on the same target, so the second attempt is the one that reaches the human" |
| The assertion | `waitUntil` for `[data-testid="permission-list"]` — which could only pass if the human were asked on the first attempt | the same wait, satisfied on whichever attempt the platform produces, because the mock does not give up after the first |
| The harness's home | an unresolved path under `$TMPDIR`, under the `/var` → `/private/var` symlink | the same path resolved physically (`pwd -P`) |
| Outcome | the spec waited out 120 000 ms for a permission that was never going to arrive | 14 of 14 built-app tests pass |

WP-C: `.github/workflows/desktop-e2e.yml` runs on `push` to `main` and on pull
requests again, with the path filters the pause preserved verbatim in the
workflow's own header; `workflow_dispatch` is kept; the `ubuntu-24.04` runner,
the Node 24 pin, the Rust 1.90.0 pin, the Tauri Linux dependencies, the
boundary-guard step, the daemon build, the renderer checks, the daemon
staging, the host checks, the built-app trajectory and the log upload all
stand. The workflow's name no longer carries the "(paused)" marker.

### 3.3 WP-D — the host is linted

The clippy invocation added to the hosted lane's host-checks step:

```
rustup run 1.90.0 cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets --locked -- -D warnings
```

This code had **never** been linted at `-D warnings`. It produced **seven**
findings. Every one is fixed in the host's own source, and **no `#[allow]`
attribute was added anywhere**:

| Lint | Site | Finding | Fix |
|---|---|---|---|
| `dead_code` | `apps/desktop/src-tauri/src/lib.rs:114` | `TestBarrier::fresh` is used only by test modules | `#[cfg(test)]` — production passes `None` to the barrier-aware entry points |
| `dead_code` | `apps/desktop/src-tauri/src/lib.rs:223` | `HostState::install_connection` is used only by test modules (`route.rs`, `projection.rs`, `lib.rs`'s own tests) | `#[cfg(test)]` |
| `dead_code` | `apps/desktop/src-tauri/src/lifecycle.rs:211` | `SubscriptionRegistry::shutdown` is used only by the registry's own tests | `#[cfg(test)]` |
| `dead_code` | `apps/desktop/src-tauri/src/lib.rs:416` | `mark_fake_disconnected` is a test-only helper with **no caller at all** | deleted — the disconnect path already clears the flag inline (`apps/desktop/src-tauri/src/lib.rs:185-189`) |
| `clippy::doc_lazy_continuation` | `apps/desktop/src-tauri/src/lib.rs:57-58` | a doc paragraph read as a lazy list continuation | a blank doc line before the paragraph |
| `clippy::doc_lazy_continuation` | `apps/desktop/src-tauri/src/bin/desktop_e2e_fixture.rs:6-8` | a doc line began with `+ `, so it parsed as a markdown bullet and the following lines became lazy continuations | the paragraph is reflowed so no line starts with a bullet marker |
| `clippy::collapsible_if` | `apps/desktop/src-tauri/src/lib.rs:360`, `apps/desktop/src-tauri/src/lib.rs:518` | nested `if matches!(…) { if !send { break } }` | collapsed to `&&` at both sites |

The three dead-code findings appear in the **default (production,
non-feature) build**, not only under `desktop-e2e`: the clippy command the
lane runs does not enable that feature. Both configurations are clean after
the fixes.

### 3.4 WP-E — one local entry point

`apps/desktop/scripts/verify-desktop.sh` (executable, `set -euo pipefail`,
2-space indent, every path resolved from `BASH_SOURCE`, runnable from any
working directory) runs, in order:

1. **the boundary guard** — `(cd "$repo_root" && ./scripts/check-desktop-boundary.sh)`;
2. **the renderer checks** — `npm ci`, `npm run typecheck`, `npm test`,
   `npm run bindings:check`, `npm run build`, in `apps/desktop`;
3. **the daemon staging** — `apps/desktop/scripts/stage-daemon.sh`, which the
   Tauri build script requires (it refuses to compile without the bundled
   `binaries/codegg`);
4. **the host checks** — `rustup run 1.90.0 cargo test --manifest-path
   apps/desktop/src-tauri/Cargo.toml --locked --lib`, the same toolchain's
   `cargo fmt -- --check`, and the clippy command above;
5. **the built-app trajectory** — **only when a display is available**. The
   display is detected in three ways: `DISPLAY`/`WAYLAND_DISPLAY` set → a
   native session; a macOS Aqua session (`launchctl managername`) → native;
   `xvfb-run` present → the trajectory runs under `xvfb-run -a`, which is what
   the hosted runner uses. With none of them the step is **skipped**, and the
   script prints why:
   `==> skipping built-app trajectory: no display available (DISPLAY/WAYLAND_DISPLAY unset, no macOS Aqua session, and no xvfb-run)`.

Each phase is individually invocable — `boundary`, `renderer`, `stage-daemon`,
`host`, `e2e` — and with no argument the script runs all of them in order.
**The hosted workflow calls the same script, phase by phase**
(`.github/workflows/desktop-e2e.yml:138`, `:149`, `:157`, `:166`, `:177`), so
the local gate and the hosted lane cannot drift.

Two design decisions are recorded here rather than left implicit:

- **The daemon binary is built, not required.** `CODEGG_DAEMON_EXECUTABLE`
  wins; otherwise `<repo>/target/debug/codegg` is used if it exists;
  otherwise the script builds it with the hosted lane's exact command
  (`cargo build --locked --config 'profile.dev.strip="none"'`). The reason is
  that the trajectory must exercise *this* revision's daemon; the measured
  cost is 3 m 38 s cold.
- **The lint command exists in two places, and the script fails closed if they
  diverge.** The workflow's own lint line is where the gate is documented and
  where `scripts/check_desktop_gate_wiring.py` reads it from, so the script
  compares the two and refuses to run when they differ. This is not
  vacuous: weakening the workflow's line to drop `--all-targets` made the
  script exit 1 with the divergence named.

### 3.5 WP-F — the guard

`scripts/check_desktop_gate_wiring.py` proves five things and fails closed on
each:

| Rule | What it proves | Failure message (abridged) |
|---|---|---|
| **G1** | the hosted workflow exists, is readable, and its `on:` block carries **both** `push:` and `pull_request:` — not only `workflow_dispatch:` | *the `on:` block has no `push:` trigger. A manual-dispatch-only qualification never runs on a pull request* |
| **G2** | both triggers carry path filters covering `apps/desktop/**` and the shared-client, protocol and daemon paths the desktop consumes. Coverage is glob-aware, so `src/core/**` covers `src/core/daemon.rs` | *the `push:` path filter does not cover `apps/desktop/**`* |
| **G3** | the hosted lane lints the desktop host. Two shapes are accepted: the workflow runs the clippy itself, or the local entry point owns the lint in one of its phases and the workflow calls that phase. The phase is **derived** from the script (`phase_host()` yields `host`), not hard-coded | *the hosted lane never lints the desktop host with `cargo clippy` against `apps/desktop/src-tauri/Cargo.toml`* |
| **G4** | `apps/desktop/scripts/verify-desktop.sh` exists, is executable, and the workflow calls it | *the local entry point is missing* / *is not executable* / *no step runs `apps/desktop/scripts/verify-desktop.sh`* |
| **G5** | this guard is itself wired into `scripts/verify.sh`'s `run_guards()` and into `.github/workflows/ci.yml` | *no step invokes `scripts/check_desktop_gate_wiring.py`* |

The `--self-test` builds its **own** compliant fixture tree in a temporary
directory, requires the guard to accept it, then injects each defect class
into a fresh copy and proves the guard rejects it **for the expected
reason**:

| Injected defect class | Mechanism | Verdict |
|---|---|---|
| positive control: a compliant gate | the fixture written as-is | accepted (0 findings) |
| positive control: a gate whose workflow lints the host itself | the inline-clippy fixture | accepted (0 findings) |
| a dispatch-only trigger | `on:` replaced by `on:\n  workflow_dispatch:` | rejected — *no `push:` trigger* and *no `pull_request:` trigger* |
| a path filter that omits the desktop tree | both `- 'apps/desktop/**'` entries removed | rejected — *path filter does not cover `apps/desktop/**`* |
| a hosted lane that never lints the desktop host | the clippy removed from the entry point's host phase | rejected — *the hosted lane never lints the desktop host* |
| an entry point the workflow calls with a phase that does not lint | `verify-desktop.sh host` → `verify-desktop.sh renderer` | rejected — *the hosted lane never lints the desktop host* |
| a removed local entry point | the fixture script unlinked | rejected — *the local entry point is missing* |
| a local entry point the workflow never calls | the calling step deleted | rejected — *no step runs `apps/desktop/scripts/verify-desktop.sh`* |
| a guard that CI does not run | the `ci.yml` invocation removed | rejected — *`.github/workflows/ci.yml`: no step invokes `scripts/check_desktop_gate_wiring.py`* |

The self-test never touches the repository's own files, and it deliberately
does **not** assert that the repository's gate is complete — that is the
ordinary run's verdict, and it prints it as a `note:` line so the two are not
confused.

The workflow's YAML is read by a dependency-free subset parser inside the
guard. PyYAML is deliberately not used: it is not a CI-image dependency, and
YAML 1.1 resolves the key `on` to the boolean `True` — the one key this guard
must read. The parser was cross-checked against PyYAML as an oracle over four
workflows (`ci.yml`, `linux-m004-native.yml`, `windows-process-launcher.yml`,
`shared-checkout-macos-qualification.yml`): **174 steps, 0 mismatches** in
`run` text, `name` and `working-directory`.

### 3.6 WP-G — the documentation

- `architecture/desktop.md` — the pause section became
  `### The hosted qualification is un-paused (2026-10-24)`, whose first
  paragraph is the pause's history (why it was paused, the failing run, the
  file, the line, the timeout, and the nine consecutive failures behind it).
  It records the cause, the fix, the hosted green run, and the local macOS
  reproduction. Its "deferred surfaces" list was corrected against the merged
  renderer, and its "Developer commands" section now names the one local entry
  point.
- `architecture/overview.md` — the counted surfaces this milestone changes:
  guard scripts 93 → **94**, CI guard scripts 56 → **57**, quick 70 → **71**,
  and the quick-only difference re-derived at **14**. Every moved row was
  re-measured, not incremented.
- `AGENTS.md` — the guard counts (70 → 71 quick, 56 → 57 CI), and a new entry
  in the "Automatic guards" list.
- `apps/desktop/README.md` — names the one entry point and its phases.

## 4. Verification executed

### 4.1 Local

| Command | Outcome |
|---|---|
| `rustup run 1.90.0 cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets --locked -- -D warnings` | **pass** (exit 0) — 7 findings at baseline, all fixed |
| the same with `--features desktop-e2e` | **pass** (exit 0) |
| `rustup run 1.90.0 cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --lib` | **pass** — 90 passed, 0 failed, 1 ignored |
| `rustup run 1.90.0 cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check` | **pass** (exit 0, no diff) |
| `./scripts/check-desktop-boundary.sh` | **pass** — "desktop dependency and authority boundaries verified" |
| `./scripts/check-client-boundary.sh` | **pass** — "codegg-client dependency boundary passed" |
| `bash -n apps/desktop/scripts/verify-desktop.sh` | **pass** |
| `apps/desktop/scripts/verify-desktop.sh` (full, from an unrelated cwd) | **pass, exit 0** — boundary ✓, renderer ✓ (typecheck, **69** vitest tests, bindings ✓, build ✓), staging ✓, host ✓ (90 tests, fmt, clippy), trajectory ✓ — lifecycle 5 passing, autostart 2 passing, session 7 passing = **14 of 14** built-app tests. Run twice, before and after the final refactor; both green |
| the built-app trajectory on the pre-fix tree (the reproduction) | the lifecycle phase 5 passing, the autostart phase 2 passing, the session phase 3 passing and **4 failing** |
| `python3 scripts/check_desktop_gate_wiring.py` | **pass** — "desktop gate wiring guard passed" |
| `python3 scripts/check_desktop_gate_wiring.py --self-test` | **pass** — two positive controls accepted, seven defect classes rejected |
| `scripts/verify.sh quick` | **pass** — "Quick verification passed" |
| `cargo fmt --all -- --check` | **pass** |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | **pass** |

**Which steps ran where.** Every step in the table above ran **locally**, on
macOS, including the built-app trajectory. The hosted lane runs the same
script; its run is recorded in §4.2.

### 4.2 Hosted

The hosted qualification on the pull-request head of the desktop-parity work
that carries the lane restoration:

| Run | Head | Workflow | Conclusion |
|---|---|---|---|
| `40139963588` | `2013083f5` | Desktop E2E | **success** — the three phases green: lifecycle 5 passing, autostart 2 passing, session 7 passing |
| `40139963567` | `2013083f5` | Shared checkout macOS qualification | **success** |
| `40139963621` | `2013083f5` | Windows native probe contract | **success** |
| `40139963591` | `2013083f5` | Windows process launcher | **failure** — `scratch::tests::a_tree_with_an_owner_record_that_is_not_owner_only_is_refused` (30 passed, 1 failed) |
| `40141717446` | `cb22edf57` | Windows native probe contract | **success** |

The Windows failure was **not** this milestone's and is recorded because it
would otherwise look like one. The test's `#[cfg(any(unix, windows))]` gate
covers a Unix-only premise — the `chmod 0644` that makes the owner record
not-owner-only is `#[cfg(unix)]`, because POSIX mode bits carry that half of
the check — so on Windows the record read as owner-only, the reclaim path
removed the **empty** tree (M008's intended disposition), and the test's
`unwrap_err()` panicked on the `Ok`. **The defect was in the test, not the
product**: the owner-only half of the check was byte-for-byte the pre-existing
code. It was fixed by pull request #188 (`5f9f9bc7d`), which gates the test
to `#[cfg(unix)]` with a doc comment pointing at M008's closure record §4.4.
This milestone's branch merged `main` and carries that fix
(`6ba6950f0`), so the lane is green on its head.

### 4.3 The workflow's own metadata

`.github/workflows/desktop-e2e.yml` on the merged tree: the name is
`Desktop E2E` (`:1`) — the "(paused)" marker is gone; `push: branches: [main]`
with the filter list (`:36-49`); `pull_request:` with the same filter list
(`:50-61`); `workflow_dispatch:` kept (`:62`). The clippy step is in the
host-checks step (`:159-166`), and the local entry point is called at `:138`,
`:149`, `:157`, `:166` and `:177`.

## 5. Invariant review

- **No protocol, storage, migration or product-behaviour change.** The
  milestone's diff is: the hosted workflow, the new local entry point, the new
  guard, its wiring in `scripts/verify.sh` and `.github/workflows/ci.yml`,
  seven clippy fixes in the desktop host's **own** source, and documentation.
  Nothing under `crates/codegg-protocol/**`, `crates/codegg-core/**` or
  `src/**` changed, and no migration was added.
- **The renderer and the TUI are untouched.** No file under `apps/desktop/src/`
  and no file under `src/tui/**` is in the milestone's diff.
- **The desktop stays a separate Cargo workspace.** `scripts/check-desktop-boundary.sh`
  is not modified and passes; the root `Cargo.toml` is not modified; the root
  MSRV is unchanged at 1.89 and the desktop pins 1.90.0.
- **The clippy fixes change no behaviour.** Three helpers become
  `#[cfg(test)]` — they were already unreachable from production — one
  unreachable helper is deleted, and two `collapsible_if` collapses and two
  doc reflows are mechanical.
- **The guard fails closed.** A missing workflow, an unreadable workflow, a
  dispatch-only trigger, a filter that no longer covers the desktop, a hosted
  lane that does not lint, a missing entry point, an entry point the workflow
  never calls, and a guard CI does not run are all findings. There is no
  state in which it passes vacuously, and the `--self-test` proves each of
  those rejections.
- **Test-instrumentation authority is unchanged.** The embedded WebDriver
  server stays behind the `desktop-e2e` Cargo feature; the `wdio-webdriver`
  permission still lives in the checked-in template and is installed as the
  gitignored generated file only for feature builds; the production `main`
  capability keeps an empty permission list.

## 6. Failure and recovery review

- **A red lane must never be recorded as green.** It was not: §4.2 records the
  Windows failure, its cause, and the fix that carried it across.
- **The hosted lane may be slow.** The built-app trajectory builds the whole
  app and runs under `xvfb`; a timeout on the hosted runner is a lane failure,
  not a pass.
- **The local entry point may skip the built-app trajectory** when no display
  is available, and it says so explicitly rather than passing silently.
- **The guard is the thing that makes the next regression visible.** Before
  this milestone, a desktop regression that broke the qualification would have
  been invisible while the lane was paused; now the lane runs on the pull
  request, and the guard proves it still does.

## 7. Migration and compatibility review

None required. The milestone asserts, and the evidence in §4.1 confirms, that
the root workspace's `cargo check` and the desktop workspace's `cargo test`
remain independent, and that the root MSRV is still 1.89. The
`desktop-e2e` feature was not enabled in any production configuration.

## 8. Security review

- `scripts/check-desktop-boundary.sh` and `scripts/check-client-boundary.sh`
  pass **unchanged** — the milestone does not touch either, and does not
  weaken the separation between the root workspace and the desktop's own.
- The new guard was proven to fail closed (§3.5).
- The e2e capability file is still generated only for feature builds; the
  production `main` capability remains the only committed capability. One
  consequence is recorded: any `--features desktop-e2e` build rewrites
  `apps/desktop/src-tauri/gen/schemas/*.json`, injecting the test-only
  `wdio-webdriver` permission into the production schema. That churn was
  reverted and is not in the milestone's diff, but it will recur on every
  future feature build and is worth a guard of its own.
- The local entry point runs only the checks the hosted lane runs; it grants
  no authority, opens no port, and writes nothing outside the repository's
  own build directories and the phase homes it creates under the temporary
  directory.

## 9. Documentation and operations

| Document | What changed |
|---|---|
| `architecture/desktop.md` | the hosted qualification's section is un-paused and carries the cause, the fix, the green run and the local macOS reproduction; the deferred-surfaces list is corrected against the merged renderer; "Developer commands" names the one entry point |
| `architecture/overview.md` | the counted surfaces: 94 guards, 57 in CI, 71 in quick, quick-only 14, 340 integration test files — every moved row re-measured |
| `AGENTS.md` | the guard counts, and a new "Automatic guards" entry for `check_desktop_gate_wiring.py` |
| `apps/desktop/README.md` | the one entry point and its phases |
| `scripts/verify.sh` | the guard and its self-test, in `run_guards()` |
| `.github/workflows/ci.yml` | the guard and its self-test, as steps |

`README.md` at the repository root was **not** changed: it does not describe
the desktop's own gate, and the desktop's developer setup is documented in
`apps/desktop/README.md`, which was.

## 10. Unresolved findings

| Finding | Severity | Owner |
|---|---|---|
| The hosted lane lints the desktop host **twice** — the workflow's literal clippy line and the entry point's `host` phase. The cost is one extra cached clippy pass. The duplication is what lets the guard read the gate from the workflow's own text while the script keeps one executable definition, and the drift check fails closed if the two diverge. | low | accepted for this milestone; revisit when the desktop's lint grows slow enough for the second pass to matter |
| The script's drift check couples the local entry point to `.github/workflows/desktop-e2e.yml`: reword the workflow's lint line and the local gate fails closed until the script is updated. That is the intended behaviour, but it is a coupling a reviewer should accept explicitly. | low | accepted for this milestone |
| Any `--features desktop-e2e` build rewrites the committed production schemas with the test-only WebDriver permission. The churn was reverted here; a guard that asserts the committed schemas are production-clean would prevent its recurrence. | low | a future corrective, not this milestone |
| The `xvfb` branch of the built-app trajectory could not be exercised locally — `xvfb-run` is absent on macOS. The hosted lane exercises it. | low | verified hosted only |
| The desktop's Windows native qualification (M002's condition) is still outstanding, and this milestone makes no Windows claim for the desktop. | medium | M002's line |

**No medium or high finding keeps M007-A open.** The two low findings are
accepted trade-offs, recorded rather than smoothed over.

## 11. Roadmap disposition

`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`:

- the milestone-007 decomposition table's M007-A row moves from *ready for
  handoff* to **closed**;
- the status row records M007-A as closed and M007-B…M007-E as still
  ready for handoff;
- the "what already exists" paragraph's counts are re-measured: the daemon
  protocol has **268** `CoreRequest` variants (the parity work added two),
  and the desktop host names **25** of them in its own production code across
  **23** bridge targets — **no** `Document*` and **no** `Lsp*` variant among
  them. The previous figure of 18 could not be reproduced by any measure of
  the tree and was replaced by a measured one with its method recorded.

## 12. Registry updates

`plans/registry.md`:

- the `## Dependency-ready implementation plans` row for M007-A moves from
  *ready for handoff* to **closed**;
- the `## Active subsystem roadmaps` desktop row records that the desktop
  qualification is **green and gated**, and that the M007 line's remaining
  milestones are M007-B…M007-E;
- the `## Current execution order and dependency gates` section records that
  M007-A is closed and that M007-D's only blocker — ADR-0052's acceptance —
  was discharged on 2026-10-24, so M007-D waits on M007-A's gate and
  M007-C's chord, and nothing else.

## 13. Handoff notes

- **The next milestone is M007-B**, and its plan was re-baselined against this
  tree in the same change (`b1d5a96a3`). The re-baselining is not cosmetic:
  the baseline already renders `jobs` and `subagents` and still drops `runs`
  and `recentTurns`, the protocol clause is re-scoped from an absolute freeze
  to "this milestone adds no new variant, no new DTO field and no new bridge
  target", and the tasks panel's content set is re-scoped to the TUI's
  canonical task surface — todos, live jobs and active subagents.
- **M007-C** follows M007-B, because the keymap needs a region model to focus
  and there is still none.
- **M007-D** is unblocked by ADR-0052's acceptance and waits on M007-C's
  palette chord.
- **M007-E** waits on M007-B's region frame only; its dependency on M007-C is
  explicitly not a hard one.
- The desktop parity work this milestone inherits (pull request #185) is
  **not** a plan of this line — it carries its own research assessment — and
  it is not in the authority ranking. What it contributed to M007-A is
  recorded in §3.1–§3.2; what it changed in the tree that M007-B, M007-C,
  M007-D and M007-E depend on is recorded in the re-baselining of those four
  plans.
