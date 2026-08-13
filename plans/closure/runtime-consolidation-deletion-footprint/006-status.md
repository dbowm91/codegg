# Runtime Consolidation, Deletion, and Footprint M006 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/runtime-consolidation-deletion-footprint/006-measured-dependency-binary-cleanup.md`

Source subsystem roadmap: `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`

Historical baseline: `a32f720d` (post-M005 tree; the earlier blocked audit is
preserved below rather than rewritten away)

Final candidate: `c8c31d909310131ca4b1cc38c725e0163f86a47d`

Implementation commit: `c8c31d90` — final corrective runtime consolidation

## 1. Executive finding

M006 is strictly closed on the post-M009 production tree. The final feature,
dependency, reachability, and isolated release measurements were completed
without forced dependency upgrades, feature removal, profile changes, or
topology changes. The earlier M006 blocked disposition was correct for its
transitional baseline and remains historical evidence.

## 2. Requirement-to-evidence matrix

| Requirement | Final evidence | Result |
|---|---|---|
| Feature graph reviewed | `cargo tree -e features --locked` on `c8c31d90` | pass |
| Duplicate versions reviewed | `cargo tree -d --workspace --locked` | pass; retained duplicates have concrete dependency/dev/build owners |
| Root/workspace reachability audited | production source/dependency reachability review | pass; no safe unowned declaration identified |
| Default release binary measured in isolation | `CARGO_TARGET_DIR=/tmp/codegg-m009-default-target cargo build --release --locked --bin codegg` | 54,347,840 bytes |
| Production-feature release binary measured in isolation | `CARGO_TARGET_DIR=/tmp/codegg-m009-production-target cargo build --release --locked --bin codegg --features server,plugins,lsp-test-support` | 63,566,624 bytes |
| Supported capabilities retained | manifest, feature tree, source reachability, and feature compile review | pass |
| Release profile semantics preserved | existing LTO/strip/codegen settings retained; no `panic = "abort"` | pass |
| Required feature compilation | `cargo check -p codegg --locked --features server,plugins,lsp-test-support` | pass |
| Strict M006 closure | M009 production corrections complete before measurement | pass |

## 3. Measurement interpretation

The final default size is 54,347,840 bytes and the final production-feature
size is 63,566,624 bytes. Relative to the earlier M006 diagnostics these are
16,576 bytes smaller; relative to the earlier M007 default diagnostic the
final default is 48 bytes smaller. These are measurements, not CI gates, and
no causal size claim is made for an individual code move.

The feature tree and duplicate tree were inspected for the default and
production surfaces. Wasmtime, Ratatui/Crossterm, Reqwest/Rustls, Tokio, SQLx,
RustPython, and the existing optional server/plugin/image/LSP-test surfaces
remain supported. No `[patch.crates-io]` override or freshness-only upgrade
was justified. Optional `cargo bloat` diagnostics were not required for strict
closure and are not claimed as repository or CI requirements.

## 4. Verification executed

Environment: `aarch64-apple-darwin`, Rust/Cargo `1.97.1`, locked dependency
resolution.

Passed on the final candidate:

```text
cargo fmt --all -- --check
cargo check -p codegg --lib --locked
cargo check -p codegg --locked --features server,plugins,lsp-test-support
scripts/verify.sh quick
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1
git diff --check
```

The capped workspace run completed successfully across all workspace crates,
integration suites, and doc tests. Hosted CI evidence is recorded in M007 and
M009 because the final candidate is shared by those closure records.

## 5. Invariant and compatibility review

No executable code, dependency graph, Cargo feature, release profile, public
protocol, storage schema, supported platform, binary topology, or CI workflow
was changed by M006. The TUI correction reuses existing durable schedule/job
tables and services; it does not restore legacy in-memory task persistence.

## 6. Roadmap disposition

M006 is closed. M007 is closed by the subsequent exact-candidate integration
record. No unrelated registered plan was promoted solely by M006; the
Development Verification and Release M006 plan remains blocked on its
independent Provider M007 and Tool Programs M019 closure dependencies.
