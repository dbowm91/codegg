# Post-Audit Correctness, Simplification, and Footprint Roadmap

Status: active

Repository baseline reviewed: `0323d68e0c37c0495540d39ec0d6d9520f124125`

Long-term references:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`
- `architecture/core.md`
- `architecture/tool.md`
- `architecture/testing.md`
- `architecture/overview.md`

Related ADRs:

- None required for the planned work. Existing single-daemon ownership, scheduler authority, tool policy, protocol, and single-binary decisions remain intact.
- An ADR is required only if implementation discovers that a user-visible protocol, daemon ownership, persistent storage, or executable topology change is necessary. The plans below are intentionally written to avoid such changes.

## 1. Purpose and ownership boundary

This workstream addresses the concrete defects and simplification opportunities found in the August 2026 post-closure repository audit without reopening the already-closed architecture lines.

It owns:

- bounded and SSRF-safe untrusted HTTP retrieval used by built-in web fetch and research URL collection;
- daemon-stop process identity verification and correct CLI JSON serialization;
- TUI text parsing, Unicode wrapping, and small render-duplication cleanup;
- no-feature-loss dependency/default-feature reduction and measured upstream maintenance review;
- contraction of routine CI/static verification where checks are redundant or based on an invalid premise;
- removal of the global 32 MiB test-stack workaround after identifying and correcting the actual stack-heavy path;
- removal of pass-through execution representations only where they establish no independent invariant;
- final measured footprint, verification, documentation, and closure evidence.

It consumes, but does not redefine:

- daemon and scheduler execution authority;
- provider, ACP, projection, plugin, Git, and Tool Program contracts;
- manual release cadence;
- the existing default single-binary topology;
- runtime-safety/Landlock work already recorded under `runtime-safety-resource-footprint`.

The governing rule is:

> Fix concrete correctness and safety defects first, then remove demonstrably redundant code, dependencies, and verification machinery. Do not trade feature coverage or architectural clarity for speculative reduction.

## 2. Work classification

### Invariants

- Untrusted HTTP content is bounded during streaming, not after whole-body accumulation.
- SSRF validation must constrain the address actually used for the network connection; DNS validation followed by an unrelated resolver pass is insufficient.
- Explicit caller output limits remain hard upper bounds.
- Daemon stop must signal only the daemon instance whose identity matches live protocol evidence.
- Machine-readable CLI JSON is emitted by a JSON serializer, not handwritten escaping.
- TUI layout estimation and rendering use one Unicode display-width model.
- Routine CI contains only checks that provide distinct, correct signal for this repository's scope.
- Dependency reduction preserves all supported user-visible features.
- Existing daemon/scheduler/tool authority and single-binary topology remain unchanged.

### Capabilities

- Web fetch and research URL collection reject oversized bodies without buffering them fully.
- DNS rebinding cannot move an approved outbound request to an unapproved local/special-use address between validation and connection.
- `codegg daemon stop` cannot terminate an unrelated PID merely because metadata is stale.
- JSON output remains valid for arbitrary valid UTF-8 content, including control characters.
- TUI reasoning-tag detection and wrapping remain correct for multiline and wide-Unicode input.
- Ordinary CI remains small enough for this local-development-oriented project while retaining meaningful correctness coverage.

### Infrastructure and polish

- One small bounded HTTP body reader may be shared by the two in-tree untrusted-fetch consumers when that reduces duplication without creating a global networking abstraction.
- Cargo features are narrowed only where source evidence proves unused defaults.
- Static guards and compatibility layers are deleted when the compiler, existing ownership boundary, or upstream behavior already provides the same invariant.
- Execution representations are collapsed only when a layer merely mirrors another type and no distinct policy/provenance state would be lost.

## 3. Explicit non-goals

This roadmap must not:

- split CodeGG into daemon/TUI binaries;
- redesign the scheduler, daemon, ACP, provider, plugin, projection, Git, or Tool Program subsystems;
- create a generalized HTTP framework for every provider and MCP client;
- add a browser/crawler, redirect-following policy expansion, proxy subsystem, or Cloudflare bypass work;
- replace RustPython with a handwritten Python parser merely to reduce size;
- raise MSRV solely to chase dependency freshness;
- remove image, clipboard, QR, syntax highlighting, plugin, server, research, LSP, or other documented capability;
- add CI matrices, coverage gates, benchmark gates, cargo-audit on every PR, automatic dependency bots, release automation, or binary-size gates;
- add new static guards for preferences that are already enforced by Rust types, crate boundaries, or direct tests;
- require a full local workspace validation after every milestone when focused tests plus `scripts/verify.sh quick` are sufficient;
- reopen the runtime-safety workstream's remaining supported-Linux Landlock evidence condition.

## 4. Current-state summary

At the reviewed baseline:

- `src/tool/webfetch.rs` computes the effective output cap as `max_length.min(max_output_chars.max(max_length))`, which always resolves to `max_length`; the framework-provided outer limit is therefore ineffective.
- `src/tool/webfetch.rs` and `src/research/sources/url.rs` call `response.bytes().await` before enforcing their 5 MiB limit, so chunked or misleading responses can consume substantially more memory before rejection.
- `src/security/ssrf.rs` validates DNS answers and re-resolves before send, but the reqwest connection performs its own hostname resolution afterward. Validation therefore does not pin the actual connection address and leaves a DNS-rebinding TOCTOU boundary.
- `codegg daemon stop` reads metadata/PID, verifies that the socket answers, then signals the stored PID without proving the live daemon's `daemon_id` matches the metadata record.
- `OutputFormat::Json` manually escapes a subset of JSON control characters rather than using `serde_json`.
- `src/tui/components/messages.rs` has separate wrapping/counting implementations with differing Unicode-width behavior, and `find_any_tag()` mixes absolute offsets with line-local indexing.
- `src/tui/components/dialogs/share.rs` duplicates its render body across `Widget` and `Component` implementations.
- `qrcode` is used for text rendering but currently uses default features that include image/SVG/pic support. `comrak` and `rustpython-parser` deserve measured feature/contributor inspection, but replacement is not justified without evidence.
- routine CI is already one bounded job, but it still carries a Tokio test-flavor scanner/baseline based on the incorrect premise that bare `#[tokio::test]` defaults to a multithreaded runtime; it also runs `cargo check` immediately before all-target Clippy and runs guard self-tests on every PR.
- `RUST_MIN_STACK=33554432` globally masks a stack-heavy daemon-socket test/runtime path rather than documenting the actual root cause.
- command intent/planning/routing/outcome code contains several mirror/pass-through representations that should be removed only where repository inspection confirms they do not establish a policy, provenance, compatibility, or persistence boundary.

## 5. Target architecture

### 5.1 Untrusted HTTP boundary

Both in-tree untrusted URL consumers use a small shared or equivalently consistent mechanism that:

1. parses and validates HTTP(S) URLs;
2. resolves the host once for the request attempt;
3. rejects every disallowed resolved address;
4. causes the HTTP client to connect only to the validated address set while retaining the original hostname for TLS/SNI/Host semantics;
5. streams response chunks under a cumulative byte cap;
6. stops reading and returns a typed error once the cap is crossed;
7. separately applies the model/output character cap after safe body collection/decoding.

Automatic redirect following remains disabled for the built-in web-fetch path. Any explicit redirect handling must repeat URL/address validation rather than inherit trust from the previous host.

### 5.2 Daemon lifecycle and CLI serialization

Daemon metadata remains diagnostic, but `daemon stop` requires live protocol identity to match metadata before signalling. Legacy PID-only behavior may remain only when it can prove identity safely; otherwise it should fail closed with actionable diagnostics.

JSON output is produced with `serde_json` from typed values.

### 5.3 TUI text rendering

One canonical display-width-aware wrapping primitive drives both rendered lines and line-count estimation. Thinking-tag scanning maintains separate line-local and absolute offsets. Shared dialog rendering eliminates literal duplicate layout construction.

### 5.4 Dependency and CI posture

Dependency work follows: disable unnecessary default features, inspect the feature tree, measure release contributors, and stop. Major replacement is out of scope unless a dependency is both dominant and removable without a new subsystem.

Routine CI remains one job. It should prefer distinct signal over ceremonial duplication: generated assets, high-value security/ownership checks, formatting, Clippy, and bounded tests. Local `verify.sh quick` remains the fast compilation entry point.

### 5.5 Execution model simplification

The desired conceptual path remains:

```text
typed intent -> validated execution plan -> executor -> persisted outcome
```

A separate representation remains only when it captures a state transition or invariant that another layer cannot express. Renaming/reorganizing without deleting real complexity does not count as success.

## 6. Dependency graph

```text
M001 HTTP safety and bounded streaming -------+
M002 daemon/CLI correctness ------------------+
M003 TUI correctness/duplication -------------+
M004 dependency slimming/upstream review -----+--> M008 integration and closure
M005 CI and guard simplification -------------+
M006 test stack/resource correction ----------+
M007 execution-model simplification ----------+
```

Dependency classes:

- M001 through M007 have no hard dependency on one another and are independently executable against the reviewed baseline, subject to rebasing on current `main` before editing.
- M004 has a soft dependency on M003 only for final binary measurements if TUI source cleanup changes linkage; implementation may proceed independently.
- M006 has a soft dependency on M005 because CI environment cleanup should not hide the stack investigation, but either may land first if the final state is reconciled.
- M008 has hard dependencies on M001-M007 and is the only milestone that may mark this workstream closed.
- The remaining Linux evidence condition in `runtime-safety-resource-footprint` is operationally independent and must not block this roadmap.

## 7. Ordered milestones

### M001 — Untrusted HTTP safety and bounded streaming

Status: closed

Plan: `plans/implementation/post-audit-correctness-simplification/001-untrusted-http-safety-and-bounded-streaming.md`

Correct the output-cap bug, bind SSRF validation to the actual connection destination, stream bodies under hard byte limits, and consolidate only the duplicated untrusted-body mechanics.

### M002 — Daemon stop identity and CLI JSON correctness

Status: closed

Plan: `plans/implementation/post-audit-correctness-simplification/002-daemon-stop-identity-and-cli-json-correctness.md`

Require live daemon identity before signalling and replace handwritten JSON escaping with serializer-backed output.

### M003 — TUI text-layout correctness and render deduplication

Status: ready

Plan: `plans/implementation/post-audit-correctness-simplification/003-tui-text-layout-correctness-and-render-deduplication.md`

Fix thinking-tag offset handling, unify Unicode-aware wrapping/counting, and remove duplicated ShareDialog rendering without changing UI behavior.

### M004 — Dependency feature slimming and upstream maintenance review

Status: ready

Plan: `plans/implementation/post-audit-correctness-simplification/004-dependency-feature-slimming-and-upstream-review.md`

Disable provably unused defaults such as qrcode image renderers, test safe Comrak narrowing, measure RustPython/other dominant contributors, and record upstream/MSRV risk without dependency churn for its own sake.

### M005 — Routine CI and static-guard simplification

Status: ready

Plan: `plans/implementation/post-audit-correctness-simplification/005-routine-ci-and-static-guard-simplification.md`

Delete the invalid Tokio-flavor baseline/scanner machinery, remove redundant hosted compilation and routine guard self-tests where signal is duplicated, and keep the existing one-job/manual-release posture.

### M006 — Test stack and resource-root-cause correction

Status: ready

Plan: `plans/implementation/post-audit-correctness-simplification/006-test-stack-and-resource-root-cause-correction.md`

Identify the actual stack-heavy daemon-socket path, make the smallest code/test-structure correction, and remove the global 32 MiB `RUST_MIN_STACK` requirement when evidence proves it is no longer needed.

### M007 — Execution-model pass-through and duplication cleanup

Status: ready

Plan: `plans/implementation/post-audit-correctness-simplification/007-execution-model-pass-through-cleanup.md`

Inspect command planner/routing/outcome layers and remove only representations or compatibility shims that merely mirror another type and have no distinct invariant or consumer.

### M008 — Integration, measurement, and closure

Status: blocked on M003-M007

Plan: `plans/implementation/post-audit-correctness-simplification/008-integration-measurement-and-closure.md`

Reconcile documentation, run minimal broad verification, capture final dependency/binary measurements, confirm no feature or architecture regression, and create one closure record for the workstream.

## 8. Verification posture

Milestones use focused tests for changed behavior plus `scripts/verify.sh quick` when the change affects production code or manifests. Do not run or require repeated full-workspace validation unless the affected surface cannot be covered otherwise.

M008 owns the one broad integration pass and hosted CI evidence. No milestone may add a new workflow lane, matrix, artifact upload, scheduled job, release process, or continuous size/audit gate.

Static guards are justified only for invariants that cannot be expressed more directly through types, feature boundaries, compile tests, or focused unit/integration tests.

## 9. Security, compatibility, storage, protocol, and migration

Security:

- M001 is security-sensitive because SSRF validation must control the actual socket destination.
- M002 is lifecycle-safety-sensitive because PID reuse must not terminate unrelated processes.
- No other milestone may weaken sandbox, command, permission, scheduler, or provider security semantics.

Compatibility:

- CLI command names, config/state paths, daemon endpoint discovery, protocol versions, supported features, and user-visible TUI behavior remain stable.
- Dependency feature changes must preserve supported target/features.

Storage:

- no schema migration is planned.

Protocol:

- no wire-format change is expected. M002 should reuse existing daemon identity already exposed by protocol responses/hello rather than invent a new message unless repository reality makes that impossible.

Migration:

- none expected beyond deleting obsolete CI/baseline files and adjusting internal types/imports.

## 10. Exit conditions

The workstream is complete only when:

- all M001-M007 acceptance criteria are satisfied;
- oversized untrusted responses are bounded before accumulation;
- outbound SSRF validation constrains the actual connected address;
- caller output limits are effective;
- daemon stop proves live identity before signalling;
- CLI JSON uses serializer-backed encoding;
- multiline thinking-tag and Unicode wrapping regressions are fixed;
- qrcode and any other accepted dependency defaults are narrowed with no feature loss;
- no dependency replacement is performed without measured benefit;
- invalid/redundant CI machinery is removed while the single routine job remains sufficient;
- the global 32 MiB stack override is removed or, if a platform-level requirement is proven, replaced by a narrowly documented scope rather than kept as an unexplained global workaround;
- execution-model cleanup deletes real pass-through complexity without changing execution authority;
- final release-size and feature-tree measurements are recorded against the baseline;
- `scripts/verify.sh quick` and the existing hosted `verify` job pass on the accepted final tree;
- one compact closure record documents evidence and any remaining low-severity/deferred items.

## 11. Deferred work

The following remain outside this roadmap unless new evidence makes them necessary:

- arbitrary non-UTF-8 argv expansion;
- binary topology split;
- replacing RustPython with a custom parser;
- broad HTTP/provider client unification;
- major Comrak/MSRV migration;
- periodic/scheduled dependency automation;
- new CI lanes or cross-platform matrices;
- release automation;
- redesign of daemon shutdown into a new protocol family when identity matching can safely correct the current path;
- unrelated cleanup discovered while touching neighboring modules.
