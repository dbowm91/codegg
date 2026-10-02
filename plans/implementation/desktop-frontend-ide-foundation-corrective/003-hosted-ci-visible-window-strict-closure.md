# Desktop Frontend and IDE Foundation Corrective C003 — Hosted-CI Reconciliation and Built-App Visible-Window Strict Closure

Status: ready for handoff

Repository baseline: `3f222c2e5f957efdbb6a530276ff5b63a095e435`

Source corrective addendum:

- `plans/subsystems/desktop-frontend-ide-foundation-m003-lifecycle-corrective-addendum.md`

Predecessor implementation/evidence:

- `plans/closure/desktop-frontend-ide-foundation/003-status.md`
- `plans/closure/desktop-frontend-ide-foundation-corrective/001-status.md`
- `plans/closure/desktop-frontend-ide-foundation-corrective/002-status.md`
- C001 implementation `7b17808f`
- C002 implementation `dc32a6ad`

Applicable architecture:

- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md`
- `architecture/desktop.md`

Primary class: verification / operational closure / test infrastructure

Hard dependency:

- C002 production implementation is landed and locally/live qualified.

External test-platform references reviewed:

- Tauri 2 WebDriver overview: `https://v2.tauri.app/develop/tests/webdriver/`
- Tauri WebDriver CI guide: `https://v2.tauri.app/develop/tests/webdriver/ci/`

## 1. Objective

Strict-close desktop foundation M003 without adding M004 session functionality.

C003 owns the two remaining closure obligations:

1. reconcile hosted root-CI evidence for the C002-containing mainline, distinguishing an unrelated baseline/test failure from a desktop regression without weakening either test surface; and
2. replace the one-off “visible WebView evidence” requirement with a repeatable built-app WebDriver trajectory that drives the real Tauri window, real IPC `Channel`, real local daemon connection, reconnect, renderer reload, native close, and daemon-survival behavior.

On successful C003 closure:

- C002 receives additive strict-closure evidence without rewriting its historical conditional closure record;
- M003 becomes strict `closed`;
- M004 becomes `ready`;
- M005/M006 remain deferred;
- M002 remains separately conditional for Windows transport/graceful-stop qualification.

## 2. Current hosted-CI disposition

Current main hosted run:

- workflow run: `37062733033`
- head: `3f222c2e5f957efdbb6a530276ff5b63a095e435`
- result: failed in `Workspace tests`;
- formatting, static guards, Workspace Clippy, live-Eggwork setup all passed;
- failing test:
  `codegg::interactive_process_sessions::environment_overrides_apply_while_denied_vars_stay_stripped`.

The same test passed on prior green hosted run `37051423825` at `41513fd38b72fa521e509cc9b95eb06b766c70de`.

Between that green head and current main, the C002 production changes are confined to the desktop application plus desktop documentation/planning. No `interactive_process` production/test file changed.

Therefore C003 must treat the red run as **unclassified/unrelated-to-desktop until reproduced or disproved**, not as C002 success and not as a desktop regression.

Required disposition:

- obtain a fresh hosted run on a main revision containing C002;
- if the PTY test passes and the job is green, record the run as C002 hosted evidence;
- if the same PTY test fails again, hand the failure to the interactive-process test/runtime owner and require a separate corrective or accepted evidence disposition before claiming root CI green;
- do not skip, quarantine, loosen, retry-loop, or exclude the test merely to close desktop M003;
- do not modify desktop code to compensate for an unrelated PTY environment failure.

A single green retry is sufficient if no C002/desktop-related test or guard fails and the repeated PTY failure does not reproduce. Repeated red signal requires separate ownership.

## 3. Why a built-app harness is required

C001/C002 prove host lifecycle behavior at command/controller level, but the final M003 condition intentionally covers surfaces that unit tests cannot prove:

- a real WebView renders the bundled frontend;
- Tauri `invoke` commands cross the actual IPC boundary;
- Tauri `Channel<DesktopEvent>` traffic reaches the renderer;
- clicking the visible reconnect control causes a real connected -> connected generation transition;
- renderer reload/unload invokes the Rust subscription teardown path correctly;
- native window close reaches the Rust host teardown hook;
- closing/reloading the GUI does not stop daemon-owned work;
- a second frontend can remain attached while the desktop comes and goes.

This must be retained as executable regression coverage rather than a manual one-time transcript.

## 4. Test-stack decision

Use WebdriverIO with Tauri's maintained `@wdio/tauri-service`.

Default provider for C003:

- `driverProvider: "embedded"`;
- test-only `tauri-plugin-wdio-webdriver`;
- no `tauri-plugin-wdio` unless a concrete test requirement cannot be met through ordinary WebDriver element interaction and the existing external fixture/control seams.

Rationale:

- Tauri currently recommends `@wdio/tauri-service`;
- the embedded provider supports macOS, Linux, and Windows without a platform-native external WebDriver;
- ordinary M003 assertions need visible DOM interaction, not privileged arbitrary backend execution;
- avoiding `tauri-plugin-wdio` keeps the test renderer from acquiring extra backend-execution/mock/log privileges.

The embedded WebDriver plugin must be test/debug feature-gated and absent from ordinary production builds.

Acceptable project shape:

```text
apps/desktop/
  e2e/
    specs/
    wdio.conf.ts
    fixture/
  src-tauri/
    Cargo.toml
    tauri.conf.json
    [optional test-only config]
```

Exact layout may follow npm/package conventions already used by `apps/desktop`.

## 5. Production-boundary requirements for test instrumentation

### Feature isolation

Add an explicit Cargo feature such as `desktop-e2e` or equivalent.

Only that feature may compile/register `tauri-plugin-wdio-webdriver`.

Normal:

```bash
cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml
npm run tauri build
```

must not enable the WebDriver plugin.

### Capability isolation

The production `main` capability remains empty.

Do not add shell, process, filesystem, HTTP, clipboard, updater, global-shortcut, or generic Tauri API permissions to make E2E convenient.

If the WebDriver provider needs generated capability metadata, keep it in a test-only capability/config that cannot be selected by the production build.

### Mechanical guard

Extend `scripts/check-desktop-boundary.sh` so it fails if:

- the WebDriver plugin is enabled unconditionally;
- a production capability references test-only WDIO permissions;
- `tauri-plugin-wdio` is added without a documented C003 stop/review;
- a production renderer origin/CSP is relaxed for tests;
- a generic `core_request`, shell, filesystem, or process bridge appears.

## 6. Deterministic E2E fixture/control seam

Do not add a privileged test command to the production Tauri bridge.

Use a separate test process/helper that talks to the isolated daemon through the same native protocol/client crates. It may:

- start or attach a TUI-kind observer client;
- register a deterministic temporary workspace/project in LocalOwner test context;
- request `SnapshotDaemon` and inspect `connected_clients`;
- mutate/archive/restore a project to generate a project-catalog invalidation;
- poll daemon state;
- terminate only the isolated test daemon during the explicit autostart trajectory.

Use:

- isolated `CODEGG_DAEMON_HOME`;
- isolated temp workspace;
- explicit `CODEGG_DAEMON_EXECUTABLE`;
- a daemon binary built from the tested revision;
- bounded timeouts and deterministic cleanup.

The fixture must never touch the operator's real daemon home or projects.

## 7. Built-app M003 trajectory

Run against an actual compiled Tauri app.

### Existing-daemon reuse

1. start an isolated daemon explicitly;
2. attach a TUI-kind observer and record daemon client baseline;
3. register one deterministic project/workspace;
4. launch the real desktop app under WebdriverIO;
5. wait for rendered `connected` status;
6. assert rendered daemon identity matches the fixture daemon;
7. assert the deterministic project appears in the rendered project list;
8. assert daemon client inventory contains exactly the expected observer + one desktop client.

### Actual reconnect

9. click the rendered `Reconnect` button;
10. wait for the UI to remain connected;
11. assert daemon inventory converges to exactly one desktop client plus observer, not two desktop connections;
12. mutate/archive/restore the project through the fixture;
13. assert the rendered project catalog changes without a manual refresh, proving real Tauri `Channel` delivery from the current subscription.

### Renderer reload

14. trigger a real WebView/browser reload through WebDriver;
15. wait for reconnect/project rendering;
16. assert daemon client count and desktop subscription behavior converge without accumulation;
17. repeat catalog mutation and assert only the current renderer receives the update.

### Native close

18. close the main window through the WebDriver/native window path;
19. poll daemon inventory until the desktop client disappears;
20. assert the TUI-kind observer remains connected;
21. assert the daemon remains responsive.

### Explicit autostart

22. stop the isolated daemon;
23. launch a fresh built desktop with explicit `CODEGG_DAEMON_EXECUTABLE`;
24. assert a new daemon starts and the window renders connected state;
25. close the desktop window;
26. assert the autostarted daemon remains running and responsive after desktop exit.

Run the reconnect/reload/close cycle at least twice in one qualification to catch accumulation.

## 8. CI placement

Do not add Tauri/Node/WebKit dependencies to the root `CI / verify` job.

Prefer a dedicated path-gated workflow, for example:

```text
.github/workflows/desktop-e2e.yml
```

Trigger on changes to at least:

- `apps/desktop/**`;
- `crates/codegg-client/**`;
- projection/native protocol files consumed by desktop;
- local daemon transport files;
- the desktop E2E workflow/harness itself.

Linux is sufficient for routine hosted built-app qualification. Use the Tauri-documented Linux display setup (WebKitGTK plus `xvfb`) when required by the selected provider/runtime.

A macOS local/hosted run is valuable because C002's native-close behavior was developed on macOS, but M003 strict closure requires one supported display-backed macOS or Linux trajectory, not a cross-platform matrix. Windows remains M002-gated.

Node should be pinned to a currently supported runner/tool version rather than inheriting an ambient version.

## 9. Required evidence and failure handling

### Hosted root CI

Record a green `CI / verify` run containing C002.

If the interactive-process environment test fails repeatedly:

- open/register a separate corrective under the interactive-process subsystem;
- capture the exact failure output/environment;
- keep M003 operationally blocked until root CI has an accepted green/disposition;
- do not merge unrelated corrective scope into desktop C003.

### Desktop E2E

Record:

- exact OS/arch;
- Rust and Node versions;
- app build command and feature/config used;
- WDIO/Tauri service versions;
- fixture daemon path and isolated-home shape;
- client-count sequence;
- reconnect/reload/close assertions;
- project mutation visible in the real renderer;
- autostart daemon-survival result.

Capture screenshots/logs only as supplementary artifacts; machine assertions are closure authority.

## 10. Verification commands

Record actual commands. Expected minimum:

```bash
bash scripts/verify.sh quick
bash scripts/check-desktop-boundary.sh
cargo test -p codegg-client --locked
cargo test -p codegg --test single_daemon_lifecycle --locked
git diff --check

cd apps/desktop
npm ci
npm run typecheck
npm test
npm run bindings:check
npm run build
rustup run 1.90.0 cargo test --manifest-path src-tauri/Cargo.toml --locked
npm run test:e2e
```

Use the dedicated build/test feature for E2E rather than enabling the WebDriver plugin in normal production configuration.

## 11. Documentation/closure updates

On success:

- create `plans/closure/desktop-frontend-ide-foundation-corrective/003-status.md`;
- add an additive C002 strict-evidence note pointing to the hosted green run;
- mark corrective addendum C001/C002/C003 closed;
- mark foundation M003 strict `closed`;
- update `architecture/desktop.md` with the E2E/test-only plugin boundary;
- update `plans/registry.md`;
- move M004 from `blocked` to `ready`;
- leave M002 conditional for Windows evidence;
- leave M005/M006 deferred.

Do not rewrite predecessor closure records to erase why C001/C002 were required.

## 12. Acceptance criteria

C003 closes only when:

1. a hosted root `CI / verify` run containing C002 is green, or an unrelated repeated failure has been separately owned/resolved and a subsequent run is green;
2. production desktop builds do not contain/enable the WebDriver test plugin;
3. production capabilities/CSP remain least-privilege;
4. a built-app WebDriver trajectory renders the real daemon/project state;
5. actual reconnect leaves one current desktop client;
6. real Tauri `Channel` project invalidation updates the current renderer;
7. renderer reload does not accumulate daemon clients/subscriptions;
8. native close removes the desktop client while TUI and daemon survive;
9. explicit-path desktop autostart works and desktop exit leaves the daemon running;
10. the trajectory is repeatable through a checked-in E2E harness;
11. no unresolved high/medium M003 lifecycle/security finding remains.

When all criteria pass, M003 is strict-closed and M004 is ready.

## 13. Stop conditions

Stop and register a separate plan if:

- the visible-window trajectory exposes a new daemon/client correctness defect outside the already-defined lifecycle boundary;
- testability requires adding production shell/filesystem/process authority;
- testability requires a generic production-only backdoor bridge;
- root hosted CI exposes a reproducible non-desktop failure needing production correction;
- macOS/Linux built-app testing requires weakening CSP or remote-origin IPC;
- Windows becomes the only blocker (belongs to M002).

## 14. Closure record requirements

`plans/closure/desktop-frontend-ide-foundation-corrective/003-status.md` must include:

- C002-containing hosted CI run id/result;
- disposition of run `37062733033` and the interactive-process test;
- WebDriver architecture/version/provider;
- proof test plugin is non-production;
- exact built-app E2E commands;
- rendered assertions;
- daemon client-count trajectory;
- TUI coexistence;
- reconnect/reload/current-channel evidence;
- native close result;
- explicit autostart result;
- security/MSRV/toolchain impact;
- final M003/M004 roadmap transition.
