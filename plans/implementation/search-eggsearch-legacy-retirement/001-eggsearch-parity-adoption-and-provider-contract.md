# Plan 001 — Eggsearch Parity Release Adoption and Provider Contract

Status: blocked

Source roadmap:

- `plans/subsystems/search-eggsearch-legacy-retirement-corrective-addendum.md`

Milestone: M001 eggsearch parity release adoption and provider-contract reconciliation

Primary class: compatibility + dependency adoption

Planning baseline: `78e4914a588ac80a3e95616452d4073140567cb4`

Hard dependencies:

1. `eggstack/eggsearch` CodeGG legacy-search-parity M003 is accepted.
2. A tagged eggsearch release exists that contains the exact accepted M001/M002
   provider surface and M003 qualification candidate.

Relevant predecessor evidence:

- `plans/closure/search-eggsearch-integration/005-status.md`
- `plans/closure/self-contained-installation-corrective/002-status.md`
- `architecture/search_backend.md`
- `RELEASING.md`

## 1. Objective

Adopt the qualifying upstream eggsearch release and make CodeGG's stable
`websearch` provider contract match the actual upstream provider inventory
before deleting the legacy builtin backend.

This plan deliberately leaves the builtin backend in place. It isolates
version/provider-contract migration from the larger deletion in M002 so a
failed provider mapping cannot be masked by simultaneous backend removal.

## 2. Upstream evidence required before implementation

Record from the eggsearch M003 closure:

- exact tagged version;
- exact release commit SHA;
- `provider_status` inventory IDs/count;
- final historical-CodeGG-provider migration/disposition matrix;
- Kagi disposition;
- Google News disposition;
- explicit-provider failure semantics;
- `make release-check` outcome.

Do not infer planned provider IDs from the upstream roadmap if its closure
differs.

## 3. Current CodeGG pin ownership

Audit and update every existing pin/version identity owner together.

At the planning baseline these include at least:

- `src/install.rs::PINNED_EGGSEARCH_VERSION`;
- `src/upgrade/managed.rs::PINNED_EGGSEARCH_VERSION`;
- `scripts/release/lib-release.sh`;
- `README.md`;
- `RELEASING.md`;
- `architecture/mcp.md`;
- `architecture/search_backend.md`;
- installer/release tests that assert the sidecar version;
- any current upstream-compatibility documentation referring to 0.3.9.

Use source search at implementation time rather than assuming this list is
complete.

The release bundle must continue staging `codegg-eggsearch` as a sibling and
validating `--version`; do not switch to PATH-only discovery.

## 4. Provider-schema reconciliation

The public `websearch.provider` enum and
`search_backend::eggsearch::translate_provider_hint` must be generated or
manually kept in exact agreement with the accepted upstream matrix.

Expected mappings if upstream closes as currently planned:

```text
auto            -> omit providers / intentional auto
duckduckgo      -> duckduckgo
mojeek          -> mojeek
wikipedia       -> wikipedia
arxiv           -> arxiv
openalex        -> openalex
pubmed          -> pubmed
hn_algolia      -> hn_algolia
github          -> github_repositories
exa             -> exa
tavily          -> tavily
brave           -> brave_api
brave_api       -> brave_api
serpapi         -> serpapi
kagi            -> kagi, only if upstream shipped it
google_news     -> removed, unless upstream closure explicitly changed disposition
```

The eggsearch M003 closure is authoritative.

### Explicit provider semantics

Only `provider = "auto"` or an absent provider field may select automatic
provider routing.

A caller-supplied provider value that is:

- unknown to CodeGG;
- known historically but retired;
- unsupported by the pinned eggsearch release;

must produce an actionable validation error. It must not turn into an empty
provider list and silently execute automatic search.

This rule also applies to helper paths that accept provider arrays or historical
aliases.

## 5. Google News migration

Unless the upstream M003 closure provides a supported exact provider:

- remove `google_news` from the exposed provider enum;
- remove literal translation to a nonexistent upstream provider;
- update docs/prompts/examples that name it;
- add a migration diagnostic where historical inputs can still reach runtime:
  use `intent = "news"` and optionally select a current provider that actually
  advertises news support.

Do not silently map `google_news` to Brave, Tavily, or another engine; source
identity and retrieval intent are different semantics.

## 6. Kagi migration

Follow the upstream closure exactly:

- if Kagi current-v1 shipped, expose/map `kagi`;
- if upstream rejected Kagi on current terms/contract grounds, remove the
  CodeGG provider hint and produce actionable migration guidance.

Under no circumstance preserve CodeGG's old direct Kagi v0 path in M001. The
legacy backend is still physically present until M002, but this plan must
prevent the default eggsearch facade from advertising a provider that the
pinned upstream cannot honor.

## 7. Doctor/bootstrap compatibility

Update search diagnostics so the newly pinned release is checked against the
same ten-tool required/recommended surface plus the provider-contract evidence
needed by CodeGG.

Do not make every optional provider a startup requirement.

Provider diagnostics should distinguish:

- the eggsearch version is wrong/incompatible;
- a provider ID expected by the CodeGG wrapper is absent from the upstream
  inventory;
- a provider is present but disabled;
- a provider requires a missing credential;
- a provider is temporarily degraded/cooldown.

If current `provider_status` already provides these distinctions, consume its
stable structured fields rather than adding parallel probing.

## 8. Production changes expected

At minimum inspect/update:

- `src/tool/websearch.rs`;
- `src/search_backend/eggsearch.rs`;
- `src/search_backend/bootstrap.rs`;
- installation/release/upgrade pin owners;
- fake/real eggsearch compatibility tests;
- provider argument-mapping tests;
- doctor/preflight tests;
- `README.md`, `RELEASING.md`, `architecture/search_backend.md`,
  `architecture/mcp.md`, and config/tool docs.

Do not edit `src/search/*` except for narrow tests/docs needed to keep the
temporary builtin compatibility branch buildable until M002.

## 9. Compatibility and migration

No database/storage migration.

Config values for `backend="builtin"` and `fallback_to_builtin` remain
parseable/executable during M001; M002 owns their removal.

Model-facing provider compatibility is intentionally narrower where a
historical provider has no accepted upstream implementation. This is a
correctness change: explicit unsupported sources fail instead of pretending to
work.

## 10. Verification

Focused:

```bash
cargo test --test search_backend_eggsearch -- --test-threads=1
cargo test --test search_backend_arg_mapping -- --test-threads=1
cargo test --test fake_eggsearch_mcp -- --test-threads=1
cargo test --test eggsearch_real_compat -- --ignored --nocapture
```

Use the exact newly pinned `codegg-eggsearch` binary for the real compatibility
smoke.

Run release/install pin tests applicable to the touched ownership points, then:

```bash
cargo fmt --all -- --check
scripts/verify.sh quick
```

If the version bump affects release packaging semantics, also run the existing
release packaging verification; do not invent a new CI lane.

Required negative cases:

1. unknown explicit provider is rejected;
2. retired historical provider is rejected with migration guidance;
3. `auto` still intentionally omits provider selection;
4. every retained provider maps to an ID advertised by the pinned real
   eggsearch binary;
5. missing optional/required provider credentials surface as provider
   degradation, not CodeGG schema mismatch.

## 11. Acceptance criteria

M001 closes only when:

- CodeGG pins the qualifying released eggsearch version everywhere;
- bundle/version validation expects that version;
- the exposed provider enum matches the final upstream migration matrix;
- every retained explicit provider reaches a real upstream ID;
- unknown/retired explicit provider values do not silently auto-route;
- Google News and Kagi dispositions match upstream closure;
- real-binary compatibility passes against the pinned release;
- doctor/bootstrap diagnostics remain actionable;
- builtin backend behavior is otherwise unchanged and reserved for M002;
- `scripts/verify.sh quick` passes.

## 12. Stop conditions

Stop and create/revise upstream corrective work if:

- the tagged release does not contain the M003-qualified provider inventory;
- CodeGG requires a provider behavior not represented by the upstream closure;
- explicit-provider identity cannot be preserved through the current eggsearch
  MCP contract;
- the version bump exposes a ten-tool contract break;
- supporting a historical hint would require reintroducing a direct provider
  client.

## 13. Closure evidence required

Create
`plans/closure/search-eggsearch-legacy-retirement/001-status.md` with:

- upstream M003 closure/release references;
- old/new pin matrix;
- final provider mapping/disposition table;
- explicit-provider negative-path evidence;
- real-binary version/tool/provider inventory output;
- installer/release/doctor verification;
- focused/broad test outcomes;
- unresolved findings by severity;
- recommendation for M002 readiness.
