# Plan 001 — Pinned Eggsearch Development Integration and Paired Compatibility

Status: implemented

Source roadmap:

- `plans/subsystems/search-eggsearch-legacy-retirement-corrective-addendum.md`

Milestone: M001 pinned upstream dependency adoption, provider contract and one-unit development verification

Primary class: development dependency integration + compatibility

Planning baseline: `78e4914a588ac80a3e95616452d4073140567cb4`

Hard dependencies:

1. `eggstack/eggsearch` CodeGG legacy-search-parity M003 is accepted.
2. A tagged eggsearch release exists that contains the exact accepted M001/M002
   provider surface and M003 qualification candidate.

Dependency disposition (2026-10-13): **satisfied**. Upstream
`eggstack/eggsearch/plans/closure/codegg-legacy-search-parity/005-status.md`
strictly closes parity M003–M005, confirms published immutable v0.4.1
(tag `v0.4.1` at `32769d684417567dfb30fc5adea1475cee7f0ed0`),
and unblocks CodeGG downstream adoption at `>=0.4.1`.
The published v0.4.1 release passed the ten-tool/provider contract smoke;
CodeGG must still run its **paired source-development** compatibility suite
against the exact adopted upstream binary before closing M001; CodeGG
installer/release qualification belongs to a future milestone. v0.4.2 is
not automatically substituted without equivalent *upstream* contract
qualification.

Relevant predecessor evidence:

- `plans/closure/search-eggsearch-integration/005-status.md`
- `plans/closure/self-contained-installation-corrective/002-status.md`
- `architecture/search_backend.md`
- `RELEASING.md`

## 1. Objective

**Pre-release/development integration, not a CodeGG release.** CodeGG is not
operationally ready for public release and this milestone MUST NOT require,
trigger, simulate or imply CodeGG release publication. Adopt a **qualified,
immutable eggsearch source/binary contract** into CodeGG's development tree
to prepare for a much later release. Update the stable native `websearch`
provider contract to reflect the upstream inventory. Keep the managed
sidecar identity/pin coherent for future packaging, but treat release scripts
and docs as static compatibility configuration, not a release operation.

The milestone's primary acceptance evidence is a repeatable **paired
CodeGG+eggsearch test** from the development checkout, not a successful
CodeGG release package. Tests must exercise a real Rust CodeGG search runtime
and native wrappers against a **real pinned eggsearch subprocess**, in one
isolated command without working LLM inference, a user session, provider
credentials, live network or a deployable CodeGG binary.

The builtin backend remains until the separate M002 deletion. This isolates
version/provider contract migration from removal and allows faults to be
diagnosed independently. A successful compatibility test is **not** proof
that the entire CodeGG coding agent is operable or release-ready.

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

Preserve the **future** release-bundle sibling staging/version identity
contract for `codegg-eggsearch`; this milestone updates its static pin
expectations but does **not** stage/publish a CodeGG release. Development
compatibility runs against an explicitly provided pinned upstream executable
and must not silently select an unrelated PATH binary.

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
- installation/release/upgrade **static pin/configuration** owners;
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

## 10. Development paired-system compatibility gate

### 10.1 Required single-command harness

Add a repository-owned entrypoint, e.g. `scripts/test-eggsearch-pair.sh`
or equivalent Cargo alias, which qualifies the **CodeGG runtime + actual
eggsearch stdio subprocess** as a unit. It accepts an explicitly chosen
`CODEGG_EGGSEARCH_BIN` or an immutable v0.4.1 source checkout; it may
build the upstream binary with `cargo build --locked` into a temporary
target directory. It MUST NOT silently download a moving branch or run
CodeGG release scripts.

The harness must:
- reject missing/wrong-version binaries via an **exact** release-version
  check; record CodeGG revision, upstream tag and binary digest if known;
- use a private temporary HOME/config/workspace and clean up on exit;
  require no login, API keys, network, daemon/TUI operability or privilege;
- launch the real eggsearch process over MCP stdio using CodeGG's existing
  `bootstrap_search_runtime` and `SearchRuntimeContext`, verifying one
  shared session, bounded timeouts and process shutdown;
- assert successful MCP initialize, full ten-tool advertisement and
  `provider_status(probe=false)`, including release-expected provider IDs,
  without requiring optional provider credentials;
- test native wrapper→upstream mapping and negative cases without external
  fetching. Unsupported/retired explicit providers must fail *before* any
  network call and never silently auto-route. Where an actual provider
  request cannot run offline, use exact schema/protocol checks and record
  the limitation instead of accepting any error as success;
- pair this process test with existing deterministic fake-MCP contract and
  tagged retrieval-dimension/structured evidence fixtures, **invoked by
  the same command**; treat tests as one compatibility suite;
- classify version/protocol, argument/schema, structured receipt, policy,
  environment/credential, and optional network/provider failures distinctly;
  return nonzero for every compatibility failure;
- work for Linux/macOS source checkouts and keep the Rust test usable
  cross-platform; no hosted network CI lane or release publishing.

Proposed canonical development invocation **after the harness is implemented**:

```bash
CODEGG_EGGSEARCH_BIN=/absolute/path/to/eggsearch \
  scripts/test-eggsearch-pair.sh
```

The script must run the real paired-process check and its deterministic
fake-MCP/structured fixture checks together. `CODEGG_EGGSEARCH_BIN` is
required unless a caller explicitly passes a checked-out immutable v0.4.1
source root. Compilation may need pre-fetched Cargo dependencies, but the
**compatibility test phase itself must not require external network**. If
running in Windows where the shell script is unavailable, the Cargo
integration-test entrypoint must run the equivalent checks.

The real subprocess is needed for MCP handshake/tool inventory,
`provider_status(probe=false)`, exact binary version and shutdown.
**Provider-level search success cannot be asserted in offline mode**, and
eggsearch may reject an offline search *before* provider routing. Test
explicit CodeGG provider validation/argument translation offline at the
wrapper seam, not by interpreting a general offline-policy error as proof
that upstream processed the provider hint.

The existing `tests/eggsearch_real_compat.rs` performs live provider calls
and allows most non-schema errors. It can remain an **optional --live**
profile but does not replace the required offline paired-binary gate.

### 10.2 Focused tests

Focused:

```bash
cargo test --test search_backend_eggsearch -- --test-threads=1
cargo test --test search_backend_arg_mapping -- --test-threads=1
cargo test --test fake_eggsearch_mcp -- --test-threads=1
cargo test --test eggsearch_real_compat -- --ignored --nocapture
```

Use the exact adopted **eggsearch v0.4.1 executable** (source-built or a
verified release artifact), NOT a public CodeGG bundle, for the paired test.
The CodeGG search runtime is compiled from the active development checkout.

Run applicable offline version-identity and static packaging contract
tests for pin ownership, then:

```bash
cargo fmt --all -- --check
scripts/verify.sh quick
```

If the pin changes static release packaging metadata, run only the
existing **offline** release-tool tests; do not publish a CodeGG release,
build release archives as an operational gate, or require a release candidate.
Real installer/release qualification is deferred until CodeGG is operable.

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
- a **single-command offline paired CodeGG-runtime/real-eggsearch-process** suite passes (version, MCP handshake, ten tools, provider inventory, native wrapper schema, structured evidence, error handling and teardown);
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
- paired-process command, CodeGG and eggsearch identities, offline MCP/schema/provider inventory results, isolation/teardown and optional live results separately;
- static installer/pin contract checks and development search diagnostics pass; no CodeGG release execution is required;
- focused/broad test outcomes;
- unresolved findings by severity;
- recommendation for M002 readiness.
