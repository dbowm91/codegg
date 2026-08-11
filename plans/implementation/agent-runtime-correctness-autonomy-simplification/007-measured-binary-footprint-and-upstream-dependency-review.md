# Agent Runtime Correctness, Autonomy, and Simplification M007 — Measured Binary Footprint and Upstream Dependency Review

Status: ready

Source subsystem roadmap:

- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md`
- Milestone M007

Repository baseline reviewed: `e88d6f4f67ff729c894b228ddb8b5324582f3fbc`

Primary class: performance/maintenance polish

Dependencies:

- hard: none
- soft: M006 may slightly change final release size; use the current implementation baseline for candidate decisions and let M009 capture the final integrated number

Relevant references:

- `plans/000-long-term-specification.md`
- `plans/003-planning-process.md`
- historical dependency/footprint closure records under `plans/closure/post-audit-correctness-simplification/`
- `architecture/testing.md`
- plugin/runtime documentation

Target closure record:

- `plans/closure/agent-runtime-correctness-autonomy-simplification/007-status.md`

## 1. Objective

Perform a fresh, measurement-led binary-size and upstream-dependency review after earlier feature-slimming passes, prioritizing whole-subsystem/codegen contributors and security-relevant optional runtimes rather than repeating low-yield blanket Cargo feature trimming.

Preserve every supported user-visible feature unless an equivalent implementation reduces size/complexity without behavior loss.

## 2. Explicit non-goals

Do not:

- split CodeGG into daemon/TUI binaries solely to improve an individual binary-size number;
- remove plugins, notifications, syntax highlighting, image support, research, server, LSP, archive/install support, or other documented capabilities;
- replace large dependencies speculatively without measured contributor evidence;
- replace RustPython Parser as part of this milestone;
- raise MSRV solely for footprint gains;
- add a binary-size CI gate, cargo-bloat dependency, scheduled dependency scan, dependency bot, or automatic cargo-audit workflow;
- update the entire lockfile without a concrete dependency reason;
- optimize for size in a way that materially harms interactive latency/tool throughput without measurement;
- use `panic = "abort"` without first proving that no supported panic-isolation/catch-unwind behavior depends on unwinding.

## 3. Current implementation evidence

Inspect at minimum:

- root/workspace `Cargo.toml` and `Cargo.lock`;
- release profile settings;
- optional feature definitions, especially `plugins`, `server`, `image`, and LSP-related features;
- `src/tui/components/notification.rs` and `notify-rust` dependency closure;
- Syntect/Comrak syntax rendering path and embedded assets/features;
- plugin install/archive code and EggLSP archive/download code (`tar`, `flate2`, related codecs);
- Wasmtime dependency declaration, enabled features, and exact locked patch version under plugin builds;
- any remaining duplicate crate versions reported by `cargo tree -d`;
- prior M004/M005 closure records documenting already-narrowed qrcode, Comrak, RustPython, Reqwest/TLS, SQLx, arboard, futures, and grep dependencies.

Historical evidence to respect:

- the prior focused dependency pass reduced a roughly 54.46 MiB release binary by only about 33 KiB;
- RustPython Parser was measured at roughly 596 KiB `.text`, Comrak roughly 136 KiB in that historical build;
- later dependency normalization already removed several umbrella/default feature closures;
- therefore another broad "disable defaults everywhere" sweep is unlikely to produce meaningful returns.

## 4. Invariants that cannot regress

- supported features and CLI/TUI behavior remain available;
- default TLS ownership remains explicit and does not accidentally reintroduce native TLS;
- SQLite/storage features remain sufficient for migrations/macros/runtime use;
- plugin runtime remains sandboxed according to current Wasmtime/plugin policy;
- no dependency security downgrade is accepted for size;
- release/profile changes are compared on the same target/toolchain/build inputs;
- any accepted size optimization records its runtime/compile-time tradeoff;
- dependency updates are scoped and lockfile churn is explainable;
- no size measurement becomes a routine CI gate.

## 5. Baseline measurement requirements

Before changing manifests/profile settings, capture on one documented host/toolchain:

```bash
cargo build --release --locked
stat -f%z target/release/codegg  # macOS, or equivalent Linux stat
cargo bloat --release --bin codegg --crates --locked -n 50
cargo tree -d --locked
cargo tree -e features --locked
```

If `cargo bloat` is unavailable, it may be installed/used locally as diagnostic tooling; do not add it to repository dependencies or CI.

Record:

- target triple;
- `rustc --version` / Cargo version;
- exact baseline commit;
- release binary bytes;
- top code contributors;
- major optional/default feature closures relevant to candidate work.

If plugins are not part of the default binary, separately inspect a plugin-feature release build only for plugin-runtime dependency safety/size. Do not conflate default and all-feature measurements.

## 6. Candidate selection policy

Only investigate candidates meeting at least one condition:

- material `.text`/binary contributor;
- large feature/dependency closure that CodeGG uses through a narrow API;
- duplicate dependency family with a safe compatible unification path;
- security/maintenance risk that justifies a patch/minor update;
- release-profile setting likely to reduce size materially with acceptable performance.

Candidate classes to measure, not presume:

### A. Desktop notifications

`notify-rust` is used through a small notification manager surface. Inspect its transitive closure by target. Determine whether feature flags or a narrower version/config can preserve macOS/Linux/Windows behavior without a custom platform reimplementation.

Do not remove notifications merely because their API surface is small.

### B. Syntax/Markdown rendering

Measure Syntect/Comrak and embedded syntax/theme assets. Earlier Comrak default-feature narrowing already occurred. Only pursue additional changes if current bloat/feature evidence shows a meaningful contributor and behavior-preserving configuration exists.

### C. Archive/install support

Inspect `tar`, `flate2`, compression codecs, plugin install, and EggLSP download use. Shared dependencies may already amortize cost. Do not create a custom archive parser or external-process dependency for size alone.

### D. Plugin runtime / Wasmtime

For `--features plugins`:

- determine exact locked Wasmtime version;
- check current applicable RustSec/Bytecode Alliance security advisories and maintained patch release for the selected major/minor line at implementation time;
- ensure the lock is at or above all relevant fixed patch versions;
- inspect enabled Wasmtime features and disable only features demonstrably unused by CodeGG's runtime;
- preserve fuel/resource limits, WASI/component behavior actually required by plugins, and sandbox semantics;
- do not downgrade or pin below a security fix for binary size.

This upstream check must use current primary sources during implementation because patch/security state is time-sensitive.

### E. Release profile experiments

Experiment locally with current profile versus size-oriented alternatives such as `opt-level = "s"` where safe.

Compare at minimum:

- release bytes;
- startup/TUI responsiveness via a simple repeatable local timing if materially affected;
- representative parser/render/tool throughput only when a candidate changes hot code behavior.

Do not adopt `opt-level = "z"`, `panic = "abort"`, or aggressive settings solely from binary size without behavior/performance review.

## 7. Ordered work packages

### Work package A — Fresh graph and release baseline

1. rebase/inspect current `main`;
2. capture default release size and crate bloat;
3. capture duplicate/version and feature trees;
4. identify the top 5–10 realistic CodeGG-owned candidates;
5. discard candidates already proven low-yield unless current evidence changed.

### Work package B — Upstream/security review

1. verify Wasmtime exact lock and current patched release/advisories if plugins are supported;
2. inspect maintenance status of any other dominant candidate before altering features/versions;
3. avoid broad `cargo update`;
4. document accepted update, rejected update, or no-change decision with primary-source rationale.

### Work package C — Small candidate experiments

For each candidate:

1. create an isolated change or feature/profile experiment;
2. build with the same target/toolchain;
3. record byte and bloat delta;
4. run focused feature tests;
5. reject changes whose benefit is trivial relative to complexity/maintenance cost;
6. restore rejected experiments completely.

Prefer at most a few meaningful candidates rather than dozens of micro-optimizations.

### Work package D — Accept only no-feature-loss improvements

Accepted changes must:

- preserve supported API/UI behavior;
- keep dependency ownership explicit;
- not add custom replacement code larger/more fragile than the dependency saving;
- pass focused feature tests;
- record before/after measurement.

### Work package E — Documentation

Update dependency/architecture docs only when feature ownership or upstream maintenance requirements materially change.

Do not commit generated cargo-bloat reports; summarize measurements in the closure record.

## 8. Verification matrix

Use change-specific checks rather than all of these unconditionally.

Examples:

```bash
cargo check --workspace --all-targets --locked
cargo check --workspace --all-targets --features plugins --locked   # when plugin deps change
cargo test --lib tui::components::notification                      # when notification path changes
cargo test --lib tui::components::messages                          # when rendering deps change
cargo test --features plugins plugin                                # when plugin runtime changes
cargo build --release --locked
```

Then run:

```bash
scripts/verify.sh quick
```

M009 owns final broad integration verification. Do not require all optional feature combinations unless the changed dependency affects them.

## 9. Static guards and CI

Add no binary-size/dependency static guard and no new workflow.

If a manifest feature choice is subtle, a focused compile/test for the affected feature is sufficient. Record manual upstream audit evidence in the closure record.

## 10. Acceptance criteria

M007 closes only when:

- a fresh current default release baseline and crate bloat profile are recorded;
- duplicate and feature trees are reviewed;
- Wasmtime/plugin runtime exact lock is verified against current relevant upstream security fixes, or the closure record explicitly states plugins/Wasmtime are not selected in the relevant build and why;
- only measured, behavior-preserving dependency/profile changes are accepted;
- rejected candidates and their insufficient tradeoffs are documented so the same low-yield work is not repeated;
- all supported features touched by accepted changes retain focused test coverage;
- final milestone release size is measured against the baseline;
- `scripts/verify.sh quick` passes;
- no binary split, feature removal, parser rewrite, new CI gate, scheduled audit, or release automation is introduced.

A valid outcome is "no production dependency/size change justified" if measurement shows no worthwhile candidate. Measurement and upstream safety closure are the objective, not forced byte reduction.

## 11. Stop conditions

Stop a candidate when:

- measured reduction is negligible relative to maintenance complexity;
- a replacement changes public behavior or platform support;
- a dependency update raises MSRV or creates broad API churn without security necessity;
- a release-profile change materially regresses responsiveness/tool throughput;
- Wasmtime feature removal weakens required plugin sandbox/runtime behavior;
- a proposed optimization requires topology or subsystem redesign.

## 12. Required closure evidence

`plans/closure/agent-runtime-correctness-autonomy-simplification/007-status.md` must include:

- implementation commit/PR if any production change landed;
- measurement host/toolchain/target and baseline commit;
- before/after release bytes;
- top bloat contributors before/after when meaningful;
- candidate table: accepted/rejected/no-change with byte delta and rationale;
- exact Wasmtime lock/upstream advisory disposition for plugin builds;
- focused test and quick-verification outcomes;
- confirmation of no user-visible feature loss, topology change, or new verification/release automation;
- unresolved dependency risks by severity.