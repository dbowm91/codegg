# Desktop Frontend and IDE Foundation Corrective C003 — Closure Status

Status: closing (hosted `CI / verify` reconciliation is GREEN; built-app
`Desktop E2E` verdict pending on the latest harness fix)

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation-corrective/003-hosted-ci-visible-window-strict-closure.md`

Source subsystem roadmaps:

- `plans/subsystems/desktop-frontend-ide-foundation-m003-lifecycle-corrective-addendum.md`
- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` (M003)

Repository baseline reviewed: `3f222c2e5f957efdbb6a530276ff5b63a095e435`

Implementation commits:

- `eac14e01` — Desktop C003: hosted-CI reconciliation harness and built-app visible-window E2E
- `e2c954c9` — Desktop E2E workflow: install ripgrep for the boundary guard (hosted-only fix; no production/harness behavior change)
- `292c7d7f` — Desktop E2E workflow: stage the daemon resource before host checks (hosted-only fix; fresh checkouts lack the gitignored bundle resource)
- `3512d5d8` — Desktop E2E harness: resolve spec paths absolutely from the config dir (hosted-only fix; config-relative literals silently matched zero specs)
- `b515a8fb` — Desktop E2E harness (temporary): diagnostic page-state probe first
- `7244865c` — Desktop E2E harness: run WebdriverIO from src-tauri for frontendDist (superseded: binary is asset-embedded, CWD-independent; kept the absolute spec paths)
- `a510c415` — Desktop E2E harness: embed the production bundle via TAURI_CONFIG devUrl strip (real root cause of `about:blank`: debug + devUrl ⇒ empty assets + vite-server URL)
- `0f34a3c0` — Desktop E2E harness: socket fixture plus per-phase WebdriverIO runs (app spawns once per invocation; worker stdio ownership impossible; session env never reaches the app)
- `4ce1b623` — Desktop E2E trajectory: synchronize catalog mutations with installed subscriptions (display-only subscription-id seam; a broadcast with no installed subscriber is lost, not queued)
- `52650323` — Desktop E2E trajectory: in-page host-vs-renderer diagnostics plus daemon logs (rebase of the diagnostic commit onto the Eggplan bump; content identical)
- `e86f4d36` — Desktop E2E trajectory: daemon-side catalog read plus full diagnostic capture (fixture `project_list`, host-vs-daemon DIAG, fixture server log artifact)
- `1e31e8c9` — Desktop E2E fixture: workspace per probe plus probe archival on shutdown (1-workspace-1-project binding; machine-global catalog tidied on shutdown)
- `84ae31af` — Desktop E2E harness: per-phase WebDriver ports (4445/4446) and reload-mutation diagnostics (one app instance serves one port per invocation)
- `(this commit)` — Desktop E2E trajectory: hidden `e2e-anchor` window (desktop-e2e only) + `windowLabel: main` session pin so the native-close probe survives its own window destroy; idempotent fixture probe seeding across daemon restarts (same-named reuse, no duplicate rows); strict host≡daemon catalog convergence (presence + count + uniqueness) in both specs; per-phase launcher-log preservation

Hosted runs (latest):

- `CI / verify` run `37073506756` on `3512d5d8` — SUCCESS (reconciliation verdict; PTY test passed)
- `Desktop E2E` run `37089690924` on `84ae31af` — FAILURE (post-mortem in §10: closeWindow-on-last-window kills the embedded session; re-`start` stacked a same-named probe — both fixed by this commit; next run pending)

## 1. Executive finding

DRAFT. C003 lands the two strict-closure obligations as executable,
repeatable machinery rather than one-off transcripts:

1. Hosted root-CI reconciliation is recorded without weakening any test
surface: run `37062733033` is classified as an unrelated PTY-environment
flake with three-way evidence (identical prior signature with green rerun,
zero interactive-process files in the desktop delta, 5/5 local passes), the
test is untouched, and a fresh hosted run on the C003-containing mainline is
the reconciliation verdict (pending).
2. The one-off "visible WebView evidence" requirement is replaced by a
checked-in built-app WebDriver trajectory (WebdriverIO + embedded provider,
test-only `desktop-e2e` feature, deterministic Rust fixture, dedicated
path-gated workflow) driving the real Tauri window through existing-daemon
reuse, visible reconnect, real IPC `Channel` invalidation, renderer reload,
native close, TUI coexistence, explicit autostart, and daemon survival.

## 2. Requirement-to-evidence matrix

| Requirement (plan §) | Evidence | Result | Notes |
|---|---|---|---|
| Hosted disposition without weakening the PTY test (§2, §9) | §4 + §10 below; test file byte-identical | pass — `CI / verify` `37073506756` SUCCESS on the C003-containing mainline | No skip/quarantine/retry-loop/exclusion; no desktop compensation code |
| Embedded-provider WebdriverIO stack, no `tauri-plugin-wdio` (§4) | `webdriverio 9.32.0`, `@wdio/tauri-service 1.4.0`, `driverProvider: "embedded"`; boundary guard forbids `tauri-plugin-wdio` | pass (local) | Full trajectory needs no backend-execution privileges |
| Feature isolation: normal builds lack the plugin (§5) | `cargo tree`: 0 wdio crates default, 1 with `desktop-e2e`; `serde_json` promotion is the only dep-shape change; `libc 0.2.189→0.2.190` patch in the desktop lockfile only | pass | Machine-checked by the boundary guard |
| Capability isolation: production `main` stays empty (§5) | `main.json` permissions `[]`; wdio permission lives only in the `e2e/` template + gitignored generated file; Tauri compiles every `capabilities/*.json`, so a static test capability is impossible by construction | pass | Discovered and proven during implementation (generated schemas briefly absorbed test state; reverted) |
| Mechanical guard extension (§5) | `scripts/check-desktop-boundary.sh` C003 section: optional-dep, feature-map, gated-registration (python), `tauri-plugin-wdio` ban, prod config/capability wdio ban, generated-file absence, template presence | pass local + hosted root CI | Negative-tested (generated file trips the guard) |
| Deterministic fixture/control seam (§6) | `src-tauri/src/bin/desktop_e2e_fixture.rs`: isolated temp-scoped home, explicit executable, TUI-kind observer, workspace/project register, snapshot, archive/restore, identity-checked stop, reattach, shutdown; proven live via the real TS client | pass (local live) | Never touches the operator home (fail-closed, negative-tested) |
| Built-app M003 trajectory, cycles ×2 (§7) | `e2e/specs/m003-lifecycle.e2e.ts` (reuse, reconnect, reload, repeat, native close) + `m003-autostart.e2e.ts` (explicit autostart, survival) | pending hosted | `Desktop E2E` run `37089690924` |
| CI placement: no Tauri/Node/WebKit in root verify (§8) | Root `ci.yml` untouched; new path-gated `desktop-e2e.yml` (ubuntu-24.04, Node 24, 1.90.0, xvfb) | pass | Root quick green locally |
| Verification commands (§10) | §4 below | pass local; hosted pending | `test:e2e` runs in the dedicated workflow, not root CI |
| Docs/closure updates (§11) | This record; `architecture/desktop.md` E2E boundary; registry/roadmap/addendum reconciliation | done in closure commit | Predecessor records gain additive notes only |

## 3. Production implementation evidence

Desktop host (`apps/desktop/src-tauri/src/lib.rs`, additive only):

- `desktop-e2e` feature registration of `tauri_plugin_wdio_webdriver::init()`
  under `#[cfg(feature = "desktop-e2e")]`; the non-feature builder is
  byte-identical to C002.
- Test-only fail-closed `require_e2e_daemon_home()` (cfg-gated): e2e builds
  refuse `desktop_connect` unless `CODEGG_DAEMON_HOME` is temp-scoped, so a
  stray launch surfaces as `disconnected` and can never attach to the
  operator's real daemon. Production builds do not contain this function.
- No bridge, DTO, lifecycle, generation, teardown, or protocol change.

Renderer (`apps/desktop/src/App.tsx`, additive only):

- `data-testid` seams (`connection-status`, `daemon-identity`,
  `connection-generation`, `reconnect-button`, `project-list`,
  `project-item`) plus a visible Generation row. All 8 existing renderer
  tests pass unchanged.

Fixture (`src-tauri/src/bin/desktop_e2e_fixture.rs`, new, Tauri-free):

- Stdio JSON protocol (`start`, `snapshot`, `register_project`,
  `archive_project`, `restore_project`, `stop_daemon`, `reattach`,
  `shutdown`); bounded timeouts; deterministic cleanup; identity-checked
  kills; temp-scope fail-closed (negative-tested, including the macOS
  `/var/folders` vs `/tmp` distinction found during implementation).

Harness (`apps/desktop/e2e/`, new):

- `wdio.conf.ts` (embedded provider, explicit spec order, mocha 240s,
  `e2e/logs` output), `fixture-client.ts` (spawn/protocol/polling),
  `test-browser.ts` (structural typing for the four browser commands the
  trajectory uses — the global `WebdriverIO.Browser` mapped types do not
  evaluate under the repo's TS 7.0.2 toolchain; runtime calls are unchanged
  first-party APIs), `e2e/tsconfig.json` (separate project so `tsc -b`
  bundles never include the harness), `capabilities/e2e.json` template,
  `build-e2e-app.sh` (install-template + feature build / clean),
  `run-e2e.sh` (canonical entry: fixture build → app build → wdio → clean).

Workflow (`.github/workflows/desktop-e2e.yml`, new):

- Path-gated (desktop, `codegg-client`, protocol, local daemon transport,
  itself); `ubuntu-24.04` (WebKitGTK 4.1 + xvfb; avoids the ubuntu-latest
  26 migration); Node 24 pinned; Rust 1.90.0; boundary guard + renderer +
  host checks before the trajectory; E2E logs artifact (supplementary).

## 4. Verification executed

Local on macOS arm64 (Darwin 25.6.0). Root toolchain 1.89; desktop 1.90.0;
Node v26.10.0 / npm 11.19.1 locally; workflow pins Node 24.

```bash
bash scripts/verify.sh quick
# passed (fmt, agent schema, core-boundary, client-boundary,
# desktop-boundary + new C003 guards, sandbox, execution-ownership,
# tui-authority, http-route-disposition, audit-coverage, scheduler-bypass,
# provider guards, eggwork routing, workspace check)
bash scripts/check-desktop-boundary.sh
# passed; negative-tested (generated e2e.json trips the guard, then cleaned)
cargo test -p codegg-client --locked --lib
# 3 passed
# NOTE: codegg-client gui_client integration (7 tests) fails in THIS worktree
# with "path must be shorter than SUN_LEN" — the pre-existing deep-worktree
# environment limitation recorded in C001 §9/§10 (client crate untouched by
# C003). CI owns the full run; see hosted verdict below.
cargo test -p codegg --test single_daemon_lifecycle --locked
# 8 passed
git diff --check
# clean
cargo fmt --all -- --check
# clean
rustup run 1.90.0 cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
# clean

cd apps/desktop
npm ci
npm run typecheck            # tsc --noEmit (app) + e2e project — clean
npm test                     # 8/8 renderer tests
npm run bindings:check       # bridge DTO fields match
npm run build                # vite build ok
rustup run 1.90.0 cargo test --manifest-path src-tauri/Cargo.toml --locked --lib
# 26 passed, 0 failed, 1 ignored (live, run explicitly in C002)
rustup run 1.90.0 cargo test --manifest-path src-tauri/Cargo.toml \
  --locked --features desktop-e2e --lib   # with generated capability, then cleaned
# 26 passed, 0 failed, 1 ignored
node -e "import('./e2e/wdio.conf.ts')…"  # config loads: embedded provider, both specs
```

Fixture live proof (this worktree, real `target/debug/codegg`, real TS
client, no WebView — proves the exact control seam the specs drive):

```text
start ok → baseline 1 total / 0 desktop (observer only)
register second project ok → archive + restore ok (invalidations emitted)
identity-checked stop ok → reattach to restarted daemon (new id)
shutdown ok, isolated home removed
```

PTY flake characterization (test untouched, 5/5 local passes):

```bash
cargo test -p codegg --test interactive_process_sessions \
  environment_overrides_apply_while_denied_vars_stay_stripped   # ×5 → 5 passed
```

Hosted:

- `CI / verify` run `37073506756` on `3512d5d8` — SUCCESS (19m15s; reconciliation verdict: full workspace suite green including the PTY environment test)
- `Desktop E2E` run `37089690924` on `84ae31af` — PENDING (built-app verdict)

## 5. Invariant review

- Singleton daemon remains the only durable authority; desktop cleanup never
  stops daemon-owned work (re-proven by the fixture stop/reattach/shutdown
  cycle and asserted in both specs).
- TUI stays first-class: the trajectory's coexistence peer IS a TUI-kind
  client for the whole run; counts prove exactly observer + one desktop.
- One active desktop subscription per generation: unchanged production code
  (C002 gate/arming/teardown intact); the trajectory asserts no accumulation
  across two reconnect/reload cycles via daemon-side counts, not pixels.
- Least privilege: production capability empty, CSP local-only, no new
  bridge/permission/origin — mechanically guarded, green in root CI.
- Root MSRV 1.89 intact; desktop stays an isolated 1.90 workspace; root
  workspace check green.
- No storage migration, no CoreFrame/protocol change, no daemon
  authorization change (desktop delta is host/harness-only).

## 6. Failure and recovery review

- Stray app launch (ambient session before fixture setup): fail-closed
  `disconnected` via the e2e-only home guard; specs take a fresh session
  after setup regardless.
- Fixture crash/EOF: `cleanup()` removes the isolated home and signals the
  started daemon; spec `after` hooks always `shutdown()` + clear worker env.
- Daemon kill races: identity-checked (live id == autostarted id,
  back-to-back with the signal), with death-wait tolerating transport errors
  as death evidence (same discipline as C001/C002 live tests).
- Stale-env cross-file leakage: each spec owns its fixture/home and clears
  `CODEGG_DAEMON_HOME`/`CODEGG_DAEMON_EXECUTABLE` in `after`.
- Broadcast-vs-subscribe race (found on hosted run `37082528407`): reconnect
  commit takes the old subscription while the renderer re-subscribes
  asynchronously, and daemon broadcasts are lost — not queued — when no
  desktop subscriber is installed. The trajectory now waits for a fresh
  renderer-visible subscription id (display-only state mirroring the bridge
  handle; no new bridge command, no production behavior change) before every
  catalog mutation, and fails distinctly if a subscription never installs.
  This is harness/spec synchronization, not a product defect: the C002 host
  linearizes subscribe against disconnect/reconnect deterministically.
- Catalog workspace binding (found on hosted run `37086733436` via the new
  host-vs-daemon DIAG): the catalog binds one workspace to at most one
  project (`workspace_project_binding`), so repeat `register_project` calls
  into one workspace return the existing project with ok:true — no new
  project, no invalidation. The fixture now registers every probe project in
  its own workspace. This is harness/test-data design, not a product defect.
- Catalog scope (found locally while proving the above): the project catalog
  itself is machine-global by daemon design (pre-existing C001/C002
  property — the isolated home scopes socket/lock/logs, not catalog rows).
  The fixture tracks every probe id it creates and archives each on
  `shutdown` (best effort, after the WebdriverIO invocations), leaving the
  default unarchived view exactly as the phase found it; proven by a
  two-phase local run. CI runners are ephemeral, so this only tidies local
  runs.
- Hosted PTY flake recurrence: classified, owned below; the desktop verdict
  does not depend on reinterpreting it.

## 7. Migration and compatibility review

No storage migration. No CoreFrame/protocol version change. Bridge DTOs
unchanged (`bindings:check` green; Generation row reuses the existing
`connectionGeneration` field). Old renderers cannot talk to the new host
(expected — bundled together), unchanged from C002. The desktop `Cargo.lock`
gains the wdio closure plus a benign `libc 0.2.189→0.2.190` patch; root
`Cargo.lock` untouched. `package-lock.json` gains the WDIO 9 tree.
Rollback is a clean redeploy of the prior bundle plus desktop crate revert.

## 8. Security review

Production attack surface is unchanged: no new renderer permission, no
remote origin, no `unsafe-eval`, no generic bridge, no filesystem/shell/
process/network authority — all mechanically guarded (green). The embedded
WebDriver HTTP server (default port 4445, `TAURI_WEBDRIVER_PORT`-overridable)
exists only in `desktop-e2e` feature builds driven against isolated temp
homes; its capability is generated, gitignored, and absent from production
trees. The fixture requires temp-scoped homes and an explicit daemon binary
and performs identity-checked kills only. No secrets logged; isolated homes
removed by fixture cleanup. `tauri-plugin-wdio` (backend execute/mock/log
privileges) is absent and banned by the guard.

## 9. Documentation and operations

- `architecture/desktop.md`: new "Built-app E2E boundary (C003)" section
  (feature/capability/fixture/guard/seam contract) + E2E developer commands.
- Predecessor closures left immutable except the authorized additive notes
  (C002 evidence appendix + this record; see §11).
- Operator note: desktop E2E requires a display (hosted: xvfb via the
  workflow) or a local macOS/Linux session; `e2e/run-e2e.sh` is canonical.
  `src-tauri/target/` and `e2e/logs/` stay untracked.

## 10. Unresolved findings / hosted disposition

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | `CI / verify` run `37062733033` (C002-containing) failed in `interactive_process_sessions::environment_overrides_apply_while_denied_vars_stay_stripped` | Reconciled by green run `37073506756` (see disposition below) | Fresh hosted run went green with the PTY test passing; classification recorded, test untouched |
| medium | Built-app visible-window verdict outstanding until hosted `Desktop E2E` completes | Blocks strict M003 closure and M004 unblock | Run `37089690924` — PENDING |
| low | Later `CI / verify` run `37075319670` (e2e-harness-only commit `b515a8fb`) failed in `causal_observe_replay::live_request_preparation_delta::observe_mode_leaves_live_definitions_byte_identical` (`evaluation_millis < P95 budget`, 0.18s) | None on desktop: same load-sensitive single-sample latency class already classified in causal-frontier M005 closure; commit touched only `apps/desktop/e2e/` | Classified, no scope widened: the green `37073506756` verdict stands; final closure-commit CI is awaited as fresh signal |

Disposition of run `37062733033` (recorded regardless of pending verdicts):

- Failed exactly one test at `tests/interactive_process_sessions.rs:255`
  (`CODEGG_PTY_TEST_VAR=pty-visible` missing from PTY `env` output, 0.031s);
  6242 passed before the fail-fast stop; 5788 not run. Formatting, all static
  guards, and Workspace Clippy passed.
- Identical signature to run `35861891751` (`env` output missing the
  override), which went green on `--failed` rerun and is recorded as an
  unrelated timing flake in
  `plans/closure/tool-selection-advisor-retrieval-architecture-closure-corrective/001-status.md`
  §10.
- `git diff 41513fd3..3f222c2e --name-only` contains zero
  interactive-process files: between the prior green hosted run
  `37051423825` and the red run, C002 changed only desktop host/renderer,
  `architecture/desktop.md`, and planning docs.
- Local: 5/5 passes of the exact test on macOS arm64.
- Therefore classified **unrelated-to-desktop environmental flake until
  reproduced or disproved** — not C002 success, not a desktop regression.
  The test is NOT skipped, quarantined, loosened, retried in-tree, or
  excluded; no desktop code compensates for it. A single green run with the
  PTY test passing is sufficient per plan §2; a repeated red signal goes to
  the interactive-process test/runtime owner as a separate corrective (stop
  condition, not desktop scope).

## 11. Roadmap disposition

- C003: **closing** (this record; implementation `eac14e01` + hosted-only
  harness/workflow fixes through `7244865c`; root-CI reconciliation GREEN
  via `37073506756`).
- On green `Desktop E2E` (currently `37089690924`): C003 → **closed**; C001/C002
  gain strict-closure evidence additively (historical conditional records
  stand); corrective addendum C001/C002/C003 closed; foundation M003 strict
  **closed**; M004 `blocked` → `ready` (research-reconciled plan hands off
  immediately); M005/M006 stay deferred; M002 stays separately conditional
  for Windows transport/graceful-stop qualification.
- If the PTY test fails repeatedly: register a separate corrective under the
  interactive-process subsystem, keep M003 operationally blocked, do not
  merge that scope into desktop C003 (plan §9/§13).
- If the visible-window trajectory exposes a new daemon/client correctness
  defect outside the lifecycle boundary: stop and register separately; do
  not declare M003 closed (plan §13).

## 12. Registry updates

DRAFT (same commit as final verdict):

- Dependency-ready table: C003 `ready` → `closed` (this record;
  implementation `eac14e01`); M004 row `blocked` → `ready` with the strict
  M003 handoff noted.
- Blocked work: M004 hard-block row retired (unblocked by this closure);
  C003 strict-closure-evidence row retired.
- Recently-closed: C003 row added (hosted `CI / verify` run id + `Desktop
  E2E` run id); C002 row gains an additive strict-evidence pointer.
- Active subsystem roadmaps: desktop M003-corrective → C003 closed;
  desktop-foundation → M003 closed, M004 ready.
- Corrective addendum §11 + foundation roadmap M003/M004 status: strict
  closure reflected.
- Implementation plan status: `ready for handoff` → `implemented`.
- Unblock audit (same commit, per process): the only registered plan gated
  on strict M003 is M004 (session/control-plane vertical slice); all its
  other hard dependencies are closed and its interface contracts
  (`HeadlessProjectionConsumer`, typed lag/resync, `ProjectGet` workspaces,
  controller leases, C003 E2E harness) are stable → M004 moves to `ready`.
  M005/M006 remain deferred (gated on M004, not M003). M002 Windows evidence
  is operationally separate and unchanged. Nothing else unblocked, nothing
  silently unblocked.
