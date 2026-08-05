# Runtime Safety, Resource Control, and Footprint Milestone 007 — Binary Topology and Footprint Reduction

Status: blocked on M002, M003, M005, and M006

Source subsystem roadmap:

- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`
- Milestone 007

Dependencies:

- hard: M002 canonical bounded process execution;
- hard: M003 typed argv and shell-routing convergence;
- hard: M005 dependency feature and namespace normalization;
- hard: M006 deprecated parser and dependency maintenance;
- soft: M004 grep resource correctness should be included in final measurements when available.

Repository baseline reviewed: `4d540ce315c9ef2a1c07544cd42df0efc43708e1`

Primary class: measurement-led footprint and executable-boundary polish

Target closure record:

- `plans/closure/runtime-safety-resource-footprint/007-status.md`

## 1. Objective

Measure CodeGG's release binary composition after the preceding correctness and dependency work, apply safe no-feature-loss reductions, and decide from quantitative evidence whether the current combined daemon/TUI executable should be split into separate composition binaries.

This milestone has two valid closure outcomes:

1. **measured split implemented** — separate binaries materially reduce the ordinary user-invoked or deployment-specific executable while preserving single-daemon ownership, shared business logic, protocol compatibility, and manageable aggregate installed size; or
2. **measured no-split decision** — dependency/source reductions land, but a binary split does not meet the materiality and maintenance thresholds, so the current topology is retained.

A no-split decision is successful closure when supported by reproducible measurements. The plan must not force architecture churn merely because the root executable is broad.

## 2. Explicit non-goals

This milestone must not:

- remove documented features to improve size;
- disable clipboard, LSP, provider, server, image, plugin, syntax rendering, Python, database, Git, ACP, or TUI behavior solely for measurement results;
- make optional Wasmtime/plugin support part of the default graph;
- replace major subsystems solely because one crate appears high in `cargo bloat`;
- change the single active daemon model;
- redesign IPC, scheduler, session, projection, or frontend architecture;
- introduce dynamic plugin loading for ordinary Rust features merely to shrink one file;
- add UPX or opaque executable compression;
- weaken release hardening flags without evidence;
- add a binary-size CI gate, scheduled measurement workflow, artifact upload, target matrix, or release automation;
- optimize debug binaries and report those results as deployment footprint;
- treat on-disk binary bytes as resident memory or startup performance.

## 3. Current implementation evidence

Inspect at minimum:

- root `Cargo.toml`, binary targets, features, and release profile;
- root `src/main.rs` and application bootstrap/composition modules;
- daemon startup, TUI startup, CLI dispatch, and IPC client/server composition;
- workspace crate ownership and dependency direction;
- optional `server`, `plugins`, `image`, and clipboard features;
- packaging/install/release documentation;
- M005 and M006 closure records and final feature trees;
- production call sites that require daemon-only versus frontend-only dependencies.

The reviewed baseline already uses a size-oriented release profile:

```toml
lto = true
strip = true
codegen-units = 1
```

Large optional plugin support is feature-gated. The likely remaining opportunities are therefore:

- dependency feature normalization from M005/M006;
- unused or duplicate direct dependencies found by measured inspection;
- moving daemon-only and TUI-only composition imports out of a shared binary target;
- ensuring library crates do not unnecessarily re-export or instantiate both sides;
- avoiding duplicated generated assets or static tables where one shared representation suffices;
- executable topology only when usage-specific size reduction outweighs aggregate duplication and packaging cost.

The implementation agent must not assume the daemon/TUI split is valuable before measuring the post-M006 tree.

## 4. Invariants that cannot regress

- one active daemon remains the owner of scheduling, durable execution, projects, sessions, and shared system resources;
- multiple frontends may connect without embedding a second daemon authority;
- existing daemon/client protocol remains compatible unless an accepted ADR and migration explicitly authorize a change;
- the default documented user workflow remains available;
- all supported feature gates remain buildable;
- optional large dependencies remain optional;
- business logic remains in libraries, not duplicated between binary targets;
- configuration paths, state paths, project identity, authentication, and endpoint discovery remain compatible;
- release binaries remain stripped and optimized according to the accepted release profile;
- measurements specify target triple, toolchain, feature set, lockfile state, and binary file measured;
- aggregate installation size and per-process runtime mappings are considered separately;
- no size claim is made from incremental/stale build artifacts.

## 5. Measurement protocol

### 5.1 Build environment

Record:

- commit SHA;
- Rust toolchain version;
- host and target triple;
- Cargo profile;
- feature set;
- whether the build is clean and locked;
- linker, when materially relevant;
- exact binary path and file size;
- measurement tool versions when optional local tools are used.

Use a clean or otherwise demonstrably fresh release build:

```bash
cargo clean
cargo build --release --locked
```

A full `cargo clean` may be omitted for iterative work, but final recorded measurements must be from a fresh target directory or equivalent isolated `CARGO_TARGET_DIR`.

### 5.2 Required build variants

Measure at minimum:

- current/default release feature set;
- `--no-default-features` when it is a supported usable build;
- each materially large optional feature individually when supported: `server`, `plugins`, `image`;
- representative intended installed topology after any proposed split.

Do not require unsupported arbitrary feature combinations. Record the exact supported commands.

### 5.3 Inspection tools

Use local, non-CI tools as appropriate:

```bash
cargo tree -e features
cargo tree -d
cargo bloat --release --crates
cargo bloat --release --functions
```

Equivalent platform tools such as `size`, `nm`, `otool`, `readelf`, or linker map output are acceptable. Do not add these tools as required repository dependencies or hosted workflow steps.

Commit only a concise Markdown summary in the closure record. Do not commit raw multi-megabyte maps or generated reports.

### 5.4 Metrics

Record separately:

- stripped on-disk bytes for each installed executable;
- aggregate installed executable bytes;
- optional shared-library/runtime assets if packaging introduces them;
- startup time or RSS only when measured through a simple, repeatable local smoke and clearly labeled as separate from binary size;
- top crate/function contributors relevant to an actual proposed change.

Do not use compressed archive size as the primary binary metric.

## 6. Safe reduction order

Apply reductions in this order:

1. verify M005/M006 feature-tree reductions are present;
2. remove proven unused direct dependencies or imports;
3. correct remaining target-specific dependencies declared globally;
4. narrow generated/static assets only when deduplication is behavior-preserving;
5. move composition-only imports to the binary that owns them;
6. evaluate binary split;
7. consider profile/linker adjustments only with measured benefit and no unacceptable build/debug/compatibility cost.

Do not start with a binary split, allocator replacement, major dependency rewrite, or custom linker configuration.

Every reduction requires before/after measurement on the same target/toolchain/feature set.

## 7. Binary split decision rule

A split is justified only when all conditions are met:

1. the primary frontend/client executable or daemon-only deployment executable becomes smaller by at least both:
   - 10 percent; and
   - 5 MiB;
2. aggregate installed executable bytes do not grow by more than 15 percent unless a documented packaging mode installs only the selected role;
3. shared business logic is not duplicated in source;
4. daemon protocol and state ownership remain unchanged;
5. startup/invocation compatibility can be preserved with a simple documented migration;
6. release/package complexity remains bounded and manual;
7. no new background service manager or installer framework is needed.

The percentage/byte threshold is a decision heuristic, not a CI gate. When measurement is near the boundary, prefer the simpler existing topology unless there is a clear deployment benefit such as headless systems never needing TUI dependencies.

A split may also be justified below the aggregate threshold for a formally supported role-specific package/install mode, but the closure record must explain why users can avoid installing the other binary and how release ownership remains manageable.

## 8. Preferred split architecture when justified

Use thin composition binaries backed by shared libraries.

Potential target shape:

```text
codeggd
  daemon/server/scheduler composition
  owns persistent runtime authority

codegg
  CLI/TUI/client composition
  connects to codeggd
  may retain explicit foreground/bootstrap compatibility where current behavior requires it
```

Exact names must align with current CLI terminology and packaging. Do not assume `codeggd` if the repository already has a canonical daemon name.

Required properties:

- no business logic duplicated between `main` files;
- daemon-only dependencies are not linked into the client binary;
- TUI/clipboard/frontend-only dependencies are not linked into a headless daemon binary;
- shared protocol/domain/config types remain in workspace libraries;
- daemon discovery/start behavior is explicit and backward compatible;
- `codegg` must not silently create a second durable authority when a daemon is active;
- existing scripts/systemd/manual service examples are updated;
- Cargo package configuration includes all intended binaries without automated release changes.

A compatibility dispatcher is acceptable when small: for example, an existing `codegg daemon` command may exec or invoke the daemon binary. It must not re-link the entire daemon graph into the client merely to preserve syntax.

## 9. No-split outcome requirements

When the split threshold is not met or complexity is disproportionate:

1. retain the existing binary topology;
2. land any independently justified dependency/source reductions;
3. record default and representative feature measurements;
4. record the estimated or prototype split sizes if available;
5. explain which shared dependencies dominate and why separation does not materially help;
6. remove temporary prototype targets/files;
7. close M007 without a follow-up unless new deployment constraints arise.

Do not register another split plan based only on preference.

## 10. Profile and linker evaluation

The current release profile is already aggressive. Evaluate additional changes only after source/dependency work.

Potentially acceptable experiments:

- `panic = "abort"` if unwinding is not part of public/plugin/runtime correctness and all cleanup remains explicit;
- workspace-level removal of debug symbols already covered by strip;
- platform linker dead-code settings when stable and portable enough;
- splitting debug symbols from release artifacts only when packaging owns them manually.

Any accepted profile change must:

- build all supported targets/features affected;
- preserve panic/cancellation/cleanup assumptions;
- avoid materially worse build time without a deployment benefit;
- be documented in the closure record.

Do not adopt nightly-only flags, custom standard-library builds, or unstable compiler options.

## 11. Expected production-code changes

Possible areas:

- root `Cargo.toml` binary targets and target-specific dependency features;
- `src/main.rs` and extracted thin composition modules;
- daemon/TUI bootstrap code;
- workspace library boundaries when composition imports currently pull opposite-role dependencies;
- packaging/release/service documentation;
- small smoke tests for binary invocation/protocol compatibility;
- removal of proven unused dependencies/assets.

A measured no-split result may require little or no production topology change beyond safe dependency cleanup.

## 12. Storage, protocol, migration, and compatibility effects

Storage:

- no database schema migration expected;
- both binaries must use the same canonical config/state/project identity paths;
- a split must not create separate default databases or caches.

Protocol:

- preserve daemon/client protocol version and endpoint discovery;
- no breaking message/schema change is expected;
- role-specific binaries may expose compatible version/help metadata.

Compatibility:

- existing `codegg` invocation remains available or receives a direct documented replacement;
- headless daemon operation remains available;
- existing service files/scripts are updated when target names change;
- package metadata lists intended binaries;
- manual crates.io release remains one maintainer-operated process with explicit binary contents.

## 13. Ordered work packages

### Work package A — Post-dependency baseline

1. verify M002–M006 accepted revisions are present;
2. record clean default/no-default/optional feature sizes;
3. capture focused feature trees and dominant crate/function contributors;
4. distinguish daemon-only, frontend-only, and shared dependency groups.

### Work package B — Safe source/manifest reductions

1. remove proven unused direct dependencies;
2. correct target/feature ownership exposed by measurements;
3. deduplicate static/generated assets where simple;
4. measure each coherent change set;
5. discard changes without benefit or with compatibility cost.

### Work package C — Split prototype

Only when dependency grouping suggests material value:

1. create thin daemon and client/TUI targets on the implementation branch;
2. move composition imports without duplicating business logic;
3. build both cleanly with the same lockfile/profile;
4. measure per-binary and aggregate bytes;
5. run daemon/client connection smoke;
6. evaluate packaging and compatibility.

### Work package D — Decision

1. apply the materiality rule;
2. choose implemented split or no-split;
3. remove abandoned prototype code;
4. record quantitative rationale;
5. do not defer the decision ambiguously.

### Work package E — Compatibility and documentation

For split outcome:

1. preserve/bridge existing CLI invocation;
2. update service, install, and release docs;
3. add role-specific smoke tests;
4. verify shared state and endpoint behavior.

For no-split outcome:

1. update footprint notes only where useful;
2. retain current user documentation;
3. avoid permanent prototype scaffolding.

## 14. Focused verification

Required for both outcomes:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo build --release --locked
scripts/verify.sh quick
```

Run affected supported feature builds explicitly. For a split outcome also run focused smokes equivalent to:

```text
client --version/help
headless daemon starts with isolated temporary state
client discovers/connects to daemon
second daemon authority is rejected or follows current singleton behavior
shutdown leaves no child process
existing compatibility invocation routes correctly
```

Use current test harnesses and strict timeouts. Do not add large startup benchmarks or cross-target matrices.

Record final clean release measurements after all code changes.

## 15. Static guards

Do not add a continuous size threshold guard.

For a split outcome, add only architectural guards already compatible with repository conventions, such as:

- client composition cannot import daemon-private modules/dependencies;
- daemon composition cannot import TUI/clipboard modules;
- both binaries depend on shared protocol/domain libraries rather than duplicate types.

Prefer Cargo feature/target boundaries and compile tests over regex where practical.

## 16. Acceptance criteria

M007 is complete only when:

- clean baseline and final release measurements identify target/toolchain/features;
- preceding dependency reductions are included;
- safe source/manifest reductions are measured and behavior-preserving;
- all documented features remain available;
- optional large features remain gated;
- the split decision is explicit and quantitative;
- a split is implemented only when the materiality/complexity rule is met;
- any split preserves single-daemon authority, shared state, protocol compatibility, and thin composition binaries;
- aggregate installed size and role-specific size are both reported;
- a no-split outcome removes prototype scaffolding and records why separation is not worthwhile;
- focused build/smoke tests and `scripts/verify.sh quick` pass;
- hosted verification passes on the accepted tree;
- no size CI gate, artifact workflow, release automation, feature deletion, or unrelated architecture rewrite is introduced.

## 17. Stop conditions

Stop and report blocked when:

- M002, M003, M005, or M006 is not closed;
- clean measurements cannot be reproduced with the repository toolchain/lockfile;
- a split requires breaking protocol/state ownership;
- packaging cannot preserve existing invocation without linking both dependency graphs into each binary;
- a dominant dependency can be removed only through feature reduction or a major subsystem rewrite;
- the repository's supported binary/install contract is ambiguous enough to require an ADR.

When an ADR is needed, record the measured alternatives and stop before implementation. Do not force a topology change.

## 18. Required closure evidence

`plans/closure/runtime-safety-resource-footprint/007-status.md` must include:

- accepted commit/PR;
- toolchain, target, profile, feature sets, and exact build commands;
- clean baseline and final per-binary/aggregate sizes;
- dominant contributor summary;
- each accepted safe reduction and measured effect;
- split prototype measurements when attempted;
- final split/no-split decision against the stated rule;
- protocol/state/invocation/package compatibility result;
- focused smoke/build commands, quick verification, and hosted run reference;
- unresolved findings by severity;
- explicit confirmation that no feature was removed and no continuous size/release automation was added.