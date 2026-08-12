# Runtime Consolidation, Deletion, and Footprint M006 — Closure Status

Status: blocked

Source implementation plan: `plans/implementation/runtime-consolidation-deletion-footprint/006-measured-dependency-binary-cleanup.md`

Source subsystem roadmap: `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`

Repository baseline reviewed: `a32f720d` (post-M005 tree)

## 1. Executive finding

The M006 dependency and binary-footprint audit is complete, but strict closure
is blocked by the named hard dependency: M003 corrective physical extraction
has not yet moved the remaining permission/dispatch and context-policy bodies
out of `src/agent/loop.rs`. M006 therefore did not make speculative manifest,
feature, dependency, profile, or topology changes against the transitional tree.

The audit found no safe cleanup to land now. M006 should be rerun after
`plans/implementation/runtime-consolidation-deletion-footprint/008-m003-corrective-physical-extraction.md`
closes, so the final measurements describe the consolidated runtime.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Post-M005 feature tree reviewed | `cargo tree -e features --locked` | pass |
| Duplicate versions reviewed | `cargo tree -d --workspace --locked` | pass; duplicates have concrete owners or are dev/build-only |
| Root direct dependency reachability audited | Production `rg` audit across `src/` and workspace crates | pass; no unowned root declaration was identified |
| Default release binary measured | Isolated `CARGO_TARGET_DIR=/tmp/codegg-m006-default-target cargo build --release --locked --bin codegg` | 54,364,416 bytes |
| Production feature release binary measured | Isolated `CARGO_TARGET_DIR=/tmp/codegg-m006-production-target cargo build --release --locked --bin codegg --features server,plugins,lsp-test-support` | 63,583,200 bytes |
| Optional `cargo bloat` diagnostic | `cargo bloat --release --bin codegg --crates -n 30 --locked` | pass; 34.7 MiB text section, diagnostic only |
| No supported capability removed | `Cargo.toml`, feature tree, and source reachability review | pass |
| No profile semantic change accepted | Existing `lto`, `strip`, and `codegen-units` retained; `panic = "abort"` rejected as unneeded | pass |
| Required feature compilation passes | `cargo check -p codegg --locked --features server,plugins,lsp-test-support` | pass |
| Strict M006 closure | M003 corrective physical extraction | blocked |

## 3. Dependency and feature dispositions

- Wasmtime 36 remains unchanged. No freshness-only upgrade or runtime
  replacement is justified.
- Ratatui 0.29 and Crossterm 0.28 remain unchanged; no bounded maintenance
  migration was needed for this pass.
- Reqwest, Rustls, Tokio, SQLx, RustPython, and the TUI stack remain supported
  production dependencies with active consumers.
- The default feature set remains `arboard` only. Optional server, plugin,
  image, and LSP-test features remain available and compile as documented.
- Duplicate versions are retained where they belong to distinct dependency
  constraints or dev/build paths. No `[patch.crates-io]` override was added.
- `panic = "abort"` was not added: the measured opportunity does not justify
  changing panic/unwinding semantics without a separate semantic review.

## 4. Verification executed

Environment: `aarch64-apple-darwin`, Rust/Cargo `1.97.1`, locked dependency
resolution.

Passed:

```text
cargo tree -e features --locked
cargo tree -d --workspace --locked
cargo build --release --locked --bin codegg
cargo check -p codegg --locked --features server,plugins,lsp-test-support
cargo bloat --release --bin codegg --crates -n 30 --locked
```

`cargo bloat` was run as optional diagnostic tooling and is not a repository
dependency or CI requirement. The default and production-feature release builds
were performed in isolated temporary targets to keep the measurements
reproducible and independent of incremental artifacts.

Additional final-tree verification passed:

```text
scripts/verify.sh quick
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1
git diff --check
```

The workspace test run passed with 4,171 unit tests plus all integration and
workspace-crate suites. Hosted CI is owned by M007 and is not claimed here.

## 5. Invariant and compatibility review

No executable code, dependency graph, Cargo feature, release profile, public
protocol, storage schema, supported platform, binary topology, or CI workflow
changed. Existing default CLI/TUI, server/plugin/image feature gates, Wasmtime
runtime selection, and MSRV remain intact.

## 6. Roadmap disposition

M006 remains blocked. M007 remains blocked on M002–M006. No downstream plan is
promoted to ready by this record. After M003 corrective extraction closes, this
measurement pass must be repeated before M006 can be strictly closed.
