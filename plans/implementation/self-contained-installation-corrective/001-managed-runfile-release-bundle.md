# Self-Contained Installation M001 — Managed Runfile Release Bundle

Status: implemented (closure at
plans/closure/self-contained-installation-corrective/001-status.md;
implementation `f9ec8602`)

Corrective roadmap:
plans/subsystems/self-contained-installation-corrective-addendum.md

Historical distribution closures:

- plans/closure/distribution-installation/001-status.md
- plans/closure/distribution-installation/002-status.md

Repository baseline reviewed: 5af6766a04d6326ba1e7bb3695b2f0422399e9ee
Upstream eggsearch reviewed: 0.3.9

## 1. Objective

Replace the obsolete single-executable release invariant with an exact,
version-qualified bundle that installs CodeGG's required companion executables
together.

The end-user still installs CodeGG once and invokes only `codegg`.

## 2. Current evidence

- `scripts/release/package-binary.sh` packages one already-built `codegg`.
- `scripts/release/lib-release.sh`, release tests and `RELEASING.md` encode the
  one-member archive contract.
- `install.sh` sets `CODEGG_INSTALLER_MEMBER=codegg` and rejects archives that
  contain anything else.
- `Cargo.toml` defines `codegg-sandbox-helper`.
- production sandbox execution resolves only a canonical sibling
  `codegg-sandbox-helper`.
- default search currently launches external `eggsearch mcp stdio`.

The packaging contract and runtime contract are therefore inconsistent.

## 3. Canonical runfile manifest

Define the manifest centrally in `scripts/release/lib-release.sh` (or one equivalent
release authority) so packager, verifier, installer tests and docs consume the same
names.

For supported Unix archives require:

- `codegg`
- `codegg-sandbox-helper`
- `codegg-eggsearch`

Add a fixed, minimal third-party notice/license member if required for eggsearch
redistribution. Do not loosen archive validation to arbitrary extra files.

Use target-specific manifests where platform behavior genuinely differs. Explicitly
define Windows names/status rather than applying Unix assumptions implicitly.

## 4. Eggsearch provenance and pinning

Pin the bundled upstream eggsearch version in release metadata; baseline is 0.3.9.
Release packaging must accept an already-built upstream binary or build it in a
separate deterministic preparation step, then validate before packaging:

- expected binary identity/version;
- expected target architecture/OS where inspectable;
- regular executable file;
- no symlink/device/archive traversal;
- checksum recorded with the CodeGG release artifact inputs.

Rename the installed sidecar to `codegg-eggsearch` so it is clearly installation
owned and cannot accidentally resolve an unrelated PATH executable.

Do not compile CodeGG against eggsearch's internal modules solely to make packaging
easier. Upstream explicitly documents MCP/CLI as the stable application contract.

Record the eggsearch source/version/license in release documentation and notices.

## 5. Packaging changes

Update `package-binary.sh` (or split it into a small runfile-aware helper) to accept
all required already-built inputs. Stage only the allowlisted manifest, normalize
executable modes, inspect the staged archive member list, then atomically publish the
target archive.

Update:

- `lib-release.sh`;
- `package-binary.sh`;
- `finalize-release.sh`;
- `verify-release.sh`;
- `test-release-tools.sh`;
- `RELEASING.md`.

Checksums remain deterministic and cover the final archive. Do not weaken origin,
target-name or manifest validation from the closed distribution work.

## 6. Installer transaction

Update `install.sh` and `scripts/release/test-installer.sh` for the exact bundle.

Requirements:

- validate every archive member before extraction;
- reject absolute paths, traversal, links, devices, duplicate names and unexpected
  members;
- stage executable runfiles before modifying the destination;
- install the three siblings into one canonical directory;
- preserve executable modes;
- use backup/rollback or an equivalent bundle-level commit so an error while replacing
  helper N cannot silently leave a mixed old/new installation;
- do not follow an existing destination symlink into an attacker-controlled location;
- retain fixed GitHub release origin/checksum verification;
- cleanup staging/backup files on success and bounded best-effort recovery on failure.

The `codegg` user command path remains stable.

## 7. Source-build policy

A raw `cargo install codegg` cannot cause Cargo to install another package's
eggsearch binary. Do not claim that path is self-contained unless an intentionally
stable embedding/sidecar build mechanism is added later.

For this milestone:

- the supported end-user prebuilt installer is the self-contained contract;
- source developers may build/provide the pinned eggsearch sidecar through documented
  repository tooling;
- docs must clearly distinguish source/developer installation from the supported
  prebuilt bundle;
- if the project is not yet publishing release assets, do not advertise the
  self-contained installer as available until the first qualifying release exists.

## 8. Tests

Expand release tests to cover:

- exact per-target runfile manifest;
- missing helper and missing eggsearch rejection;
- unexpected member/traversal/symlink/device rejection;
- wrong eggsearch version/identity rejection;
- executable permissions;
- deterministic checksum/finalization;
- successful fresh install with all siblings;
- upgrade from historical single-codegg layout;
- rollback when replacement of the second/third runfile fails;
- rerun/idempotent install;
- no credential/config/source leakage in archives.

## 9. Verification

- `scripts/release/test-release-tools.sh`
- `scripts/release/test-installer.sh`
- `scripts/release/verify-release.sh` against fixture artifacts
- shellcheck/available repository shell checks
- `cargo fmt --all -- --check`
- `scripts/verify.sh quick`
- `git diff --check`

If target binaries are available, verify the final archive by extracting into a
temporary install root and executing `codegg --version`,
`codegg-sandbox-helper --help`/safe identity probe, and
`codegg-eggsearch --version`.

## 10. Acceptance

Close only when the canonical release verifier rejects any artifact that cannot
satisfy the runtime sibling contracts and the installer can upgrade/fresh-install the
whole bundle without relying on a pre-existing eggsearch executable.

The old “exactly one top-level executable” assertion must be removed or superseded
everywhere, not merely bypassed.
