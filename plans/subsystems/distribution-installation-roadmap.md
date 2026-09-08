# Distribution and Installation Roadmap

Status: closed

Long-term references:

- `plans/000-long-term-specification.md#1-product-definition`
- `plans/000-long-term-specification.md#2-primary-product-goals`
- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Related roadmaps and evidence:

- `plans/subsystems/development-verification-release-roadmap.md` — manual cadence/publication and minimal hosted verification remain authoritative;
- `RELEASING.md` — current manual crates.io and optional GitHub binary-release procedure;
- `plans/closure/development-verification-release/006-package-inventory.md` — workspace package ordering for crates.io, intentionally not reimplemented here.

Related ADRs:

- None. This roadmap preserves the single `codegg` executable, user-scoped daemon topology, manual release cadence, and existing runtime architecture. A future proposal to split daemon/TUI binaries or make hosted automation authoritative for releases would require separate justification.

## 1. Purpose and ownership boundary

CodeGG's runtime and feature set have outgrown its end-user distribution surface. This roadmap makes supported binary installation a first-class manual release product without rebuilding the CI/release apparatus that was intentionally simplified.

It owns:

- the canonical naming/content/checksum contract for prebuilt `codegg` release assets;
- a small maintainer packaging/validation procedure for those assets;
- documented required binary targets for the supported installer path;
- a simple user-facing POSIX installer for supported Linux/macOS hosts;
- README/release documentation for choosing binary installer versus Cargo/source install;
- post-install smoke/version verification.

It does not own release cadence, version choice, crates.io credentials, automated publishing, a package-manager ecosystem, Windows support-tier expansion, or runtime service installation/daemon autostart.

## 2. Work classification

### Invariants

- Release cadence and the decision to publish remain manual maintainer actions.
- Routine GitHub Actions remains the existing bounded verification job; binary distribution MUST NOT add a required release/build matrix to ordinary CI.
- The release artifact remains one `codegg` executable per target. No daemon/TUI binary split is introduced.
- Every supported installer artifact is covered by a SHA-256 checksum manifest and the installer fails closed on mismatch.
- The installer downloads only from the canonical GitHub repository over HTTPS.
- The installer never requires or invokes `sudo` automatically.
- Unsupported OS/architecture combinations fail with an actionable source/Cargo-install alternative rather than downloading a guessed target.
- Release archives do not contain user configuration, credentials, databases, planning files, or build-tree debris.
- Binary installation does not imply automatic daemon startup or system service registration.

### Capabilities

- A maintainer can turn a verified target binary into a deterministic, correctly named release asset and checksum set.
- A manually created GitHub Release can carry a complete supported Linux/macOS binary set.
- Linux/macOS users can install or upgrade with one small shell installer into a user-writable directory.
- Users can pin a specific release version or choose the repository's latest release.
- Installation verifies checksum before replacing an existing binary and validates the installed binary's version output.

### Infrastructure

- `scripts/release/` or equivalent small packaging/validation helpers.
- A stable artifact naming convention.
- `checksums.txt` generated from the exact uploaded artifacts.
- `install.sh` at a stable repository path.
- Manual release instructions describing native/cross-host build responsibility and upload order.

### Polish

- README copy/paste installation command and alternatives.
- Actionable diagnostics for unsupported target, missing tools, checksum mismatch, unwritable install directory, and version mismatch.
- Removal/correction of stale text calling binary releases merely hypothetical once this roadmap lands.

## 3. Non-goals

- Automated GitHub Release creation.
- Tag-triggered, scheduled, or workflow-dispatch publication.
- New mandatory CI lanes or cross-platform test matrices.
- Fixed release cadence.
- Homebrew, MacPorts, apt/deb, rpm, Nix, Scoop, Chocolatey, Winget, MSI, PKG, DMG, AppImage, Flatpak, or container packaging.
- Windows installer support in this roadmap. Existing best-effort Windows build instructions may remain, but Windows does not become a guaranteed support tier.
- Binary signing/notarization, SBOM, provenance attestations, or supply-chain signing in this initial distribution slice.
- Automatic source compilation fallback inside the installer.
- Automatic modification of shell startup files or `PATH`.
- System service/launchd/systemd installation or daemon autostart.
- Splitting CodeGG into multiple executables.

## 4. Current state

`RELEASING.md` already establishes a sound governance boundary:

- crates.io publication is manual;
- GitHub Actions does not publish, tag, or create releases;
- optional manual GitHub binary release uses `gh release create`;
- the release topology is one `codegg` executable;
- documented build targets include Apple Silicon/macOS x86_64, Linux x86_64/aarch64, and Windows x86_64.

What is missing is a stable asset contract and end-user installer. There is no supported copy/paste path that maps host OS/architecture to a known release asset, verifies its checksum, and installs it atomically into a user directory.

The previous verification/release roadmap intentionally deferred package-distribution expansion. The September 2026 audit and explicit maintainer request now promote a bounded binary/installer slice. That product prioritization does not reverse the decision to keep cadence and publication manual.

## 5. Target architecture

### Supported binary release set

The initial supported installer set is:

```text
x86_64-unknown-linux-gnu
aarch64-unknown-linux-gnu
x86_64-apple-darwin
aarch64-apple-darwin
```

Windows may continue to receive a manually built artifact when available, but it is not required for installer-set completeness and does not become a supported platform tier under this roadmap.

### Asset contract

A GitHub release tagged `v<VERSION>` contains stable per-target asset names scoped by the release tag:

```text
codegg-x86_64-unknown-linux-gnu.tar.gz
codegg-aarch64-unknown-linux-gnu.tar.gz
codegg-x86_64-apple-darwin.tar.gz
codegg-aarch64-apple-darwin.tar.gz
checksums.txt
```

Each tarball contains a minimal top-level payload, preferably:

```text
codegg
LICENSE*
README.md        # optional if the packaging helper can include it consistently
```

The executable filename inside the archive is always `codegg`. Do not embed credentials, config, user state, target directories, or planning evidence.

`checksums.txt` contains SHA-256 entries for every uploaded binary archive. Ordering is deterministic. The installer retrieves the manifest and verifies the exact selected archive before extraction/install.

Stable asset names are intentional: version identity comes from the GitHub release/tag, enabling both:

```text
/releases/latest/download/<asset>
/releases/download/v<VERSION>/<asset>
```

without scraping GitHub APIs.

### Maintainer workflow

```text
maintainer chooses version/cadence
  -> existing verification/release preflight
  -> build codegg --release for each intended target on an appropriate host/toolchain
  -> package each already-built binary through a small deterministic helper
  -> validate the complete release set and checksums
  -> manually create/tag GitHub Release and upload assets
  -> run installer smoke checks against the published release
```

The helper does not become a cross-compilation orchestration system. Building remains ordinary Cargo/toolchain work on appropriate hosts.

### Installer workflow

```text
detect uname OS + machine
  -> map to one supported target
  -> resolve latest or validated explicit version
  -> create private temporary directory
  -> download checksums.txt + exact target archive from fixed GitHub HTTPS origin
  -> verify SHA-256
  -> extract only expected payload
  -> verify executable/version
  -> atomically install to $CODEGG_INSTALL_DIR/codegg
  -> print PATH guidance if destination is not on PATH
```

Default install directory: `${HOME}/.local/bin` unless `CODEGG_INSTALL_DIR` is set.

No automatic Cargo fallback is used. The script reports the documented `cargo install`/source alternative when no supported binary exists. This keeps an unavailable binary or checksum failure from being silently converted into a long, semantically different build operation.

## 6. Dependency graph

```text
M001 — Manual prebuilt artifact contract
   |
   v
M002 — Verified user installer and installation docs
```

Dependency classes:

- M001 has no hard runtime dependency. It consumes the closed manual release policy and current single-binary topology.
- M002 is hard-dependent on M001 because installer target mapping, filenames, manifest format, and release completeness must be stable first.
- The active CI reproducibility corrective work is operationally important before an actual release but does not block authoring/implementing M001 packaging mechanics. No binary release should be published from a red mainline candidate.

## 7. Milestones

### M001 — Manual prebuilt artifact contract

Class: infrastructure / capability.

Status: closed — see `plans/closure/distribution-installation/001-status.md`.

Implementation plan:

- `plans/implementation/distribution-installation/001-manual-prebuilt-artifact-contract.md`

Objective:

Define and implement the stable release-asset/checksum contract and small maintainer packaging/validation helpers for the four supported Linux/macOS targets while preserving manual release authority.

Deliverable boundary:

- artifact naming/content contract;
- packaging helper for an already-built target binary;
- deterministic checksum-manifest generation/validation;
- release-set completeness validation;
- documented build/package/upload procedure;
- archive smoke/extraction/version checks;
- no workflow automation.

Exit conditions:

- all four required target names map unambiguously to one asset each;
- packaging an already-built binary produces only expected files;
- checksum manifest covers exactly the release archives intended for upload;
- validation detects missing/duplicate/wrongly named/tampered assets;
- manual GitHub Release instructions use the stable contract;
- one binary topology remains intact.

### M002 — Verified installer and end-user installation

Class: capability / polish.

Status: closed — see `plans/closure/distribution-installation/002-status.md`.

Implementation plan:

- `plans/implementation/distribution-installation/002-installer-and-end-user-installation.md`

Objective:

Add a minimal POSIX installer that securely maps supported Linux/macOS hosts to M001 release assets, verifies checksums, installs atomically to a user directory, and provides clear alternatives for unsupported hosts.

Deliverable boundary:

- `install.sh` or equivalent stable installer path;
- OS/architecture mapping for four required targets;
- latest/explicit-version resolution using fixed GitHub release URLs;
- checksum verification with standard platform tools;
- safe temporary extraction and atomic replacement;
- configurable user install directory;
- post-install version smoke;
- README/release docs and installer tests using local fixtures/mock download seam rather than live GitHub network.

Exit conditions:

- supported host mappings select the correct M001 asset;
- unsupported hosts fail before download with actionable Cargo/source instructions;
- checksum mismatch prevents extraction/install;
- script never invokes sudo or edits shell profile;
- version/install directory input is validated against injection/path hazards;
- existing binary is not replaced until verification succeeds;
- installer behavior is testable without publishing a real release;
- published-release smoke procedure is documented for maintainers.

## 8. Cross-cutting requirements

### Storage and migration

No production data/schema migration. Installer replaces only the target executable path selected by the user. It does not migrate CodeGG config, databases, credentials, sockets, or daemon state.

### Protocol and compatibility

No runtime protocol change. Binary release must represent the normal `codegg` executable and preserve its existing CLI/version behavior.

### Security and authorization

- Fixed canonical HTTPS origin.
- Validate explicit version before constructing a URL.
- Quote every shell variable/path.
- Use private temp directory and cleanup trap.
- Verify checksum before extraction/install.
- Extract only the expected archive and verify an expected regular executable; reject path traversal/unexpected payload where practical.
- Never execute downloaded helper scripts.
- No sudo, shell-profile modification, credential handling, or daemon service installation.
- Failure leaves the existing executable intact.

### Concurrency, cancellation, and recovery

Packaging helpers operate on explicit files and use temporary output where needed before rename. Installer interruption before final rename leaves the existing installed binary intact and removes temporary files on normal shell exit/signals supported by trap.

Two concurrent installers to the same destination are not a supported coordination mechanism; atomic final replacement prevents a partially written executable, but operators should serialize installs.

### Observability and audit

Helpers print target, version, output filename, checksum, and validation result but never secrets. Installer prints source release/version, target mapping, destination, and final version.

### Performance and resource use

Archives remain minimal. No source compiler/toolchain is downloaded by the installer. Packaging does not invoke a matrix build.

### Documentation and operations

`README.md` should present binary installer as the low-friction path once M002 closes, with Cargo/source install as explicit alternatives. `RELEASING.md` remains the maintainer authority and must describe manual artifact production/upload and post-publish smoke.

## 9. Verification strategy

Use local synthetic release directories and small fixture binaries/scripts for packaging/installer unit-shell tests. Validate naming, checksum success/failure, OS/arch mapping, version parsing, install directory handling, archive traversal/unexpected payload defense where feasible, and atomic replacement behavior.

Do not make live GitHub release availability a routine CI/test requirement. Actual release smoke is maintainer evidence after a manual publication.

## 10. Risks and decision points

- Cross-building GNU Linux aarch64 or macOS targets can require host/linker/toolchain support. This roadmap intentionally packages already-built binaries rather than owning cross-compilation infrastructure.
- macOS signing/notarization may become relevant if Gatekeeper/user expectations make unsigned CLI distribution impractical. That would require a later separately justified distribution-security milestone.
- A future guaranteed Windows tier needs its own installer/archive/service semantics and support evidence.
- If crates.io publication remains unavailable because package names/metadata are blocked, binary distribution still functions; do not make M001/M002 depend on crates.io.

## 11. Completion definition

This roadmap closes when maintainers have a stable manual artifact/checksum contract for four Linux/macOS targets and users have a checksum-verifying, non-root, atomic installer for those artifacts, with documentation that preserves manual release cadence and honest platform support.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | closed | `plans/implementation/distribution-installation/001-manual-prebuilt-artifact-contract.md` | `plans/closure/distribution-installation/001-status.md` | — |
| M002 | closed | `plans/implementation/distribution-installation/002-installer-and-end-user-installation.md` | `plans/closure/distribution-installation/002-status.md` | — |
