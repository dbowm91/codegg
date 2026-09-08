# Distribution and Installation Milestone 002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/distribution-installation/002-installer-and-end-user-installation.md`

Source subsystem roadmap:

- `plans/subsystems/distribution-installation-roadmap.md#7-milestones`

Repository baseline reviewed: `faa6603d3334488d7f30f1944116050036d2c2a5`

Implementation commits:

- `73f0cf41` — feat(release): add verified POSIX installer and end-user installation (distribution M002)

## 1. Executive finding

M002 is complete. A stable top-level POSIX `install.sh` maps the four
supported Linux/macOS hosts to the exact M001 release assets, downloads
over the hard-coded canonical HTTPS origin, verifies SHA-256 against
`checksums.txt` before extraction or execution, validates the archive
payload and pre-install `--version` smoke, and atomically installs `codegg`
into a user-writable directory with PATH guidance. Unsupported hosts fail
before any download with source-install alternatives. The offline harness
`scripts/release/test-installer.sh` passes **146/146** with no network, no
GitHub release, and no publication. `README.md` presents three truthful
install paths, `RELEASING.md` gains post-publication installer smoke, and no
Rust change, workflow, CI lane, package-manager integration, signing, or
daemon/service feature was added. No corrective pass is required.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Four supported OS/arch mappings select the correct M001 asset | `install.sh`: `codegg_installer_map_target`; harness tests 1-7 assert all mappings incl. `amd64`/`arm64` spellings; integration installs resolve the host target end-to-end | pass |
| Unsupported hosts fail before download with Cargo/source alternatives | Mapping runs before tool check/download; PATH-shim `uname` tests assert `unsupported OS/architecture`, `cargo install` alternative text, and absence of `downloading`/fixture-seam output | pass |
| Explicit version input cannot escape the canonical release URL | `codegg_installer_normalize_version` allowlist grammar + `codegg_installer_download_url` re-validation; 17 version unit tests (incl. `;`, `$()`, backtick, `/`, whitespace, URL rejection); exact latest/pinned URL strings asserted per asset | pass |
| Checksum verification mandatory before extraction/install | `codegg_installer_verify_checksum` runs before listing/extraction/execution; 11 manifest unit tests (wrong/missing/duplicate/superstring/traversal/absolute/empty/malformed/uppercase, binary-mode `*` accepted) plus tamper integration tests | pass |
| Archive traversal/symlink/unexpected payload rejected | `codegg_installer_check_archive_members` list-before-extract; 7 payload unit tests (traversal, absolute, symlink, device, extra, missing, valid) + 5 end-to-end rejections with re-finalized manifests | pass |
| Existing binary intact on every pre-commit failure path | Sentinel `codegg` + byte-identical `cmp` assertions after tamper, missing manifest/archive, duplicate entry, version-mismatch smoke, all bad payloads, unsupported host, missing tools, file-as-dir failures; clean-retry test replaces the intact sentinel afterwards | pass |
| Default install user-local; no sudo/profile/service behavior | Default `$HOME/.local/bin`, `CODEGG_INSTALL_DIR` override; static tests assert no `sudo` string, no word `eval`, no `>>` appends, no profile/service paths, no `$(cargo`/`` `cargo`` execution; `mkdir -p` exact path only | pass |
| Install-dir quoting/path spaces | Spaced-dir and `; $ ()`-metachar-dir end-to-end installs succeed with working binaries at literal paths | pass |
| Offline fixture tests, no live GitHub dependence | 146/146 harness assertions offline; production origin has no env override (`CODEGG_DOWNLOAD_URL`-family ignored at runtime and absent from code lines; only test-only `_CODEGG_INSTALLER_TEST_RELEASE_DIR` seam) | pass |
| README/release docs truthful | README installer table + pinned/custom-dir/inspect-first docs, crates.io 404 verified and stated, source path retained; `RELEASING.md` Step 10 installer smoke (Linux + macOS) + no-advertise rule for incomplete sets | pass |
| No workflow/CI/package-manager/daemon-autostart addition | Diff touches only `install.sh`, `scripts/release/test-installer.sh`, `README.md`, `RELEASING.md`, `architecture/testing.md`; no `.github/`, `.rs`, config, or service changes | pass |
| `codegg --version` reliable, no Rust change needed | `./target/debug/codegg --version` → `codegg 0.1.0`; `#[command(version)]` in `src/main.rs`; zero `.rs` files touched | pass |

## 3. Production implementation evidence

New top-level installer `install.sh` (587 lines, POSIX `sh`, executable,
`set -eu`, no bash-only syntax, no `local`, quoted variables throughout):

- `codegg_installer_map_target` — `uname -s`/`uname -m` normalization to the
  exact M001 table (`Linux x86_64/amd64 → x86_64-unknown-linux-gnu`, `Linux
  aarch64/arm64 → aarch64-unknown-linux-gnu`, `Darwin x86_64 →
  x86_64-apple-darwin`, `Darwin arm64/aarch64 → aarch64-apple-darwin`).
- `codegg_installer_normalize_version` — empty means latest; else strip one
  leading `v` and enforce
  `^[0-9]+\.[0-9]+\.[0-9]+([.+_-][A-Za-z0-9.+_-]*)?$`.
- `codegg_installer_download_url` — fixed
  `https://github.com/dbowm91/codegg` origin; `releases/latest/download/`
  vs `releases/download/v<V>/`; version re-validated defensively.
- `codegg_installer_require_commands` — `uname curl tar mktemp mkdir chmod
  mv cp rm grep awk` plus `sha256sum`/`shasum -a 256`, before any network or
  destination mutation.
- `codegg_installer_download` — `curl -fsSL --proto '=https' --tlsv1.2
  --connect-timeout 20 --max-time 300` to controlled temp filenames; the
  only redirect is the test-only local-copy seam for fixed filenames.
- `codegg_installer_verify_checksum` — strict manifest grammar, unsafe
  entries fail closed anywhere in the file, exact-basename single-match
  selection, recomputed hash compared exactly.
- `codegg_installer_check_archive_members` — one `codegg` regular-file
  member required (leading `./` tolerated); absolute/traversal/symlink/
  device/unexpected rejected before extraction.
- Pre-install `codegg --version` smoke (runs only after checksum + payload
  validation; pinned version must appear in output, latest must be
  well-formed `codegg X.Y`).
- Atomic commit: `mktemp` staging file inside the destination directory,
  `chmod 755`, `mv` rename over the entry (replaces rather than follows a
  stale symlink), existing binary untouched until this point.
- Post-install `--version` smoke, install/path/daemon notes
  (`export PATH=...` guidance printed, never applied; profile edits
  explicitly disclaimed; daemon-takes-effect-on-next-launch note).

New harness `scripts/release/test-installer.sh` (787 lines, `bash`,
`set -euo pipefail`): sources the installer with `CODEGG_INSTALL_LIB_ONLY=1`
for function unit tests; runs integration through `sh` (plus `bash` and
`dash` success paths) with the fixture seam; builds fixtures with the real
M001 `package-binary.sh`/`finalize-release.sh`; crafts malicious payloads
with `tar`/`python3` (`../evil`, `/tmp/evil`, symlink, char-device, extra,
missing members).

Final interface:

```text
install.sh                                    # stable repository path
CODEGG_VERSION       optional; default latest; 0.1.1 or v0.1.1
CODEGG_INSTALL_DIR   optional; default $HOME/.local/bin
```

## 4. Verification executed

### Commands run (all local)

```bash
scripts/release/test-installer.sh
scripts/release/test-release-tools.sh
sh -n install.sh
shellcheck -S warning install.sh
shellcheck -S warning scripts/release/test-installer.sh
./target/debug/codegg --version
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

### Results

- `test-installer.sh`: **146 passed, 0 failed** (offline; `sh` + `bash` +
  `dash` success paths; shellcheck 0.11.0 present and passing at warning
  level for both new scripts).
- `test-release-tools.sh`: **62 passed, 0 failed** (M001 no-regression).
- `sh -n install.sh`: pass.
- `./target/debug/codegg --version` → `codegg 0.1.0` (no Rust change).
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  pass (finished, no warnings).
- `scripts/verify.sh quick`: pass (fmt, agent schema, core boundary,
  sandbox, execution ownership, workspace check).

Local-only truth: no hosted CI run was used; none is required by this plan
(installer tests are explicitly offline/change-specific, and no Rust or
workflow surface changed). No tags or GitHub releases exist in this
repository (`git tag` empty, `gh release list` empty), so live
published-release smoke was unavailable; it is prescribed as maintainer
evidence in `RELEASING.md` Step 10 (see §10).

## 5. Invariant review

- Manual release cadence/publication preserved: no workflow, schedule,
  dispatch, or automation added; installer consumes releases, never creates
  them.
- One `codegg` executable per target: installer places exactly
  `<dir>/codegg`; no daemon/TUI split.
- Fixed HTTPS origin: `https://github.com/dbowm91/codegg` is a literal;
  runtime-ignored `CODEGG_DOWNLOAD_URL`-family overrides proven by test.
- No privilege escalation, profile edits, service management, credential
  handling, or toolchain installation: by construction plus static tests.
- No Cargo fallback: checksum/download failures exit nonzero with
  alternatives printed, never compiled.
- No GitHub API token: plain release URLs, no authorization headers.
- Private temp state (`mktemp -d` + `chmod 700`, `EXIT/HUP/INT/TERM` trap
  covering temp dir and destination staging file).

## 6. Failure and recovery review

- Every pre-commit failure (download, checksum, listing, extraction,
  executable bit, pre-install smoke, staging) leaves the existing
  destination binary byte-identical (asserted per path) and removes temp
  state via trap; failed runs leave no `.codegg-install.*` staging files
  (asserted across all destination fixtures).
- Checksum mismatch is final for the run; tampered payload is never
  executed (canary-touching fixture + one-byte tamper → canary untouched).
- Post-rename smoke failure reports clearly that replacement already
  happened; no installer transaction framework was built, per plan.
- Concurrent installers unsupported by design: unique staging names plus
  atomic rename prevent partial files; serialization documented in code
  comments and plan scope.
- Clean retry after failures succeeds and replaces the intact sentinel
  (tested).

## 7. Migration and compatibility review

No user runtime migration. Source installation unchanged and still
documented first-class. crates.io path documented as unavailable until the
first manual publication (live 404 verified at implementation time), so no
false install promise is made. Historical releases without M001 assets are
explicitly not installer-compatible in README and `RELEASING.md`. No config,
credential, database, socket, or daemon-state directory is inspected or
mutated (HOME-config fixture byte-identical after default-path install).

## 8. Security review

Untrusted inputs (`CODEGG_VERSION`, `CODEGG_INSTALL_DIR`, manifest bytes,
archive members, URLs) are treated as data: version allowlist grammar,
install-dir dash/newline guards with fully quoted use, manifest parsed as
data with exact-match selection, archive members allowlisted before
extraction, URLs built from literals plus validated version only. No `eval`,
no command substitution on untrusted values, no `sudo`, no profile/service
writes, no credential handling. PATH-shim tests prove unsupported hosts
exit before the download seam; canary tests prove checksum-failed payloads
never execute; metachar-dir tests prove path-data discipline. `curl` is
pinned to `https` protocol and TLS 1.2 with bounded timeouts.

## 9. Documentation and operations

- `README.md`: installer-first Install section (target table, default,
  pinned, custom-dir, inspect-before-run, PATH/daemon notes), truthful
  crates.io-unpublished note, retained source path.
- `RELEASING.md` Step 10: maintainer post-publication smoke (Linux + macOS,
  latest/pinned/custom-dir), required success signals, no-advertise rule
  for incomplete sets, offline harness pointer.
- `architecture/testing.md`: short Local Commands note that release/
  installer fixture tests are offline and change-specific, not routine CI.
- Operator commands: `sh install.sh`, `scripts/release/test-installer.sh`
  (both `--help`-documented where applicable).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | No live published-release smoke exists yet: no tag/release has been published from this repository. | Offline harness covers all acceptance criteria, but the first real GitHub release is still unproven end-to-end. | Maintainer runs the new `RELEASING.md` Step 10 smoke (one Linux + one macOS host) at the first real publication before advertising installer support. |
| low | `curl --proto`/`--tlsv1.2` flags assume a modern curl on supported hosts. | Very old toolchains could reject the flags; the failure is loud (nonzero before mutation), not silent. | None unless a supported host reports otherwise; the required-commands check runs first. |

No critical, high, or medium findings.

## 11. Roadmap disposition

M002 is strictly closed: every acceptance criterion in §13 of the
implementation plan is satisfied by offline evidence, and the only
remaining evidence (first-publish smoke) is future maintainer procedure,
not an implementation defect — the same disposition M001 used for its
first-release evidence. The subsystem roadmap completes with this
milestone: M001 closed + M002 closed satisfies the roadmap §11 completion
definition (stable manual artifact/checksum contract plus
checksum-verifying, non-root, atomic installer with honest docs).

Dependency audit for unblocking (registry Blocked-work section plus
subsystem dependency graphs): no registered implementation plan lists
distribution M002 as a hard or interface dependency. The blocked items
(runtime-safety C002 Landlock evidence; maintainability M002 on
maintainability M001; maintainability M005 on maintainability M002/M003)
are independent of this closure. Deferred unregistered product work
(Homebrew/deb/rpm/Nix, Windows installer, signing/notarization,
SBOM/provenance, package-manager automation) intentionally remains
unregistered. **Nothing is unblocked by this closure; no plan changes
status.**

## 12. Registry updates

- `plans/registry.md`: Distribution subsystem row `active (M001 closed,
  M002 ready)` → `closed (M001 closed, M002 closed)`; remove the M002 row
  from dependency-ready implementation plans; record this closure under
  recently completed control points; drop distribution M002 from the
  post-audit execution-order ready list (M001/M002 both closed).
- `plans/subsystems/distribution-installation-roadmap.md`: `Status: active`
  → `closed`; M002 `ready` → `closed` with closure link; milestone table
  updated.
- `plans/implementation/distribution-installation/002-installer-and-end-user-installation.md`:
  status → implemented/closed with closure link.
