# Self-Contained Installation M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/self-contained-installation-corrective/002-runtime-resolution-and-clean-host-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/self-contained-installation-corrective-addendum.md#m002--managed-runtime-resolution-and-clean-host-qualification`

Repository baseline reviewed: `cf7a06bb`

Implementation commits:

- `cf7a06bb` — feat(install): managed runtime resolution and clean-host qualification (self-contained-installation M002)

## 1. Executive finding

M002 is complete. Default runtime resolution now consumes the
installation-owned runfiles from M001: explicit `[mcp.eggsearch]` stays
authoritative, explicit `[search.eggsearch].command` stays an advanced
override (distinguished via `has_explicit_command()`), otherwise the
canonical `codegg-eggsearch` sibling wins, with PATH `eggsearch` retained
only as documented legacy/source-build compatibility. The shared
`src/install.rs` resolver owns the canonical-exe-dir rule (no cwd, no
helper-specific env, no PATH for location); the sandbox helper keeps its
strict same-directory trust checks through that shared rule with no PATH
fallback. Bootstrap/doctor record managed-sidecar presence/version, the
ten-tool bundled surface, and an installation-specific hint (reinstall the
bundle, never bare "install eggsearch" as the lead); startup stays
non-fatal. `codegg doctor installation` owns the runfile report,
eggsact stays in-process 1.2.5 with the curated palette, and the
temporary-home clean-host harness qualifies the install → `codegg` →
doctor → `/connect`-proxy → wrapper → deterministic-tool → helper path
with no Rust, no external eggsearch/eggsact, and no master-key env vars.
No unresolved blocking findings; no corrective pass required. The parent
corrective workstream closes with this milestone.

## 2. Requirement-to-evidence matrix

| Requirement (plan §) | Evidence | Result | Notes |
|---|---|---|---|
| Explicit `[mcp.eggsearch]` authoritative; explicit `[search.eggsearch].command` override; otherwise canonical sibling `codegg-eggsearch`; PATH legacy only (§2) | `src/install.rs::resolve_eggsearch_command()` + `resolve_eggsearch_command_for()` test seam; `bootstrap_eggsearch()` resolution block with `resolution_source` (`explicit-mcp`/`explicit-command`/`managed-sidecar`/`legacy-path`); `EggsearchConfig::has_explicit_command()`/`explicit_command()` | pass | Packaged smoke resolves `managed-sidecar` with absolute path (see §4) |
| Distinguish absent vs defaulted command (§2) | `has_explicit_command()`/`explicit_command()` in `crates/codegg-config/src/schema.rs`; absent-resolves-managed + explicit-wins tests | pass | `command()` legacy default retained for compat only |
| Shared runfile resolver, sandbox keeps stricter checks, no cwd/helper-env authority (§2) | `src/install.rs` (`installation_dir_for()`, `validate_installation_sibling()`, `managed_eggsearch_path[_for]()`, `trusted_sandbox_helper_path[_for]()`); `src/security/sandbox.rs::sandbox_helper_path()` delegates to the shared rule; resolver-ignores-cwd/env tests | pass | No `CODEGG_SANDBOX_HELPER`/`PATH`/`current_dir` in resolution; sandbox guard passes |
| Version/tool-surface checks: managed presence/version at bootstrap/doctor; MCP discovery authoritative; required 2 + recommended 7; bundled ten-tool expectation; installation-specific diagnostic; non-fatal startup (§3) | `probe_managed_sidecar()`, `installation_hint_for()`, `EGGSEARCH_BUNDLED_COMPLETE_SURFACE` (10 incl. `provider_status`), `bundled_surface_complete` + `missing_bundled_tools()`, summary `Resolution`/`Managed sidecar`/`Bundled surface`/`Installation hint` lines; startup never crashes | pass | Dev-checkout `doctor search` shows `legacy-path` + bundle-reinstall hint; packaged layout shows `managed-sidecar` + 0.3.9 (see §4) |
| Sandbox helper qualification via trusted sibling; Linux Landlock smoke asserts enforcement; Unix-unsupported distinguishes fallback from missing; no PATH fallback (§4) | `sandbox_helper_qualification_through_trusted_resolver` (real helper via `sandbox_helper_path()`, Enforced frame decode, `resolve_sandbox_enforcement()` truthfulness, probe reason); `trusted_helper_resolves_from_installed_layout_without_path_fallback`; clean-host helper bare-probe | pass | This macOS host is Landlock-unsupported: helper present + executable, enforcement unsupported (fallback), missing-runfile error distinct; Linux enforcement covered by status-channel + probe contract (see §10 low) |
| Eggsact in-process 1.2.5; clean-host has no eggsact executable yet exposes curated palette; no 86-tool expansion (§5) | `eggsact_is_inprocess_curated_and_pinned` (lockfile 1.2.5, no `Command::new("eggsact")` spawn, 8+5=13 curated palette, in-process `text_equal` call, `eggsact_contract_line()`); `doctor deterministic-tools` shows 8 always-visible + 5 deferred | pass | — |
| Doctor/install diagnostics: exe location/version, sibling present/missing/invalid, managed version + tool coverage, helper resolvability + kernel probe, eggsact indication, no secrets; data not syntax (§6) | `InstallationReport::describe()`/`summary_lines()` + `codegg doctor installation` (single + `all`); `DoctorSubsystem::Installation`; `print_installation_report()`; secret-free test | pass | Tool coverage stays in `doctor search`; installation points there. CLI exposure is additive via the positional syntax owned here |
| Clean-host harness: no Rust/Cargo, no eggsearch/eggsact on PATH, no master-key envs, no config; steps 1–9 (§7) | `scripts/release/test-clean-host.sh` (26 green; fixture bundle from built `codegg` 0.1.0 + built helper + `eggsearch 0.3.9` fixture for `aarch64-apple-darwin`, finalize+verify, sanitized PATH/HOME, `--version`, both doctors, `--help` TUI proxy, `auth status`/`providers` /connect proxy, sidecar `--version`, deterministic doctor, helper bare-probe) | pass | Full interactive TUI pty launch is a documented proxy (`--help` + doctors); `/connect` TUI dialog itself is M003-closed and exercised headlessly here (see §10) |
| External deps documented precisely (§8) | `README.md` Requirements rewrite (hosted use needs network + credentials; prebuilt needs no Rust/eggsearch/eggsact/helper-package/Python/master-key env; Git/LSP feature-scoped; downloader/shell install-time only) | pass | — |
| Regression guards: absent-vs-explicit config test; PATH-stripped packaged test; trusted helper sibling test; eggsact static/no-process test; docs grep (§9) | `tests/installation_m002_qualification.rs` (9 green: absent-vs-explicit, PATH-stripped, trusted sibling + env, eggsact pin/scan/palette, prebuilt docs grep, ten-tool contract, helper qualification, secret-free report, packaged layout smoke) + `src/install.rs` unit tests (6 green) | pass | — |

## 3. Production implementation evidence

Ownership:

- `src/install.rs` owns the installation contract: `PINNED_EGGSEARCH_VERSION`
  (0.3.9)/source, `PINNED_EGGSACT_VERSION` (1.2.5), platform-aware sibling
  names, `installation_dir[_for]()`, `validate_installation_sibling()`,
  `managed_eggsearch_path[_for]()`, `trusted_sandbox_helper_path[_for]()`,
  `resolve_eggsearch_command[_for]()` + `EggsearchResolution`,
  `describe_sibling()`, `probe_eggsearch_version()` +
  `check_eggsearch_version_output()`, `managed_sidecar_hint()`,
  `eggsact_contract_line()`, `InstallationReport`.
- `crates/codegg-config/src/schema.rs` owns the absent-vs-explicit
  distinction (`has_explicit_command()`, `explicit_command()`).
- `src/security/sandbox.rs` owns the strict helper path by delegating to
  the shared installer rule while preserving the historical `trusted
  sandbox helper` diagnostic vocabulary; no PATH/cwd/env fallback.
- `src/search_backend/bootstrap.rs` owns managed bootstrap:
  `EGGSEARCH_BUNDLED_COMPLETE_SURFACE` (10), extended `BootstrapReport`
  (`resolution_source`, `managed_sidecar_present/path/version`,
  `installation_hint`, `bundled_surface_complete`), `probe_managed_sidecar()`,
  `installation_hint_for()`, `missing_bundled_tools()`, updated
  `summary_lines()`.
- `src/main.rs` owns `DoctorSubsystem::Installation`,
  `print_installation_report()`, and the `installation` single + `all`
  dispatch; `src/lib.rs` exposes `pub mod install`.
- `scripts/release/test-clean-host.sh` owns the reproducible
  temporary-home qualification path.
- `tests/installation_m002_qualification.rs` owns the nine regression +
  qualification guards.
- `README.md` owns the §8 runtime/install-time dependency contract;
  `architecture/search_backend.md` owns the managed-resolution + doctor
  contract; `architecture/security.md` notes the shared sibling rule;
  `docs/execution-ownership.toml` classifies the bounded `--version` probe
  (`standalone_compat`).

No runtime code shells out to an eggsact executable. No PATH fallback was
added for the sandbox helper. Normal startup remains non-fatal when search
cannot initialize.

## 4. Verification executed

### Commands run (all local)

```bash
cargo fmt --all -- --check
git diff --check
cargo test --lib install -- --test-threads=1
cargo test --lib search_backend::bootstrap -- --test-threads=1
cargo test --lib security::sandbox -- --test-threads=1
cargo test --test installation_m002_qualification -- --test-threads=1
cargo test --test eggsact_deterministic_tools -- --test-threads=1
cargo test --test search_backend_eggsearch -- --test-threads=1
cargo test --test fake_eggsearch_mcp -- --test-threads=1
cargo test --test sandbox_policy_wiring -- --test-threads=1
cargo test --test connect_tui_restoration -- --test-threads=1
cargo test --bin codegg cli_surface_tests -- --test-threads=1
bash scripts/release/test-release-tools.sh
bash scripts/release/test-installer.sh
bash scripts/release/test-clean-host.sh
cargo clippy --workspace --all-targets --locked -- -D warnings
bash scripts/verify.sh quick
sh -n scripts/release/test-clean-host.sh (+ shellcheck warning-level)
target/debug/codegg doctor installation
target/debug/codegg doctor search
target/debug/codegg doctor deterministic-tools
```

### Results

- `cargo fmt --all -- --check`: pass. `git diff --check`: pass.
- `install` lib tests: 6 passed (absent-vs-explicit, missing-fallback,
  env-ignored, hint wording, pin accept/reject).
- `search_backend::bootstrap` lib tests: 20 passed.
- `security::sandbox` lib tests: 10 passed (incl. trusted-helper
  ignores-inherited-override).
- `installation_m002_qualification`: 9 passed (see §2 last row).
- `eggsact_deterministic_tools`: 26 passed.
- `search_backend_eggsearch`: 9 passed. `fake_eggsearch_mcp`: 28 passed.
- `sandbox_policy_wiring`: 20 passed.
- `connect_tui_restoration`: 2 passed (M003 /connect surface unregressed).
- `cli_surface_tests` (codegg bin): 10 passed (incl. updated
  `doctor_advertised_subsystems_match_enum` with `installation`).
- `test-release-tools.sh`: 79 passed, 0 failed.
- `test-installer.sh`: 165 passed, 0 failed.
- `test-clean-host.sh`: 26 passed, 0 failed. Fixture: built
  `target/debug/codegg` (`codegg 0.1.0`) + built
  `target/debug/codegg-sandbox-helper` + `eggsearch 0.3.9` fixture,
  packaged for `aarch64-apple-darwin`, finalized + verified
  (`finalize-release.sh` + `verify-release.sh --allow-incomplete-target-set`),
  installed to a temp dir, sanitized `PATH=<install>:/usr/bin:/bin:/usr/sbin:/sbin`
  (no `rustc`/`cargo`/`eggsearch`/`eggsact`), fake `HOME` (no config),
  master-key env vars unset. Observed: `codegg --version` → `codegg 0.1.0`;
  `doctor installation` + `doctor search` + `doctor deterministic-tools`
  exit 0; `codegg-eggsearch --version` → `eggsearch 0.3.9`; helper
  bare-probe refuses (unsupported-kernel macOS host, distinct from missing
  runfile); `auth status` + `providers` run with isolated HOME.
- Packaged-layout managed proof (manual, same bundle shape): installed
  `doctor installation` reports `Managed eggsearch: present` +
  `Managed eggsearch version: eggsearch 0.3.9` + helper present +
  `eggsact in-process library 1.2.5`; installed `doctor search` reports
  `Resolution: managed-sidecar`, `Command: <install>/codegg-eggsearch`,
  `Managed sidecar version: eggsearch 0.3.9`, non-fatal MCP diagnostic +
  bundle-reinstall hint. Dev-checkout `doctor search` (no sidecar) reports
  `Resolution: legacy-path` + the same bundle-reinstall hint (never bare
  "install eggsearch" as the lead).
- Strict workspace Clippy (`--all-targets --locked -- -D warnings`): pass.
- `scripts/verify.sh quick`: pass (fmt, agent schema, core-boundary,
  sandbox, execution-ownership incl. new `src/install.rs` site,
  TUI-authority, workspace `cargo check`).
- `sh -n` + `shellcheck -S warning` on `test-clean-host.sh`: pass.
- No live network, no real GitHub release, no VM hypervisor: the
  temporary-home harness is the reproducible qualification path the plan
  permits (`VM/container/temporary-home`); the Linux-Landlock enforcement
  smoke beyond the status-channel + probe contract remains the §10 low.

## 5. Invariant review

- Release origin/checksum/allowlist invariants untouched (M001): the
  harness consumes `package-binary.sh`/`finalize-release.sh`/
  `verify-release.sh` without weakening them.
- Sandbox helper keeps the strict trusted-sibling rule (shared, not
  weakened): same-directory canonicalization, regular-file + executable
  checks, no PATH/cwd/env authority; `check_sandbox_contract.py` passes.
- Managed eggsearch never consults cwd or helper-specific env; explicit
  overrides remain explicit and are never silently replaced.
- Eggsact remains in-process; CodeGG keeps the curated 8+5 palette and
  progressive disclosure; no second search cache, matrix, or daemon added.
- `codegg-core` boundary untouched (`check-core-boundary.sh` passes);
  execution ownership declares the bounded probe; TUI authority passes.
- Secrets never printed: installation/search reports print paths/versions/
  tool names only; provider-status summarizer still redacts values.

## 6. Failure and recovery review

- Missing managed sidecar: bootstrap falls back to legacy PATH for
  compat, records `managed_sidecar_present=false`, and surfaces a
  bundle-reinstall hint; the process does not crash and `doctor` exits 0.
- Corrupt/wrong-version sidecar: `--version` probe records the raw output
  plus the mismatch; spawn failure names the sidecar path and the bundle
  reinstall; explicit overrides keep their own diagnostics.
- Explicit `[mcp.eggsearch]` failure does not get a misleading managed
  hint (`explicit-mcp` source is excluded).
- Helper missing vs unsupported kernel are distinct: missing names
  resolution (`could not be resolved`); unsupported names the Landlock
  probe reason and reports `Unavailable` (never silent `FullHost`).
- Installer/rollback semantics unchanged from M001; the harness extracts
  a verified tarball and leaves no staging files (temp root removed).

## 7. Migration and compatibility review

- No config/schema/protocol/storage migration. Absent `command` keeps
  working (now better: resolves managed instead of PATH). Explicit
  `command` and `[mcp.eggsearch]` behavior is preserved and still wins.
- Fresh and upgraded M001 bundles gain resolution without reinstallation;
  dev checkouts without a sidecar keep the legacy PATH behavior with a
  clearer diagnostic.
- `doctor` gains an additive `installation` subsystem; existing
  subsystems and the deprecated `--subsystem` spelling are unchanged.
- Windows keeps explicit `.exe` sibling names in the shared resolver;
  Windows remains optional best-effort per M001.

## 8. Security review

- Sibling validation rejects symlink escape, non-regular files, and
  non-executable candidates; destination commits still use renames.
- `--version` probe is a bounded blocking spawn of the already-validated
  sibling with no args beyond `--version`; output is a single line,
  checked for `eggsearch` identity + `0.3.9` pin; no secrets in outputs.
- Doctor/Installation reports never print credentials, keys, headers, or
  secret-bearing config (test asserts).
- No new network, daemon, scheduler, sandbox-framework, or credential
  surface. Managed MCP dispatch still goes through `McpService`.

## 9. Documentation and operations

- `README.md`: §8 runtime vs install-time contract (hosted use needs
  network + credentials; prebuilt needs no Rust/eggsearch/eggsact/
  helper-package/Python/master-key env; Git/LSP feature-scoped;
  downloader/shell install-time only) + bundle resolution note retained.
- `architecture/search_backend.md`: managed resolution contract,
  absent-vs-explicit semantics, ten-tool surface, doctor sections.
- `architecture/security.md`: shared sibling rule note.
- `docs/execution-ownership.toml`: new `src/install.rs`
  `standalone_compat` site for the bounded probe.
- Operator view: publish per M001; qualify with
  `bash scripts/release/test-clean-host.sh [--artifact <tar.gz>]`
  (26 checks; prints artifact/install/PATH/HOME evidence); inspect with
  `codegg doctor installation` + `codegg doctor search` +
  `codegg doctor deterministic-tools`.
- Guards: `tests/installation_m002_qualification.rs` + `src/install.rs`
  unit tests + offline release suites; no new CI lanes, scanners, or
  gates (per registry verification policy).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Full interactive TUI pty launch and TUI-driven `/connect` credential entry were proxied headlessly (`--help` parses; `auth status`/`providers` run with isolated HOME; M003 dialog suite still green) | No evidence gap for resolution/diagnostics, but not a literal pty keystroke recording | Re-run `test-clean-host.sh` against the first real published bundle on a pty host and attach the transcript to the release checklist; no code change expected |
| low | Supported-Linux Landlock enforcement was qualified via the trusted resolver + real helper executability + Enforced status-channel decode + Landlock probe truthfulness on this macOS host, not a Linux Landlock `Enforced{abi}` exec on this machine | Same residual as the historical Runtime-Safety C002 Linux-fixture gap; M002 does not claim that operational evidence | First supported-Linux packaging must run the harness on Linux and record the kernel ABI + `Enforced` line; the shared helper path is already the code that emits it |
| low | No real upstream `eggsearch 0.3.9` binary was available; the harness sidecar is a pinned-identity fixture (`eggsearch 0.3.9`) plus the built `codegg`/helper | Real `--version` magic + MCP handshake shape beyond the fixture is unproven here | First real-bundle packaging must run `package-binary.sh` + `verify-release.sh` + `test-clean-host.sh --artifact <real tarball>` before upload; the pin check is substring-based so no code change is expected |
| — | No medium-or-higher finding remains | — | — |

## 11. Roadmap disposition

Milestone M002 closed. The parent `self-contained-installation-corrective`
workstream (M001+M002) is closed: M001 delivered the bundle, M002 delivers
resolution + diagnostics + clean-host qualification. No corrective pass
required.

Dependency audit (registry Blocked work + subsystem graphs): no registered
`blocked`/`proposed` plan lists M002 (or this workstream) as a hard or
interface dependency. Dependency-security M005 remains blocked on the
external generalized updater interface; Architecture-convergence M009
remains conditionally closed on compatible-host root-runtime/all-feature
evidence; Runtime-safety C002 remains conditionally closed on
supported-Linux fixture evidence. This closure satisfies none of those
external blockers and creates no new follow-up plan, so nothing is
unblocked or reclassified beyond this workstream itself. The deferred
`installation` doctor ownership noted in command-surface M002 closure is
now fulfilled additively (that workstream stays closed; history is not
rewritten).

## 12. Registry updates

- `plans/registry.md`: `self-contained-installation-corrective` subsystem
  `active` → `closed` (current milestone M001+M002 closed); M002
  `ready` → `closed` (this record, implementation `cf7a06bb`); M002
  recorded under recently closed work; execution-order item 11 rewritten
  (M001+M002 closed, workstream closed); blocked-work section unchanged
  after explicit audit (nothing newly unblocked).
- `plans/subsystems/self-contained-installation-corrective-addendum.md`:
  `Status: active` → `closed`; M002 `ready` → closed with this closure
  link; milestone table/exit conditions marked satisfied.
- `plans/implementation/self-contained-installation-corrective/002-runtime-resolution-and-clean-host-qualification.md`:
  `ready` → `implemented` (closure here).
- `plans/implementation/self-contained-installation-corrective/001-managed-runfile-release-bundle.md`:
  unchanged (already `implemented`).
