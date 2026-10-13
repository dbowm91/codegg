# Search / Eggsearch Development Integration and Legacy Backend Retirement

Status: active (M001 closed; paired-compatibility C001 ready; M002 blocked on C001)

Repository planning baseline: `78e4914a588ac80a3e95616452d4073140567cb4`

Controlling planning/process:

- `plans/003-planning-process.md#7-corrective-passes`
- `plans/subsystems/search-eggsearch-pair-compatibility-post-closure-corrective-addendum.md` (new post-closure C001 paired-test qualification)
- closed predecessor: `plans/subsystems/search-eggsearch-integration-roadmap.md`
- accepted predecessor closure:
  `plans/closure/search-eggsearch-integration/005-status.md`
- accepted installation boundary:
  `plans/closure/self-contained-installation-corrective/002-status.md`
- upstream corrective:
  `eggstack/eggsearch/plans/subsystems/codegg-legacy-search-parity-corrective-addendum.md`

## 1. Purpose

The original search/eggsearch integration deliberately retained
`search.backend = "builtin"` and `fallback_to_builtin` until the compatibility
value of CodeGG's in-tree search provider stack was understood.

That removal criterion is now actionable.

Current CodeGG:

- uses eggsearch as the default and sole normal owner of external search;
- bundles a pinned `codegg-eggsearch` sidecar in the supported prebuilt
  installation;
- keeps roughly 2.7k lines of legacy provider/search implementation under
  `src/search/*` plus `src/search_backend/legacy.rs`;
- keeps a second CodeGG-owned `webfetch` HTTP/extraction path for builtin
  mode/fallback;
- exposes modern `websearch` / `webfetch` arguments whose semantics are not
  preserved by that fallback;
- supports only `websearch` and `webfetch` in builtin mode, while
  repository/security/research/batch/evidence search requires eggsearch.

The remaining justification for the duplicate backend is source-specific
provider compatibility. The upstream eggsearch corrective now owns migration
or explicit disposition of that provider surface.

This CodeGG corrective consumes that upstream contract and then removes the
duplicate external-search execution owner.

**Development-stage scope (2026-10-13 clarification):** CodeGG as a whole
is not operationally ready and has **no near-term public release target**.
M001 qualifies the native CodeGG search runtime and exact pinned eggsearch
child process as a **single development unit**. It does not require full
agent operation, model inference, production installation, packaging,
release branches/tags, hosted publication or an external installer. Existing
`codegg-eggsearch` release conventions are retained as static future
compatibility configuration only. Production release readiness requires a
separate future plan and gate, and is not inferred from M001/M002 closure.

## 2. Dependency boundary

```text
eggsearch CodeGG legacy-search parity M001/M002
                    |
                    v
eggsearch M003 qualification + tagged release
                    |
                    v
CodeGG M001 development pin adoption + paired-process qualification
                    |
                    v
CodeGG M002 builtin backend retirement
```

The former upstream release blocker is **satisfied** as of 2026-10-13:
`eggstack/eggsearch/plans/closure/codegg-legacy-search-parity/005-status.md`
records strict closure of the upstream parity work and immutable published
`v0.4.1` tag at `32769d684417567dfb30fc5adea1475cee7f0ed0`.
CodeGG M001 is **closed** at
`plans/closure/search-eggsearch-legacy-retirement/001-status.md`: it adopted
and qualified that **upstream** published source/binary contract (`v0.4.1`)
in its development checkout. M002 is now ready. Upstream's immutable tag
is the dependency source; it does **not** imply CodeGG itself must publish a
release. M001 MUST use the pinned upstream artifact/source, never a moving
branch or `latest`.

## 3. Current-state findings that justify retirement

### 3.1 Builtin is a partial backend

Only `websearch` and `webfetch` have builtin implementations.
`repo_search`, `repo_fetch`, `repo_map`, `security_search`,
`research_search`, `batch_fetch`, and evidence bundling require eggsearch.

The builtin option is therefore not a complete alternate search subsystem.

### 3.2 Fallback is semantically lossy

The stable CodeGG `websearch` wrapper exposes modern controls including
`intent`, `freshness`, `safe_search`, and `excerpt_count`. The legacy
dispatcher consumes only query/result count/provider hint.

The stable `webfetch` wrapper exposes extraction, link, focus, and cache
controls. `execute_builtin` consumes only URL/output length plus its own HTTP
behavior.

A fallback can therefore turn one validated request into a materially
different request after an eggsearch failure. That is not a sound long-term
compatibility contract.

### 3.3 Supported installation already carries eggsearch

The prebuilt installation contract installs `codegg`,
`codegg-sandbox-helper`, and the pinned `codegg-eggsearch` together and
resolves the search sidecar relative to the CodeGG executable.

Source installs already document eggsearch as a separate required sidecar for
the normal search surface.

### 3.4 Duplicate security and maintenance ownership remains

Builtin mode retains:

- direct provider endpoint/credential logic;
- provider selection/routing;
- HTTP response parsing;
- a second web-fetch network/extraction path;
- CodeGG-owned SSRF/retry/response handling for that path;
- legacy backend config/doctor/provenance/tests.

Eggsearch independently owns those concerns for normal operation.

## 4. Invariants

- Stable model-facing CodeGG wrapper names remain:
  `websearch`, `webfetch`, `repo_search`, `repo_fetch`, `repo_map`,
  `security_search`, `research_search`, `batch_fetch`, and
  `evidence_bundle`.
- Raw `mcp__eggsearch__*` tools remain hidden by default.
- `research` remains CodeGG-owned orchestration/synthesis; only external
  evidence retrieval is eggsearch-owned.
- Local `grep`, `glob`, filesystem reads, Git, LSP, and workspace
  inspection remain native CodeGG capabilities.
- `codesearch` remains a separate compatibility-alias decision. This
  corrective does not require its removal.
- `search.backend = "disabled"` remains available.
- External evidence remains `external_untrusted` at the CodeGG boundary.
- The bundled eggsearch version remains immutable/pinned and version-validated
  by installer/release/upgrade paths.
- No silent explicit-provider remapping is allowed.
- Removing legacy search code must not be used as justification to delete
  shared Eggfetch, HTML extraction, SSRF, or HTTP helpers that still have
  non-search consumers.
- Generic `ToolBackendKind::BuiltinLegacy` may be removed only if a repository
  census proves no non-search backend still uses it.

## 5. Upstream provider compatibility gate

Before CodeGG M001 may begin, the qualifying eggsearch release must carry an
accepted final disposition for every historical CodeGG provider hint.

Expected shape, subject to upstream closure evidence:

| Historical CodeGG hint | Expected disposition |
|---|---|
| `auto` | intentional automatic eggsearch routing |
| `duckduckgo` | exact |
| `mojeek` | exact |
| `wikipedia` | upstream parity provider |
| `arxiv` | upstream parity provider |
| `openalex` | exact |
| `pubmed` | upstream parity provider |
| `hn_algolia` | upstream parity provider |
| `github` | map to upstream repository-discovery provider |
| `exa` | exact |
| `tavily` | exact |
| `brave` | explicit current Brave-provider mapping |
| `serpapi` | upstream parity provider |
| `kagi` | upstream current-v1 provider or explicit retirement |
| `google_news` | explicit retirement unless upstream later establishes a supported contract |

The upstream M003 closure record is authoritative if the final matrix differs.

## 6. Milestones

### M001 — Pinned eggsearch development adoption and provider-contract reconciliation

Status: closed (CodeGG pinned-binary acceptance recorded in M001 closure).

Plan:

- `plans/implementation/search-eggsearch-legacy-retirement/001-eggsearch-parity-adoption-and-provider-contract.md`

Satisfied upstream release prerequisites:

- parity M003-M005 accepted and qualified in the v0.4.1 closure;
- immutable published `v0.4.1` release carries that accepted surface.

Closure evidence: `plans/closure/search-eggsearch-legacy-retirement/001-status.md`.

Outcome:

- bump CodeGG's pinned eggsearch version through the existing release/install
  ownership points;
- align `websearch.provider` schema and translation with the real upstream
  inventory;
- remove provider values explicitly retired upstream;
- make unknown/unsupported explicit provider requests fail actionably rather
  than degrade to automatic routing;
- qualify wrappers and search diagnostics against the real upstream process
  using one repeatable offline CodeGG+eggsearch paired test; preserve
  installer/release pin metadata as **static compatibility** only;
- retain builtin backend temporarily so provider-contract migration and backend
  deletion are independently reviewable.

### M002 — Builtin external-search backend retirement

Status: **blocked** on strict paired-compatibility corrective C001. The prior M001 implementation/closure remains accepted; this new gate strengthens native-wrapper request and isolated-child evidence before destructive legacy deletion.

Plan:

- `plans/implementation/search-eggsearch-legacy-retirement/002-builtin-search-backend-retirement.md`

Outcome:

- remove `SearchBackendConfig::Builtin`;
- remove `fallback_to_builtin`;
- remove `src/search/*` and `src/search_backend/legacy.rs`;
- remove the CodeGG builtin `webfetch` network/extraction branch;
- simplify dispatch, provenance, bootstrap, doctor/config/docs/tests around
  `eggsearch | disabled`;
- preserve stable wrappers and higher-level CodeGG orchestration.

## 7. Configuration and migration policy

This repository is still documented as 0.1.0/pre-first-release, so this is the
lowest-cost time to simplify the config contract. However, existing source
users may already have local configs.

M002 must therefore provide an explicit migration diagnostic/documentation:

- `backend = "builtin"` -> configure/restore the managed or explicit
  eggsearch sidecar, or choose `backend = "disabled"`;
- `fallback_to_builtin = true` -> remove the setting; search failure remains
  a truthful eggsearch failure instead of semantic substitution.

Do not retain a hidden compatibility execution path merely so the old values
continue to run.

The implementation may either reject removed enum/config values with a clear
parse/migration error or use the repository's established deprecation codec if
one already exists and does not preserve the old execution behavior.

## 8. Verification strategy

M001 is a **pre-release development contract** milestone:

- strict request translation tests and canonical upstream v0.4.1 schemas;
- a single-command, isolated **CodeGG native wrapper/runtime +
  real pinned eggsearch stdio subprocess** test (MCP handshake, ten tools,
  nonprobing provider_status inventory, explicit errors and teardown);
- deterministic fake-MCP fixture regression tests for structured
  research/security receipts, no network or provider credentials required;
- source/binary exact identity plus static install/upgrade pin checks,
  **not** CodeGG installer/release qualification;
- optional live-provider diagnostic kept separate and labeled non-gating;
- provider-hint negative tests and normal `scripts/verify.sh quick`.

A test pass validates this **pair**, not the whole CodeGG daemon, TUI,
coding-agent loop, release bundle, security sandbox, or product usability.
No CodeGG release tasks are authorized by this workstream.

M002 is ownership/deletion focused **after C001 strict closure**:

- search backend dispatch tests;
- config migration/error tests;
- static/source census showing no production legacy external-search owner;
- normal `scripts/verify.sh quick`;
- broader workspace tests if enum/backend simplification touches generic tool
  backend diagnostics.

No new CI lane, network-dependent CI, compatibility matrix, or permanent
source scanner is required solely for this cleanup. Reuse the paired
compatibility check after M002 backend deletion to prove the simplified
production path still preserves the same interface.

## 9. Completion definition

The workstream closes when:

- CodeGG pins a released eggsearch version carrying the accepted parity
  contract;
- every advertised provider hint maps exactly to a supported upstream provider
  or has been deliberately removed with actionable migration guidance;
- explicit unknown providers do not silently become automatic routing;
- `SearchBackendConfig::Builtin` and `fallback_to_builtin` no longer select
  executable production behavior;
- `src/search/*`, `search_backend::legacy`, and builtin webfetch external
  retrieval are removed;
- all normal external search/fetch/provider execution crosses eggsearch;
- stable CodeGG wrappers, research orchestration, local coding tools, and
  `backend="disabled"` remain intact;
- documentation/doctor/config examples describe only the surviving contract.

## 10. Milestone status

| Milestone | Status | Implementation plan | Blocker |
|---|---|---|---|
| M001 pinned development adoption + paired contract | closed | `plans/implementation/search-eggsearch-legacy-retirement/001-eggsearch-parity-adoption-and-provider-contract.md`; closure `plans/closure/search-eggsearch-legacy-retirement/001-status.md` | Implementation `9dd8a1c8`; exact v0.4.1 paired process and quick verification passed |
| M002 builtin backend retirement | blocked | `plans/implementation/search-eggsearch-legacy-retirement/002-builtin-search-backend-retirement.md` | Historical M001 closed; new paired-compatibility C001 `plans/implementation/search-eggsearch-pair-compatibility-corrective/001-real-wrapper-and-private-child-qualification.md` must strictly close before deletion |
