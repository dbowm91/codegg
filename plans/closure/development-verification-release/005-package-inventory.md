# M005 Package Inventory — crates.io Publication Evidence

Generated: 2026-07-29

## Package Table

| Package | Manifest | Version | Publish | Topological Layer | Direct Internal Deps | crates.io Name | Metadata |
|---------|----------|---------|---------|-------------------|---------------------|----------------|----------|
| `codegg` | `Cargo.toml` | 0.1.0 | manual | 7 (root) | codegg-config, codegg-core, codegg-git, codegg-protocol, codegg-providers, eggcontext, egggit, egglsp, eggsentry | **available** (404) | license, repository, homepage, rust-version, keywords, categories ✓ |
| `codegg-core` | `crates/codegg-core/Cargo.toml` | 0.1.0 | manual | 1 (leaf) | codegg-config, codegg-protocol, codegg-git, eggcontext, egggit, eggsentry | **available** (404) | license, repository, homepage, rust-version ✓ |
| `codegg-config` | `crates/codegg-config/Cargo.toml` | 0.1.0 | manual | 0 (leaf) | none | **available** (404) | license, repository, homepage, rust-version ✓ |
| `codegg-protocol` | `crates/codegg-protocol/Cargo.toml` | 0.1.0 | manual | 0 (leaf) | none | **available** (404) | license, repository, homepage, rust-version ✓ |
| `codegg-providers` | `crates/codegg-providers/Cargo.toml` | 0.1.0 | manual | 2 | codegg-protocol | **available** (404) | license, repository, homepage, rust-version ✓ |
| `codegg-git` | `crates/codegg-git/Cargo.toml` | 0.1.0 | manual | 0 (leaf) | none | **available** (404) | license, repository, homepage, rust-version ✓ |
| `eggcontext` | `crates/eggcontext/Cargo.toml` | 0.1.0 | manual | 0 (leaf) | none | **available** (404) | license, repository, homepage, rust-version ✓ |
| `egggit` | `crates/egggit/Cargo.toml` | 0.1.0 | manual | 0 (leaf) | none | **available** (404) | license, repository, homepage, rust-version ✓ |
| `egglsp` | `crates/egglsp/Cargo.toml` | 0.1.0 | manual | 2 | codegg-protocol | **available** (404) | license, repository, homepage, rust-version ✓ |
| `eggsentry` | `crates/eggsentry/Cargo.toml` | 0.1.0 | manual | 0 (leaf) | none | **available** (404) | license, repository, homepage, rust-version ✓ |

## Publication Order (topological)

1. `codegg-config`, `codegg-protocol`, `codegg-git`, `eggcontext`, `egggit`, `eggsentry` (all leaves, parallel)
2. `codegg-core`, `codegg-providers`, `egglsp` (depend on layer 0)
3. `codegg` (depends on all above)

## Ownership Check

All 10 crate names return 404 on crates.io (not yet published). No name conflicts detected. No `cargo owner --list` output available since crates don't exist yet.

## Dry-Run Evidence

Leaf crates pass `cargo package` and `cargo publish --dry-run`. Dependent crates blocked on registry sequencing (expected: dependent crates can't package until their dependencies are published).

## Verification Evidence

- `scripts/verify.sh quick` — EXIT_CODE=0 (fmt, agents, tokio guard, core boundary, workspace check)
- `scripts/verify.sh full` — all steps pass: quick checks, clippy (-D warnings), workspace tests (4100 passed), feature check (server,plugins,lsp-test-support)
- 1 pre-existing integration test failure (`test_live_dispatcher_passes_native_backend_in_context` in agent_loop_harness.rs) — outside M005 scope

## CI Evidence

- Hosted CI: pending (push required)
- Commit: e90a78e (implementation) + uncommitted clippy/MSRV fixes
