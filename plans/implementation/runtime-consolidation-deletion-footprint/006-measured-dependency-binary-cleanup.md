# Runtime Consolidation, Deletion, and Footprint M006 — Measured Dependency and Binary-Footprint Cleanup

Status: closed — final-tree measurements accepted by M009

Source roadmap:

- `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`

Relevant references:

- `plans/003-planning-process.md`
- `architecture/testing.md`
- `Cargo.toml`
- previous post-audit dependency/footprint closure records

Repository baseline reviewed: `a32f720d` (post-M005 tree; M003 corrective extraction remains incomplete)

Primary class: polish / measured optimization

Dependencies:

- hard: M001-M005 closed (M001 is now closed; M002-M005 remain outstanding);
- interface: supported feature set and default single-binary topology remain unchanged;
- downstream: M007 requires M006 measurements and accepted dependency disposition.

Target closure record:

- `plans/closure/runtime-consolidation-deletion-footprint/006-status.md`

## 1. Objective

Measure the consolidated runtime after obsolete code and verification machinery have been deleted, then make only evidence-backed dependency/default-feature/profile changes that reduce release footprint or maintenance burden without removing supported capability.

This milestone is intentionally last among implementation changes. It MUST NOT use dependency churn to compensate for reachable legacy code that earlier milestones should delete.

## 2. Baseline facts

At the reviewed baseline:

- release builds already use `lto = true`, `strip = true`, and `codegen-units = 1`;
- server/WebSocket, Wasmtime plugin, and image support are feature-gated;
- Wasmtime is configured with `default-features = false` and a narrow runtime/Cranelift/std set;
- Reqwest disables defaults and uses Rustls;
- the default feature set primarily enables clipboard support;
- several large capabilities are legitimately unconditional because they are core product behavior, including SQLite/session state, TUI, LSP/tooling, auth/crypto, and provider/runtime orchestration.

These facts mean the default strategy is measurement followed by narrow feature reduction, not dependency replacement.

## 3. Explicit non-goals

Do not:

- replace Tokio, SQLx, Reqwest, Rustls, RustPython, Wasmtime, Ratatui, Crossterm, or other major libraries solely for binary size;
- split daemon/TUI/server/plugin functionality into separate shipping binaries;
- remove supported image, clipboard, plugin, server, LSP, auth, Git, search, syntax highlighting, or provider capability;
- add a continuous binary-size CI gate;
- add automatic dependency update bots or scheduled audits;
- upgrade a dependency merely because a newer major/minor exists;
- raise MSRV solely to obtain a newer crate release;
- add custom allocators/linkers/build systems without measured evidence and separate scope;
- optimize debug/test binary size.

## 4. Required measurements

Capture a reproducible local baseline on the post-M005 tree, including command, target, features, compiler version, and resulting sizes.

At minimum inspect:

```bash
cargo tree -e features
cargo tree -d
cargo build --release --locked
```

Use `cargo bloat --release --crates` if installed/available. It is optional diagnostic tooling; do not make it a repository dependency or CI requirement. If unavailable, use normal binary size plus `cargo tree`/linker map tools already present in the environment.

Measure at least:

- default `codegg` release binary;
- production feature combination used by maintainers (`server,plugins` plus other documented production features as appropriate);
- any helper binaries that are actually shipped, if materially large.

Record before/after sizes in the closure record, not as hard-coded architecture promises.

## 5. Ordered work packages

### A. Direct dependency reachability audit

For root direct dependencies:

1. determine whether each is referenced by production code, only tests/build tooling, or only an optional feature;
2. remove direct declarations that have no root consumer and are not intentionally exposed through feature wiring;
3. remember that removing a direct manifest line that remains transitively reachable does not itself prove binary-size reduction;
4. avoid moving dependencies between crates purely to make root `Cargo.toml` look smaller.

Prioritize dependencies associated with code deleted in M001-M005.

### B. Feature-tree narrowing

Inspect default features for dependencies with nontrivial optional capability.

Accept a feature reduction only when:

- current production code does not use the feature;
- supported targets/features compile without it;
- focused behavior tests show no feature loss;
- feature unification elsewhere in the workspace does not silently re-enable it.

Check workspace-wide feature unification, not only root manifest declarations.

### C. Upstream maintenance review

Review only dependencies that are materially behind, security-sensitive, or block maintenance.

For each candidate, classify:

- stay on supported/LTS line;
- safe minor/patch update;
- major migration deferred;
- replacement rejected.

Specific expectations:

- Wasmtime 36 should remain unless current upstream support/security/compatibility evidence gives a concrete reason to move; do not churn an LTS runtime for version freshness.
- Ratatui/Crossterm may be evaluated as one TUI-stack maintenance item. Upgrade only if migration is small, supported targets remain compatible, and it does not broaden this milestone into TUI redesign.
- security advisories found through locally available tooling take precedence over size preferences, but any large remediation should be recorded distinctly.

### D. Release profile experiments

The existing release profile is already aggressive.

The only additional experiment explicitly in scope is `panic = "abort"` if:

- no supported FFI/plugin/embedding boundary relies on unwinding;
- tests/documentation demonstrate expected panic semantics;
- measured size reduction is nontrivial enough to justify the semantic change.

Do not land `panic = "abort"` merely because it often reduces size. If semantic uncertainty exists, reject/defer it.

Avoid exotic linker flags or `opt-level = "z"` changes unless measurement shows a material benefit without unacceptable runtime regression. Such changes are optional, not acceptance requirements.

### E. Duplicate version inspection

Use `cargo tree -d` to identify duplicate major/minor versions that materially contribute to footprint.

Resolve only when:

- an ordinary compatible dependency update unifies them;
- the change is low-risk;
- it does not require patching/forking upstream solely for deduplication.

Do not add `[patch.crates-io]` overrides for cosmetic graph uniformity.

### F. Validate supported feature combinations

For every accepted dependency/feature/profile change, run focused compile/tests for the affected capability and at least:

```bash
cargo check -p codegg --locked
cargo check -p codegg --locked --features server,plugins,lsp-test-support
scripts/verify.sh quick
```

Adjust the feature list to current documented production combinations after M001-M005; do not enable real external-server smoke tests merely for this pass.

## 6. Security, compatibility, MSRV, platform review

Security:

- do not reduce crypto/TLS/security features without verifying equivalent behavior;
- do not switch from maintained safe Rust dependencies to smaller unmaintained alternatives;
- advisory remediation may justify an update even if size increases.

Compatibility:

- preserve supported Linux/macOS/Windows compilation expectations represented by current code/docs;
- preserve default CLI/TUI behavior and optional server/plugin/image capability.

MSRV:

- current `rust-version` is authoritative unless an intentional project decision changes it;
- dependency updates that force an MSRV bump are out of scope unless required for a high-severity security issue and explicitly documented.

## 7. Verification and evidence

Required final evidence for M006:

- post-M005 dependency feature tree summary;
- duplicate-version summary;
- default and production-feature release binary sizes before M006 changes and after accepted changes;
- disposition of each materially considered dependency change;
- focused tests/compile for changed features;
- `scripts/verify.sh quick`;
- `git diff --check`.

A full hosted CI run is owned by M007 unless M006 changes fundamental build/feature wiring enough that ordinary merge confidence requires one earlier run.

## 8. Explicit acceptance criteria

M006 is complete only when:

1. Measurements are taken on the consolidated post-M005 tree, not the pre-deletion baseline alone.
2. Default and production-feature release binary sizes are recorded reproducibly.
3. `cargo tree -e features` and duplicate-version evidence are reviewed.
4. Every removed/narrowed dependency feature is proven unused for supported capability and affected builds/tests remain green.
5. No user-visible feature is removed for footprint reduction.
6. No major dependency replacement is performed without measured material benefit and explicit compatibility justification.
7. Wasmtime is not upgraded merely for freshness; its final disposition is documented.
8. Ratatui/Crossterm or other ordinary maintenance updates are either safely completed as bounded work or explicitly deferred; lack of upgrade does not block closure.
9. MSRV is not raised merely for dependency freshness.
10. No new binary split, linker framework, continuous size gate, dependency bot, or release automation is introduced.
11. Any `panic = "abort"` change, if accepted, has explicit semantic review and measured benefit; otherwise it remains unchanged.
12. Required default/production feature checks and `scripts/verify.sh quick` pass.
13. The closure record reports actual measured deltas and does not claim reduction when the change only cleaned the manifest.
14. If deletion alone produced most of the footprint improvement and no further safe dependency changes are justified, recording that result and making no extra dependency changes counts as successful completion.

## 9. Stop conditions

Stop optimization when the next meaningful reduction would require feature loss, a major dependency rewrite, an MSRV change, a binary topology split, or significant runtime-performance regression. The roadmap explicitly prefers a slightly larger maintainable binary over churn.

## 10. Execution disposition

The original M006 measurement audit was performed against the post-M005 branch
state and was correctly blocked by the hard M003 dependency at that time. The
remaining physical extraction was completed by M009; final-tree measurements
and strict closure are recorded in
`plans/closure/runtime-consolidation-deletion-footprint/006-status.md`.

The audit found no safe dependency declaration, feature, duplicate-version,
upstream-maintenance, or release-profile change to land in M006. The measured
result and dispositions are recorded in
`plans/closure/runtime-consolidation-deletion-footprint/006-status.md`.
