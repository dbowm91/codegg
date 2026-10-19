# Agent Tool-Use Operability Corrective C002 — Closure Status

Status: blocked
Source plan: `plans/implementation/agent-tool-use-operability-post-audit-corrective/002-live-workspace-repository-search.md`
Implementation commit: `590c713ea377f50a741a69284060f3fdd483bf12` (`plans/agent-tool-use-operability-20261019`)
Recommendation: blocked pending a trustworthy CodeGG-session workspace binding contract from Eggsearch or an approved session-scoped sidecar design.

## 1. Finding and disposition

The alias and documentation are corrected, and the pinned Eggsearch pair fixture proves configured local search works offline. Production session/workspace binding is not available in the current interface: the Eggsearch request has no CodeGG workspace-root field, and the child uses static `[local].roots`. CodeGG must not send a model-selected root or imply that a configured global root is the active session workspace. No speculative authority change was made.

## 2. Changes landed

- `codesearch` now advertises and forwards only its supported query and token budget, and reports its compatibility behavior truthfully.
- `repo_search` continues to forward `include_local` exactly.
- `architecture/search_backend.md` and `docs/tools.md` disclose that local search follows Eggsearch's configured roots and is not currently bound to the active CodeGG workspace.
- Built pinned Eggsearch v0.4.3 at upstream commit `5e7b853e7eebf29c58f84959805c8774279d7ea4` and ran the real pair fixture with that binary.

## 3. Requirement-to-evidence matrix

| Requirement | Evidence | Status |
|---|---|---|
| Alias capabilities truthful | `tool::codesearch` schema/forwarding regression | Pass |
| `include_local` exact semantics | `tool::repo_search` and adapter source; pinned pair fixture | Pass |
| Offline local-only symbol hit and out-of-root decoy excluded | `scripts/test-eggsearch-pair.sh` with pinned v0.4.3; local provider reported and no remote attempts | Pass for configured fixture |
| Active session/subagent root supplied by trusted runtime | Production `SearchRuntimeContext` has config and shared MCP handle only; static sidecar config supplies roots | Blocked: interface lacks binding |
| Disabled/missing index, no-match, local status and remote fallback matrix | Not qualified through production session runtime | Missing |
| Bounded relevant result preserved under 15k cap | Not completed as the full production matrix | Missing |

## 4. Before/after trace

Before: a real child could perform local search when configured, but this did not establish that the roots belonged to the current CodeGG session; the `codesearch` alias also implied unsupported controls. After: the alias no longer advertises unsupported controls and docs state the root limitation. The actual production miss cannot be repaired safely with the current request contract.

## 5. Verification

Passed:

- `cargo test --locked -p codegg --lib tool::codesearch` — 1 test.
- `cargo test --locked -p codegg --lib tool::repo_search` — 2 tests.
- `scripts/test-eggsearch-pair.sh` with `CODEGG_EGGSEARCH_BIN=/tmp/codegg-c002-target/release/eggsearch` — pinned local-only fixture passed: unique local marker found, out-of-root decoy excluded, `providers_queried=local_workspace`, zero remote attempts.
- `scripts/verify.sh quick` — passed at the CodeGG implementation commit.

The test script invokes `verify.sh quick`; the final standalone quick run also passed. No public provider network calls were made. The installed unqualified binary was v0.3.4; qualification used the explicitly built pinned v0.4.3 binary.

## 6. Compatibility and migration

No CodeGG storage migration or protocol change. The existing Eggsearch static-root behavior is preserved. No second index, persistent cache, or implicit public-search fallback was introduced.

## 7. Security, failure, restart, and contention

No model-controlled path grants local authority. The local-only fixture shows no remote fallback under its explicit local-only policy. Session switching, subagent binding, disabled-index status, and sidecar disappearance are not qualified in production; stale or globally configured roots therefore remain an unresolved correctness and privacy concern.

## 8. Limitations and severity

Severity: high capability-integrity/privacy qualification gap. The pinned sidecar's static roots are not evidence of active-workspace binding. This requires an upstream request field/sidecar lifecycle contract or a separately accepted CodeGG session-scoped service design; neither is available in this repository change.

## 9. Unblock audit

C007 remains blocked on C002. Nothing downstream became eligible. C002 can resume when Eggsearch exposes a trusted session-root binding contract or an approved design provides a session-scoped child with lifecycle, isolation, and explicit local-only failure semantics.
