# Dependency Security and Workspace Consolidation M005 — Generic Updater Interface and CodeGG Adoption

Status: blocked

Repository baseline reviewed: `b3459640765261057e902a9df05bc71faa6cecc4`

Source subsystem roadmap:

- `plans/subsystems/dependency-security-workspace-consolidation-roadmap.md`

Hard predecessor:

- M002 accepted closure.

Interface blocker:

- a generalized, independently consumable updater package must exist outside CodeGG with a stable written contract. Gregg's current `gregg-update` implementation is evidence for the design but remains Gregg-specific and is not itself the approved CodeGG dependency.

Primary class: infrastructure.

## 1. Objective

Replace CodeGG's shell-script/curl-based self-update execution path with a small generic binary-updater library that performs explicit candidate acquisition, checksum/identity validation, staging, and executable replacement while leaving CodeGG-specific CLI UX and release policy in CodeGG.

The security/maintenance objective is to share updater mechanics with other Eggstack binaries rather than duplicate them.

## 2. Current evidence

CodeGG currently:

- checks GitHub release metadata using `eggfetch-core`;
- constructs a version-pinned installer invocation;
- launches external `curl` to retrieve the repository installer script;
- relies on that downloaded shell script for the installation/update operation.

The Gregg workspace contains `gregg-update`, which already separates much of the desired mechanical surface: stable-version comparison, target mapping, release-asset/checksum retrieval, SHA-256 verification, staged candidate identity validation, Cargo fallback, permission checks and executable replacement. It is not directly reusable because its repository/target/release assumptions are Gregg-specific and parts of its transport execute external `curl`.

## 3. Required upstream interface before CodeGG work begins

The external generic updater package must provide a small contract equivalent in capability to:

- application identity: crate/program/current version;
- repository/release asset policy supplied by the caller rather than hard-coded Gregg constants;
- target detection/asset naming that is configurable or caller-provided;
- staged candidate abstraction with checksum verification;
- candidate identity/version validation hook;
- safe current-executable permission/preflight and replacement;
- explicit result types for already-current / binary update / source fallback where fallback is supported;
- no dependency on greggd, systemd, launchd, Windows service management, TUI state, or Eggpool;
- no implicit sudo/elevation;
- acquisition that is transport-injectable or natively supports a Rust HTTP client so CodeGG need not execute `curl`.

A package that merely renames `gregg-update` while retaining hard-coded `eggstack/gregg` release assumptions does not satisfy the blocker.

## 4. CodeGG invariants

- Update checks remain bounded and are never a startup correctness requirement.
- Release/version selection must not accept an unverified candidate.
- Update failures leave the existing executable intact.
- No shell script fetched from the network is executed by the normal update path after migration.
- No service-manager or daemon restart machinery is introduced.
- CodeGG's one-binary distribution contract remains unchanged.
- The installer may remain supported for fresh installation; this milestone only changes the in-place updater path.
- No release automation, signing/notarization, or fixed cadence is added.

## 5. CodeGG-side work packages after blocker resolution

### WP1 — Validate the external package

Inspect the published/generalized package source and packaged artifact. Confirm MSRV, dependency footprint, replacement semantics, target support, checksum handling and transport interface against the written blocker contract.

Stop if CodeGG would need Gregg-specific constants, external `curl`, or service lifecycle dependencies.

### WP2 — Define CodeGG release policy adapter

Keep CodeGG-specific facts in CodeGG:

- repository identity;
- binary/asset naming;
- supported targets;
- current version;
- UI/CLI output;
- whether unsupported targets may use Cargo fallback.

Do not move CodeGG release semantics into the generic updater package merely to avoid a small adapter.

### WP3 — Use Eggfetch/native Rust acquisition

Prefer CodeGG's existing Eggfetch transport profile for release metadata/assets if the generic updater supports injected acquisition. Preserve explicit timeout and redirect policy.

Download the exact candidate and checksum under a bounded temp/staging area. Verify checksum and candidate identity/version before replacement.

### WP4 — Replace `upgrade()` mechanics

Retire the network-fetched installer-script execution path from normal self-update. Preserve the user-facing `version`/update command contract as closely as practical.

Keep fresh-install `install.sh` support and its tests unless separately deprecated.

### WP5 — Failure-mode tests

Use deterministic local fixtures/test doubles for:

- already-current;
- valid newer candidate;
- missing asset/fallback policy;
- checksum mismatch;
- candidate reports wrong version/program identity;
- unwritable current executable;
- interrupted/failed download;
- replacement failure;
- unsupported target.

No public GitHub network dependency is required for tests.

## 6. Dependency/size policy

Before adopting the external crate, compare its closure to the current update path. A modest dependency increase is acceptable if it removes shell/curl execution and centralizes shared verified-update mechanics. Do not import a full daemon/service framework.

If the generic crate itself depends on Eggfetch, prefer feature unification with CodeGG's existing version/profile. If it uses another full HTTP/TLS stack, stop and assess whether the maintenance/security benefit still exists.

## 7. Verification

After the interface blocker resolves and CodeGG implementation lands:

```bash
cargo test --lib upgrade -- --test-threads=1
cargo tree -i <generic-updater-crate> --locked
cargo tree -d --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
scripts/verify.sh quick
```

Run release/install-script tests as regression coverage for fresh installation, even though the self-update path no longer executes the script.

## 8. Acceptance criteria

- CodeGG normal self-update does not download and execute a shell installer script.
- Candidate bytes are verified before executable replacement.
- Wrong checksum/program/version candidates fail closed.
- Existing executable remains valid on download/verification/replacement failure.
- No greggd/service-manager ownership enters CodeGG.
- No external `curl` is required by the CodeGG self-update path.
- Dependency graph reuses existing transport/security primitives where practical and does not introduce an unjustified second TLS/HTTP stack.
- Focused and broad verification are green.

## 9. Stop conditions

Keep M005 blocked or stop implementation if:

- no generalized package exists;
- the package remains Gregg-specific;
- adoption requires copying/forking its implementation into CodeGG;
- it requires system-service lifecycle machinery;
- it requires a second heavyweight HTTP/TLS stack without compelling evidence;
- safe executable replacement cannot preserve the current installation contract.

## 10. Required closure evidence

Closure must name the exact upstream package/version and contract, CodeGG adapter changes, candidate-verification tests, failure semantics, dependency-tree impact, broad verification, and any platform that remains on the legacy/manual update path.
