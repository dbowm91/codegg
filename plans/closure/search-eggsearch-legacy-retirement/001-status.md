# Search / Eggsearch Legacy Retirement M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/search-eggsearch-legacy-retirement/001-eggsearch-parity-adoption-and-provider-contract.md`

Source subsystem roadmap:

- `plans/subsystems/search-eggsearch-legacy-retirement-corrective-addendum.md`

Repository baseline reviewed: `3e8416f4` (rebased upstream plan baseline)

Implementation commits or pull requests:

- `9dd8a1c89253630b1c4ca556f276042325f19836` — v0.4.1 pin/provider-contract adoption, diagnostics, offline paired-process harness, tests, and documentation.

## 1. Executive finding

M001 is closed. CodeGG now adopts the immutable eggsearch v0.4.1 contract, validates explicit provider selections against that contract, reports useful provider inventory/health distinctions, and has a repeatable offline command that exercises the CodeGG runtime with the actual pinned eggsearch process. The separate builtin backend remains available for M002 to retire. This is development compatibility evidence; it makes no CodeGG release or full-agent-operability claim.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Qualified immutable upstream release | Eggsearch `plans/closure/codegg-legacy-search-parity/005-status.md`; tag `v0.4.1`, commit `32769d684417567dfb30fc5adea1475cee7f0ed0`; 44 IDs in `KNOWN_PROVIDER_IDS`; upstream M005 `make release-check` (including dry run) exit 0 | pass | Upstream closure is the authority for provider disposition and release qualification. |
| Pin ownership is coherent and exact | `src/install.rs`, `src/upgrade/managed.rs`, `scripts/release/lib-release.sh`, docs, fixtures, and `scripts/release/test-release-tools.sh`; exact version output validation rejects `0.4.10` | pass | The historical 0.3.9 research compatibility fixture remains historical; no release was produced. |
| Provider contract and migration behavior | `src/search_backend/eggsearch.rs`, `src/tool/websearch.rs`, `src/search_backend/bootstrap.rs`, provider schema and focused tests | pass | Matrix below; unknown, retired and unsupported explicit hints fail before dispatch. |
| Offline real-process compatibility | `scripts/test-eggsearch-pair.sh`, `tests/eggsearch_pair_compat.rs`, final run recorded in §4 | pass | One process, ten tools, `provider_status(probe=false)`, complete required inventory, and clean shutdown. |
| Native wrapper/provider mapping and negative cases | `search_backend_arg_mapping`, eggsearch unit tests, fake MCP suite, and pair harness | pass | No network/provider requests were made. |
| Bootstrap/doctor diagnostics distinguish incompatibility from provider state | bootstrap tests (22) and provider inventory fixture assertions | pass | Missing, disabled, missing credential, degraded/cooldown, and routable are distinct provider conditions. |
| Existing builtin compatibility reserved for M002 | Config/backend implementation unchanged; M002 plan remains the owner of deletion | pass | M001 did not edit `src/search/*` production behavior. |
| Required repository sanity | `cargo fmt --all -- --check`; `scripts/verify.sh quick` | pass | Exact results in §4. |

### Provider mapping and disposition

The wrapper's retained values resolve to these IDs advertised by v0.4.1:

| CodeGG input | Eggsearch provider ID / disposition |
|---|---|
| absent, `auto` | omit `providers`; intentional automatic routing |
| `duckduckgo`, `mojeek`, `wikipedia`, `arxiv`, `openalex`, `pubmed`, `hn_algolia`, `exa`, `tavily`, `serpapi`, `kagi` | same ID |
| `github` | `github_repositories` |
| `brave`, `brave_api` | respective `brave`, `brave_api` IDs; upstream current-v1 contract |
| `google_news` | retired; actionable `intent="news"` migration guidance; no source substitution |
| unknown/unsupported value | actionable validation error; no automatic fallback |

This reconciles the historical plan's expected Brave alias with the authoritative upstream M003 closure, which retains both `brave` and `brave_api`. The runtime contract check covers the 14 IDs referenced by the stable wrapper: `duckduckgo`, `mojeek`, `wikipedia`, `arxiv`, `openalex`, `pubmed`, `hn_algolia`, `github_repositories`, `exa`, `tavily`, `brave`, `brave_api`, `serpapi`, and `kagi`. The upstream's complete `provider_status` inventory contains 44 IDs; optional providers are not startup requirements.

### Pin ownership matrix

| Owner | Before | Adopted |
|---|---|---|
| Managed install and upgrade identity | eggsearch 0.3.9 | eggsearch 0.4.1 |
| Static release/bundle compatibility metadata | eggsearch 0.3.9 | eggsearch 0.4.1, tag `v0.4.1`, exact source commit above |
| User/architecture/tool documentation and current fixtures | eggsearch 0.3.9 | eggsearch 0.4.1 |
| Historical compatibility fixture | 0.3.9 research baseline | preserved as historical evidence |

## 3. Production implementation evidence

CodeGG's managed install, upgrade checks and static release metadata now identify eggsearch 0.4.1 and the immutable v0.4.1 source revision. The version parser requires the exact executable/version token. The native provider enum and adapter follow the accepted upstream inventory; provider arrays and historical aliases are validated before dispatch, including security/research helper paths. Google News now produces migration guidance rather than impersonating another source. Bootstrap diagnostics inspect the structured provider status without making optional credentials mandatory.

The new pair harness accepts an explicit binary or source checkout. For source, it verifies the exact immutable release SHA and exports that tree for an offline build. It records the CodeGG revision, upstream tag and binary digest, uses isolated temporary state, runs the real subprocess compatibility test and deterministic wrapper/fake-MCP/evidence fixtures together, then runs quick verification. No database, wire protocol, or storage migration was introduced.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg --lib search_backend::eggsearch::tests -- --test-threads=1
cargo test -p codegg --lib search_backend::bootstrap::tests -- --test-threads=1
cargo test -p codegg --test search_backend_arg_mapping -- --test-threads=1
cargo test -p codegg --lib research::sources::eggsearch -- --test-threads=1
cargo test -p codegg --test search_backend_eggsearch -- --test-threads=1
cargo test -p codegg --test fake_eggsearch_mcp -- --test-threads=1
scripts/release/test-release-tools.sh
CODEGG_EGGSEARCH_SOURCE=/home/sugarwookie/projects/eggsearch scripts/test-eggsearch-pair.sh
```

### Results

- Eggsearch adapter tests: 28 passed.
- Bootstrap tests: 22 passed.
- Provider argument mapping: 18 passed.
- Research eggsearch source tests: 18 passed.
- Search backend integration: 9 passed.
- Fake eggsearch MCP contract: 30 passed.
- Offline release-tool tests: 79 passed.
- Final pair run on CodeGG `9dd8a1c89253630b1c4ca556f276042325f19836` with eggsearch `v0.4.1` / `32769d684417567dfb30fc5adea1475cee7f0ed0`, binary SHA-256 `ca87df75f7b131dd2cf4e9941e220550ab97493cc252bc6869d37d4ecdc2eac3`: exact version, MCP initialize, ten advertised tools, `provider_status(probe=false)`, all 14 wrapper provider IDs, contract assertions, and process shutdown passed. The harness's `scripts/verify.sh quick` and `cargo fmt --all -- --check` passed.
- No live provider searches, ignored `eggsearch_real_compat` profile, hosted CI, all-features suite, CodeGG release, package, or installer qualification was run. These are outside M001's development-stage acceptance boundary; upstream release qualification is recorded above.

## 5. Invariant review

- Explicit provider identity is preserved or rejected before MCP dispatch; it never degrades into automatic routing.
- Optional provider availability and credentials do not become global startup requirements.
- The CodeGG+eggsearch process pair shares the production runtime's one managed MCP session and is shut down by the harness.
- Search invocation remains network-free in the compatibility gate; provider success is not inferred from an offline error.
- CodeGG retains its existing policy, validation and disclosure boundaries. The harness supplies private HOME/config/workspace state and does not require credentials or privilege.
- The builtin backend remains until M002; no capability is claimed for its removal.

## 6. Failure and recovery review

The harness returns nonzero for an incompatible version, startup/handshake or schema mismatch, provider inventory mismatch, contract test failure, or teardown failure. It removes temporary state on exit. Unsupported provider values fail locally before side effects. Provider cooldown/credential failures are classified as provider state, not binary incompatibility. Process crash recovery beyond bounded harness startup/shutdown is not a new production behavior in M001.

## 7. Migration and compatibility review

No database or persistent configuration migration is required. `backend="builtin"` and `fallback_to_builtin` remain valid through M001. The public provider enum deliberately narrows for retired Google News; its diagnostic gives the supported intent-based migration path. Kagi is mapped to upstream current-v1 as qualified; the old direct Kagi-v0 implementation remains only in the temporary builtin code until M002 removes that backend. Pin mismatch is reported exactly.

## 8. Security review

No provider credentials are needed for the pair test, and no secrets are logged or supplied. The test uses isolated temporary home/config/workspace directories and does not make external provider requests. Existing URL/domain, authorization, and SSRF policy remain CodeGG-owned and unchanged. Caller-controlled provider selectors are bounded by the known enum/mapping and rejected before process dispatch when unsupported.

## 9. Documentation and operations

Updated the install, release, MCP, search backend, tool, and README surfaces for the current version/provider contract. Added `scripts/test-eggsearch-pair.sh` as the one-command development qualification entrypoint. Doctor/bootstrap messages now distinguish incompatible binary/inventory from disabled, unconfigured, credential-degraded, cooldown, and routable provider states. M002's plan is now eligible and requires rerunning the same pair gate after legacy deletion.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | No live provider-search request was made in this offline development gate. | Confirms schema/contract and real process interoperability, not remote provider availability or credentials. | Optional live qualification may be run separately; it is not an M001 blocker. |
| low | CodeGG release, hosted CI and installer qualification were not run. | No CodeGG publication/readiness claim follows from this milestone. | Defer to a future release-operability plan; not an M001 blocker. |

No high or critical findings remain within M001 scope.

## 11. Roadmap disposition

Milestone closed; the next dependency may proceed. M002 builtin backend retirement is ready and must preserve/rerun the exact offline paired-process gate. Evidence workflow M002, M003, and M004 are also ready now that their legacy-retirement M001 dependency and M004's strict C001 dependency are satisfied. Evidence M005 remains blocked on M002 and M004.

## 12. Registry updates

- Mark this plan implemented and M001 closed in the legacy-retirement roadmap and registry.
- Move legacy-retirement M002 from blocked to ready.
- Move evidence-workflow M002/M003/M004 from blocked to ready; keep M005 blocked on M002+M004.
- Remove the corresponding M002 and M004 entries from the registry's blocked-work table and update current execution-order/deferred summaries to record the closure.
