# Dependency Security and Workspace Consolidation Roadmap

Status: active; M001-M006 closed; M005 CodeGG adoption implemented via Eggup

Repository baseline reviewed: `b3459640765261057e902a9df05bc71faa6cecc4`

Canonical long-term references:

- `plans/000-long-term-specification.md` §4.2, explicit ownership;
- `plans/000-long-term-specification.md` §4.7, correctness before transparent magic;
- `plans/002-long-term-roadmap.md`, maintainable implementation and bounded infrastructure;
- `plans/003-planning-process.md`, bounded milestone and closure requirements.

Related accepted evidence:

- `plans/closure/runtime-safety-resource-footprint/005-status.md` — prior dependency-feature normalization;
- `plans/closure/agent-runtime-correctness-autonomy-simplification/007-status.md` — measured footprint and Wasmtime feature contraction;
- `plans/closure/http-client-consolidation/003-status.md` — Eggfetch adoption and the retained DashMap 5/6 split.

Related ADRs: none required at roadmap creation. Open an ADR only if implementation proposes a new durable cross-repository package contract that changes product architecture rather than dependency ownership.

## 1. Purpose and ownership boundary

This workstream reduces CodeGG's dependency security exposure and maintenance burden without turning dependency minimization into an end in itself. It owns:

- resolving actionable advisory/unsoundness findings in the active dependency graph;
- removing avoidable duplicate major-version trains where CodeGG directly controls one side;
- narrowing dependency feature ownership when source evidence shows broader features are unused;
- centralizing stable workspace dependency/version/package policy without globally widening feature sets;
- reducing optional image-feature closure to the formats CodeGG actually supports;
- qualifying existing extracted crates for independent reuse where their boundaries are already general;
- defining the CodeGG side of a future generic binary-updater contract rather than duplicating security-sensitive updater mechanics;
- bounded remediation of newly published dependency advisories that appear after a milestone closes, without rewriting historical closure evidence or broadening into general updates.

Binary size is a useful secondary metric. Maintenance ownership, security posture, explainable feature activation, and avoiding duplicate implementations are the primary objectives.

## 2. Classification

Primary classification: **infrastructure**.

M001 and M006 are also **invariant** work because memory-safety/TLS advisories and audit exceptions must not be hidden by lockfile churn. M004 is **polish/infrastructure** because it qualifies existing boundaries rather than adding product capability. M005 is **infrastructure** with a cross-repository interface dependency.

## 3. Current-state evidence

At the reviewed baseline and subsequent accepted closure points:

1. The root package originally used `ratatui = "0.29"`, while the optional image stack already resolved Ratatui 0.30-era crates. M001 moved the supported graph to Ratatui 0.30/Crossterm 0.29 and patched `lru 0.18.2`.
2. CodeGG root/core/providers originally used DashMap 5 while `eggfetch-core 0.1.4` used DashMap 6. M001 converged CodeGG ownership onto DashMap 6.
3. Root/core SQLx declarations originally enabled `macros` and `migrate`; M001 narrowed them to source-demonstrated `derive` plus SQLite/runtime/chrono/json ownership. `sqlx-mysql`/`rsa` remain lock-union-only and the RSA audit exception is documented accordingly.
4. M002 established `[workspace.package]` and `[workspace.dependencies]` as the shared version/default-policy owner while leaving crate-specific feature additions local.
5. M003 disabled `image` defaults and `ratatui-image` `image-defaults`, retaining PNG/JPEG/GIF/WebP plus proven built-in BMP support while removing optional image-default dependency families.
6. M004 qualified `egggit`, `eggsentry`, `codegg-protocol`, and `eggcontext` as publishable boundaries and explicitly retained CodeGG-specific crates internally without creating speculative new crates.
7. Historical M005 closure confirmed that no generalized external updater package then satisfied CodeGG's required interface. The follow-up CodeGG adoption now consumes Eggup's immutable-pinned core/acquisition APIs; see `plans/closure/dependency-security-workspace-consolidation/007-codegg-eggup-adoption.md`. The original 005 closure remains unchanged as historical evidence.
8. During M001 closure, newly published RUSTSEC-2026-0285 was detected against resolved `rustls 0.23.41`. The advisory is medium severity and identifies `rustls >=0.23.45` as patched. M001-M003 correctly recorded it as separate bounded maintenance because it was post-baseline and unrelated to their scoped dependency changes. M006 owns that remediation.

## 4. Invariants and non-goals

The workstream MUST preserve:

- CodeGG's Rust 1.89 MSRV unless a separately approved plan changes it;
- `#![deny(unsafe_code)]` boundaries and narrowly documented existing exceptions;
- supported TUI behavior and key/input/rendering semantics;
- SQLite storage/schema behavior and handwritten migration ordering;
- Eggfetch's explicit `http1,tls-rustls,json` ownership and security-sensitive validated-destination behavior;
- deterministic packaged WebPKI trust behavior unless separately approved;
- the single default CodeGG binary topology;
- optional feature semantics: server, plugins, image, LSP test support, and clipboard remain opt-in/default exactly as currently documented unless a milestone explicitly owns that feature;
- historical closure records without rewriting them to conceal earlier assumptions.

This roadmap does **not** authorize:

- replacing SQLite/SQLx as a storage architecture;
- writing custom HTTP, image, archive, terminal, crypto, parser, or database implementations to save bytes;
- replacing Axum/Tower with Eggserve merely for ecosystem uniformity;
- adding Eggress as a dependency without a real outbound-egress mediation requirement;
- depending on greggd or importing service-manager/systemd/launchd machinery;
- a generic CodeGG-wide HTTP wrapper on top of Eggfetch;
- a new generic network-policy crate before at least two independent consumers justify the boundary;
- automatic dependency bots, scheduled advisory jobs, new CI lanes, binary-size thresholds, or fixed release cadence;
- publishing crates or releases automatically as part of this workstream;
- using a security advisory as justification for broad lockfile modernization when a targeted patch exists.

## 5. Target architecture

After M001-M004 close:

- the default and optional dependency graph contains no known avoidable duplicate DashMap major owned by CodeGG;
- Ratatui/LRU are on a sound supported line with no stale `lru` advisory path retained solely by CodeGG's direct TUI dependency;
- SQLx enables only source-demonstrated features and its remaining RSA advisory exception is explicitly lock-union/unreachable-to-supported-build evidence;
- common workspace dependency versions and package policy have one authoritative manifest location, while crate-specific features remain local and minimal;
- the optional image graph is restricted to the formats CodeGG advertises plus the proven BMP extra;
- reusable extracted crates have explicit publishability/internal dispositions and package boundaries that do not depend on the root application;
- CodeGG-specific crates remain internal when no independent consumer justifies a stable public contract.

M005 is a later interface-convergence milestone. Its target is a generic updater crate owned outside CodeGG that accepts application/repository/asset policy and can use an injected/native Rust transport. CodeGG must not copy Gregg's updater implementation into its root crate while waiting for that interface.

M006 restores the current TLS dependency graph to a patched Rustls 0.23 line while preserving the Eggfetch/WebPKI ownership established by HTTP-client consolidation.

## 6. Dependency graph and milestone order

```text
M001 security + duplicate graph convergence
        |
        v
M002 workspace dependency ownership normalization
        |
        +-------------------+
        |                   |
        v                   v
M003 optional image       M004 reusable crate
     graph slimming            boundary qualification

M005 generic updater interface + CodeGG adoption
  hard: M002 (satisfied)
  interface: independently published/generalized updater contract (blocked)

M006 Rustls advisory remediation + planning reconciliation
  ready now; independent of blocked M005
  evidence basis: M001-M004 closed + current Cargo.lock/RustSec state
```

M003 and M004 were ordered after M002 to avoid repeated manifest churn. M006 is new-evidence maintenance and has no dependency on M005.

## 7. Milestones

### M001 — Security and duplicate dependency-graph convergence

Plan: `plans/implementation/dependency-security-workspace-consolidation/001-security-and-duplicate-graph-convergence.md`

Status: **closed**. Closure: `plans/closure/dependency-security-workspace-consolidation/001-status.md` (implementation `3bd54ccd`).

Goals:

- resolve current `lru` unsoundness exposure by moving the direct TUI dependency train to a fixed supported Ratatui/LRU line;
- converge CodeGG-owned DashMap usage on the Eggfetch-compatible major where source compatibility permits;
- narrow SQLx feature ownership using source census and reconcile MySQL/RSA lock-union/audit evidence;
- record fresh `cargo tree -d`, advisory, and release-size evidence without making them permanent gates.

### M002 — Workspace dependency ownership normalization

Plan: `plans/implementation/dependency-security-workspace-consolidation/002-workspace-dependency-ownership-normalization.md`

Status: **closed**. Closure: `plans/closure/dependency-security-workspace-consolidation/002-status.md` (implementation `05e7b258`).

Goals:

- establish `[workspace.package]`, `[workspace.dependencies]`, and bounded workspace lint inheritance where it removes duplicated ownership;
- centralize versions/default-feature policy without creating a workspace-wide feature superset;
- centralize internal path/version declarations where member release semantics are shared;
- preserve intentional package-specific metadata and feature differences.

### M003 — Optional image feature-graph slimming

Plan: `plans/implementation/dependency-security-workspace-consolidation/003-optional-image-feature-graph-slimming.md`

Status: **closed**. Closure: `plans/closure/dependency-security-workspace-consolidation/003-status.md`.

Goals:

- remove `ratatui-image`'s `image-defaults` expansion;
- retain PNG/JPEG/GIF/WebP behavior plus the already-supported built-in BMP path;
- measure optional-image graph contraction and preserve image tests.

### M004 — Reusable crate boundary qualification

Plan: `plans/implementation/dependency-security-workspace-consolidation/004-reusable-crate-boundary-qualification.md`

Status: **closed**. Closure: `plans/closure/dependency-security-workspace-consolidation/004-status.md` (implementation `b95d37ec`).

Goals (met):

- separate `eggcontext` deterministic tokenizer primitives from volatile model-name policy sufficiently for a stable reusable contract;
- improve package metadata/docs/tests required for standalone consumption;
- explicitly retain `codegg-git`, `codegg-config`, and `codegg-providers` as CodeGG-owned without a second consumer/stable generic boundary;
- perform package dry-runs without automatic publication.

### M005 — Generic updater interface and CodeGG adoption

Plan: `plans/implementation/dependency-security-workspace-consolidation/005-generic-updater-interface-and-codegg-adoption.md`

Status: **implemented by follow-up adoption**. M002's hard dependency is satisfied by `plans/closure/dependency-security-workspace-consolidation/002-status.md`. The external updater interface is provided by Eggup and CodeGG's managed bundle adoption is recorded in `plans/closure/dependency-security-workspace-consolidation/007-codegg-eggup-adoption.md`. The original 005 closure remains immutable and records the earlier blocked state.

Hard dependency: M002 accepted closure.

The dependency is now closed by Eggup's independently consumable acquisition/core APIs. CodeGG retains release/version/target/archive policy and its existing Eggfetch transport profile; Eggup owns generic bounded acquisition and local verified transaction mechanics. No Gregg implementation, greggd dependency, or service-manager behavior was adopted.

### M006 — Rustls advisory remediation and planning reconciliation

Plan: `plans/implementation/dependency-security-workspace-consolidation/006-rustls-advisory-remediation-and-planning-reconciliation.md`

Status: **closed**. No hard predecessor; M005 does not block this work.

Closure: `plans/closure/dependency-security-workspace-consolidation/006-status.md` (lock-only Rustls 0.23.41 → 0.23.45; RUSTSEC-2026-0285 absent; broad verification green).

Goals:

- reproduce current Rustls reverse dependencies and RUSTSEC-2026-0285 applicability before changing the lockfile;
- move the supported Rustls 0.23 resolution from the affected line to a patched release at or above the authoritative fixed floor (0.23.45 at plan creation);
- prefer a targeted lock-only patch update and change an owning dependency only if an exact transitive constraint makes that necessary;
- preserve Eggfetch HTTP/1 + Rustls/WebPKI ownership, provider streaming/cancellation/redirect semantics, validated-destination behavior and EggLSP download behavior;
- prove the advisory is absent without adding an ignore or broad dependency churn;
- reconcile the stale active-registry control point while preserving historical closure records.

## 8. Cross-cutting security requirements

- Run a fresh advisory check against the candidate lockfile; do not assume the advisory set from roadmap creation remains current.
- Every ignored RustSec advisory must have a currently reachable package path, a documented applicability assessment, and an explicit reason a fixed version cannot be used. Delete ignores for unreachable packages.
- Dependency updates must be scoped. Do not run a broad lockfile update and then attribute unrelated churn to this workstream.
- Preserve deterministic Rustls/WebPKI ownership already established by the Eggfetch roadmap.
- M006 must not add a direct CodeGG Rustls dependency solely to force transitive resolution and must not add an ignore for RUSTSEC-2026-0285.
- Do not weaken SSRF/DNS-pinning, archive traversal, plugin sandbox, Landlock, authorization, or secret-redaction behavior while changing dependencies.

## 9. Verification policy

Focused milestones may use temporary dependency and size inspection commands as closure evidence:

```text
cargo tree -d --locked
cargo tree -e features --locked
cargo tree -i <package> --locked
cargo audit
cargo bloat --release --bin codegg --crates --locked -n 40
```

M006 additionally owns focused reverse-tree checks for `rustls`, `rustls-webpki` and root-store dependencies plus existing provider/streaming/static-routing/download tests. It must not create a new generic TLS test framework or public-network CI test merely for a patch-level dependency update.

Normal broad local posture remains:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
scripts/verify.sh quick
```

No new permanent scanner, advisory CI lane, dependency-count gate, size threshold, scheduled job, or release automation is authorized. Measurements belong in closure evidence.

## 10. Primary risks

1. Ratatui 0.30 compatibility risk was bounded and closed in M001.
2. SQLx feature names could have concealed required macro machinery; M001 proved the narrower surface by compile/test evidence.
3. Workspace dependency inheritance is additive for features; M002 avoided a maximal feature union.
4. Optional image slimming could have changed decoder support; M003 retained and tested the supported formats.
5. Publication work can create maintenance obligations; M004 qualified packages without forcing publication.
6. Generic updater design can become a cross-repository framework project; M005 remains blocked until a small external interface exists.
7. A targeted Rustls patch may require companion `rustls-webpki`/crypto lock changes or expose an exact upstream pin. M006 must keep changes causally attributable and stop if remediation requires a transport-stack migration, trust-store change, Eggfetch fork, or broad update.

## 11. Deferred work and explicit no-adoption decisions

- Upstreaming CodeGG's validated-destination/SSRF address policy into Eggfetch is deferred until Eggfetch or another independent consumer needs the same policy contract. Current CodeGG pinning is correct and should not be moved solely for aesthetics.
- Eggserve is not adopted for the current Axum/Tower server. CodeGG's server is application/middleware-heavy and feature-gated; replacing it would increase migration risk without demonstrated ownership reduction.
- Eggress is not adopted. It becomes relevant only if CodeGG gains a first-class egress mediation/routing requirement for tools, plugins, sandboxes, or remote execution.
- greggd is not adopted. Only generic updater mechanics are relevant; daemon/service activation remains out of scope.
- RustPython, Comrak, archive libraries, notification libraries, and release optimization flags are not reopened without new measured or security evidence.

## 12. Completion criteria

The dependency/security portion of this roadmap is current when:

- M001-M004 have accepted closure records;
- M006 has accepted closure evidence for the post-baseline Rustls advisory;
- current advisory review shows no unresolved critical/high or unexplained fixable medium dependency finding in the supported graph;
- every remaining audit ignore is reachable and justified;
- CodeGG-owned dependency duplicates are either converged or explicitly retained with evidence;
- workspace dependency/package ownership has one documented source of truth without feature over-unification;
- optional image support retains documented formats under a narrower graph;
- reusable extracted crates have explicit public/internal dispositions backed by package/tests rather than naming alone;
- broad local verification is green and no medium-or-higher M006 finding remains open.

M005 may remain blocked without preventing M006 closure; if so, the roadmap remains active only for that named external updater-interface dependency after M006 closes, and the registry/closure records must say so explicitly.
