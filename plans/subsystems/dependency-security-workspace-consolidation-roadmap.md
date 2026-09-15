# Dependency Security and Workspace Consolidation Roadmap

Status: active; M001 closed, M002 ready, M003-M004 predecessor-gated, M005 externally blocked

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
- defining the CodeGG side of a future generic binary-updater contract rather than duplicating security-sensitive updater mechanics.

Binary size is a useful secondary metric. Maintenance ownership, security posture, explainable feature activation, and avoiding duplicate implementations are the primary objectives.

## 2. Classification

Primary classification: **infrastructure**.

M001 is also **invariant** work because memory-safety advisories and audit exceptions must not be hidden by lockfile churn. M004 is **polish/infrastructure** because it qualifies existing boundaries rather than adding product capability. M005 is **infrastructure** with a cross-repository interface dependency.

## 3. Current-state evidence

At the reviewed baseline:

1. The root package uses `ratatui = "0.29"`, while the optional image stack already resolves Ratatui 0.30-era crates. `Cargo.lock` contains `lru 0.12.5` and `lru 0.18.0`; both are below current RustSec fixed floors for 2026 unsoundness advisories. The earlier footprint work explicitly deferred broad Ratatui migration, so this is new evidence rather than retroactive rewriting of closure.
2. CodeGG root/core/providers use DashMap 5 while `eggfetch-core 0.1.4` uses DashMap 6. The HTTP-consolidation closure recorded this split as intentionally out of scope.
3. Root/core SQLx declarations enable `macros` and `migrate`; production source heavily uses runtime queries and `sqlx::FromRow`, while the active schema migrator is handwritten. `Cargo.lock` still resolves `sqlx-mysql` and `rsa 0.9.10`, and `.cargo/audit.toml` suppresses RUSTSEC-2023-0071 because that RSA path is described as unused SQLx/MySQL closure.
4. The workspace repeats common package metadata and dependency versions/features across member manifests rather than using a substantial `[workspace.package]` / `[workspace.dependencies]` baseline.
5. The optional root `image` feature enables explicit formats on `image` but leaves image defaults active and enables `ratatui-image`'s `image-defaults`, expanding the optional graph beyond PNG/JPEG/GIF/WebP.
6. Existing extracted crates already include useful ownership seams: `egggit`, `eggsentry`, `codegg-protocol`, and `eggcontext`. New microcrates are not justified merely to reduce root source size.
7. CodeGG's updater still executes a downloaded installer script through external `curl`, while the Gregg workspace contains reusable binary-first update mechanics with checksum/candidate validation and executable replacement. That implementation is Gregg-specific today and therefore is not a safe direct dependency for CodeGG.

## 4. Invariants and non-goals

The workstream MUST preserve:

- CodeGG's Rust 1.89 MSRV unless a separately approved plan changes it;
- `#![deny(unsafe_code)]` boundaries and narrowly documented existing exceptions;
- supported TUI behavior and key/input/rendering semantics;
- SQLite storage/schema behavior and handwritten migration ordering;
- Eggfetch's explicit `http1,tls-rustls,json` ownership and security-sensitive validated-destination behavior;
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
- publishing crates or releases automatically as part of this workstream.

## 5. Target architecture

After M001-M004 close:

- the default and optional dependency graph contains no known avoidable duplicate DashMap major owned by CodeGG;
- Ratatui/LRU are on a sound supported line with no stale `lru` advisory path retained solely by CodeGG's direct TUI dependency;
- SQLx enables only source-demonstrated features; if `sqlx-mysql`/`rsa` become unreachable, the stale RSA audit ignore is removed;
- common workspace dependency versions and package policy have one authoritative manifest location, while crate-specific features remain local and minimal;
- the optional image graph is restricted to the formats CodeGG advertises;
- reusable extracted crates have explicit publishability/internal dispositions and package boundaries that do not depend on the root application;
- CodeGG-specific crates remain internal when no independent consumer justifies a stable public contract.

M005 is a later interface-convergence milestone. Its target is a generic updater crate owned outside CodeGG that accepts application/repository/asset policy and can use an injected/native Rust transport. CodeGG must not copy Gregg's updater implementation into its root crate while waiting for that interface.

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
  hard: M002
  interface: independently published/generalized updater contract
```

M003 and M004 are ordered after M002 to avoid repeated manifest churn. They may be implemented independently once M002 closes.

## 7. Milestones

### M001 — Security and duplicate dependency-graph convergence

Plan: `plans/implementation/dependency-security-workspace-consolidation/001-security-and-duplicate-graph-convergence.md`

Status: **closed**. Closure: `plans/closure/dependency-security-workspace-consolidation/001-status.md` (implementation `3bd54ccd`).

Goals:

- resolve current `lru` unsoundness exposure by moving the direct TUI dependency train to a fixed supported Ratatui/LRU line;
- converge CodeGG-owned DashMap usage on the Eggfetch-compatible major where source compatibility permits;
- narrow SQLx feature ownership using source census and remove unreachable MySQL/RSA closure if possible;
- reconcile `.cargo/audit.toml` with the resulting graph;
- record fresh `cargo tree -d`, advisory, and release-size evidence without making them permanent gates.

### M002 — Workspace dependency ownership normalization

Plan: `plans/implementation/dependency-security-workspace-consolidation/002-workspace-dependency-ownership-normalization.md`

Status: **ready** (unblocked by M001 closure `plans/closure/dependency-security-workspace-consolidation/001-status.md`; build on the converged Ratatui 0.30 / crossterm 0.29 / DashMap 6 / SQLx derive-only baseline).

Goals:

- establish `[workspace.package]`, `[workspace.dependencies]`, and bounded workspace lint inheritance where it removes duplicated ownership;
- centralize versions/default-feature policy without creating a workspace-wide feature superset;
- centralize internal path/version declarations where member release semantics are shared;
- preserve intentional package-specific metadata and feature differences.

### M003 — Optional image feature-graph slimming

Plan: `plans/implementation/dependency-security-workspace-consolidation/003-optional-image-feature-graph-slimming.md`

Status: **blocked on M002 accepted closure**.

Goals:

- disable `image` default features;
- remove `ratatui-image`'s `image-defaults` expansion unless a real supported behavior requires it;
- retain PNG/JPEG/GIF/WebP behavior;
- measure optional-image graph/artifact contraction and preserve all image tests.

### M004 — Reusable crate boundary qualification

Plan: `plans/implementation/dependency-security-workspace-consolidation/004-reusable-crate-boundary-qualification.md`

Status: **blocked on M002 accepted closure**.

Goals:

- qualify `egggit`, `eggsentry`, `codegg-protocol`, and `eggcontext` against independent-package criteria;
- refactor `eggcontext` model-name policy away from low-level tokenizer primitives if needed for a stable reusable contract;
- improve package metadata/docs/tests only where required for standalone consumption;
- explicitly retain `codegg-git`, `codegg-config`, and `codegg-providers` as CodeGG-owned unless evidence demonstrates a second consumer and stable generic boundary;
- perform package dry-runs where appropriate but do not publish automatically.

### M005 — Generic updater interface and CodeGG adoption

Plan: `plans/implementation/dependency-security-workspace-consolidation/005-generic-updater-interface-and-codegg-adoption.md`

Status: **blocked**.

Hard dependency: M002 accepted closure.

Interface dependency: a generalized updater package must exist outside CodeGG with a written contract that is not Gregg-specific and does not require greggd/service-manager ownership. The package should own candidate acquisition/verification/staging/replacement mechanics while allowing CodeGG to retain its own CLI UX and use a native Rust HTTP transport such as Eggfetch.

This roadmap intentionally does not authorize creating that external package inside CodeGG or copying Gregg's implementation here.

## 8. Cross-cutting security requirements

- Run a fresh advisory check against the candidate lockfile; do not assume the advisory set from roadmap creation remains current.
- Every ignored RustSec advisory must have a currently reachable package path, a documented applicability assessment, and an explicit reason a fixed version cannot be used. Delete ignores for unreachable packages.
- Dependency updates must be scoped. Do not run a broad lockfile update and then attribute unrelated churn to this workstream.
- Preserve deterministic Rustls/WebPKI ownership already established by the Eggfetch roadmap.
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

Normal broad local posture remains:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
scripts/verify.sh quick
```

No new permanent scanner, advisory CI lane, dependency-count gate, size threshold, scheduled job, or release automation is authorized. Measurements belong in closure evidence.

## 10. Primary risks

1. Ratatui 0.30 may require TUI API or Crossterm compatibility changes. M001 must keep the migration bounded to behavior-preserving compatibility work.
2. SQLx feature names can appear unused while derive/macro internals remain required. M001 must prove feature removal by compile/test evidence rather than manifest inspection alone.
3. Workspace dependency inheritance is additive for features. A careless central baseline can accidentally widen every member; M002 must centralize versions/default policy, not maximal features.
4. Optional image slimming can change decoder support. M003 must retain exactly the formats CodeGG documents and tests.
5. Publication work can create maintenance obligations. M004 qualifies packages but does not force publication.
6. Generic updater design can become a cross-repository framework project. M005 must remain blocked until a small existing interface is available; it must not invent service lifecycle or release automation inside CodeGG.

## 11. Deferred work and explicit no-adoption decisions

- Upstreaming CodeGG's validated-destination/SSRF address policy into Eggfetch is deferred until Eggfetch or another independent consumer needs the same policy contract. Current CodeGG pinning is correct and should not be moved solely for aesthetics.
- Eggserve is not adopted for the current Axum/Tower server. CodeGG's server is application/middleware-heavy and feature-gated; replacing it would increase migration risk without demonstrated ownership reduction.
- Eggress is not adopted. It becomes relevant only if CodeGG gains a first-class egress mediation/routing requirement for tools, plugins, sandboxes, or remote execution.
- greggd is not adopted. Only generic updater mechanics are relevant; daemon/service activation remains out of scope.
- RustPython, Comrak, archive libraries, notification libraries, and release optimization flags are not reopened without new measured or security evidence.

## 12. Completion criteria

The roadmap is complete when:

- M001-M004 have accepted closure records;
- current advisory review shows no unresolved critical/high or unexplained memory-safety dependency finding in the supported graph;
- every remaining audit ignore is reachable and justified;
- CodeGG-owned dependency duplicates are either converged or explicitly retained with evidence;
- workspace dependency/package ownership has one documented source of truth without feature over-unification;
- optional image support retains documented formats under a narrower graph;
- reusable extracted crates have explicit public/internal dispositions backed by package/tests rather than naming alone;
- broad local verification is green and no medium-or-higher workstream finding remains open.

M005 may remain blocked without preventing M001-M004 closure; if so, the roadmap remains active only for that named external interface dependency and the closure record must say so explicitly.
