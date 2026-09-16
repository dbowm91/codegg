# Dependency Security and Workspace Consolidation M005 — Closure Status

Status: blocked

Source implementation plan:

- `plans/implementation/dependency-security-workspace-consolidation/005-generic-updater-interface-and-codegg-adoption.md`

Source subsystem roadmap:

- `plans/subsystems/dependency-security-workspace-consolidation-roadmap.md#m005--generic-updater-interface-and-codegg-adoption`

Repository baseline reviewed: `d2877a7b940e9c7ada25c7a3df852c53e9c9e98d` (M004 closure; plan snapshot baseline `b3459640`)

Implementation commits or pull requests:

- (this change) — M005: retire curl/shell in-place execution path, fail-closed disposition with deterministic tests

## 1. Executive finding

M005 full adoption remains blocked: no generalized, independently
consumable updater package exists outside CodeGG with the stable
written contract the plan requires. Per the plan's stop conditions,
CodeGG did not adopt a Gregg-specific dependency, did not copy or fork
an updater implementation, did not introduce service-manager or daemon
machinery, and did not add a second heavyweight HTTP/TLS stack.

What landed is the CodeGG-side hardening that is allowed while blocked:
the network-fetched installer-script execution path is retired from the
normal self-update surface. `src/upgrade/` no longer spawns external
`curl`, never fetches or executes a shell script, acquires no candidate
bytes, and attempts no executable replacement. The normal path is
check-only through the existing Eggfetch transport
(`check_for_updates()` with explicit 10s timeout and bounded redirects)
plus a pure fail-closed disposition (`describe_upgrade()`). Fresh
installation via `install.sh` remains supported as a manual operator
action with the existing `CODEGG_VERSION` pin contract. Broad
verification is green; no new dependency was introduced.

Recommendation: blocked (interface dependency unresolved). The roadmap
remains active only for that named external interface dependency, as
explicitly allowed by its completion criteria.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Validate the external package (WP1) | Gregg workspace members `gregg-protocol, greggd, gregg` contain no `gregg-update` or generic updater crate; `crates/gregg/src/bin/` holds only `lock_helper.rs`/`probe_top.rs`; Eggfetch workspace members `eggfetch-core, eggfetch-cli, eggfetch-python, eggfetch-ffi, eggfetch-node, eggfetch-bench` contain no updater crate; no Eggstack-owned generalized updater with a stable written contract was found | pass (blocker confirmed) | Stop conditions triggered; adoption halted per plan §9 |
| Do not adopt Gregg-specific constants, external `curl`, or service lifecycle deps | No new dependency added; `cargo tree -i eggfetch-core` shows only CodeGG consumers; `rg "Command::new" src/upgrade/` has no production match; no greggd/systemd/launchd/service code introduced | pass | WP1 stop rule honored |
| Define CodeGG release policy adapter (WP2) | CodeGG-specific facts stay in CodeGG: repository identity and release URL in `check_for_updates()`, binary/asset naming via `INSTALLER_SCRIPT_URL`, current version via `CARGO_PKG_VERSION`, CLI output in `cmd_upgrade()`, fresh-install pin via `installer_invocation()`; no CodeGG semantics moved into an external package | pass | Adapter is the check-only surface plus fresh-install pin |
| Use Eggfetch/native Rust acquisition (WP3) | `check_for_updates()` uses `eggfetch_core::Client` with `timeout 10s`, `follow_redirects(true)`, `max_redirects(10)`; no `curl` subprocess remains in `src/upgrade/`; `rg "curl" src/upgrade/` matches only doc comments and the printed manual fresh-install guidance string | pass | Existing transport reused; no second TLS/HTTP stack |
| Retire `upgrade()` shell-script mechanics (WP4) | `src/upgrade/mod.rs`: `upgrade()` now delegates to pure `describe_upgrade()`; `std::process::Command::new("curl")` deleted; no `env_clear()`/`PATH` plumbing remains; `cmd_upgrade()` unchanged check-only contract | pass | Fresh-install `install.sh` support and its pin test retained |
| Failure-mode tests without network (WP5) | `tests/upgrade.rs` 11 tests pass (5 pre-existing + 6 new `describe_upgrade` cases); no test hits GitHub | pass (hardening scope) / blocked (verified-replacement scope) | See §3 for mapping of the 9 plan cases |
| Dependency/size policy (§6) | No manifest changed; `cargo tree -d --locked` shows no new duplicate attributable to this change; no daemon/service framework imported | pass | Modest-dependency question moot: zero new deps |
| Acceptance: no shell installer executed by normal self-update | CodeGG never fetches or executes `install.sh`; `upgrade()` acquires no bytes and replaces nothing | pass | Manual operator `curl … | sh` remains fresh-install guidance only |
| Acceptance: candidate bytes verified before replacement | No candidate bytes acquired and no replacement attempted, so there is nothing to verify and nothing to replace | blocked (vacuous) | Full verified-replacement acceptance requires the external package |
| Acceptance: wrong checksum/program/version fail closed | `describe_upgrade()` fails closed on missing tag and invalid semver; valid newer fails closed with intact-executable guidance; checksum/identity cases reduce to intact-by-construction (no acquisition) | pass (hardening) / blocked (byte-verification) | Covered deterministically without network |
| Acceptance: existing executable remains valid on failure | No code path mutates the running binary; new tests assert newer never yields `Ok("Upgraded to …")` | pass |  |
| Acceptance: no greggd/service-manager ownership | No such dependency or code introduced | pass |  |
| Acceptance: no external `curl` required by self-update path | No `Command::new` in `src/upgrade/`; Eggfetch is the sole transport; `curl` appears only in printed manual guidance | pass |  |
| Acceptance: reuse transport/security primitives, no unjustified second stack | Eggfetch + `sha2`-free (no hashing needed: nothing downloaded); `semver` validation retained; zero new crates | pass |  |
| Acceptance: focused and broad verification green | §4 commands all pass | pass |  |

WP5 plan-case mapping (deterministic, no network):

- already-current → `test_describe_upgrade_already_current_is_noop`, `test_describe_upgrade_current_only_is_noop`;
- valid newer candidate → `test_describe_upgrade_valid_newer_fails_closed_with_manual_guidance` (asserts pin, URL, intact messaging);
- missing asset/fallback policy → `test_describe_upgrade_missing_latest_fails_closed` (fail-closed; no Cargo fallback introduced per non-goals);
- checksum mismatch → vacuous fail-closed by construction (no bytes acquired), documented in `describe_upgrade()` rustdoc and asserted via never-`Ok`-for-newer test;
- candidate wrong version/program identity → invalid-semver fail-closed test plus intact-by-construction note (no candidate execution occurs);
- unwritable current executable → intact-by-construction (no replacement attempted);
- interrupted/failed download → intact-by-construction (no download occurs on the in-place path; metadata failures surface as `AppError::Upgrade` from `check_for_updates()`);
- replacement failure → intact-by-construction (no replacement attempted);
- unsupported target → intact-by-construction (no target-specific binary resolution on the retired path).

## 3. Production implementation evidence

Landed changes (uncommitted at review baseline `d2877a7b`; committed with this closure):

- `src/upgrade/mod.rs`: module rustdoc states the M005 hardening and
  the remaining external-interface block; `INSTALLER_SCRIPT_URL` docs
  clarified as fresh-install guidance never fetched or executed by
  CodeGG; `upgrade()` reduced to `check_for_updates().await` plus pure
  `describe_upgrade()`; new public `describe_upgrade(&VersionInfo)`
  implements the entire in-place disposition (already-current `Ok`,
  missing/invalid tag `Err`, valid newer `Err` with `CODEGG_VERSION`
  pin and installer URL); `std::process::Command::new("curl")`,
  `env_clear()`, and `PATH` plumbing deleted.
- `tests/upgrade.rs`: imports `describe_upgrade`; 6 new deterministic
  tests pin the fail-closed disposition and guard against reintroducing
  an automatic `Upgraded to` path.
- `architecture/upgrade.md`: purpose, install-function, API table, and
  invariants rewritten for check-only plus fail-closed `upgrade()` /
  `describe_upgrade()`; curl-subprocess and dead-code notes removed.
- `.opencode/skills/upgrade/SKILL.md`: `upgrade()` /
  `describe_upgrade()` semantics, error variants, security
  considerations (HTTPS metadata only, no shell, no external curl,
  fail-closed), and test list updated; `PATH`/`env_clear()` section
  removed.
- `docs/execution-ownership.toml`: `src/upgrade/` reason updated from
  curl-based metadata to Eggfetch-only bounded check with retired
  in-place path (owner remains `standalone_compat`, one-shot
  administrative check).
- `CHANGELOG.md`: Unreleased Fixed entry records the retired execution
  path and the preserved check/pin surface.

Deliberately absent (per stop conditions and non-goals):

- No external updater crate adopted; no `gregg-update` copy or fork;
- no new crate created inside CodeGG to stand in for the external package;
- no binary download, SHA-256 verification, staging, candidate
  `version` execution, executable replacement, or Cargo fallback
  implemented locally (that would duplicate the shared mechanics the
  roadmap requires to live outside CodeGG);
- no greggd, systemd, launchd, Windows service, TUI, Eggpool, sudo, or
  elevation machinery;
- no second HTTP/TLS stack (no reqwest, no native curl binding);
- no release automation, signing/notarization, fixed cadence, new CI
  lane, scanner, bot, or size gate;
- no storage, protocol, schema, config, scheduler, daemon, or
  authorization change.

## 4. Verification executed

### Commands run

```bash
cargo test --test upgrade -- --test-threads=1
cargo test --lib upgrade -- --test-threads=1
cargo fmt --all -- --check
python3 scripts/check_execution_ownership.py
rg -n "Command::new|curl" src/upgrade/
cargo tree -d --locked
cargo tree --locked -p codegg --depth 1
cargo tree --locked -i eggfetch-core --depth 1
CARGO_BUILD_JOBS=1 cargo clippy -p codegg --lib --tests --locked -- -D warnings
scripts/verify.sh quick
```

Focused `upgrade()` mechanics per the plan additionally covered by
`cargo test --lib upgrade` (zero lib tests, pass) and the `tests/upgrade.rs`
integration suite above. Release/install-script regression coverage is
the retained fresh-install pin test
(`test_installer_invocation_pins_supported_env`); no network-dependent
GitHub test was added per the plan.

### Results

- `cargo test --test upgrade -- --test-threads=1`: 11 passed, 0 failed
  (5 pre-existing + 6 new `describe_upgrade` cases).
- `cargo test --lib upgrade -- --test-threads=1`: 0 tests matched, pass
  (no lib-level upgrade tests; decision surface is covered by the
  integration suite).
- `cargo fmt --all -- --check`: pass (after one `cargo fmt` normalization
  of a long assertion line).
- `python3 scripts/check_execution_ownership.py`: `execution-ownership guard ok`.
- `rg -n "Command::new|curl" src/upgrade/`: no `Command::new` match;
  `curl` matches only module/doc comments and the printed manual
  fresh-install guidance string, never a spawn site.
- `cargo tree -d --locked`: pass; no new duplicate attributable to this
  change (observed pre-existing duplicates unchanged).
- `cargo tree --locked -p codegg --depth 1` / `-i eggfetch-core`:
  Eggfetch remains the sole HTTP transport; no reqwest, curl crate,
  `self-replace`, or updater crate introduced.
- `CARGO_BUILD_JOBS=1 cargo clippy -p codegg --lib --tests --locked -- -D warnings`: pass.
- `scripts/verify.sh quick`: passed (fmt, agent schema, core-boundary,
  sandbox, execution-ownership, tui-authority guards, workspace check).

## 5. Invariant review

Plan §4 CodeGG invariants:

- Update checks remain bounded and never a startup correctness
  requirement: `check_for_updates()` retains explicit 10s timeout and
  `max_redirects(10)`; no startup path calls it. Pass.
- Release/version selection must not accept an unverified candidate: no
  candidate is accepted at all; the only `Ok` is already-current. Pass.
- Update failures leave the existing executable intact: no code path
  writes or replaces the executable. Pass.
- No shell script fetched from the network is executed by the normal
  update path after migration: no fetch and no execution occur in
  CodeGG; the installer URL appears only in printed manual guidance.
  Pass.
- No service-manager or daemon restart machinery introduced: none. Pass.
- One-binary distribution contract unchanged: no packaging or topology
  change. Pass.
- Installer remains supported for fresh installation: pin contract and
  test retained; `cmd_upgrade()` manual guidance unchanged. Pass.
- No release automation, signing/notarization, or fixed cadence added:
  none. Pass.

## 6. Failure and recovery review

- Already-current and current-only dispositions return `Ok` without
  side effects; covered by two deterministic tests.
- Missing latest tag and invalid semver return `Err` without side
  effects; covered by two deterministic tests.
- Valid newer returns `Err` with manual guidance and explicit
  intact-executable messaging; covered by two deterministic tests
  (guidance content plus never-`Ok`-for-newer guard across `2.0.0`,
  `1.0.1`, `10.0.0`).
- Because the retired path performs no acquisition, staging,
  verification, or replacement, checksum mismatch, identity mismatch,
  permission, download-interruption, replacement-failure, and
  unsupported-target hazards cannot mutate the installation; recovery
  is trivial (nothing to roll back). This is documented in rustdoc
  rather than simulated with fixtures that would imply a replacement
  capability CodeGG deliberately does not have.
- Network/metadata failures (`request build failed`, `request failed`,
  non-success status, JSON parse) propagate as `AppError::Upgrade`
  without touching the executable (pre-existing behavior, unchanged).
- No daemon, scheduler, storage, lease, or contention surface touched;
  no cancellation race introduced (`describe_upgrade()` is pure;
  `upgrade()` is a single check-then-dispose sequence).

## 7. Migration and compatibility review

- Public API is additive: new `describe_upgrade()`; existing
  `current_version()`, `check_for_updates()`, `upgrade()`,
  `installer_invocation()`, `VersionInfo`, and installer constants keep
  their signatures. `upgrade()` changes behavior only by failing closed
  with guidance instead of spawning `curl` (the prior spawn path was
  unreachable from the CLI and, as written, only fetched script bytes
  without executing an install).
- `cmd_upgrade()` CLI contract unchanged (check-only plus printed
  manual installer line).
- No schema, config, protocol, storage-layout, or migration change. No
  MSRV change. Rollback is `git revert` of this change.
- All platforms remain on the legacy/manual path: every platform gets
  check-only plus manual fresh-install guidance; no platform performs
  verified in-place replacement. Required closure evidence for platform
  carve-outs is therefore: none replaced on any platform.

## 8. Security review

- Attack surface reduced: external `curl` subprocess removed; no shell
  string constructed; no network bytes executed; no candidate process
  spawned for version probing; no file replacement attempted.
- Trust boundaries unchanged otherwise: release metadata still fetched
  over HTTPS through Eggfetch with explicit timeout and bounded
  redirects; installer URL is guidance text, not a fetch.
- No secret, SSRF/pinning, archive-traversal, plugin-sandbox,
  Landlock, authorization, or redaction behavior changed.
- No advisory silenced; no audit ignore touched; no dependency
  version/feature/source changed (manifests untouched).
- `docs/execution-ownership.toml` updated so the static guard reflects
  the Eggfetch-only, subprocess-free reality; guard passes.

## 9. Documentation and operations

- Updated: `src/upgrade/mod.rs` rustdoc, `architecture/upgrade.md`,
  `.opencode/skills/upgrade/SKILL.md` (version 1.2.0 content; version
  header unchanged), `docs/execution-ownership.toml`,
  `CHANGELOG.md` (Unreleased Fixed).
- Checked and intentionally left unchanged: `src/main.rs:1015`
  (`cmd_upgrade()` check-only contract preserved as closely as
  practical), `install.sh` fresh-install support and its pin test,
  prior closure records (history preserved), subsystem roadmap scope
  (no new capability claimed).
- No new CI lane, scanner, bot, size gate, or release automation added.
  Temporary diagnostics (`cargo tree`, `rg`) remain closure evidence only.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | No generalized external updater package exists, so verified in-place binary replacement is unavailable | Operators must manually fresh-install newer versions; deferred automatic-update convenience | None in CodeGG until a small Eggstack-owned generic updater crate with the plan §3 contract (caller-supplied repository/asset policy, configurable targets, staged checksum-verified candidates, identity/version hook, safe replacement, explicit already-current/binary/source-fallback results, no service machinery, no implicit sudo, injectable or Rust-native transport) is published and independently consumable |
| low | `cmd_upgrade()` manual guidance still prints a `curl … \| sh` installer line | An operator who follows it executes a network-fetched shell script manually; CodeGG itself does not fetch or execute it | None in this workstream; retained per plan invariant that fresh-install `install.sh` support stays unless separately deprecated. A future installer-hardening pass could offer a binary-asset alternative once the external updater exists |
| info | `describe_upgrade()` needs no SHA-256 path today because nothing is downloaded | A reviewer expecting checksum tests may misread coverage as thin | None; checksum verification becomes required only when a real candidate-acquisition path lands with the external package |

No critical or high findings. No new medium-or-higher workstream
finding introduced by the hardening itself.

## 11. Roadmap disposition

Milestone M005 remains blocked solely on the external generalized
updater interface (its M002 hard dependency was already satisfied; this
hardening adds no updater contract and unblocks nothing new).

Dependency audit for unblocking (registry Blocked work + subsystem
dependency graphs):

- Dependency security/workspace M005 is the only milestone in this
  workstream awaiting the external updater contract. No other
  registered implementation plan lists dependency-security M005 as a
  hard or interface dependency.
- Remaining unrelated conditional blockers (architecture-convergence
  M009 compatible-host Clippy evidence, runtime-safety C002 Landlock
  fixture evidence) are untouched by this workstream.
- HTTP client consolidation M001-M003 remain closed; this change reuses
  the published `eggfetch-core 0.1.4` surface and introduces no
  Git/path dependency.

If source evidence later reveals a published generalized updater crate
satisfying the plan §3 contract with an acceptable Eggfetch-unified
transport footprint, that adoption is a separate follow-up plan, not a
silent expansion of this hardening.

## 12. Registry updates

- `plans/registry.md`: Active-subsystem M005 row retains `blocked`
  with the external-interface blocker narrowed to reference this
  closure (`005-status.md`) and the landed hardening; Blocked-work M005
  row retained with the same closure reference; no new
  dependency-ready plan registered; Recently closed work unchanged
  (a `blocked` closure is not recorded as `closed`).
- Subsystem roadmap: M005 externally-blocked note retained with a
  pointer to this closure; M001-M004 completion standing unchanged;
  completion criteria note (M005 may remain blocked) still applies.
- Implementation plan: Status `blocked` → `implemented` (hardening
  landed; closure record is the gate and records `blocked` for full
  adoption).
