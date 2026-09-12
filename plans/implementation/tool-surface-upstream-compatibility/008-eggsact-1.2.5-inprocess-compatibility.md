# Tool-Surface Upstream Compatibility M008 — Eggsact 1.2.5 In-Process Compatibility

Status: implemented

Repository baseline reviewed: `2798501b61edcd43acb215924500635dc705bbb2`

Planning branch: `agent/tool-surface-compat-2026-09`

Source corrective addendum:

- `plans/subsystems/tool-surface-upstream-compatibility-corrective-addendum.md`

Dependencies:

- none hard; this milestone is independent of M006/M007 and may execute in parallel.

Audited upstream baseline: eggsact `1.2.5`.

Relevant CodeGG files:

- `Cargo.toml` / `Cargo.lock`;
- `src/eggsact/adapter.rs`;
- `src/tool/deterministic.rs`;
- `src/tool/integrated_config.rs`;
- `src/preflight/*`;
- `architecture/deterministic_tools.md`;
- `architecture/preflight.md`;
- `architecture/native_crates.md`.

## 1. Objective

Update CodeGG's in-process eggsact integration so profile/audience selection and harness-side preflight track the current 1.2.5 API without copying stale upstream capability lists or broadening CodeGG's model-facing tool surface by default.

## 2. Current implementation evidence

At the reviewed baseline:

- root `Cargo.toml` declares `eggsact = "1.1.4"`, a caret-compatible requirement rather than an exact pin;
- architecture docs describe that declaration as though 1.1.4 were the active integration baseline;
- `EggsactRuntime` correctly uses the in-process `eggsact::agent::ToolRegistry` rather than MCP;
- runtime profile parsing calls `Profile::from_str_opt(&config.profile).unwrap_or(Profile::Default)`;
- `src/tool/integrated_config.rs` separately hard-codes four known profiles: `codegg_core`, `codegg_core_min`, `default`, `full`;
- eggsact 1.2.5 exposes additional built-in profiles including `codegg_preflight`, `codegg_patch`, `codegg_config`, `codegg_unicode_security`, `codegg_shell`, `codegg_repo_audit`, and `human_math`;
- CodeGG manually defines a curated eggsact-backed model-facing palette instead of blindly exposing the entire upstream registry;
- eggsact 1.2.5 adds typed `DependencyPreflight` and other typed-first service composition while retaining raw handlers for 1.x compatibility;
- eggsact's new MCP discovery surface is irrelevant to CodeGG's normal in-process integration and would duplicate CodeGG `tool_search` if imported.

## 3. Correctness defect to eliminate

An unknown configured eggsact profile currently falls back silently to `Profile::Default`. This changes execution policy without making the configuration error visible and can widen/narrow capability unexpectedly.

M008 MUST replace this with explicit behavior. Preferred outcome:

- parse known upstream built-ins through eggsact's own parser;
- if CodeGG intentionally supports custom profiles, construct them explicitly only through a documented configuration path and verify registry behavior;
- otherwise return a configuration/runtime error with the invalid profile name and accepted built-ins;
- never silently substitute `default`.

## 4. Non-goals

M008 MUST NOT:

- run eggsact as an MCP subprocess;
- expose eggsact's `tool_search`/`tool_invoke` facade in CodeGG;
- automatically register every eggsact tool;
- replace CodeGG's native `tool_search`/disclosure system;
- redesign all preflight policy;
- duplicate eggsact schemas/types locally when a stable typed upstream wrapper exists;
- add a new CI workflow or dependency-update bot;
- make `full` the default profile merely because it contains more tools.

## 5. Invariants

- Eggsact stays in-process.
- CodeGG continues to control the model-visible palette and deferred disclosure.
- Model audience cannot execute harness-only/hidden tools.
- Harness-side preflight may use narrower purpose-built profiles without changing model exposure.
- Invalid configuration fails visibly and deterministically.
- Existing valid `codegg_core`, `codegg_core_min`, `default`, and `full` configurations remain valid.
- Machine codes/findings remain structured and are not reduced to string parsing for policy decisions.
- Dependency upgrade must not force an MSRV change without explicit evidence and planning.

## 6. Required upstream/API audit

Before editing, inspect eggsact 1.2.5 for:

- `Profile` variants and `from_str_opt()` behavior;
- audience/exposure semantics;
- `ToolRegistry::with_profile_and_audience()` compatibility;
- any registry metadata API suitable for listing accepted profiles or validating tool availability;
- typed preflight wrappers relevant to CodeGG (`DependencyPreflight` and existing config/patch/shell/unicode wrappers);
- `ToolResponse` fields consumed by CodeGG;
- semver/MSRV/dependency changes between the currently resolved version and 1.2.5.

Record material changes only; do not create a permanent mirror of eggsact's registry.

## 7. Expected production-code changes

### 7.1 Dependency baseline

Resolve CodeGG intentionally against eggsact 1.2.5 if compatible with the repository MSRV and current API.

Prefer an explicit compatible requirement consistent with project dependency policy. If leaving `"1.1.4"` as the minimum caret requirement while the lockfile resolves 1.2.5, documentation must say exactly that. If bumping the minimum to 1.2.5 is necessary to consume new APIs, update manifest and lockfile coherently.

Do not pin an exact patch without a project-specific reason.

### 7.2 Remove duplicated profile allowlist

Delete or replace `KNOWN_EGGSACT_PROFILES` as an independently maintained list.

Validation should delegate to eggsact's authoritative profile parser or another stable upstream metadata API. If CodeGG needs accepted-name diagnostics, derive them from upstream constants/API where possible; otherwise keep one tiny compatibility helper colocated with parsing and cover it with a drift test against `Profile::from_str_opt()`.

The important invariant is that CodeGG cannot accept/reject a profile differently from the version of eggsact it actually links.

### 7.3 Fail closed on invalid profile

Change `EggsactRuntime::new()` so unknown profiles do not become `Profile::Default`.

Expected error behavior:

- identify the invalid profile;
- state that no fallback was applied;
- provide accepted examples/names where practical;
- propagate through integrated config/bootstrap in an actionable form.

Add tests proving an invalid name fails and valid new 1.2.5 profiles parse correctly.

### 7.4 Audience/exposure regression audit

Verify that CodeGG's `model` and `harness` mappings still correspond to current eggsact semantics. Add focused tests demonstrating:

- model cannot execute harness-only/hidden tools;
- harness behavior is unchanged or intentionally narrowed;
- CodeGG does not accidentally use Debug audience for production model calls.

### 7.5 Harness-side typed preflight assessment

Evaluate current CodeGG preflight call sites against eggsact 1.2.5 typed wrappers.

Highest-value candidate is `DependencyPreflight` because it provides typed ecosystem detection/verdict/machine-code behavior. Adopt it only if it replaces manual JSON field assumptions or materially strengthens type safety on an existing CodeGG path.

If adopted:

- keep policy/orchestration in CodeGG;
- use eggsact typed output for facts/verdicts;
- preserve current blocking/warn/observe semantics;
- add no new model-visible tool unless separately justified.

If not adopted, closure must briefly record why current CodeGG workflows do not consume dependency preflight yet. This is an assessment requirement, not forced feature expansion.

### 7.6 New utility surface disposition

Review eggsact 1.2.4+ utilities (`ip_inspect`, `cidr_inspect`, `codec_convert`, `radix_convert`, `datetime_convert`, `cron_inspect`) and 1.2.5 config inspection changes.

Default disposition should be deferred unless CodeGG has an existing workflow where one of these removes bespoke logic. Do not register them merely to claim parity.

If one is exposed, prefer deferred discovery rather than the immediate core palette unless evidence shows frequent coding-agent value.

### 7.7 Documentation fidelity

Update docs so they distinguish:

- manifest minimum/semver requirement;
- resolved/tested eggsact baseline;
- in-process integration ownership;
- curated CodeGG tool palette versus full upstream capability;
- intentionally excluded MCP discovery facade.

## 8. Ordered work packages

### WP1 — Resolve and compile against eggsact 1.2.5

1. Inspect lockfile/current resolved eggsact version.
2. Update manifest minimum only if required.
3. Compile focused eggsact adapter/preflight code.
4. Record any API/MSRV incompatibility before changing architecture.

### WP2 — Converge profile validation

1. Remove the stale CodeGG allowlist.
2. Delegate parsing to eggsact.
3. Replace silent `Default` fallback with explicit error.
4. Add current-profile and invalid-profile tests.

### WP3 — Verify audience/exposure behavior

1. Exercise model/harness registries using current 1.2.5 APIs.
2. Confirm hidden/harness-only behavior.
3. Preserve current CodeGG exposure flags and `tool_search` ownership.

### WP4 — Typed preflight evaluation

1. Locate manual JSON-based eggsact preflight consumption.
2. Compare against available typed wrappers.
3. Adopt only high-value replacements with bounded changes.
4. Record explicit deferral for wrappers with no current consumer.

### WP5 — Curated tool-surface review

1. Compare `build_eggsact_tools()` against current CodeGG workflow needs.
2. Do not chase parity.
3. Add/defer utilities based on demonstrated overlap reduction or coding utility.
4. Ensure immediate palette size does not increase accidentally.

### WP6 — Docs and closure evidence

Update deterministic-tools/preflight/native-crates docs and record exact eggsact resolved/tested version.

## 9. Focused verification

Expected commands, adapting test target names to repository reality:

```bash
cargo fmt --all -- --check
cargo test --lib eggsact -- --test-threads=1
cargo test --test preflight_integration -- --test-threads=1
cargo test --lib preflight -- --test-threads=1
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Also inspect `cargo tree -i eggsact` or lockfile evidence to record the actual resolved version. Do not add dependency-audit infrastructure solely for this milestone.

## 10. Acceptance criteria

M008 is implementation-complete when:

- CodeGG is verified against eggsact 1.2.5 or an explicit blocker explains why not;
- duplicated four-profile validation is gone or mechanically tied to upstream parsing;
- invalid profiles no longer silently become `default`;
- all current upstream built-in profiles relevant to CodeGG validate consistently with eggsact;
- audience/exposure behavior has regression tests;
- typed `DependencyPreflight` has been either adopted where it replaces brittle JSON handling or explicitly deferred with rationale;
- no eggsact MCP/discovery duplicate enters the CodeGG palette;
- docs state the actual dependency/testing baseline;
- focused verification and `scripts/verify.sh quick` pass.

## 11. Stop conditions

Stop and record a blocker if:

- eggsact 1.2.5 raises MSRV beyond CodeGG's accepted baseline;
- current in-process APIs require a breaking architectural migration;
- profile semantics changed in a way that conflicts with CodeGG security/exposure policy;
- adopting a typed wrapper requires redesigning unrelated preflight ownership.

## 12. Closure evidence required

The closure record must include:

- implementation commit(s);
- manifest/lockfile resolved eggsact version;
- profile parsing/invalid-profile evidence;
- model versus harness audience tests;
- typed preflight adoption or deferral rationale;
- curated-tool-surface disposition;
- verification commands/outcomes;
- documentation updates.
