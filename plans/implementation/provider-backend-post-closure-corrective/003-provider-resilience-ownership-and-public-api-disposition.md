# Provider Backend Post-Closure Corrective C003 — Provider Resilience Ownership and Public-API Disposition

Status: ready for handoff

Repository baseline: `62653a85b09ae10efe5e3c379cc68c96a62050f6`

Source corrective addendum:

- `plans/subsystems/provider-backend-post-closure-corrective-addendum.md#c003--provider-resilience-ownership-and-public-api-disposition`

Long-term requirements:

- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- None required if production retry ownership remains unchanged. If implementation proposes provider switching as a new production routing policy, stop and register an ADR/roadmap instead.

Primary class: polish / invariant

## 1. Objective

Make CodeGG's provider resilience ownership truthful and single-owner.

Prove whether `FallbackProvider` and provider-level `CircuitBreaker` have any production or supported external API consumers, then choose the narrowest safe disposition:

1. retain as clearly library-only compatibility primitives with static guards preventing production use;
2. deprecate with migration guidance and remove misleading architecture claims; or
3. remove only if package/public-API evidence establishes that the break is safe.

Do not change production turn retry/failover behavior.

## 2. Why this milestone is ready

At the baseline:

- repository search finds `FallbackProvider::new` only inside `fallback.rs` tests;
- no production provider registry/session path constructs `FallbackProvider`;
- `src/agent/provider_turn.rs` owns actual provider-turn retry behavior and preserves the selected provider/session;
- `FallbackProvider` contains a second provider-switching/backoff/circuit policy;
- `circuit.rs` is public, `CircuitBreaker` is re-exported from `codegg-providers`, and `codegg-core::resilience` re-exports it again;
- provider/resilience docs describe fallback/circuit as active provider fault-tolerance architecture;
- historical comments in `provider_turn.rs` still point to `FallbackProvider` as an owner.

This is sufficient to plan an API/ownership corrective without changing behavior.

## 3. Current implementation evidence

Production retry owner:

```text
session-selected Provider
 -> src/agent/provider_turn.rs
 -> ProviderError::retry_disposition
 -> caller RetryContext / unified retry budget
 -> same selected provider/session
```

Dormant alternate owner:

```text
FallbackProvider
 -> ordered providers
 -> per-provider CircuitBreaker
 -> retryable status/taxonomy
 -> backoff
 -> switch provider
```

If both were wired together, retry/failover counts and provider identity could become non-deterministic. The current absence of production construction avoids that bug, but docs/API surface do not make the boundary clear.

## 4. Invariants that must not regress

- production provider-turn retry remains in the existing agent/runtime chain;
- selected durable provider connection/session is not silently replaced by a fallback wrapper;
- retry budgets and uncertain-side-effect behavior remain unchanged;
- no new provider/account routing authority is introduced;
- no provider retry is nested behind EggPool proxy upstream retries as part of this work;
- supported public Rust API is not broken without explicit compatibility evidence;
- historical tests/closure records remain unchanged.

## 5. Scope

### In scope

- repository-wide internal call-site/reference audit;
- package/publication/API compatibility audit for `codegg-providers::fallback`, `codegg_providers::circuit`, root `CircuitBreaker` re-export, and `codegg_core::resilience`;
- decide retain/deprecate/remove for each surface;
- correct `provider_turn.rs` comments and active architecture docs;
- add a static guard proving production provider/session code does not instantiate the dormant fallback owner unless a future explicit plan changes the architecture;
- if retained, label the primitive as library/test/compatibility surface and keep its tests;
- if deprecated, use normal Rust deprecation with migration wording rather than immediate silent disappearance.

### Explicitly out of scope

- redesigning retry taxonomy;
- provider fallback routing;
- health/quarantine/account routing;
- EggPool integration changes;
- deleting an externally supported API without evidence;
- changing circuit-breaker implementation semantics merely for cleanup.

## 6. Required production changes

### Compatibility audit

Determine:

- whether `codegg-providers`/`codegg-core` are published or otherwise promise external Rust API compatibility;
- whether examples/tests/other workspace crates consume these exports;
- whether removal changes documented public API used by downstreams.

Record evidence, not assumptions.

### Ownership disposition

Preferred outcomes in order:

- if compatibility matters: retain/deprecate but remove claims that these primitives are production retry owners;
- if compatibility does not matter and no internal consumer exists: remove the dormant fallback module and unused re-exports, but only after a repository/API guard proves no consumer;
- retain a standalone circuit breaker only if it has an explicit non-provider consumer or supported library purpose.

Regardless of disposition, production provider retry docs must point to `provider_turn.rs`/retry taxonomy as the authority.

### Static guard

Add a narrow source/architecture guard that fails if production provider/session paths instantiate `FallbackProvider` without an explicit planning change. Do not ban the symbol globally if it is retained for tests/public compatibility.

## 7. Ordered work packages

### Work package A — API and call-graph audit

Acceptance evidence: complete internal/export/downstream disposition table.

### Work package B — Code surface disposition

Acceptance evidence: retain/deprecate/remove changes match the table; no production retry behavior diff.

### Work package C — Documentation/guard convergence

Acceptance evidence: `architecture/provider.md`, `architecture/resilience.md`, `provider_turn.rs`, and native-crate docs describe one production owner; static guard passes.

### Work package D — Reliability regression closure

Acceptance evidence: provider retry/reliability/uncertain-side-effect suites pass; dormant primitive tests pass if retained.

## 8. Failure, cancellation, restart, and contention semantics

Unchanged. The corrective must not alter admission/retry/circuit state in the production turn path.

## 9. Compatibility and migration

If deprecating an exported API, document the replacement or explain that there is intentionally no direct replacement because production failover is session/router-owned.

If removing an API, closure must include evidence that no compatibility promise/downstream package requires it.

## 10. Required tests

- production provider retry taxonomy and retry budget;
- mid-stream/visible-output no-retry behavior;
- uncertain-side-effect behavior;
- selected-provider/session identity stability;
- static guard against dormant fallback production construction;
- circuit/fallback unit tests if retained;
- compile/doc tests for deprecated/public exports if retained.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test -p codegg-providers
cargo test -p codegg-core
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Run focused reliability qualification targets and `scripts/verify.sh quick` required by current docs.

## 12. Documentation updates

- `architecture/provider.md`
- `architecture/resilience.md`
- `architecture/core.md` / `native_crates.md` if they expose the stale ownership statement;
- `src/agent/provider_turn.rs` comments;
- corrective registry/closure.

## 13. Acceptance criteria

- repository has one truthful production provider retry/failover owner;
- no active doc claims `FallbackProvider` participates in production unless evidence proves it does;
- public fallback/circuit API receives an explicit retain/deprecate/remove disposition backed by compatibility evidence;
- static guard prevents accidental second production failover owner;
- production provider retry behavior and tests are unchanged.

## 14. Stop conditions

Stop if:

- a real production `FallbackProvider` consumer is discovered;
- an external compatibility commitment makes removal unsafe and deprecation cannot preserve behavior;
- implementation would require changing retry budgets/provider-selection policy;
- the work exposes an actual retry correctness defect requiring a separate runtime corrective.

## 15. Closure evidence required

Record internal call graph/search results, package/public API evidence, exact disposition of each export/module, docs/guard changes, focused reliability/full verification, and residual findings.

## 16. Handoff notes

This corrective should prefer truthful ownership over code deletion. Keeping a small compatibility primitive is acceptable if the runtime cannot accidentally acquire a second retry/failover authority.
