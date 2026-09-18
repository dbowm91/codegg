# Self-Contained Installation M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/self-contained-installation-corrective/001-managed-runfile-release-bundle.md`

Source subsystem roadmap:

- `plans/subsystems/self-contained-installation-corrective-addendum.md#m001--multi-runfile-release-artifact-and-installer-contract`

Repository baseline reviewed: `f9ec8602`

Implementation commits:

- `f9ec8602` — feat(release): managed runfile bundle with pinned eggsearch sidecar (self-contained-installation M001)

## 1. Executive finding

M001 is complete. The obsolete single-executable release invariant is
superseded everywhere — packager, verifier, installer, both offline
suites, and release/install documentation — by an exact,
version-qualified bundle: each Unix archive carries `codegg`,
`codegg-sandbox-helper`, and `codegg-eggsearch` (plus optionally the
single fixed `THIRD-PARTY-NOTICES.txt`); the Windows archive carries the
explicitly defined `.exe` counterparts. Upstream eggsearch is pinned to
0.3.9 with identity/version validation at packaging and verification
time, and the installer commits the whole bundle transactionally with
backup/rollback. The canonical verifier rejects any artifact that cannot
satisfy the runtime sibling contracts (missing helper, missing eggsearch,
wrong sidecar version, extra/traversal/symlink/device payloads), and the
installer fresh-installs the bundle and upgrades the historical
single-codegg layout without relying on a pre-existing PATH eggsearch.
No unresolved blocking findings; no corrective pass required. Runtime
resolution and clean-host qualification remain explicitly owned by M002.

## 2. Requirement-to-evidence matrix

| Requirement (plan §) | Evidence | Result | Notes |
|---|---|---|---|
| Central runfile manifest in one release authority (§3) | `codegg_release_runfiles_for_target()` in `scripts/release/lib-release.sh`; Unix `codegg codegg-eggsearch codegg-sandbox-helper`, Windows explicit `.exe` names; packager/verifier/installer tests consume it | pass | `test-release-tools.sh` pins the exact sorted manifest per target class |
| Fixed third-party notice member, no loose extras (§3) | `CODEGG_NOTICE_MEMBER=THIRD-PARTY-NOTICES.txt`; only non-executable member ever permitted; `--notice` (≤64 KiB, regular file) in packager; `package bundle with notice` + `notice member uses the fixed allowlisted name` tests | pass | Notice is optional; a 4-member archive without it fails |
| Explicit Windows names/status (§3) | `codegg_release_is_windows_target()` + `.exe` manifest; `windows bundle uses explicit .exe runfile names` test; RELEASING.md Windows bundle paragraph | pass | Windows stays optional best-effort, never required for completeness |
| Pinned eggsearch provenance 0.3.9 (§4) | `CODEGG_EGGSEARCH_PINNED_VERSION=0.3.9`, `CODEGG_EGGSEARCH_UPSTREAM_TAG=v0.3.9`, `CODEGG_EGGSEARCH_SOURCE=https://github.com/eggstack/eggsearch`, `CODEGG_EGGSEARCH_LICENSE=MIT` in `lib-release.sh`; RELEASING.md provenance section | pass | Source corrected from the stale `anomalyco/eggsearch` link (404; live repo is `eggstack/eggsearch`, MIT); `src/search/mod.rs` doc link fixed |
| Accept already-built binary or deterministic prep step; validate identity/version, arch/OS, regular file, no symlink/device/traversal; record checksum (§4) | `package-binary.sh` requires `--sandbox-helper` + `--eggsearch`, runs `<eggsearch> --version` through `codegg_release_check_eggsearch_version_output()`, rejects symlink/non-regular/non-executable inputs, best-effort `file(1)` ELF/Mach-O/PE cross-check, prints input SHA-256 receipt (`inputs: codegg=… eggsearch(0.3.9)=…`); RELEASING.md documents the pinned-tag preparation step | pass | `--eggsearch-version` must equal the pin; drift needs a deliberate pin bump |
| Renamed sidecar `codegg-eggsearch` (§4) | `CODEGG_EGGSEARCH_SIDECAR`; staged/installed/verified under that name only; no PATH `eggsearch` resolution in release path | pass | — |
| No dependency on eggsearch internal modules (§4) | No Cargo change; no eggsearch dependency added (`Cargo.toml` untouched except nothing); RELEASING.md states the MCP/CLI boundary | pass | — |
| Source/version/license in docs and notices (§4) | RELEASING.md provenance + notice-content rule; `--notice` staged as fixed member; README source-build note names version/source | pass | — |
| Packager stages allowlist, normalizes modes, inspects members, atomic publish (§5) | `package-binary.sh` stages only the manifest (755 runfiles, 644 notice), verifies member list pre-publish, `mv` atomic publish, `--force` overwrite policy | pass | — |
| Update lib-release, package-binary, finalize, verify, test-release-tools, RELEASING.md (§5) | All six updated (finalize unchanged in behavior; manifest hashing is member-agnostic) | pass | finalize needs no member logic by design |
| Deterministic checksums covering final archive; origin/target/manifest validation not weakened (§5) | `finalize` re-run stability test; `sha256sum -c` compat test; verifier keeps strict manifest grammar + completeness + no-unexpected-files | pass | — |
| Installer validates every member pre-extraction; rejects absolute/traversal/links/devices/duplicates/unexpected (§6) | Rewritten `codegg_installer_check_archive_members()` (3 runfiles + optional notice); unit tests for each rejection class plus missing-helper/missing-eggsearch | pass | — |
| Stage runfiles before modifying destination; one canonical directory; preserve modes (§6) | Extraction validated + `chmod`ed in temp; staged via `mktemp` in destination; 755 runfiles / 644 notice | pass | — |
| Backup/rollback bundle commit; no mixed old/new on second/third-runfile failure (§6) | Backup-then-commit with `codegg_installer_rollback()`; injected-`mv`-failure test proves the previous bundle survives intact | pass | Trap cleanup restores sole-surviving backups on backup-phase failure |
| No destination-symlink following (§6) | All commits are `mv` renames over directory entries (replaces link entries, never follows); `chmod` touches staging/temp copies only | pass | — |
| Fixed origin/checksum verification retained; staging/backup cleanup (§6) | Origin/checksum code paths untouched; staging-leftover + backup-leftover sweep test; `rm -rf` backup removal | pass | — |
| `codegg` user command path stable (§6) | Destination `<dir>/codegg` unchanged; PATH guidance unchanged | pass | — |
| Source-build policy (§7) | RELEASING.md source-build policy section; README `cargo install` non-self-contained note + sidecar instructions; availability note retained (not advertised as available) | pass | — |
| Test coverage §8 (manifest, missing sidecars, traversal/symlink/device/unexpected, wrong version, permissions, determinism, fresh install, single-codegg upgrade, rollback, idempotence, leakage) | `test-release-tools.sh` 79 green (incl. historical single-codegg rejection, per-sidecar absence, wrong-version native smoke); `test-installer.sh` 165 green (incl. bundle install, single-codegg upgrade, injected rollback, rerun, leakage-via-allowlist) | pass | Permissions asserted post-extract (`-x` + mode normalization) and post-install |
| Old one-executable assertion removed/superseded everywhere (§10) | `grep` sweep: remaining `exactly one` hits are unrelated domains (scheduler records, daemon singleton); all release/installer/docs single-member assertions replaced | pass | `finalize-release.sh` comment wording is member-agnostic |

## 3. Production implementation evidence

Ownership:

- `scripts/release/lib-release.sh` owns the canonical manifest
  (`codegg_release_runfiles_for_target()`), the eggsearch pin
  (version/tag/source/license), the sidecar/helper/notice names, and the
  `codegg_release_check_eggsearch_version_output()` identity gate.
- `scripts/release/package-binary.sh` owns bundle packaging: required
  `--sandbox-helper`/`--eggsearch`, optional `--notice`, pin-locked
  `--eggsearch-version`, per-input regular-file/executable validation,
  `--version` identity probe, best-effort `file(1)` arch/OS cross-check,
  allowlist staging with normalized modes, pre-publish member
  verification, atomic publish, input-hash receipt.
- `scripts/release/verify-release.sh` owns bundle verification: exact
  per-target manifest comparison (3 runfiles + optional notice),
  unsafe-member rejection, verbose regular-file/symlink checks,
  leakage-pattern rejection, permission checks, native `codegg
  --version` + pinned `codegg-eggsearch --version` + safe helper
  identity-probe smokes.
- `install.sh` owns the bundle transaction: exact member validation,
  temp extraction + mode normalization, pre-install `codegg` +
  `eggsearch` smokes, destination staging, existing-entry backup,
  ordered commit with `codegg_installer_rollback()`, trap-safe backup
  restore, post-install bundle smoke (`codegg --version`,
  `codegg-eggsearch --version`, helper executability).
- `scripts/release/test-release-tools.sh` (79 assertions) and
  `scripts/release/test-installer.sh` (165 assertions) own the
  regression guards listed in §2.
- `RELEASING.md` owns the bundle contract, provenance/pinning,
  preparation step, source-build policy, and installer smoke criteria.
  `README.md` owns the user-facing bundle/source distinction.
- `src/search/mod.rs` one-line doc-link correction
  (`anomalyco/eggsearch` → `eggstack/eggsearch`); no logic change.

No runtime resolution change: default search bootstrap still resolves
external `eggsearch`, and no `codegg-eggsearch` sibling lookup was
added — that is M002's scope and is untouched here.

## 4. Verification executed

### Commands run

```bash
bash scripts/release/test-release-tools.sh
bash scripts/release/test-installer.sh
cargo fmt --all -- --check
git diff --check
bash scripts/verify.sh quick
```

Plus, per plan §9: `scripts/release/verify-release.sh --dir <fixture-rel>
--allow-incomplete-target-set` against a freshly packaged
bundle-with-notice fixture, and a manual extract-and-execute of the
final archive (`codegg --version`, `codegg-eggsearch --version`,
helper bare-invocation probe → exit 125). shellcheck coverage runs
inside `test-installer.sh` (`shellcheck -S warning install.sh` green);
`sh -n` syntax green for all touched scripts.

### Results

All local. `test-release-tools.sh`: 79 passed, 0 failed. Covers target
allowlist, exact per-target manifests (Unix + Windows), pin
accept/reject, full-bundle packaging per required target, notice fixed
name, Windows `.exe` manifest, leakage sweep, negative inputs (unknown/
malicious/option-like targets, missing/symlink/non-executable inputs
per runfile, wrong eggsearch version/identity, drifted
`--eggsearch-version`), spaced out-dir, overwrite policy, finalize
determinism + `sha256sum -c` compat, complete-set verification,
historical single-codegg rejection, per-sidecar absence rejection,
wrong-version native-smoke rejection, tamper/missing/duplicate
manifests, unknown/traversal/absolute manifest entries, crafted
traversal/absolute/symlink/extra/missing-codegg payloads, stale temp
files, Windows optional treatment, empty finalize.

`test-installer.sh`: 165 passed, 0 failed. Covers mapping/version/URL
units, origin-override prohibition, checksum grammar, bundle member
validation (valid bundle, traversal, absolute, symlink, device, extra,
missing codegg/helper/eggsearch), eggsearch output validation, fresh
bundle install with version/identity assertions, historical
single-codegg upgrade gaining sidecars, sentinel-bundle replacement,
idempotent rerun, tamper/manifest/download/duplicate/version-mismatch
failures with bundle-sentínel intactness, malicious payloads end to
end, injected helper-commit failure with full-bundle rollback, staging/
backup leftover sweep, clean retry, missing-tool cleanliness,
bash/dash portability, static prohibitions, `sh -n` + shellcheck.

`cargo fmt --all -- --check`: pass. `git diff --check`: pass.
`scripts/verify.sh quick`: pass (fmt, agent schema, core-boundary,
sandbox, execution-ownership, TUI-authority guards, workspace
`cargo check`).

Deviation from plan §9: no real target binaries were available, so the
`codegg --version` / helper-probe / `codegg-eggsearch --version`
final-archive verification ran against fixture shell binaries (native
smoke where the target matched the host; manual extract-and-execute
for the packaged demo archive). The plan conditions that clause on
target-binary availability, so fixture evidence plus the native
wrong-version rejection is the justified substitute. No live network,
no real GitHub release, no clean-host run — the latter is M002's
explicit scope.

## 5. Invariant review

- Release origin stays hard-coded; no mirror/URL override variable in
  executable lines (test asserts); checksum-before-extract/execute order
  preserved, including the canary test proving a tampered payload never
  executes.
- Target-name allowlist preserved and extended per-target (never
  guessed); unknown/traversal/option-like targets rejected.
- Manifest validation not weakened: strict grammar, exact-entry match,
  duplicate rejection, completeness enforcement, no-unexpected-files —
  all retained and extended to the bundle.
- Installer never escalates, never edits profiles/services, never runs
  cargo/rustup (static tests); user config untouched (test asserts).
- Sandbox helper keeps its strict trusted-sibling rule in production
  code (untouched); the bundle simply delivers the sibling the rule
  already requires.
- Eggsact remains in-process; no eggsact executable introduced
  anywhere.
- `codegg-core` boundary untouched (shell/docs/comment-only change;
  `check-core-boundary.sh` passes).

## 6. Failure and recovery review

- Packaging failure leaves no final archive (tested); existing output
  never overwritten without `--force`; symlink outputs refused.
- Verification is read-only and idempotent (re-runs green); tamper,
  missing, duplicate, unknown, and malicious-payload cases fail closed
  with the payload check (not just the hash) as the rejector.
- Installer failure before commit leaves existing runfiles byte-identical
  (bundle-sentinel assertions across checksum/manifest/download/
  duplicate/version/payload/missing-tool failures).
- Commit-phase failure rolls back: injected helper-commit failure
  restores the full previous bundle (test asserts all three sentinels
  plus the recovery message); backup-phase failure restores sole
  surviving copies via the trap.
- Post-replacement smoke failure reports explicitly that replacement
  already happened (unchanged semantics, now bundle-wide).
- Clean retry after failures succeeds and leaves no staging/backup
  files (sweep test).

## 7. Migration and compatibility review

- Fresh install writes all three runfiles (+ notice when present) into
  one directory; `<dir>/codegg` path and PATH guidance unchanged.
- Upgrade from the historical single-codegg layout succeeds and gains
  the two sidecars (dedicated test); rerun is idempotent.
- Historical releases lacking the bundle must not be advertised as
  installer-compatible (RELEASING.md + README availability note
  retained).
- Windows remains optional best-effort with an explicit manifest; the
  Linux/macOS installer path never consumes it.
- No config/schema/protocol/storage changes; no migration needed.

## 8. Security review

- Archive members validated before extraction (absolute, traversal,
  symlink, device, duplicate, unexpected); extracted runfiles
  re-validated (symlink, regular file, executable bit).
- Inputs must be regular executable files; symlinks rejected at
  packaging and verification time; destination commits use renames that
  replace (never follow) symlink entries.
- `file(1)` arch/OS cross-check is best-effort and fails closed only on
  positive contradiction (ELF vs darwin, Mach-O vs linux, PE vs
  non-Windows, arch family mismatch); fixture/text inputs skip it, so
  no false rejection path for real builds was introduced beyond genuine
  magic contradictions.
- Notice file bounded (≤64 KiB, regular file) and fixed-name; no
  config/credential/source leakage possible outside the allowlist
  (pattern sweep + allowlist tests).
- No secrets in any new output: packaging receipt prints hashes only;
  installer prints versions and paths only.
- `CODEGG_EGGSEARCH_*` pin values are data, never executed; version
  strings are validated before URL/filename use is impossible (the
  installer never takes a version-derived filename).

## 9. Documentation and operations

- `RELEASING.md`: bundle contract (Unix + explicit Windows), provenance
  and pinning, deterministic preparation step, notice rule,
  source-build policy, updated packaging commands, native smoke
  including helper/eggsearch, installer smoke criteria, cargo-install
  non-self-contained note.
- `README.md`: installer installs the bundle (only the directory needs
  `PATH`); source installs are not self-contained with sidecar
  instructions; prebuilt bundles need no separate eggsearch install.
- Operator view: publish per the updated Step 9 order (build all
  targets + pinned sidecar → package with explicit flags → finalize →
  verify → upload exact assets); smoke the installer per Step 10
  (three `installed:` lines + both version lines).
- Guards: the two offline suites plus existing static prohibitions;
  no new CI lanes, scanners, or gates added (per registry verification
  policy).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | No real upstream eggsearch 0.3.9 binary was available in this environment; identity/version/arch validation ran against fixtures | Real `--version` output shape and `file(1)` magic paths are unproven against the genuine artifact | Validate once with the real pinned binary during first-release packaging (pre-upload `verify-release.sh` run); no code change expected — the check is substring-based |
| low | Upstream org recorded as `eggstack/eggsearch` (live, MIT) correcting the stale `anomalyco/eggsearch` link, based on web evidence | If the canonical upstream home differs, one-line corrections to the pin comment, RELEASING.md, README, and doc link | Confirm at first-release packaging; supersede this record factually if needed |
| low | Windows `.exe` manifest defined but never packaged/smoked here | Windows remains optional best-effort; a malformed Windows bundle would fail its own `verify-release.sh` run | First Windows packaging must run `package-binary.sh` + `verify-release.sh --skip-version-smoke` (or native smoke) before upload |
| — | None blocking; clean-host qualification explicitly deferred | M002 owns the installed-layout smoke | — |

## 11. Roadmap disposition

Milestone M001 closed. Its hard dependent, M002 (Runtime Resolution
and Clean-Host Qualification at
`plans/implementation/self-contained-installation-corrective/002-runtime-resolution-and-clean-host-qualification.md`),
had two blockers: this M001, and final-closure dependency Provider
/connect Restoration M003 (closed at
`plans/closure/provider-connect-restoration-corrective/003-status.md`,
implementation `5cc460c0`). Both are now satisfied, so M002 moves
`blocked` → `ready` in the same commit. No other registered plan lists
M001 as a dependency; nothing else changes state.

## 12. Registry updates

- `plans/registry.md`: register the `self-contained-installation-corrective`
  subsystem (active, current milestone M001 closed / M002 ready);
  M001 `ready` → `closed` (this record, implementation `f9ec8602`);
  M002 `blocked` → `ready` (blockers M001 + provider-connect M003 both
  closed); M001 recorded under recently closed work.
- `plans/subsystems/self-contained-installation-corrective-addendum.md`:
  M001 `ready` → closed; M002 `blocked on M001` → `ready`
  (provider-connect M003 precondition already closed).
- `plans/implementation/self-contained-installation-corrective/001-managed-runfile-release-bundle.md`:
  `ready` → `implemented` (closure here).
- `plans/implementation/self-contained-installation-corrective/002-runtime-resolution-and-clean-host-qualification.md`:
  `blocked on Self-Contained Installation M001` → `ready`.
