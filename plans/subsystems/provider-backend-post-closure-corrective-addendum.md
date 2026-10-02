# Provider Backend — Post-Closure Corrective Addendum

Status: active

Repository baseline reviewed: `62653a85b09ae10efe5e3c379cc68c96a62050f6`

Predecessor work:

- `plans/subsystems/provider-wire-kernel-consolidation-roadmap.md`
- `plans/closure/provider-wire-kernel-consolidation/001-status.md`
- `plans/closure/provider-wire-kernel-consolidation/002-status.md`
- `plans/closure/provider-wire-kernel-consolidation/003-status.md`
- `plans/subsystems/provider-connections-roadmap.md`
- `plans/closure/provider-connections/008-status.md`

Cross-repository coordination:

- EggPool provider-profile metadata corrective planning commit `58b1e24c8dce4fc099ac6cba17a0804e4f0cc8a5`.
- EggPool remains authoritative only for its own runtime/templates. First-party provider documentation is authoritative for externally mutable endpoint/protocol facts.

Canonical references:

- `plans/000-long-term-specification.md#13-provider-architecture-and-eggpool`
- `plans/001-terminology-and-domain-model.md`
- `plans/003-planning-process.md#7-corrective-passes`
- `architecture/provider.md`
- `architecture/resilience.md`

No new ADR is required for C001–C003. The accepted ownership boundary remains: CodeGG owns direct-provider connection/auth/HTTP/retry/model-policy behavior; `eggpool-wire` owns shared protocol encoding/stream decoding; EggPool daemon/runtime is optional.

## 1. Why this corrective exists

The provider wire-kernel consolidation is accepted and remains historically closed. Post-closure review found three separate issues above that wire boundary.

### Finding A — native OpenAI endpoint composition duplicates the version prefix

`OpenAiConfig::default()` currently sets:

```text
https://api.openai.com/v1
```

while `OpenAiProvider::stream()` constructs:

```text
{base_url}/v1/chat/completions
```

The normal env/config registration paths create `OpenAiConfig::default_with_key`, so the default direct OpenAI request URL becomes:

```text
https://api.openai.com/v1/v1/chat/completions
```

This predates the `eggpool-wire` cutover and was not introduced by M001–M003, but it is a direct-provider correctness defect. Existing provider tests qualify body grammar more thoroughly than final URL composition and therefore did not catch it.

### Finding B — provider endpoint/discovery metadata has multiple local owners and confirmed drift

CodeGG's setup catalog is intended to be the pre-credential provider-definition authority, but endpoint/model-discovery facts remain repeated across provider constructors and discovery implementations.

Concrete current evidence:

- `setup_catalog.rs` defines `OPENCODE_GO_BASE_URL = "https://opencode.ai/go/v1"`.
- current first-party OpenCode Go documentation uses `https://opencode.ai/zen/go/v1` for Chat Completions, Responses, Anthropic Messages, and `/models`.
- CodeGG's Together endpoint `https://api.together.xyz/v1` matches current first-party Together examples; EggPool currently has the inverse drift and has a separate corrective registered.
- `crates/codegg-providers/src/eggpool.rs` exposes provider-neutral `Compatible*` aliases, but the underlying constructor still applies Eggpool-specific default-port normalization.
- `OpenAiCompatibleProvider::models()` independently performs a permissive `/models` fetch/parse while provisioning uses the strict bounded probe. The two paths intentionally differ in failure semantics but unnecessarily duplicate transport/parsing/model-normalization rules.
- several provider modules still carry static model fallbacks whose freshness is independent of setup-catalog/live-discovery authority.

This is both correctness and maintainability debt. It should be fixed without making EggPool's templates the authority for CodeGG.

### Finding C — fallback/circuit resilience primitives are documented as production architecture but have no production owner

`FallbackProvider` constructs per-provider `CircuitBreaker` instances and implements provider switching/retry behavior. Current repository search finds `FallbackProvider::new` only in `fallback.rs` tests; no production provider registry/session path constructs it.

Production turn retry ownership is instead in `src/agent/provider_turn.rs`, which explicitly preserves the selected provider/session and consumes the unified retry budget. Yet:

- `fallback.rs` and `circuit.rs` remain public modules;
- `CircuitBreaker` is re-exported from `codegg-providers` and `codegg-core::resilience`;
- `architecture/provider.md` and `architecture/resilience.md` present fallback/circuit behavior as active provider architecture;
- `provider_turn.rs` still contains historical comments describing provider switching as owned by `FallbackProvider`.

This creates a future double-retry/failover hazard and an unclear public-API compatibility question even if there is no current runtime bug.

## 2. Corrective milestones

### C001 — Direct OpenAI endpoint composition correctness

Status: ready.

Implementation plan:

- `plans/implementation/provider-backend-post-closure-corrective/001-direct-openai-endpoint-composition-correctness.md`

Objective: define one explicit API-prefix contract for `OpenAiConfig.base_url`, fix default/env/config URL composition, audit the legacy OpenAI-config convenience constructors, and add exact request-URL regressions so duplicated/missing version prefixes cannot recur.

Priority: highest. This is a current direct-provider correctness defect.

### C002 — Provider catalog and compatible-discovery authority reconciliation

Status: ready.

Implementation plan:

- `plans/implementation/provider-backend-post-closure-corrective/002-provider-catalog-and-compatible-discovery-reconciliation.md`

Objective: correct confirmed provider endpoint drift (minimum OpenCode Go), make `setup_catalog.rs` the CodeGG endpoint/setup authority, separate generic compatible endpoint validation from the Eggpool preset's port normalization, and converge strict provisioning versus best-effort runtime model discovery on one bounded compatible `/models` transport/parser core while preserving their different failure policies.

Operational coordination: EggPool provider-profile metadata M001 at `58b1e24c8dce4fc099ac6cba17a0804e4f0cc8a5` is a sibling correction. Both may proceed independently; closure should compare shared provider IDs against first-party evidence, not copy from each other.

### C003 — Provider resilience ownership and public-API disposition

Status: ready.

Implementation plan:

- `plans/implementation/provider-backend-post-closure-corrective/003-provider-resilience-ownership-and-public-api-disposition.md`

Objective: prove the actual production retry/failover owner, prevent accidental double ownership, and make an evidence-backed disposition for unused `FallbackProvider`/provider-level `CircuitBreaker` API surface: retain and clearly mark as library-only, deprecate with compatibility guidance, or remove only if package/API evidence makes that safe.

Priority: maintenance/architecture correctness; no production retry redesign is authorized.

## 3. Dependency graph

```text
C001 direct OpenAI URL correctness ───────────────┐
                                                  ├─> final corrective closure
C002 catalog/discovery reconciliation ────────────┤
                                                  |
C003 resilience ownership/API disposition ───────┘
```

C001–C003 are independently ready and may be implemented in parallel if merge conflicts are controlled.

C002 has operational cross-repo coordination with EggPool metadata M001 but no hard dependency.

## 4. Invariants

All corrective milestones must preserve:

- direct-provider operation without an EggPool process;
- pinned `eggpool-wire` ownership of standard wire codecs/stream parsers;
- stable provider IDs, durable connection kinds, credential references, project/session selection, and setup protocol unless a plan explicitly proves a compatibility-safe metadata correction;
- CodeGG ownership of HTTP transport, endpoint/auth headers, session affinity, deadlines, cancellation, and retry taxonomy;
- one retry/failover authority per logical turn;
- conservative private reasoning/tool-alias behavior from `ProviderWirePolicy`;
- secret-safe diagnostics;
- no live provider credentials in routine tests;
- historical M001–M003 closure evidence remains immutable.

## 5. Non-goals

This corrective does not authorize:

- reopening shared wire-kernel design;
- importing EggPool account/routing/quota/health state;
- a shared provider-profile crate extraction;
- broad provider additions;
- cost/pricing routing;
- changing model selection or retry budgets;
- adding runtime provider-document freshness checks;
- deleting public resilience APIs without an explicit compatibility disposition.

## 6. Completion definition

The addendum closes when:

- direct OpenAI URL composition is correct for default, configured API-prefix, and trailing-slash cases with an exact capture-server regression;
- CodeGG's current built-in provider endpoint/setup facts have a first-party-source disposition and confirmed drift is corrected;
- generic compatible model discovery has one bounded HTTP/parse core with explicit strict-vs-best-effort policy;
- Eggpool preset normalization no longer leaks into provider-neutral compatible endpoint semantics;
- `FallbackProvider`/`CircuitBreaker` production ownership and public API status are truthful in code/docs/static guards;
- provider-focused and full required verification pass with no unresolved medium-or-higher provider finding.

## 7. Corrective status

| Corrective | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| C001 Direct OpenAI endpoint composition correctness | ready | `plans/implementation/provider-backend-post-closure-corrective/001-direct-openai-endpoint-composition-correctness.md` | — | none |
| C002 Provider catalog and compatible-discovery authority reconciliation | ready | `plans/implementation/provider-backend-post-closure-corrective/002-provider-catalog-and-compatible-discovery-reconciliation.md` | — | none; coordinate with EggPool metadata M001 |
| C003 Provider resilience ownership and public-API disposition | ready | `plans/implementation/provider-backend-post-closure-corrective/003-provider-resilience-ownership-and-public-api-disposition.md` | — | none |
