# Dependency Security and Workspace Consolidation M001 — Security and Duplicate Graph Convergence

Status: ready

Repository baseline reviewed: `b3459640765261057e902a9df05bc71faa6cecc4`

Source subsystem roadmap:

- `plans/subsystems/dependency-security-workspace-consolidation-roadmap.md`

Predecessors: none. This plan is dependency-ready.

Primary class: infrastructure + invariant.

## 1. Objective

Remove the currently actionable dependency-security and duplicate-major findings that CodeGG directly controls, without broad dependency modernization or behavior changes.

The milestone owns three bounded changes:

1. move the direct Ratatui/TUI dependency train off the stale `lru` lines implicated by current RustSec unsoundness advisories;
2. converge CodeGG-owned DashMap usage from major 5 to the major 6 line already required by `eggfetch-core`;
3. prove the minimum SQLx feature surface required by production source and remove unreachable MySQL/RSA closure and its RustSec exception if the graph permits.

## 2. Why this is new work

Do not rewrite or invalidate earlier closure records. The prior runtime-safety dependency milestone intentionally retained SQLx macro/migrate features based on then-current source assessment, and the HTTP-client migration explicitly deferred DashMap migration. Earlier footprint work also deferred broad Ratatui modernization.

This milestone is justified by current repository evidence and later advisory/dependency changes:

- `Cargo.lock` contains `lru 0.12.5` and `lru 0.18.0`, both below current RustSec fixed floors for 2026 unsoundness advisories;
- root `ratatui = "0.29"` retains the older LRU train while the optional image dependency graph already resolves Ratatui 0.30-era crates;
- root/core/providers use DashMap 5 while Eggfetch uses DashMap 6;
- SQLx still resolves MySQL and RSA despite CodeGG being SQLite-only, and `.cargo/audit.toml` ignores RUSTSEC-2023-0071 on the rationale that the RSA path is unused.

## 3. Invariants

- TUI rendering, input, focus, popup, terminal restoration, and remote/local behavior must not regress.
- No change to CodeGG protocol, storage schema, migration order, provider behavior, or authorization policy.
- SQLite remains the only CodeGG-owned SQLx driver.
- `sqlx::FromRow`-based rows and all handwritten migrations must continue to compile and behave identically.
- DashMap concurrency semantics must remain correct; do not replace it with a different synchronization architecture in this milestone.
- No advisory is silenced merely to make `cargo audit` green.
- Lockfile churn must remain attributable to the targeted dependency changes.

## 4. Scope and non-goals

In scope:

- root TUI Ratatui dependency and compatibility edits necessary for a fixed supported line;
- Crossterm compatibility changes only if required by that Ratatui migration;
- all direct CodeGG-owned `dashmap` declarations/import-compatible code;
- SQLx declarations in root/core/providers and any other active member found by census;
- `.cargo/audit.toml` reconciliation;
- focused tests, active dependency documentation, and closure measurements.

Out of scope:

- TUI redesign or visual/layout changes;
- ratatui-image feature slimming (M003);
- workspace-wide dependency inheritance (M002);
- replacing DashMap with Mutex/RwLock or introducing a new concurrent-map abstraction;
- SQLx replacement, schema redesign, or migration framework replacement;
- broad `cargo update`, major Tokio/Serde/etc. modernization;
- changing server/plugin/image capability semantics;
- new CI/advisory/size gates.

## 5. Ordered work packages

### WP1 — Reproduce the graph and advisory baseline

Before editing, record:

```bash
cargo tree -d --locked
cargo tree -i lru@0.12.5 --locked
cargo tree -i lru@0.18.0 --locked
cargo tree -i dashmap@5.5.3 --locked
cargo tree -i dashmap@6.2.1 --locked
cargo tree -i sqlx-mysql --locked
cargo tree -i rsa --locked
cargo tree -e features -p sqlx --locked
cargo audit
```

Use the versions actually present at implementation time if the lock changed. Confirm the current RustSec records/fixed floors instead of relying solely on this plan's September 2026 snapshot.

Stop if the advisory or reverse-dependency evidence materially differs; update the plan/closure rather than forcing the assumed solution.

### WP2 — Ratatui/LRU convergence

Move root Ratatui to a current 0.30-compatible release that can remove the Ratatui-0.29 → `lru 0.12` path. Prefer the smallest compatibility change:

- retain current Crossterm 0.28 behavior initially if the selected Ratatui release exposes a compatible backend feature;
- otherwise migrate Crossterm only as required by Ratatui and keep that change in the same focused compatibility surface;
- do not alter application layout, event semantics, terminal lifecycle, or styling simply because newer APIs are available.

After updating, ensure the resolved LRU line is at or above the current fixed RustSec floor (at roadmap creation, `0.18.2` for the newer line). If another dependency still requires a vulnerable LRU generation, identify it precisely and stop rather than claiming closure.

Focused verification should include TUI unit/integration tests and at least one compile/test path for default plus `image` feature because ratatui-image shares the ecosystem.

### WP3 — DashMap 5 → 6 convergence

Update CodeGG-owned direct declarations in root, `codegg-core`, `codegg-providers`, and any other member discovered by census to major 6.

Compile before making semantic edits. Where API differences exist, make the smallest equivalent change and preserve atomic check/remove and iteration/entry semantics documented in architecture files.

Acceptance for this work package is an empty `cargo tree -i dashmap@5.5.3 --locked` result (or equivalent old-major absence) unless a third-party package, not CodeGG, still owns it. Do not patch third-party crates merely to force one version.

### WP4 — SQLx feature census and contraction

Perform source census for compile-time and migration features, including at least:

```text
sqlx::query!
sqlx::query_as!
sqlx::query_file!
sqlx::migrate!
Migrator
MigrateDatabase
#[derive(sqlx::FromRow)]
use sqlx::FromRow
```

Current evidence suggests production needs runtime Tokio + SQLite + derive support + chrono/json integrations, while schema migration is handwritten. Test the narrower declaration rather than assuming it:

- replace umbrella `macros` with the narrow derive feature if `FromRow` is the only macro facility required;
- remove `migrate` if no SQLx migration API is actually used;
- preserve SQLite/runtime/chrono/json features that compile-time and runtime tests demonstrate are required.

After each contraction, inspect:

```bash
cargo tree -i sqlx-mysql --locked
cargo tree -i sqlx-postgres --locked
cargo tree -i rsa --locked
cargo tree -e features -p sqlx --locked
```

If SQLx 0.8's feature wiring still pulls unused drivers for a required feature, document that as upstream closure and do not hand-patch SQLx.

### WP5 — Audit exception reconciliation

If `rsa` becomes unreachable, remove RUSTSEC-2023-0071 from `.cargo/audit.toml` and update the explanatory documentation.

If it remains reachable, rewrite the exception comment only if current reverse-dependency/applicability evidence differs from the existing explanation. An ignore must name the exact owner path and why a fixed replacement is unavailable or behaviorally unacceptable.

Do not add ignores for the LRU findings.

### WP6 — Documentation and closure measurements

Update active dependency/architecture docs that name the old Ratatui, DashMap, SQLx feature, or audit-ignore state.

Record, on one host/toolchain when practical:

```bash
cargo build --release --locked
cargo bloat --release --bin codegg --crates --locked -n 40
cargo tree -d --locked
```

Size is descriptive evidence only. The milestone succeeds on security/ownership correctness even if the release binary is unchanged or slightly larger for a justified dependency upgrade.

## 6. Failure and compatibility semantics

- TUI panic/error/terminal-restoration behavior remains unchanged.
- DashMap update must not weaken concurrent registry/session/presence/transport behavior.
- SQLx errors and migration failure paths remain CodeGG-owned and unchanged.
- No persistent data migration is introduced by manifest feature contraction.
- If a dependency upgrade changes an observable behavior, stop and classify it instead of burying it in compatibility edits.

## 7. Required verification

At minimum:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked -- --test-threads=1
scripts/verify.sh quick
cargo audit
cargo tree -d --locked
```

Also run focused TUI, storage/migration, provider, presence/collaboration, and transport tests implicated by actual source changes.

No public-network test is required.

## 8. Acceptance criteria

- No supported build resolves a RustSec-vulnerable LRU line through CodeGG's direct Ratatui choice; any remaining third-party path is explicitly classified.
- Ratatui migration preserves default and image-feature TUI behavior.
- CodeGG-owned DashMap declarations use one compatible major and the old CodeGG-owned major is absent from the lock/reverse tree.
- SQLx feature ownership is source-demonstrated and no broader than required.
- `sqlx-mysql`, `sqlx-postgres`, and `rsa` disappear if no required feature keeps them reachable; otherwise the exact unavoidable path is documented.
- `.cargo/audit.toml` contains no unreachable or stale exception.
- No critical/high or unexplained memory-safety advisory remains in the supported dependency graph.
- Broad local verification is green.
- Closure evidence records dependency-tree and advisory results without adding permanent gates.

## 9. Stop conditions

Stop and report a blocker if:

- current RustSec guidance differs materially from the assumed fixed lines;
- Ratatui migration requires a broad TUI rewrite rather than compatibility edits;
- SQLx feature contraction removes a production macro/migration capability not found by census;
- eliminating `rsa` would require replacing SQLx or patching/forking upstream;
- DashMap 6 changes semantics in a concurrency-sensitive path that cannot be proven equivalent with focused tests;
- the lockfile changes substantially outside the targeted dependency families.

## 10. Required closure evidence

The closure record must include:

- implementation commits/PRs;
- before/after reverse trees for LRU, DashMap, SQLx drivers, and RSA;
- current advisory output and disposition of every finding/ignore;
- focused compatibility tests for affected TUI/concurrency/storage owners;
- broad verification results;
- before/after release-size observation if practical;
- unresolved findings by severity;
- explicit recommendation: closed, conditionally closed, corrective pass required, or blocked.
