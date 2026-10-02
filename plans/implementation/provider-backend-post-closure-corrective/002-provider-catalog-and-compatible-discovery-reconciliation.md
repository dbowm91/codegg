# Provider Backend Post-Closure Corrective C002 — Provider Catalog and Compatible-Discovery Reconciliation

Status: ready for handoff

Repository baseline: `62653a85b09ae10efe5e3c379cc68c96a62050f6`

Source corrective addendum:

- `plans/subsystems/provider-backend-post-closure-corrective-addendum.md#c002--provider-catalog-and-compatible-discovery-authority-reconciliation`

Long-term requirements:

- `plans/000-long-term-specification.md#13-provider-architecture-and-eggpool`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- None required if endpoint/setup authority remains in CodeGG and no shared crate is extracted.

Primary class: invariant / infrastructure

## 1. Objective

Re-establish one truthful CodeGG owner for built-in provider endpoint/setup facts and one bounded compatible `/models` fetch/parse core, while preserving different policy semantics for strict provisioning versus best-effort runtime discovery.

Correct the currently confirmed OpenCode Go endpoint drift and explicitly compare shared provider facts with EggPool's parallel corrective without treating either repository as external authority.

## 2. Why this milestone is ready

Concrete repository and first-party evidence exists:

- `setup_catalog.rs` says `OPENCODE_GO_BASE_URL = "https://opencode.ai/go/v1"`.
- current first-party OpenCode Go docs use `https://opencode.ai/zen/go/v1` and expose `/models` under that prefix;
- CodeGG Together already uses the current first-party `https://api.together.xyz/v1`;
- EggPool planning commit `58b1e24c8dce4fc099ac6cba17a0804e4f0cc8a5` registers a parallel template audit because EggPool's Together value is stale while its OpenCode Go value is correct;
- the strict bounded probe in `crates/codegg-providers/src/eggpool.rs` and the best-effort `OpenAiCompatibleProvider::models()` implementation duplicate compatible `/models` HTTP/parsing logic;
- the exported `Compatible*` aliases still instantiate `EggpoolProbe`, whose constructor applies Eggpool default-port normalization.

No new external library is required.

## 3. Current implementation evidence

### Setup/endpoint authority

`provider_setup_catalog()` is documented as the canonical pre-credential provider catalog, but base URLs also appear in:

- `additional.rs`;
- native provider config constructors;
- `opencode_zen.rs` and other wrappers;
- tests/documentation.

Some duplicates use catalog constants; others still embed literals.

### Compatible model discovery

Provisioning:

```text
SetupProbeStrategy::CompatibleProbe
 -> src/core/eggpool.rs::probe*
 -> codegg_providers::EggpoolProbe
 -> strict bounded /models
```

Runtime compatible provider:

```text
OpenAiCompatibleProvider::models()
 -> GET {base_url}/models
 -> permissive JSON parse
 -> append default-capability ModelInfo
 -> fall back to configured static models on error
```

These paths need different error policy, but not two parsers/limit definitions.

### Cross-repository facts

The current evidence proves a mixed state:

- CodeGG is correct for Together, EggPool is stale;
- EggPool is correct for OpenCode Go, CodeGG is stale.

Therefore C002 must use first-party provider documentation at implementation time.

## 4. Invariants that must not regress

- `setup_catalog.rs` remains CodeGG's single pre-credential provider-definition authority.
- Fixed endpoint definitions cannot be silently repointed by durable descriptors.
- Explicit user endpoints remain supported for optional/required policies.
- Eggpool proxy host shorthand/default port applies only to the Eggpool preset.
- Generic compatible endpoints never inherit port 11300 merely because they use the same probe implementation.
- Provisioning remains strict/fail-closed; runtime model discovery remains best-effort/non-destructive where it is today.
- Model discovery is bounded by response bytes, model count, and string length.
- Credentials/URLs with secrets are not logged.
- Provider IDs/storage keys stay stable.
- No runtime dependency on EggPool metadata/templates.
- Live provider access is not required in CI.

## 5. Scope

### In scope

- Audit every built-in CodeGG setup endpoint/protocol/discovery fact against current first-party documentation.
- Correct `OPENCODE_GO_BASE_URL` to the current `https://opencode.ai/zen/go/v1` prefix unless newer first-party evidence supersedes it.
- Record Together as confirmed-correct at `.xyz` under current evidence.
- Remove unnecessary duplicate endpoint literals by routing constructors through setup-catalog constants/configuration where ownership permits.
- Split generic compatible endpoint validation from Eggpool preset normalization.
- Introduce/reuse one bounded compatible-model fetch/parser primitive with explicit caller policy:
  - strict probe for provisioning/rotation;
  - best-effort merge/fallback for `Provider::models()`.
- Ensure the generic primitive supports cancellation/deadlines appropriate to each caller without owning application retry.
- Audit static fallback model lists and document which are authoritative fallbacks versus convenience seeds; correct clearly invalid/currently unsupported IDs only with first-party evidence.
- Add cross-repo comparison evidence against EggPool provider-profile metadata M001, labeled non-authoritative.

### Explicitly out of scope

- Shared provider-profile crate extraction.
- New providers.
- Broad pricing/cost metadata.
- EggPool daemon/runtime dependency.
- Provider routing/retry changes.
- Replacing live model discovery with static model lists.
- Automatic web freshness checks.

## 6. Required production changes

### Provider endpoint/source matrix

Build a bounded matrix for every built-in setup definition:

- provider ID;
- endpoint policy;
- canonical default/fixed API prefix;
- protocol/wire family;
- models endpoint behavior;
- auth kind/header;
- first-party source + review date;
- CodeGG disposition;
- EggPool comparison when the ID is shared.

Correct confirmed drift only.

### Generic compatible endpoint normalization

Refactor the current Eggpool-specific probe seam so:

```text
generic compatible validator
  accepts explicit http/https endpoint
  preserves supplied port/path
  never injects :11300
             ^
             |
Eggpool preset wrapper
  host shorthand + default :11300 + TLS policy
```

Compatibility aliases that claim to be provider-neutral must actually use the generic semantics.

Retain historical Eggpool names only as compatibility wrappers if needed.

### Shared bounded models core

Factor the common mechanics needed by strict and best-effort callers:

- GET models path;
- auth header provided by caller/config;
- redirects/body/model-count/string limits;
- JSON shape normalization for supported OpenAI-compatible list shape;
- deterministic duplicate-ID handling;
- typed/redacted failures;
- cancellation/deadline hooks.

Caller policy remains separate:

- provisioning: any unsupported/auth/unreachable/invalid response fails the operation;
- ordinary `Provider::models()`: failure returns/retains configured seeds exactly as today unless the provider contract says otherwise.

Do not convert best-effort discovery into hidden retry/failover.

### Static fallback review

Prefer live discovery when a provider supports it. Static model entries should be conservative seeds, not a manually maintained claim of exhaustive current availability.

Where current static entries are stale but still valid fallback IDs, document that instead of churn. Remove/update only when first-party evidence proves the entry invalid or the provider no longer serves it.

## 7. Ordered work packages

### Work package A — First-party provider fact audit

Acceptance evidence: complete built-in matrix; OpenCode Go corrected; Together confirmed; no unexplained endpoint duplicate.

### Work package B — Endpoint authority convergence

Acceptance evidence: constructors use catalog/config authority or have documented specialized reason; exact final URL tests for touched providers.

### Work package C — Generic compatible probe split

Acceptance evidence: generic endpoint with no port does not gain 11300; Eggpool preset shorthand still does; IPv4/IPv6/path/trailing slash/security-negative cases pass.

### Work package D — Bounded discovery-core convergence

Acceptance evidence: strict provisioning and best-effort provider discovery share parser/fetch limits but retain distinct failure outcomes; malformed/oversized/redirect/auth/cancel cases covered.

### Work package E — Cross-repo and documentation closure

Acceptance evidence: comparison with EggPool metadata M001; differences are confirmed/intentional/open rather than silently synchronized; architecture docs updated.

## 8. Failure, cancellation, restart, and contention semantics

Provisioning remains operation-cancellable and bounded by its workflow timeout. Runtime discovery remains finite and uses the existing non-streaming timeout.

No new background task, retry loop, or global cache is authorized.

Discovery failure never erases a usable static/live model catalog unless existing caller policy already does so.

## 9. Compatibility and migration

No database migration and no provider-ID change.

Changing a fixed built-in default endpoint affects future direct use of that definition; explicit durable custom endpoints continue following current endpoint-policy rules.

If a current stored fixed provider descriptor embeds a stale endpoint despite the fixed definition winning at construction, confirm the storage/projection behavior and document it; do not silently rewrite persisted data without a separate migration requirement.

## 10. Required tests

- full setup-catalog coverage;
- OpenCode Go exact chat/models path and session-affinity behavior;
- Together endpoint confirmation fixture;
- generic compatible normalization with/without scheme/port/path;
- Eggpool preset default port semantics;
- strict probe limits/errors/cancellation;
- best-effort `OpenAiCompatibleProvider::models()` fallback behavior;
- duplicate model IDs, oversized response/count/string, invalid JSON, redirect/auth errors;
- fixed endpoint cannot be overridden;
- provider transcript/wire regression for touched wrappers.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test -p codegg-providers
cargo test --test provider_transcripts
cargo clippy -p codegg-providers --all-targets -- -D warnings
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Also run provider connection/provisioning integration targets and `scripts/verify.sh quick` according to current repository guidance.

## 12. Documentation updates

- `architecture/provider.md`
- provider connection/provisioning docs as needed;
- corrective addendum/registry;
- closure evidence matrix.

## 13. Acceptance criteria

- OpenCode Go uses the current first-party `/zen/go/v1` API prefix.
- CodeGG Together remains on current first-party `.xyz`.
- every built-in endpoint/setup fact has a first-party-source disposition;
- generic compatible endpoints do not inherit Eggpool port normalization;
- provisioning and ordinary model discovery share one bounded compatible parser/fetch core while preserving strict vs best-effort policy;
- no new retry/catalog authority is introduced;
- provider/full verification passes with no unresolved medium-or-higher finding.

## 14. Stop conditions

Stop if:

- first-party provider evidence is contradictory/unavailable for a proposed correction;
- a change requires provider-ID/storage-schema migration;
- discovery convergence would require provider-specific wire behavior inside a generic parser;
- preserving supported compatibility requires a new public config field;
- the work expands into shared provider-profile crate extraction.

## 15. Closure evidence required

Record implementation commits, provider fact matrix, official source/review date for changed facts, OpenCode Go/Together exact disposition, generic-vs-Eggpool normalization tests, strict/best-effort discovery tests, cross-repo comparison with EggPool corrective, full verification, and residual findings.

## 16. Handoff notes

External provider facts are time-sensitive. Re-check first-party docs at implementation time. The plan deliberately avoids treating EggPool as a source of truth even though the sibling audit is useful comparison evidence.
