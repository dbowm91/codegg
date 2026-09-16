# Dependency Security and Workspace Consolidation M006 — Rustls Advisory Remediation and Planning Reconciliation

Status: implemented

Repository baseline reviewed: `24cf2327bdd0ae6d54dc8be754dde670bfd070aa`

Source subsystem roadmap:

- `plans/subsystems/dependency-security-workspace-consolidation-roadmap.md`

Related closure evidence:

- `plans/closure/dependency-security-workspace-consolidation/001-status.md`
- `plans/closure/dependency-security-workspace-consolidation/002-status.md`
- `plans/closure/dependency-security-workspace-consolidation/003-status.md`
- `plans/closure/dependency-security-workspace-consolidation/004-status.md`
- `plans/closure/dependency-security-workspace-consolidation/005-status.md`

Predecessors: none. M001-M004 are already closed and provide the converged dependency baseline. M005 is independently blocked on its external updater interface and does not block M006.

Primary class: infrastructure + invariant.

## 1. Objective

Remediate the currently reachable RUSTSEC-2026-0285 Rustls vulnerability with the smallest compatible dependency/lockfile change, prove that CodeGG's Eggfetch/provider/streaming/TLS ownership remains unchanged, and reconcile the active planning registry so it no longer reports stale M001-ready state.

At plan creation, the current closure evidence records `rustls 0.23.41`; RUSTSEC-2026-0285 is fixed in `rustls >=0.23.45`. This is a bounded post-baseline advisory follow-up, not a reopening of M001-M004 and not authorization for general dependency modernization.

## 2. Why this is separate follow-up work

M001 correctly scoped itself to the dependency families known at implementation start. During its closure pass, a newly published RustSec advisory appeared for the already-resolved Rustls line. M001-M003 therefore recorded the finding explicitly as separate bounded maintenance rather than silently widening their scope.

That evidence is now mature enough for an independent handoff:

- the vulnerable line and patched floor are known;
- CodeGG's HTTP ownership is already consolidated on `eggfetch-core 0.1.4`;
- the workspace dependency baseline is stable after M002;
- no application feature redesign is required to address the advisory;
- the current registry contains one stale control-point row that still says `M001 ready`, despite M001-M004 being closed.

## 3. Security context and current evidence

RUSTSEC-2026-0285 concerns TLS 1.3 handshake messages accepted across encryption-level boundaries. The advisory is medium severity and identifies `rustls >=0.23.45` as patched. The handshake transcript remains authenticated, so the issue is bounded, but it is still a known fixable vulnerability in CodeGG's supported TLS stack and should not remain open without a technical blocker.

Current repository evidence from M001-M003 closure records:

- supported builds resolved `rustls 0.23.41` when the advisory was recorded;
- CodeGG uses Eggfetch with explicit Rustls/WebPKI ownership rather than native roots;
- provider streaming, redirects, timeout behavior, Eggpool, MCP and other HTTP consumers were already regression-tested under the Eggfetch migration;
- no ignore exists for RUSTSEC-2026-0285, and none should be added merely to close this milestone;
- `.cargo/audit.toml` contains only the separately justified RSA/SQLx lock-union exception.

Implementation MUST reproduce the current graph and advisory state before editing because another commit or upstream lock refresh may already have changed the resolved version.

## 4. Invariants

- CodeGG remains on the existing Eggfetch HTTP ownership model; do not replace or wrap Eggfetch.
- Preserve the approved Eggfetch feature profile: HTTP/1 + Rustls + JSON where owned today.
- Preserve deterministic packaged WebPKI roots. Do not add `tls-native-roots` or switch trust stores.
- Preserve provider request/stream timeout, redirect, idle-pool, SSE, cancellation and error-redaction semantics.
- Preserve WebFetch/research/MCP validated-destination and no-second-DNS behavior.
- Preserve EggLSP download redirect/body/archive-security behavior.
- Preserve Rust 1.89 MSRV.
- Do not add a direct root `rustls` dependency solely to force Cargo resolution.
- Do not add a RustSec ignore for RUSTSEC-2026-0285.
- Lockfile churn must be attributable to the Rustls patch and unavoidable semver-compatible companion packages.
- No public-network-dependent test becomes part of normal CI.

## 5. Scope and non-goals

In scope:

- fresh reverse-dependency and feature census for Rustls and its active crypto/certificate companions;
- targeted update from the vulnerable Rustls line to a patched `0.23.x` release at or above `0.23.45`;
- the minimum owning dependency change only if a transitive exact constraint prevents a lock-only Rustls update;
- focused CodeGG/Eggfetch-consumer regression verification;
- `cargo audit` reconciliation;
- active dependency-maintenance/planning documentation if version-specific text becomes stale;
- correcting the stale dependency-security control-point row in `plans/registry.md` as part of closure.

Out of scope:

- broad `cargo update`;
- moving to a new Rustls major/minor family;
- Eggfetch API redesign or an Eggfetch Git/path override;
- native trust roots, HTTP/2, HTTP/3, proxies, cookies or other transport feature expansion;
- provider protocol/auth/tool-call/model-catalog changes;
- rewriting TLS code locally;
- changing server/plugin/sandbox/security architecture;
- changing or removing the separately justified RSA audit exception unless fresh reachability evidence independently invalidates it;
- M005 updater-interface work;
- new advisory CI lanes, dependency bots or release automation.

## 6. Ordered work packages

### WP1 — Reproduce the advisory and owner graph

Before editing, run the equivalent of:

```bash
cargo tree -i rustls@0.23.41 --locked
cargo tree -i rustls --locked
cargo tree -e features -i rustls --locked
cargo tree -i rustls-webpki --locked
cargo tree -i webpki-roots --locked
cargo audit
```

Repeat the relevant reverse-tree/feature-tree checks with `--all-features` if the default graph does not expose every supported Rustls owner.

Record:

- every active owner path into Rustls;
- whether more than one Rustls version is resolved;
- the enabled crypto provider/features;
- the current `rustls-webpki`/root-store relationship;
- the exact current RustSec fixed floor.

If the vulnerable version is already absent and `cargo audit` confirms RUSTSEC-2026-0285 is gone, do not churn the lockfile. Skip to WP4, reconcile planning/docs, and close with evidence.

### WP2 — Perform the smallest patched update

Prefer a lock-only targeted update when the current semver constraints allow it. The expected first attempt is equivalent to:

```bash
cargo update -p rustls@0.23.41 --precise 0.23.45
```

Use the actual currently resolved vulnerable version in the selector. A newer compatible `0.23.x` patch is acceptable if Cargo's existing constraints select it and it does not introduce unrelated churn.

Rules:

1. Do not run unscoped `cargo update`.
2. Do not add `rustls` to a CodeGG manifest if CodeGG does not already own it directly.
3. If the targeted update requires companion patch updates such as `rustls-webpki` because Rustls itself requires them, keep those changes in scope and document the exact causal edge.
4. If an exact transitive constraint prevents a patched Rustls line, identify the owning package with `cargo tree -i` and update only that smallest owner if the change is semver-compatible and behavior-preserving.
5. If reaching a patched Rustls requires a new Eggfetch release, source fork, Git override, transport feature change, or Rustls family migration, stop and record a blocker rather than forcing the solution into CodeGG.
6. Do not change trust-store selection or crypto provider merely because another option is available.

After resolution, inspect `git diff Cargo.lock` and reject unrelated package churn before proceeding.

### WP3 — Focused HTTP/TLS consumer regression verification

Re-run the existing focused suites that exercise the transport and protocol behavior built on Eggfetch. Use the actual test names present at implementation time; expected coverage includes:

- `codegg-providers` provider client construction, redirects, model discovery, SSE/streaming and cancellation;
- Eggpool bounded request/body/error behavior;
- remote MCP/WebFetch/research static-routing or validated-address behavior;
- update-check Eggfetch client behavior;
- EggLSP download redirect/body handling and archive security;
- any existing deterministic TLS fixture if one is already present.

Representative commands may include:

```bash
cargo test -p codegg-providers --lib --locked -- --test-threads=1
cargo test --lib provider_core --locked -- --test-threads=1
cargo test --test upgrade --locked -- --test-threads=1
cargo test -p egglsp --all-features --locked -- --test-threads=1
```

Do not invent a generic TLS-test framework for this patch. If no existing local HTTPS fixture exists, compile every supported Eggfetch consumer and rely on the existing deterministic HTTP/streaming/static-routing suites plus the version/advisory evidence. A one-shot local HTTPS smoke may be recorded if it can be done without new production dependencies or public network access.

### WP4 — Advisory and dependency reconciliation

Run:

```bash
cargo tree -i rustls --locked
cargo tree -e features -i rustls --locked
cargo audit
```

Required disposition:

- RUSTSEC-2026-0285 is absent from supported resolutions and audit output;
- resolved Rustls is `>=0.23.45` and remains on the intended 0.23 line;
- no new critical/high/medium advisory was introduced by companion lock changes;
- no new audit ignore was added;
- the existing RSA ignore remains unchanged unless fresh evidence proves its documented reachability rationale is no longer true.

If `cargo audit` reports a newly published unrelated advisory during implementation, record and classify it. Fix it only when the same lock changes introduced it or when it is a direct blocker to claiming M006's security result; otherwise create/identify separate bounded work rather than widening M006 indefinitely.

### WP5 — Broad verification and planning reconciliation

Run the normal broad local posture:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked -- --test-threads=1
scripts/verify.sh quick
```

Then update active planning state:

- M006 plan `ready` -> `implemented` while implementation is complete;
- create `plans/closure/dependency-security-workspace-consolidation/006-status.md`;
- mark M006 closed only after closure evidence is accepted;
- correct the stale registry control-point row that currently says `M001 ready`;
- leave M005 blocked on the external generalized updater interface;
- do not rewrite M001-M003 historical closure records that accurately captured the advisory as post-baseline separate work.

## 7. Compatibility, failure and recovery semantics

This should be a dependency patch, not a behavioral migration.

- Existing TLS validation failures must remain failures; do not weaken certificate/hostname validation to preserve a test.
- Existing HTTP cancellation and timeout semantics remain owner-controlled above Rustls.
- Provider/MCP/EggLSP errors remain sanitized and transport-neutral as already documented.
- No data/config/protocol migration is required.
- Rollback is a lockfile/owning-dependency revert if the patch causes an unexpected compatibility regression.
- If a patch-level Rustls update changes an observable application contract, classify that behavior explicitly and stop if it cannot be fixed narrowly.

## 8. Required tests and evidence

The closure record must contain at least:

- before/after Rustls reverse dependency trees for default and relevant all-feature resolutions;
- before/after Rustls version and companion-package lockfile diff;
- `cargo audit` output proving RUSTSEC-2026-0285 is gone without a new ignore;
- focused provider/streaming/static-routing/download/update regression results;
- broad workspace Clippy/test/quick verification results;
- confirmation that WebPKI/root-store and Eggfetch feature ownership did not change;
- confirmation that no direct CodeGG `rustls` dependency was added solely for resolution control;
- any unresolved findings classified by severity;
- recommendation: closed, conditionally closed, corrective pass required, or blocked.

Hosted CI is useful if already triggered by the normal workflow, but this milestone does not require a new CI lane or network-dependent hosted smoke.

## 9. Acceptance criteria

- Supported CodeGG builds do not resolve a Rustls version affected by RUSTSEC-2026-0285.
- Rustls resolves to `>=0.23.45` on the existing 0.23 family, unless the advisory has been superseded by newer authoritative guidance recorded in closure.
- The remediation is lock-only where current constraints permit; otherwise every manifest/package change is the minimum required owner change and is justified.
- Eggfetch remains the HTTP transport owner with the same approved feature/trust profile.
- Provider streaming, Eggpool, MCP/static routing, update checks and EggLSP download behavior remain compatible.
- No public network is required for the committed test suite.
- `cargo audit` contains no RUSTSEC-2026-0285 finding and no new ignore for it.
- Broad local verification passes.
- `plans/registry.md` accurately reports M001-M004 closed, M005 blocked, and M006's final disposition.
- No unresolved medium-or-higher M006 finding remains open.

## 10. Stop conditions

Stop and report a blocker if:

- the current RustSec patched floor differs materially from this plan's `>=0.23.45` snapshot;
- current `main` already uses a patched Rustls but a different medium-or-higher TLS advisory supersedes this work;
- a patched Rustls cannot resolve without changing Eggfetch source/API or adopting a Git/path dependency;
- the update requires changing trust roots, crypto provider, HTTP protocol features, or Rustls family;
- the lockfile update introduces broad unrelated churn that cannot be separated;
- focused provider/streaming/static-routing/download tests expose a nontrivial behavior regression;
- a new security finding makes a simple Rustls patch insufficient for a truthful closure.

## 11. Non-regression note for M005

M005 remains independently blocked. M006 must not use the Rustls advisory as a reason to reintroduce automatic self-update, shell execution, external `curl`, greggd, or Gregg-specific updater dependencies. The check-only/fail-closed upgrade posture remains the current security boundary until the external updater interface exists.
