# Post-Audit Correctness, Simplification, and Footprint Milestone 004 — Dependency Feature Slimming and Upstream Maintenance Review

Status: ready

Source subsystem roadmap:

- `plans/subsystems/post-audit-correctness-simplification-roadmap.md`
- Milestone 004

Repository baseline reviewed: `0323d68e0c37c0495540d39ec0d6d9520f124125`

Primary class: footprint, dependency maintenance, and compatibility polish

Dependencies:

- hard: none
- soft: M003 should be included in final release-size measurements if it lands first

Target closure record:

- `plans/closure/post-audit-correctness-simplification/004-status.md`

## 1. Objective

Apply additional no-feature-loss dependency reductions missed by the previous footprint pass, review material upstream dependency risks, and record measurement before considering any dependency replacement.

The expected high-confidence first change is disabling `qrcode` default image/rendering features because CodeGG renders QR output as terminal characters. Comrak and RustPython are measurement/evaluation targets, not predetermined removals.

## 2. Explicit non-goals

Do not:

- split the executable;
- remove QR codes, images, syntax highlighting, Markdown, Tool Programs, plugins, LSP, clipboard, server, research, or any other supported feature;
- replace `rustpython-parser` with a handwritten parser based only on size intuition;
- bump CodeGG's MSRV solely to adopt the newest dependency release;
- perform broad `cargo update` churn unrelated to a concrete vulnerability, correctness fix, or feature-tree reduction;
- add automated dependency bots, scheduled audits, cargo-audit to every PR, binary-size gates, or release automation;
- adopt nightly-only compiler/linker flags or custom standard library builds;
- reopen the M007 single-binary decision from the prior runtime-safety footprint workstream without new measured evidence.

## 3. Current implementation evidence

Inspect at minimum:

- root and workspace `Cargo.toml` files;
- `Cargo.lock`;
- `src/tui/components/dialogs/share.rs` qrcode use;
- `src/tui/components/messages.rs` Comrak/syntect use;
- `crates/codegg-core/src/tool_program/parser.rs` RustPython use;
- prior closure `plans/closure/runtime-safety-resource-footprint/007-status.md`;
- current `cargo tree -e features`, `cargo tree -d`, and release-size measurements.

Known opportunities:

- CodeGG uses `qrcode::QrCode` only to render a character/string representation, while qrcode default features activate image/SVG/pic support. Use `default-features = false` when the current API compiles and behavior is unchanged.
- Comrak's locked dependency surface includes CLI/auxiliary crates that may be avoidable if CodeGG's used AST parsing APIs compile with defaults disabled. This must be tested against the locked Comrak line and current syntax-render behavior.
- `rustpython-parser` provides a full Python parser for a deliberately restricted Tool Program language. It may be a meaningful footprint contributor, but replacement is not justified unless contributor measurements show material impact and feature-level narrowing is unavailable.
- the lockfile contains multiple versions of several transitive crates. Deduplicate only when direct dependency constraints can be safely aligned; do not chase transitive duplication mechanically.

## 4. Invariants that cannot regress

- Every currently supported user-visible feature remains buildable and functional.
- Default and documented feature combinations retain behavior.
- QR terminal rendering remains available.
- Markdown rendering and syntax highlighting remain available.
- Tool Program parsing accepts/rejects the same supported language unless an independent correctness defect is identified.
- MSRV remains whatever the repository currently documents unless the user separately approves a change.
- dependency source/license/security posture must not become weaker for a marginal size reduction.
- optional large dependencies remain optional.
- the current single-binary topology remains the accepted default.
- size claims must use fresh release builds and record target/toolchain/features.

## 5. Measurement protocol

Before and after accepted changes, record:

```bash
cargo tree -e features
cargo tree -d
cargo build --release --locked
```

When available locally, use diagnostic-only tools such as:

```bash
cargo bloat --release --crates -n 40
cargo bloat --release --functions -n 40
```

Do not add `cargo-bloat` as a repository or CI dependency.

Final measurements should use the same target/toolchain/profile and a fresh target directory or equivalent isolated build. Record on-disk stripped binary bytes separately from compile graph reductions.

## 6. qrcode work requirements

1. verify CodeGG uses no qrcode image/SVG/pic API;
2. set `qrcode = { version = "...", default-features = false }` using the current accepted version constraint;
3. regenerate/update lockfile normally;
4. verify QR terminal rendering tests;
5. inspect whether `image`/codec transitive nodes disappear specifically from the qrcode path, recognizing that CodeGG may retain `image` through its independent image feature or other dependencies;
6. record compile graph and binary-size effect without claiming unrelated image dependencies were removed.

This change should land unless source inspection disproves the assumption.

## 7. Comrak evaluation requirements

Treat Comrak as an experiment with a clear revert condition.

1. inventory exact APIs/features CodeGG consumes: AST parsing, extensions, syntax interactions;
2. test `default-features = false` or an explicit minimal feature set supported by the locked/current compatible Comrak release;
3. run Markdown/TUI focused tests;
4. inspect feature-tree reduction;
5. keep the change only if behavior remains equivalent and no private reimplementation is required.

Do not upgrade to a newer Comrak major/minor merely to access feature controls unless that version remains within CodeGG's supported MSRV and the migration is trivial. Otherwise record it as deferred.

## 8. RustPython evaluation requirements

First measure.

1. capture `cargo bloat --crates` contribution for `rustpython-parser` and relevant transitive crates;
2. inspect current crate features/defaults and whether parser-only narrowing is possible;
3. inspect whether CodeGG enables features it does not use;
4. apply only straightforward feature narrowing with full Tool Program parser tests;
5. do not implement a custom parser in this milestone.

If RustPython is a dominant contributor but cannot be narrowed, closure should record a deferred option with measured bytes and estimated complexity. Do not register a replacement plan automatically.

## 9. Upstream maintenance review

Review material direct dependencies touched by this milestone for:

- current maintained release line;
- known RustSec/security advisory exposure against the locked version;
- MSRV compatibility;
- maintenance/deprecation status;
- default-feature changes that could surprise future updates.

Focus on qrcode, Comrak, RustPython and any other top contributor exposed by measurement. Do not turn this into a complete ecosystem audit.

Use manual/current upstream review during implementation and record conclusions in closure. Periodic/manual dependency audit remains the project policy.

## 10. Ordered work packages

### Work package A — Baseline

1. record current release size and top crate contributors;
2. capture feature trees for qrcode, Comrak, RustPython, and image-related nodes;
3. record current MSRV and toolchain used.

### Work package B — qrcode narrowing

1. disable defaults;
2. update lockfile;
3. run focused QR/TUI tests;
4. measure graph/binary effect.

### Work package C — Comrak experiment

1. test minimal/default-disabled configuration;
2. run Markdown rendering tests;
3. keep or revert based on behavior and graph reduction;
4. document the decision.

### Work package D — RustPython and dominant contributors

1. measure contribution;
2. inspect safe feature narrowing;
3. apply only low-risk feature changes;
4. explicitly reject speculative parser replacement.

### Work package E — Upstream review and reconciliation

1. inspect advisories/maintenance/MSRV for touched material dependencies;
2. perform narrowly justified patch updates only when low-risk and compatible;
3. update dependency/architecture docs only where ownership or maintenance policy changes;
4. record final measurements.

## 11. Storage, protocol, migration, and compatibility effects

Storage: none.

Protocol: none.

Migration: none expected.

Compatibility:

- feature/default changes must be implementation-internal;
- dependency patch updates may change implementation details but must preserve public behavior and supported MSRV;
- no supported feature may become opt-in if it was previously part of the documented default solely to reduce binary size.

## 12. Focused verification

At minimum:

```bash
cargo test --lib tui::components::dialogs::share
cargo test --lib tui::components::messages
cargo test -p codegg-core tool_program
cargo check --workspace --all-targets --locked
cargo build --release --locked
scripts/verify.sh quick
```

Run feature-specific checks for any manifest feature modified. Do not require a full all-features workspace test unless the actual manifest change affects several feature-gated surfaces and narrower checks are insufficient.

## 13. Static guards

Do not add dependency-size or version guards.

Cargo manifests, lockfile review, focused tests, and closure measurements are sufficient. Do not add a script that bans qrcode defaults or a continuous bloat threshold.

## 14. Acceptance criteria

M004 closes only when:

- qrcode defaults are disabled unless source evidence proves CodeGG requires them;
- QR terminal rendering remains functional;
- Comrak default-feature narrowing is tested and either safely retained or explicitly reverted with rationale;
- RustPython contribution is measured and only low-risk feature narrowing is attempted;
- no custom parser, binary split, feature deletion, or MSRV increase is introduced;
- touched material upstream dependencies are reviewed for maintenance/advisory/MSRV risk;
- final feature trees and release measurements are recorded consistently with baseline;
- accepted dependency changes pass focused tests, workspace check where needed, release build, and `scripts/verify.sh quick`;
- no automatic update/audit/size infrastructure is added.

## 15. Stop conditions

Stop a candidate reduction and keep current behavior when:

- it requires feature removal;
- it breaks the documented MSRV;
- it requires a major dependency migration or handwritten replacement;
- measured impact is negligible and maintenance complexity rises;
- source behavior cannot be proven equivalent with existing focused tests.

M004 itself should still close with a documented no-change decision for such candidates.

## 16. Required closure evidence

`plans/closure/post-audit-correctness-simplification/004-status.md` must include:

- implementation commit/PR;
- baseline/final toolchain, target, profile, feature set, and binary bytes;
- qrcode feature-tree before/after;
- Comrak keep/revert decision and evidence;
- RustPython/top-contributor measurement;
- upstream advisory/maintenance/MSRV summary;
- focused verification outcomes;
- explicit confirmation of no feature loss, binary split, MSRV increase, or new automation.
