# Dependency Security and Workspace Consolidation M002 — Workspace Dependency Ownership Normalization

Status: ready

Repository baseline reviewed: `b3459640765261057e902a9df05bc71faa6cecc4`

Source subsystem roadmap:

- `plans/subsystems/dependency-security-workspace-consolidation-roadmap.md`

Hard predecessor:

- M001 accepted closure.

Primary class: infrastructure.

## 1. Objective

Make Cargo workspace policy the authoritative owner of shared dependency versions and stable package metadata while keeping crate-specific feature activation local and minimal.

This milestone is a maintenance refactor. It must not produce behavior changes, dependency-major upgrades, or a new global feature superset.

## 2. Current evidence

The root manifest declares the workspace but common versions and policies are repeated across member manifests. Examples include Tokio, Serde, Serde JSON, Thiserror, Chrono, SQLx, DashMap, Dirs, Regex, URL, tracing, UUID and internal `codegg-*` path/version pairs.

Repeated declarations increase version/feature drift risk and made the DashMap 5/6 split harder to see. At the same time, Cargo workspace dependency inheritance is feature-additive: centralizing a maximal Tokio/SQLx feature set would widen small crates and work against this roadmap.

## 3. Invariants

- Resolved production features must not widen merely because a dependency becomes workspace-owned.
- Package-specific features remain package-specific.
- Optional dependencies remain optional in the same packages/features.
- Internal crate dependency direction and `scripts/check-core-boundary.sh` remain unchanged.
- Package names, published version constraints, MSRV and public APIs remain compatible.
- Do not normalize author/package metadata where current packages intentionally differ.

## 4. Scope and non-goals

In scope:

- `[workspace.package]` for genuinely shared stable metadata;
- `[workspace.dependencies]` for dependencies repeated across multiple members;
- shared internal path+version declarations;
- `[workspace.lints]` only where the rule is valid across the participating crates;
- member-manifest conversion to `.workspace = true` plus local feature additions;
- active contributor/architecture documentation that describes dependency ownership.

Out of scope:

- dependency-major upgrades;
- root feature redesign;
- flattening crate boundaries;
- moving code solely to reduce manifest length;
- making every dependency workspace-owned if it has one consumer;
- forcing `unsafe_code = deny` onto targets with deliberate, reviewed unsafe code;
- new build scripts, manifest generators, dependency bots, or policy scanners.

## 5. Ordered work packages

### WP1 — Produce the ownership census

For every workspace manifest, classify repeated dependencies into:

1. same version + compatible default-feature policy — strong centralization candidate;
2. same version + intentionally different feature additions — centralize version/default policy, leave member features local;
3. intentionally different major/default policy — retain local ownership and document why;
4. single consumer — normally leave local.

Also classify package metadata. Prefer centralizing `edition`, `rust-version`, license/repository/homepage and possibly workspace version only when all participating packages actually share those semantics. Do not rewrite authors or descriptions for uniformity.

### WP2 — Add bounded workspace package/dependency baselines

Introduce the smallest useful `[workspace.package]` and `[workspace.dependencies]` sections.

The dependency baseline should generally avoid broad features. Member manifests may add features to workspace dependencies; use that mechanism rather than putting the union of all features in the workspace root.

For example, a shared SQLx baseline should encode version/default-feature policy, while root/core/providers request only the features they each need after M001.

Centralize internal crates with path plus exact version where their current release/versioning model is shared.

### WP3 — Convert members incrementally

Convert manifests by dependency family or package group and run `cargo metadata` / `cargo check` after each bounded set.

Do not combine this with unrelated source cleanup. The ideal production diff is almost entirely manifest text and lockfile-stable.

### WP4 — Workspace lint inheritance

Inspect repeated lint policy. Add `[workspace.lints]` only for rules that are true for the participating crates.

`unsafe_code = deny` may be inherited by crates that already enforce it, but deliberate low-level/test targets with reviewed unsafe code must keep explicit exceptions or remain outside that inherited rule. Do not turn this into a lint-policy redesign.

### WP5 — Prove feature equivalence

Capture before/after feature trees for sensitive families such as Tokio, SQLx, Eggfetch, Ratatui, Wasmtime, image and archive dependencies. The success condition is equivalent or narrower feature ownership, not merely successful compilation.

Use temporary diff/census output in closure evidence; do not add a permanent manifest linter.

## 6. Verification

Required:

```bash
cargo metadata --locked --format-version 1
cargo tree -d --locked
cargo tree -e features --locked
cargo check --workspace --all-targets --locked
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked -- --test-threads=1
scripts/check-core-boundary.sh
scripts/verify.sh quick
cargo fmt --all -- --check
```

A lockfile change is not expected solely from replacing repeated declarations with workspace inheritance. Any lock churn must be explained.

## 7. Acceptance criteria

- Repeated dependency versions/default policies have one workspace authority where doing so is semantically correct.
- Member manifests retain only their actual feature additions and package-specific dependencies.
- No dependency feature family widens unintentionally.
- Internal path/version declarations are centralized where release semantics are shared.
- Package metadata remains truthful; no cosmetic normalization changes ownership or attribution.
- Existing crate-boundary guards remain green.
- `Cargo.lock` is unchanged or every change is explained by an intentional manifest correction.
- Broad local verification passes.

## 8. Stop conditions

Stop and record a narrower disposition if:

- workspace inheritance changes optionality or feature resolution in a way that cannot remain local;
- internal crates do not in fact share release/version semantics;
- a lint baseline would require broad exception proliferation;
- lockfile churn indicates an accidental dependency change;
- implementation starts moving source code merely to justify manifest centralization.

## 9. Required closure evidence

Record:

- centralized dependency/package families and retained local exceptions;
- before/after feature-tree comparison for sensitive dependencies;
- lockfile disposition;
- broad verification results;
- unresolved findings and whether M003/M004 are now dependency-ready.
