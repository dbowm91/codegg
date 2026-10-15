# Search / Eggsearch Paired Compatibility — M001 Post-Closure Corrective

Status: active (C001 historically conditional; C002 ready against immutable eggsearch v0.4.3)
Primary class: development integration verification / test isolation
Planning baseline: CodeGG `agent/eggsearch-evidence-workflow-20261013` after M001 implementation `9dd8a1c89253630b1c4ca556f276042325f19836`.

Controlling governance: `plans/003-planning-process.md#7-corrective-passes`.
Original source roadmap: `plans/subsystems/search-eggsearch-legacy-retirement-corrective-addendum.md`.
Original plan: `plans/implementation/search-eggsearch-legacy-retirement/001-eggsearch-parity-adoption-and-provider-contract.md`.
Original strict historical closure: `plans/closure/search-eggsearch-legacy-retirement/001-status.md` — **retain unchanged**.
Historical C001 upstream baseline: immutable `eggstack/eggsearch` `v0.4.1` at `32769d684417567dfb30fc5adea1475cee7f0ed0`. Corrected C002 upstream contract: immutable **`v0.4.3`** at `5e7b853e7eebf29c58f84959805c8774279d7ea4`, published 2026-10-13 after zero-remote M001 regressions and seven-target qualification/provenance. C002's own CodeGG qualification remains open.

## 1. Purpose and scope

CodeGG still is **not operational as a completed coding agent** and remains far from release. This corrective strengthens only the **pre-release paired-system compatibility evidence** for the CodeGG source-built search runtime and its pinned eggsearch child. It is neither an implementation of a full agent loop nor a requirement to package, publish, install, deploy, or qualify a CodeGG release.

M001 delivered the exact v0.4.1 pin, provider-contract validation, bootstrap inventory diagnostics, an offline `scripts/test-eggsearch-pair.sh` runner, the real-subprocess `tests/eggsearch_pair_compat.rs`, and fake-MCP/native-wrapper fixtures. Its closure legitimately demonstrates version, handshake, tool discovery and `provider_status` interoperability. Two **additional** requirements of the intended "single unit" testing capability are not yet directly proved.

## 2. Evidence-backed deficits

**C001-A: Real wrapper-to-process gap.** The real-process test only initializes MCP, enumerates ten tools, calls `provider_status(probe=false)` directly, and shuts down. It contains no `WebSearchTool`, `RepoSearchTool`, `ResearchSearchTool` or `SearchRuntimeContext::dispatch_*_structured` call against that child. Separate mock-MCP tests prove adapter argument translation but not that those requests cross the real tagged process boundary. `tests/eggsearch_real_compat.rs` runs actual provider/network requests and tolerates most provider errors; it does not substitute for an offline deterministic gate.

**C001-B: Child-process isolation gap.** `tests/eggsearch_pair_compat.rs` mutates process-global `HOME` and `XDG_CONFIG_HOME` with `std::env::set_var`, while `EggsearchConfig.env = Some(HashMap::new())`. `src/mcp/local.rs` explicitly calls `Command::env_clear()` and only reinjects `PATH` plus configured child env. Consequently, the child is **not** proven to receive the test's private directories; the test's `credentials=cleared` and `network_requests=none` log wording is not a proof of private child config or a network-deny policy. The local process inherits a current working directory unless otherwise set. Tagged eggsearch defaults to its platform config directory; on macOS the default is under `~/Library/Application Support` rather than necessarily XDG config.

These are **test-qualification gaps**, not evidence of a production CodeGG/eggsearch security exploit or a discovered runtime incompatibility.

## 3. Invariants

- Preserve M001's historical closure and `0.4.1` pin/provenance; don't retroactively mark it failed or edit its closure.
- Keep `SearchRuntimeContext` as native-wrapper owner and the daemon's existing single MCP session/transport. No new alternate upstream network client, process-per-tool, raw MCP model exposure, provider routing logic or fake release pipeline.
- Treat `external_untrusted` framing, MCP structured result shape, explicit provider rejection and policy checks as non-negotiable. Test assertions distinguish **transport-level success** from **tool `isError`**, local validation errors, and upstream offline policy rejection.
- No real provider network traffic, API keys, logged secrets, TUI/daemon startup, LLM credentials, privileged sandbox or CodeGG release artifacts in the required suite. Live provider tests remain opt-in and nongating.
- Child env isolation is applied to the **spawned process**, not merely the test process. Avoid process-global `std::env::set_var` in parallel async tests; ensure a private explicit config path on macOS, Linux and Windows.
- The single-command developer harness remains reusable after legacy backend deletion and subsequent repo/web search enhancements.
- Do not silently declare unsupported offline network calls compatible just because an error occurred; classify errors against both CodeGG mapping and the actual tagged upstream schema/policy.

## 4. New corrective milestone

| ID | Milestone | Status | Plan | Dependencies |
|---|---|---|---|---|
| C001 | Real native-wrapper MCP dispatch and private-child isolation proof | **conditionally closed** | `plans/implementation/search-eggsearch-pair-compatibility-corrective/001-real-wrapper-and-private-child-qualification.md`; closure `plans/closure/search-eggsearch-pair-compatibility-corrective/001-status.md` | Two native wrappers cross the pinned process and child isolation is demonstrated. Strict closure is withheld because exploratory `repo_search(include_local=true)` under Mode Off initiated remote provider dispatch. |
| C002 | Correct and requalify Mode-Off local repository search | **active** | `plans/implementation/search-eggsearch-pair-compatibility-corrective/002-v041-local-only-offline-dispatch-requalification.md` (historical filename retained) | Upstream v0.4.3 release satisfies the external gate; CodeGG pin/harness changes and real child qualification are in progress. Strict C002 closure is still required. |

C001 executed without working CodeGG full agent inference or publishing any release. Evidence-workflows M002–M004 retain their existing status; this pair-qualification stream does not change their dependencies.

**Gate:** Legacy-retirement M002 (delete duplicate CodeGG HTTP/provider backend) remains blocked. C001 proves two wrapper policy-denial trajectories and child isolation, but its exploratory local repository probe exposed a remote-dispatch defect in the pinned upstream. The corrected upstream tag **v0.4.3 now exists**, so C002 is ready; M002 must wait for **CodeGG's strict C002 paired requalification and closure**, not for any additional upstream release. After retirement, rerun the same-unit test. Evidence-workflow M005 retains its existing M002+M004 dependencies.

## 5. Corrective completion definition

- Real pinned eggsearch child receives at least two **native CodeGG wrapper** requests representing distinct tool families; one must be `websearch`, the other a repo/security/research wrapper. Current C001 covers `websearch` and `security_search` policy denials. `repo_search(include_local=true)` may now be exercised **only** with exact corrected v0.4.3 during C002 qualification; it remains unsafe as an offline fixture with historical v0.4.1/v0.4.2. The new real-child trajectory must return private local source evidence without remote provider telemetry.
- A private explicit config, private HOME/cache/data paths and sanitized env are passed to eggsearch, with testable evidence that they govern the child, not just the parent. Parent credentials and proxy variables are not visible to the child. Do not claim network containment: the exploratory repository probe initiated remote provider dispatch under the pinned release.
- Version, ten-tool and provider inventory guarantees remain; subprocess teardown remains bounded and leak-free on success/failure/cancellation.
- One offline script invokes real-process and fake-MCP controls; typed failures return nonzero with stage-specific diagnostics. The offline test suite can be rerun after retirement M002 without reintroducing legacy search.
- Closure `plans/closure/search-eggsearch-pair-compatibility-corrective/001-status.md` supplies exact tested CodeGG revision, eggsearch binary provenance/digest, negative trajectories, command results, non-run evidence and a decision on unblocking legacy-retirement M002.

C001 implementation and local verification are recorded in its closure. Upstream eggsearch owns and has published the correctness fix (v0.4.3); C002 now owns the outstanding **CodeGG** pin/real-process requalification. C001's historical conditional status does not unblock legacy-retirement M002. Closure must use a successor C002 record.
