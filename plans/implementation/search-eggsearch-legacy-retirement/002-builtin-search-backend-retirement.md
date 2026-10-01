# Plan 002 — Builtin Search Backend Retirement

Status: blocked

Source roadmap:

- `plans/subsystems/search-eggsearch-legacy-retirement-corrective-addendum.md`

Milestone: M002 builtin external-search backend retirement

Primary class: invariant + maintenance/correctness simplification

Planning baseline: `78e4914a588ac80a3e95616452d4073140567cb4`

Hard dependency:

- CodeGG M001 parity release adoption and provider-contract reconciliation
  closed.

Relevant predecessor evidence:

- `plans/closure/search-eggsearch-integration/002-status.md`
- `plans/closure/search-eggsearch-integration/005-status.md`
- `plans/closure/self-contained-installation-corrective/002-status.md`
- `architecture/search_backend.md`
- `architecture/security.md`

## 1. Objective

Remove CodeGG's duplicate external-search implementation after the pinned
eggsearch release has proven the provider compatibility surface.

After M002:

```text
websearch/webfetch/repo/security/research/evidence wrappers
                         |
                         v
                  SearchRuntimeContext
                         |
              +----------+----------+
              |                     |
          eggsearch               disabled
              |
              v
        external retrieval
```

There is no executable `builtin` external-search branch and no semantic
fallback from eggsearch failure to a different HTTP/provider implementation.

## 2. Production ownership to remove

The implementation must re-audit current main, but expected retirement scope
includes:

- `SearchBackendConfig::Builtin`;
- `SearchConfig::fallback_to_builtin`;
- builtin branches in `SearchRuntimeContext` dispatch/provenance;
- `src/search_backend/legacy.rs`;
- the legacy `src/search/` module tree and exports;
- `tool::webfetch::execute_builtin` and helpers used only by that external
  fetch branch;
- legacy search provider/env credential construction;
- builtin-specific bootstrap/doctor reporting;
- builtin/fallback-only integration tests such as
  `tests/search_backend_legacy.rs`;
- documentation/config examples describing builtin external search or fallback.

Delete code only after proving it has no surviving non-legacy consumer.

## 3. Explicit non-removals

Do **not** conflate legacy search deletion with unrelated shared infrastructure.

Preserve when still consumed elsewhere:

- `eggfetch-core` — providers, research URL sources, upgrade/client paths and
  other subsystems use it;
- `html2text` — research URL/docs adapters use it;
- `security::ssrf` — remote MCP/other network policy users may still depend
  on it;
- generic HTTP client policy used outside legacy search;
- `ToolBackendKind::BuiltinLegacy` if any non-search tool/backend still uses
  it;
- CodeGG's `research` coordinator, local sources, claim/synthesis/report
  behavior;
- local filesystem `grep`/glob/read, Git, LSP, memory, MCP resource search,
  and extension search;
- `codesearch` unless a separate compatibility audit proves its removal
  criteria are satisfied.

This milestone removes one duplicate **external search execution owner**, not
every symbol containing "search" or "builtin".

## 4. Config migration

Target search backend enum:

- `eggsearch`
- `disabled`

Remove executable semantics for:

- `builtin`
- `fallback_to_builtin`

Because pre-release/source users may already have local configuration, failure
must be actionable.

Preferred behavior:

- a removed `backend = "builtin"` value fails config loading with a message
  explaining that external search is eggsearch-owned and directing the user to
  restore/configure the managed/explicit sidecar or select `disabled`;
- a removed `fallback_to_builtin` setting is rejected as obsolete if the
  config parser can identify unknown/removed fields reliably.

If the config subsystem has an established deprecation codec that can retain
parse compatibility without retaining execution behavior, it MAY be used for
one compatibility window. It must not silently turn `builtin` into
`eggsearch` because that changes network/provider semantics.

Update `codegg.example.jsonc`, architecture/config docs, doctor help, and any
integrated-config projection accordingly.

## 5. Dispatch and failure semantics

### Eggsearch unavailable

`websearch` / `webfetch` return the same bounded actionable eggsearch
unavailable/error class as specialized wrappers. They do not invoke another
provider stack.

Update messages that currently say:

```text
Install eggsearch or set [search].backend = "builtin" / "disabled".
```

to reflect managed-sidecar recovery and `disabled` only.

### Cancellation/timeouts

Preserve existing SearchRuntimeContext/MCP cancellation and timeout ownership.
Deletion of fallback must not add retries or restart loops.

### Provenance

Search/fetch structured provenance is:

- eggsearch MCP when enabled;
- no execution when disabled/error.

Remove fallback provenance defaults that fabricate
`ToolBackendKind::BuiltinLegacy` for search wrappers.

## 6. Webfetch decomposition

`WebFetchTool` currently mixes the stable wrapper with the private builtin
HTTP implementation.

Refactor it to a thin wrapper around `SearchRuntimeContext::dispatch_web_fetch`
after deleting builtin execution.

Remove only helper code proven exclusive to builtin fetch, such as:

- private client construction used only by `execute_builtin`;
- Cloudflare-style retry branch;
- builtin response parsing/truncation helpers;
- builtin-only response-size tests.

Retain wrapper schema, tool category, structured result/provenance, and all
eggsearch fetch controls.

Do not use M002 to move CodeGG research `UrlSource` or other explicit HTTP
consumers into eggsearch; those are separate ownership decisions.

## 7. Legacy provider-tree deletion

Delete the entire in-tree provider implementation once no production consumer
remains:

- registry/routing/types;
- DuckDuckGo/Mojeek;
- Wikipedia/arXiv/OpenAlex/PubMed/HN/Google News/GitHub;
- Exa/Tavily/Brave/SerpAPI/Kagi provider clients.

Before deletion, use source search to prove only
`search_backend::legacy`, tests, and module-internal code consume the tree.

Do not retain a Cargo feature containing these providers. Feature-gating would
preserve most maintenance/security burden while weakening normal test coverage;
the upstream parity release is the intended compatibility replacement.

## 8. Bootstrap, doctor, and tool registration

Simplify search startup around:

- eggsearch configured/enabled;
- disabled.

Preserve:

- managed sidecar resolution;
- explicit command/MCP override;
- required/recommended ten-tool coverage;
- version and provider-status diagnostics;
- raw MCP exposure policy;
- output caps;
- evidence-wrapper gating.

Remove:

- fallback reporting;
- builtin backend label/implementation claims;
- tests whose only purpose is proving fallback.

Check `src/agent/tool_batch.rs` and generic backend diagnostics: do not change
their non-search meaning merely because search no longer selects
`BuiltinLegacy`.

## 9. Documentation reconciliation

Update at minimum where factual:

- `README.md`;
- `AGENTS.md`;
- `architecture/search_backend.md`;
- `architecture/security.md`;
- `architecture/config.md`;
- `architecture/tool.md`;
- `architecture/overview.md` if backend-count/module text changes;
- MCP/plugin skills that call `src/search/` a fallback;
- examples and doctor documentation.

Historical implementation/closure plans remain immutable evidence. New
corrective docs supersede their "fallback retained" end-state statements.

## 10. Tests and guards

Remove tests that only preserve deleted behavior, but replace them with tests
for the surviving invariant.

Required focused cases:

1. default config resolves eggsearch;
2. disabled config exposes no external search execution;
3. old builtin config receives actionable migration behavior;
4. eggsearch unavailable never calls any CodeGG provider client;
5. websearch/webfetch still expose stable wrapper names/schemas;
6. specialized search wrappers remain unchanged;
7. raw eggsearch MCP tools remain hidden by default;
8. two runtime contexts remain isolated;
9. no normal external-search provider endpoint/credential ownership remains
   under CodeGG production source.

A temporary source census is adequate for closure evidence. Add a permanent
guard only if the existing ownership/static-guard framework has a natural
place and the guard is simpler than compile-time deletion evidence.

## 11. Verification

Run narrow suites first after adapting/deleting legacy tests:

```bash
cargo test --test search_backend_eggsearch -- --test-threads=1
cargo test --test search_runtime_isolation -- --test-threads=1
cargo test --test fake_eggsearch_mcp -- --test-threads=1
cargo test --test search_backend_arg_mapping -- --test-threads=1
cargo test --test preflight_integration -- --test-threads=1
```

Then:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Run the existing supported release/install smoke if config/doctor/startup
changes touch that path materially.

Closure source census should include direct searches for:

```text
SearchBackendConfig::Builtin
fallback_to_builtin
search_backend::legacy
crate::search::
KAGI_API_KEY
SERPAPI_API_KEY
api.exa.ai
api.tavily.com
```

Remaining matches must be historical plans/docs, unrelated provider/model
credentials, or otherwise explicitly classified. No executable legacy
external-search owner may remain.

## 12. Acceptance criteria

M002 closes only when:

- search backend execution is `eggsearch | disabled`;
- `fallback_to_builtin` has no executable behavior;
- `src/search/*` and `src/search_backend/legacy.rs` are gone;
- builtin `webfetch` external HTTP execution is gone;
- websearch/webfetch failure does not semantically fall back;
- obsolete config receives actionable migration behavior;
- default/prebuilt managed-sidecar behavior is unchanged;
- all external search provider routing/credentials are eggsearch-owned;
- stable CodeGG wrappers and structured provenance remain intact;
- `codesearch`, higher-level research, local coding tools, and unrelated HTTP
  consumers are not regressed;
- focused tests, workspace Clippy, and `scripts/verify.sh quick` pass;
- docs/doctor/config describe the surviving architecture.

## 13. Stop conditions

Stop and create a corrective/follow-up plan if:

- a production consumer outside legacy dispatch still requires
  `src/search/*`;
- upstream parity was incomplete despite M001 closure;
- removal exposes a persisted config migration requiring a broader config
  format/version decision;
- generic `BuiltinLegacy` backend removal would affect non-search tools;
- research/local evidence behavior is accidentally coupled to the legacy
  provider tree;
- supported installer behavior depends on fallback in a way not captured by
  the preflight/install tests.

## 14. Closure evidence required

Create
`plans/closure/search-eggsearch-legacy-retirement/002-status.md` with:

- implementation SHA(s);
- file/module deletion inventory;
- config old/new matrix and migration examples;
- external-search ownership census;
- webfetch helper disposition;
- generic backend enum/caller census;
- focused/broad verification results;
- installer/doctor evidence if rerun;
- documentation reconciliation;
- unresolved findings by severity;
- final statement that eggsearch is CodeGG's sole external search/fetch
  provider execution owner.
