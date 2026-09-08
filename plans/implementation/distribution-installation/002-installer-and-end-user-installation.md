# Distribution and Installation Milestone 002 — Verified Installer and End-User Installation

Status: ready for handoff — hard dependency M001 closed; see `plans/closure/distribution-installation/001-status.md`

Repository baseline: `15632a0483a8c4b9d573ff2ce43297b29be8f42a`

Source roadmap:

- `plans/subsystems/distribution-installation-roadmap.md#7-milestones`

Hard dependency:

- M001 — `plans/implementation/distribution-installation/001-manual-prebuilt-artifact-contract.md`

Related governance:

- `RELEASING.md`
- `plans/subsystems/development-verification-release-roadmap.md`

Long-term requirements:

- `plans/000-long-term-specification.md#1-product-definition`
- `plans/000-long-term-specification.md#2-primary-product-goals`
- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Applicable ADRs: none.

Primary class: capability / polish

## 1. Objective

Add a small, auditable POSIX installer that maps supported Linux/macOS hosts to the stable M001 GitHub release assets, downloads over a fixed HTTPS origin, verifies the selected archive against the release SHA-256 manifest, extracts a controlled payload, and atomically installs `codegg` into a user-writable directory without sudo or shell-profile mutation.

The installer must support the repository's latest release and an explicitly pinned version. Unsupported hosts must fail early with clear Cargo/source installation alternatives.

## 2. Why this milestone is ready

The installer must not invent filenames, target labels, archive layout, or checksum syntax. M001 has closed (`plans/closure/distribution-installation/001-status.md`) and establishes those as a stable release interface. All hard dependencies are satisfied; no interface dependency remains unstable.

After M001 closes, the runtime requirements are simple and require no daemon/protocol changes. The intended mapping is:

```text
Linux  x86_64/amd64  -> x86_64-unknown-linux-gnu
Linux  aarch64/arm64 -> aarch64-unknown-linux-gnu
Darwin x86_64        -> x86_64-apple-darwin
Darwin arm64/aarch64 -> aarch64-apple-darwin
```

The asset name is `codegg-<target>.tar.gz`, scoped by the GitHub release/tag. `checksums.txt` is the integrity manifest.

## 3. Current implementation evidence

At the baseline there is no stable end-user install script. Users can build/install from source with Cargo, and `RELEASING.md` describes optional manual binary releases but not a checksum-verifying install path.

The repository's intentionally minimal verification/release policy means the installer must be testable using local fixture release directories or a mockable download seam. Routine tests must not depend on GitHub availability or a real published release.

The user-scoped daemon/single-executable topology means installation needs to place only the `codegg` executable. It must not configure daemon startup, credentials, sockets, project roots, or services.

## 4. Invariants that must not regress

- Installer source URL/origin is fixed to the canonical CodeGG GitHub repository over HTTPS.
- Explicit version input is validated before URL construction.
- The installer verifies SHA-256 before extracting/installing the downloaded archive.
- Checksum lookup matches the exact selected asset basename, not a substring/regex that can select another entry.
- Existing installed `codegg` is not replaced until the new artifact has passed download, checksum, extraction, payload, executable, and version checks.
- Default destination is `${HOME}/.local/bin/codegg`; `CODEGG_INSTALL_DIR` may override the directory.
- The script never invokes `sudo`, edits shell profiles, starts services, or installs Rust/Cargo.
- Unsupported OS/architecture fails before downloading a guessed asset.
- Missing checksum tool, curl, tar, unwritable destination, download failure, or checksum mismatch produces nonzero exit and leaves existing binary intact.
- Temporary files are private and cleaned on exit/signals where practical.
- Installer execution does not require a GitHub API token.
- No automatic Cargo fallback occurs after binary failure; Cargo/source alternatives are printed explicitly.

## 5. Scope

### In scope

- Add a stable repository `install.sh` or equivalent top-level user-facing installer.
- POSIX shell compatible with the intended macOS/Linux `/bin/sh` environment; avoid Bash-only syntax unless the script explicitly requires Bash and docs use it consistently.
- Detect OS/architecture via standard tools (`uname`).
- Normalize known machine spellings only; reject unknown variants.
- Support latest release using GitHub's stable latest-release asset URLs without scraping APIs.
- Support pinned version via `CODEGG_VERSION` (or a similarly simple documented variable) mapping to `v<VERSION>` release URLs.
- Support `CODEGG_INSTALL_DIR` with safe path handling.
- Download archive and checksum manifest with `curl` (or a deliberately supported small alternative set).
- Verify SHA-256 with `sha256sum` on Linux or `shasum -a 256` on macOS; fail if no supported verifier exists.
- Extract into temp directory, verify exactly expected executable payload/path, run pre-install version smoke, and atomically place binary.
- Report PATH guidance if destination is not currently discoverable.
- Add local fixture tests for mapping, URLs, checksums, failure atomicity, and installation.
- Update README and `RELEASING.md` post-publication smoke instructions.

### Explicitly out of scope

- PowerShell/Windows installer.
- Homebrew/package-manager integration.
- Automatic system-wide `/usr/local/bin` installation requiring privilege.
- Automatic `PATH` modification.
- Daemon/service autostart.
- Code signing/notarization.
- Installer self-update.
- Telemetry.
- GitHub API JSON parsing to find releases.
- Automatic Cargo/source compile fallback.
- Dependency installation.

## 6. Required production changes

### Core/domain

No Rust runtime change is expected. Confirm `codegg --version` is reliable enough to identify the built package version. If not, a small CLI version fix may be included with focused tests and called out explicitly.

### Storage and migrations

None. The installer must not inspect or mutate user CodeGG state directories.

### Protocol and DTOs

None.

### Runtime and concurrency

None in the daemon. Installer file replacement must be atomic within the destination filesystem where possible: stage a verified executable in the install directory under a temporary name, apply executable mode, then `mv`/rename to `codegg`.

Staging in the destination directory avoids cross-filesystem rename surprises. The downloaded/extracted source remains in a private temp directory.

### Frontend or operator surface

Recommended environment interface:

```text
CODEGG_VERSION       optional; default latest; explicit value such as 0.1.1
CODEGG_INSTALL_DIR   optional; default $HOME/.local/bin
```

Avoid a large option parser unless the repository already has a standard installer style. Environment variables keep the copy/paste path simple:

```bash
curl -fsSL <canonical raw install.sh> | sh
```

Pinned example:

```bash
curl -fsSL <canonical raw install.sh> | CODEGG_VERSION=0.1.1 sh
```

The README may also recommend downloading the script before execution for users who want to inspect it. Do not hide the fact that pipe-to-shell executes remote code.

### Security and authorization

#### Origin and version validation

Construct all release URLs from a hard-coded canonical HTTPS base. Validate explicit version with a conservative grammar, for example semantic-version characters only, and reject `/`, whitespace, shell metacharacters, URL delimiters, leading `-`, or arbitrary refs.

Accept either bare `0.1.1` and normalize to tag `v0.1.1`, or require one documented form. Do not allow the input to override repository/host URL.

#### Downloads

Use `curl -fL --proto '=https' --tlsv1.2` where portable enough on supported hosts. Set bounded connect/overall retry behavior only if simple; do not hide 404/checksum failures behind unbounded retries.

Download both archive and `checksums.txt` to the temp directory before verification.

#### Checksum

Parse the manifest as data. Select exactly one entry whose basename equals the expected asset. Reject zero or multiple matches, unsafe paths, or malformed hash. Recompute SHA-256 locally and compare exactly. Prefer invoking platform checksum tools in a way that does not require trusting manifest paths outside the temp directory.

#### Archive extraction

Before or during extraction, validate archive member names. Require the M001 expected payload and reject absolute paths, `..`, unexpected executable names, device files, or symlinks for `codegg`. Extract into private temp directory, then require `codegg` to be a regular file and executable after applying expected mode.

#### Installation

Create destination directory only under the exact configured path; quote all variables. Never use `eval`. Never follow a destination `codegg` symlink into another location during replacement: staging + rename should replace the directory entry, and the script should validate destination parent behavior.

No credential handling occurs.

### Documentation and static guards

README becomes the user-facing install authority. `RELEASING.md` adds manual published-release installer smoke. Do not add a network-backed installer test to routine CI.

## 7. Ordered work packages

### Work package A — Implement deterministic host mapping and URL construction

Intent: fail safely before network access.

Required changes:

1. normalize `uname -s` to Linux/Darwin only;
2. normalize `uname -m` accepted spellings;
3. map to exact M001 target/asset;
4. validate/normalize version input;
5. construct either latest or pinned fixed-origin URLs;
6. expose functions/test seam so mapping can be tested without changing the real host.

Acceptance evidence:

- table-driven shell tests for all four supported mappings;
- unsupported OS/arch/version rejected;
- exact URL/asset strings verified.

### Work package B — Implement download and checksum verification

Intent: establish integrity before extraction.

Required changes:

- locate required local commands before mutating destination;
- create private temp dir/trap;
- download manifest/archive to controlled filenames;
- select exact manifest entry and verify hash;
- stop immediately on failure.

Acceptance evidence:

- fixture success;
- wrong/missing/duplicate checksum entry fails;
- altered archive fails;
- download failure leaves destination untouched.

### Work package C — Safe extraction and pre-install smoke

Intent: reject malicious/broken archive before replacement.

Required changes:

- inspect member list before extraction if feasible with portable tar;
- reject traversal/absolute/symlink/unexpected payload;
- extract into private temp dir;
- require regular `codegg` file;
- run downloaded `codegg --version` on supported native host/fixture and verify expected version string when an explicit version is selected; for `latest`, verify nonempty valid CodeGG version and optionally derive against redirect/tag only if possible without API complexity.

Do not execute an archive that failed checksum/payload validation.

Acceptance evidence:

- malicious archive fixture rejected;
- valid fixture reaches pre-install smoke;
- failed smoke does not replace destination.

### Work package D — Atomic user installation and PATH guidance

Intent: commit only verified binary.

Required changes:

- validate/create install dir;
- ensure directory writable;
- copy verified executable to a unique temp file in destination directory;
- set executable permissions;
- rename to `codegg` only after all validation succeeds;
- run final installed `--version` smoke;
- if final smoke unexpectedly fails after rename, report clearly and retain enough information for manual recovery; optional backup/rollback is acceptable if simple and deterministic, but do not build an installer transaction framework;
- print destination and PATH guidance.

Acceptance evidence:

- preexisting sentinel binary remains unchanged on every pre-commit failure path;
- success replaces it atomically in tests;
- no sudo/profile modification.

### Work package E — Local installer harness and docs

Intent: make installer maintainable without live network dependency.

Required changes:

- add a test mode/helper that points download operations at local fixture files or a local HTTP server under tests without making production origin configurable by untrusted environment variables;
- test supported/unsupported mappings, checksum, archive security, install directory, and existing-binary atomicity;
- update README with default, pinned, Cargo/source alternatives, PATH notes;
- update `RELEASING.md` with post-publication smoke on one Linux and one macOS host at minimum when practical.

Acceptance evidence:

- installer tests are offline/deterministic;
- production path cannot be redirected to arbitrary origin through the test seam/environment.

## 8. Failure, cancellation, restart, and contention semantics

Any failure before final rename leaves the existing destination binary unchanged.

Signal/exit trap removes private download/extraction temp directory. Destination staging file should also be removed on failure where the shell can trap it.

A network interruption is a hard failure; no partial archive is accepted because checksum verification occurs after download completion.

A checksum mismatch is final for that run and must never trigger automatic source compilation/fallback.

Concurrent installers to the same destination are unsupported. Unique staging names plus atomic rename prevent partial files, but last successful rename may win. Document that upgrades should be serialized.

The installer does not stop/restart a running daemon. On Unix, replacing the executable does not mutate an already running process; the new binary is used on the next launch. Document this if users might expect an immediate daemon upgrade.

## 9. Compatibility and migration

Existing source installation remains supported:

```bash
cargo install --path .
```

Once crates.io publication is actually available, documented `cargo install codegg --version <VERSION>` remains an alternative. Do not claim crates.io availability merely because release docs describe the procedure.

Historical GitHub releases that lack M001 assets/checksums are not installer-compatible. Pinned installer requests for such versions should fail clearly rather than invent filenames/fallback.

No user config/state migration occurs on binary upgrade.

## 10. Required tests

### Focused unit/script tests

- OS/arch target mapping;
- explicit version grammar/normalization;
- latest versus pinned URL construction;
- install-dir quoting/path spaces;
- dependency/tool detection;
- exact checksum manifest parsing.

### Integration tests

Offline/local fixture flow:

- valid release fixture installs sentinel `codegg` and reports version;
- checksum mismatch fails;
- archive missing `codegg` fails;
- traversal/symlink/device/unexpected payload fails;
- unsupported target fails before download seam is invoked;
- unwritable/non-directory destination fails;
- prior installed sentinel remains byte-identical on pre-commit failure;
- successful install replaces prior executable;
- rerunning same version succeeds/idempotently replaces or reports already installed according to chosen simple behavior.

### Restart and recovery tests

No daemon restart. Test interrupted/failed install cleanup and subsequent clean retry where feasible.

### Contention and cancellation tests

No supported concurrent install coordination. A simple test may confirm unique staging names/no partial destination on one interrupted run.

### Security and negative tests

- injected version string cannot alter URL/command;
- `CODEGG_INSTALL_DIR` containing spaces/metacharacters is treated as path data, not executed;
- test download override unavailable in normal production environment unless explicitly compiled/sourced by the test harness;
- checksum mismatch prevents execution/extraction of untrusted payload beyond safe archive listing;
- archive traversal/symlink payload rejected;
- installer never executes `sudo` or edits rc/profile files.

### Migration and compatibility tests

- source/Cargo alternatives remain documented;
- existing user config directories untouched by fixture install.

## 11. Required verification commands

After M001 closes, exact script names may differ. Expected:

```bash
# offline fixture harness
scripts/release/test-installer.sh

# syntax/static shell check when shellcheck is already available locally;
# do not add shellcheck installation to routine CI solely for this plan
sh -n install.sh

# broad repository posture for any Rust/docs interactions
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

After a real manual binary release, maintainer-only smoke evidence should include:

```bash
# example shape; use canonical documented URL
curl -fsSL <install-script-url> | sh
codegg --version

CODEGG_VERSION=<released-version> sh install.sh
codegg --version
```

Actual publication smoke is closure evidence only if a release exists; do not block code implementation on making a release solely to test it if the offline harness is complete. The roadmap may close conditionally on first-publish smoke if no release has yet been issued.

## 12. Documentation updates

### README

Present three truthful paths:

1. verified prebuilt installer for supported Linux/macOS hosts;
2. Cargo install from crates.io only if/once a crate is actually published;
3. source checkout/build for development or unsupported hosts.

Include `CODEGG_INSTALL_DIR`, pinned version, PATH guidance, supported target table, and note that installer does not configure/start the daemon as a service.

### RELEASING.md

Add post-upload installer smoke, supported artifact completeness, checksum verification, and instructions not to advertise installer compatibility if required assets are missing.

### Architecture/testing

Only a short note that installer fixture tests are change-specific; do not expand routine CI.

## 13. Acceptance criteria

- M001 artifact/checksum contract is closed and consumed exactly.
- All four supported OS/arch mappings select the correct asset.
- Explicit version input cannot escape the canonical release URL structure.
- Checksum verification is mandatory before extraction/install.
- Archive traversal/symlink/unexpected payload is rejected.
- Existing binary remains intact on every pre-commit failure path.
- Default install is user-local and never invokes sudo or edits PATH/profile automatically.
- Unsupported hosts fail with accurate Cargo/source alternatives.
- Offline fixture tests cover success and security/failure behavior without live GitHub dependence.
- README and release docs are truthful about supported platforms and publication availability.
- No release workflow, CI matrix, package-manager integration, or daemon-autostart feature is introduced.

## 14. Stop conditions

Stop and report when:

- M001 has not closed or its filenames/checksum syntax remain unstable;
- macOS/Linux POSIX tooling cannot safely validate/extract the chosen archive contract without adding a substantial installer dependency;
- required security would need signing/notarization rather than checksum integrity alone;
- the design requires arbitrary-origin configuration in production to make tests work;
- existing `codegg --version` cannot reliably identify the binary and fixing it becomes a larger CLI redesign;
- Windows/package-manager/service-install scope starts entering the implementation;
- release automation appears necessary—record a governance decision instead of adding it.

## 15. Closure evidence required

- implementation commits/PRs;
- final install script path and documented environment interface;
- target mapping table;
- offline fixture test results;
- checksum success/mismatch evidence;
- archive traversal/symlink negative evidence;
- existing-binary atomicity evidence;
- version/path injection negative tests;
- `sh -n` and any local shellcheck evidence actually run;
- README/RELEASING updates;
- formatting/lint/quick verification outcomes for repository changes;
- live published-release smoke if available, otherwise explicit conditional first-release smoke requirement;
- statement that no sudo/profile edits/daemon service/release automation were added.

## 16. Handoff notes

Keep the production download origin hard-coded. Testability should come from sourcing functions with injected fixture paths or a clearly test-only harness, not from a public `CODEGG_DOWNLOAD_URL` environment variable that turns the installer into arbitrary remote-code execution by configuration.

Do not automatically compile from source when a binary download fails. A checksum mismatch or missing asset is a release/distribution error that should remain visible; source installation is a deliberate alternative the user chooses.
