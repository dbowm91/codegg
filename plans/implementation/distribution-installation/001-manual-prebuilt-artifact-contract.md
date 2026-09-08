# Distribution and Installation Milestone 001 — Manual Prebuilt Artifact Contract

Status: ready for handoff

Repository baseline: `15632a0483a8c4b9d573ff2ce43297b29be8f42a`

Source roadmap:

- `plans/subsystems/distribution-installation-roadmap.md#7-milestones`

Related governance:

- `plans/subsystems/development-verification-release-roadmap.md`
- `RELEASING.md`

Long-term requirements:

- `plans/000-long-term-specification.md#1-product-definition`
- `plans/000-long-term-specification.md#2-primary-product-goals`
- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Applicable ADRs: none.

Primary class: infrastructure / capability

## 1. Objective

Define and implement a stable, manually produced GitHub binary-release artifact contract for CodeGG's supported Linux/macOS installer targets, including deterministic asset names, minimal archive contents, SHA-256 manifest generation, release-set validation, and maintainer documentation, without adding release automation or changing the single-binary topology.

## 2. Why this milestone is ready

The repository already has:

- a single `codegg` executable release topology;
- manual version/cadence authority;
- documented Cargo release builds for macOS, Linux, and Windows targets;
- an optional manual `gh release create` step;
- an existing closed verification/release roadmap that explicitly rejected automated release ownership.

What is missing is a concrete artifact interface that an installer can rely on. No runtime architecture change is needed.

The supported initial installer target set is already fixed by the distribution roadmap:

```text
x86_64-unknown-linux-gnu
aarch64-unknown-linux-gnu
x86_64-apple-darwin
aarch64-apple-darwin
```

M001 does not need M002 and does not need live GitHub publication to be implemented or tested.

## 3. Current implementation evidence

`RELEASING.md` currently documents direct `cargo build --release --target ...` commands and an optional GitHub release command that uploads `release/codegg-*` plus `release/checksums.txt`, but it does not define:

- exact archive names;
- archive content layout;
- deterministic checksum-manifest semantics;
- a validator for required target completeness;
- how to package a binary built on another host/toolchain;
- how to distinguish required Linux/macOS assets from optional Windows assets;
- a stable interface for a future installer.

The root crate/package release path and crates.io package graph are separate concerns. This milestone packages the already-built executable and does not require publishing any crate.

## 4. Invariants that must not regress

- Release/version/cadence decisions remain manual.
- GitHub Actions does not build/upload/publish release assets as part of this milestone.
- The release payload remains one `codegg` executable per target.
- Required target asset names are stable across versions; the GitHub release tag carries version identity.
- Every required archive appears exactly once in `checksums.txt`.
- Checksums use SHA-256 and are generated from the exact files that will be uploaded.
- Packaging cannot accidentally include `target/`, `.git`, configs, databases, credentials, planning files, logs, or unrelated source-tree content.
- A target label is supplied explicitly and validated against the supported set; a packaging script must not guess a cross-built binary's target from the host alone.
- Validation is local/offline and does not require a live GitHub release.
- Windows remains optional/best-effort under the existing release documentation and is not required for M001 closure.

## 5. Scope

### In scope

- Add a small release-packaging directory/script surface under `scripts/release/` or an equivalent existing scripts location.
- Define canonical asset names for four required targets.
- Package an explicit already-built `codegg` binary into a minimal `.tar.gz` archive.
- Validate that the input is a regular file and plausibly executable for the intended release flow.
- Generate deterministic `checksums.txt` entries for release archives.
- Validate a complete release directory: exact required filenames, no duplicates, checksum correctness, expected archive payload, version metadata/smoke where runnable.
- Allow optional extra supported artifacts only when validation treats them explicitly rather than silently folding unknown files into the checksum set.
- Update `RELEASING.md` with build → package → validate → manual upload order.
- Add focused shell/script tests with fixture executables/files.

### Explicitly out of scope

- Cross-compilation orchestration.
- GitHub Actions release jobs.
- Automated tags/releases.
- Signing/notarization.
- SBOM/provenance.
- Windows installer/archive requirements.
- Homebrew/deb/rpm/etc.
- Crates.io publication or package-name ownership.
- Automatic version bumps/changelog generation.
- Runtime daemon/service installation.

## 6. Required production changes

### Core/domain

No runtime Rust production change should be necessary unless `codegg --version` is currently unusable as a release smoke signal. If version output is broken/missing, a tiny CLI correction is allowed but must preserve existing CLI compatibility and should be split/documented clearly in closure.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None. Packaging helpers are maintainer tooling, not daemon runtime.

### Frontend or operator surface

The maintainer-facing script interface should be explicit, for example:

```bash
scripts/release/package-binary.sh \
  --target aarch64-apple-darwin \
  --binary target/aarch64-apple-darwin/release/codegg \
  --out-dir release

scripts/release/finalize-release.sh --dir release
scripts/release/verify-release.sh --dir release
```

Exact filenames may differ. Keep the number of scripts small. A single script with `package`, `finalize`, and `verify` subcommands is also acceptable if simpler.

The contract must produce:

```text
release/
  codegg-x86_64-unknown-linux-gnu.tar.gz
  codegg-aarch64-unknown-linux-gnu.tar.gz
  codegg-x86_64-apple-darwin.tar.gz
  codegg-aarch64-apple-darwin.tar.gz
  checksums.txt
```

Archive payload must contain exactly the required executable plus any explicitly approved static documentation files. The preferred minimum is just `codegg`; including LICENSE/README is acceptable only if consistent and tested.

### Security and authorization

- Treat binary/target/output paths as untrusted shell arguments: quote them and reject option/target injection.
- Use a private temp directory with trap cleanup.
- Do not use `eval` or construct shell commands from unchecked text.
- Reject symbolic-link input for the release executable unless behavior is explicitly justified; package the intended regular file, not an arbitrary symlink target.
- Never traverse arbitrary source directories to assemble an archive.
- Create archive from a controlled staging directory.
- Verify archive member names do not contain absolute paths or `..` components.
- Checksum generation and validation must operate on exact basenames in the release directory.

### Documentation and static guards

Update `RELEASING.md` to make the artifact contract canonical. README end-user installer docs belong to M002, though M001 may mention that these assets are the supported installer inputs.

No CI static guard is needed. The release verifier is explicitly invoked by the maintainer before manual upload.

## 7. Ordered work packages

### Work package A — Lock the artifact naming/content contract

Intent: create the stable interface M002 will consume.

Required changes/actions:

1. Define a central allowlist of four required target triples in the packaging tooling.
2. Map each target to exact archive basename `codegg-<target>.tar.gz`.
3. Define archive member layout and executable permissions.
4. Define deterministic `checksums.txt` syntax, preferably conventional two-column SHA-256 output compatible with `sha256sum -c` where possible.
5. Document optional Windows treatment separately; do not make unknown filenames valid by default.

Acceptance evidence:

- fixture tests assert exact filenames/member paths/permissions;
- same target always produces same contract name independent of host.

### Work package B — Implement safe packaging helper

Intent: package one explicit built binary at a time without owning builds.

Required changes:

- validate target string against allowlist;
- validate input regular file;
- create controlled staging dir;
- copy binary as `codegg` with executable mode;
- create tar.gz at a temporary output path then atomically rename into release directory;
- avoid embedding host-dependent absolute paths/owners where available tooling permits normalization.

Acceptance evidence:

- package fixtures for every target label;
- invalid target, missing input, directory input, symlink/path hazard, and existing-output behavior tested.

### Work package C — Generate and verify checksum manifest

Intent: bind upload set to installer-verifiable integrity metadata.

Required changes:

- calculate SHA-256 for exact supported archives present;
- sort manifest deterministically by basename;
- overwrite manifest atomically only after all hashes succeed;
- verify manifest entries reference no absolute/traversal paths;
- validator recomputes hashes and fails on missing/extra/duplicate required asset.

Acceptance evidence:

- tampered archive fails;
- missing target fails release-set completeness;
- duplicate/unknown required-like asset fails clearly;
- manifest stable across repeated generation for unchanged files.

### Work package D — Validate archive payload and version smoke

Intent: prevent structurally valid but unusable uploads.

Required changes:

- inspect each tarball and require expected member set;
- reject traversal/absolute/unexpected executable names;
- where target is runnable on current host, extract to temp dir and execute `codegg --version`, comparing against package/release version expectation if available;
- where not runnable, document that binary execution smoke is performed on the build/native host before packaging and record it in release checklist.

Do not add binary-format parsers merely to inspect target architecture unless an existing standard host tool can do so portably and simply.

Acceptance evidence:

- wrong archive payload rejected;
- native fixture/runnable binary smoke covered.

### Work package E — Reconcile manual release documentation

Intent: make maintainers use the contract consistently.

Required changes:

Update `RELEASING.md` sequence:

1. choose version and verify mainline;
2. build required targets on appropriate hosts/toolchains;
3. run native `codegg --version` smoke for each built binary;
4. package each with the helper;
5. generate/verify complete checksums/release set;
6. inspect no unintended files;
7. create Git tag/release manually;
8. upload exactly the validated assets;
9. perform post-publication checks (M002 will add installer smoke).

Acceptance evidence: release doc has one artifact naming source of truth and no stale wildcard contract that could include arbitrary files.

## 8. Failure, cancellation, restart, and contention semantics

Packaging failure must leave no final archive at the target filename unless that archive was already valid from a previous run. Use temp output + rename.

Checksum generation failure must not leave a partially written `checksums.txt` representing only some archives.

Validation is read-only except temporary extraction. Interruption cleans temporary staging where practical.

Concurrent packaging to the same target/output directory is unsupported; scripts should refuse to overwrite an existing final archive by default or require an explicit safe replacement mode. Maintainers serialize release packaging.

Re-running validation is idempotent.

## 9. Compatibility and migration

No user runtime migration.

`RELEASING.md` currently treats GitHub binary release as optional and uses broad `release/codegg-*` upload syntax. Once M001 closes, manual GitHub binary releases that claim installer support must use the stable four-target asset/checksum contract. Historical releases need not be repackaged.

Crates.io release ownership remains unchanged and independent.

## 10. Required tests

### Focused unit/script tests

- target allowlist and filename mapping;
- valid packaging of fixture executable;
- executable mode preserved inside archive;
- missing/invalid/symlink input rejected;
- output path quoted safely;
- checksum generation deterministic;
- tampered file mismatch;
- missing required target rejected;
- unexpected/traversal archive member rejected.

### Integration tests

- create four fixture archives → finalize checksums → verify complete set;
- alter one byte → verification fails;
- omit one target → verification fails;
- rerun verification unchanged → succeeds.

### Restart and recovery tests

Not applicable; verify idempotent rerun after a simulated partial temp output does not accept the partial file.

### Contention and cancellation tests

No parallel release support required. Test cleanup on command failure/signals where practical for shell tooling.

### Security and negative tests

- malicious target string rejected;
- malicious path/archive member cannot escape staging/extraction;
- unknown artifact cannot be silently considered required/supported;
- no source/config/credential fixture gets included unless explicitly staged.

### Migration and compatibility tests

No runtime migration. Ensure `RELEASING.md` still preserves manual crates.io procedure and single-binary topology.

## 11. Required verification commands

The implementation should add one focused release-tool test entry, for example:

```bash
# exact name depends on implementation
scripts/release/test-release-tools.sh

# package a locally built native codegg and verify its archive when feasible
cargo build --release
scripts/release/package-binary.sh --target <native-target> --binary target/release/codegg --out-dir /tmp/codegg-release-test
scripts/release/verify-release.sh --dir /tmp/codegg-release-test --allow-incomplete-target-set

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Do not require building all four targets during routine verification. Full four-target completeness is maintainer release evidence, not ordinary CI.

If a helper needs an `--allow-incomplete-target-set` mode for per-host testing, final release validation must default to complete-set enforcement and the relaxed mode must be explicit.

## 12. Documentation updates

- `RELEASING.md` — canonical artifact contract and manual workflow.
- `plans/subsystems/distribution-installation-roadmap.md` only if implementation needs a material contract adjustment.
- Possibly `architecture/testing.md` to state release-tool tests are optional/change-specific, not routine CI expansion.
- README installer instructions are M002.

## 13. Acceptance criteria

- Four required Linux/macOS target archive names are stable and documented.
- Packaging consumes an explicitly built binary; it does not become a build matrix.
- Archive payload is minimal and safe.
- Checksums are generated deterministically and validation fails on missing/tampered/malformed release sets.
- Maintainer can validate release assets entirely before uploading them.
- Manual cadence/publication and one-binary topology remain unchanged.
- No new GitHub Actions workflow/job/release automation is added.
- M002 has a precise target/asset/checksum interface to implement against.

## 14. Stop conditions

Stop and report when:

- building/package validation appears to require a new cross-compilation/release framework rather than packaging already-built binaries;
- target support cannot be demonstrated on appropriate native/cross hosts and the contract would be aspirational;
- single-binary topology changes;
- macOS distribution is unusable without signing/notarization and that becomes a release-blocking requirement;
- the implementation starts altering crates.io cadence/credentials/automation;
- a new hosted release workflow becomes necessary to meet the proposed design—record that as a policy decision instead of adding it silently.

## 15. Closure evidence required

- implementation commits/PRs;
- final target/asset/member/checksum contract;
- packaging and validation script paths;
- fixture/script test outcomes;
- one native real `codegg` packaging/version smoke where feasible;
- sample complete synthetic release-set verification;
- tamper/missing-asset negative evidence;
- updated `RELEASING.md` excerpt/summary;
- explicit confirmation no release workflow/CI matrix was added;
- formatting/lint/quick verification outcomes actually run;
- any target that remains unverified and why.

## 16. Handoff notes

Keep these scripts boring. They are release hygiene, not a release orchestration product.

Do not embed the version in the asset basename. GitHub's release/tag already provides version identity, and stable names make the M002 latest-release URL both simple and API-free.
