# Agent Tool-Use Operability Corrective C002 — Follow-up Closure Status

Status: closed
Source plan: `plans/implementation/agent-tool-use-operability-post-audit-corrective/002-live-workspace-repository-search.md`
Historical blocker record (unchanged): `plans/closure/agent-tool-use-operability-post-audit-corrective/002-status.md`
Predecessor scope contract: `plans/closure/agent-tool-use-operability-post-audit-corrective/008-status.md`
CodeGG implementation commit: `c23951db96e93d9ddf9cc022f98d03b336df93ac`
Additional production-path qualification commit: `928609dd0cc44bb44400d92df98d05b9e449af7e`
Eggsearch pinned implementation: `32f48bb57a43274d763c7f9ba919eab84d125edc` (v0.4.4)
Recommendation: strict closure; C007 is now dependency-ready.

## 1. Finding and disposition

The original C002 blocker was architectural, not a failure to forward `include_local`: CodeGG had no trusted way to bind a session's accepted workspace to Eggsearch local roots. C008 supplied that contract and its real-child isolation proof. This follow-up closes C002's remaining production and capability evidence: real runtime startup, accepted session and subagent workspace scope, local-only outcomes, truthful alias behavior, bounded output, provider telemetry, and compatibility behavior.

The original `002-status.md` remains as immutable evidence of why C002 was blocked before C008. This follow-up record strictly closes the successor work; it does not rewrite that history.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Status |
|---|---|---|
| Runtime-started managed Eggsearch uses the accepted active workspace, not a model-selected root | Real Eggsearch 0.4.4 process pair; native `repo_search` under accepted workspace A; spoofed `host_local_scope` argument ignored | Pass |
| Two session roots are isolated under concurrent and switched use | Real AgentLoop→broker→RepoSearchTool trajectory: A/B concurrent, then A→B→A; each output contains its own marker only | Pass |
| Subagent invocation retains its accepted workspace root | Same real AgentLoop/pinned child test with `AgentMode::Subagent` and a separate session identity; only workspace B marker returned | Pass |
| Owner/repository filters and aliases remain truthful | `fake_eggsearch_mcp` repository argument mapping tests; `tool::codesearch` schema/forwarding test; `codesearch` advertises only controls it forwards | Pass |
| `include_local` keeps its exact meaning and local-only requests do not silently broaden to remote | Real process-pair local-only positive, no-match, `include_local=false`, and disabled-local cases under Eggsearch mode Off; telemetry reports local routing and zero remote attempts | Pass |
| Missing or disabled local index reports a classified outcome | Real child disabled-local policy case; Eggsearch root-removal and disabled-scope tests; MCP child timeout/reap coverage | Pass |
| Bounded result preserves structured evidence and reports truncation | Real pair caps model-facing output to 256 characters, asserts truncation/provenance and retained structured marker/path | Pass |
| Untrusted provenance and SSRF behavior remain intact | Existing repository evidence framing and fetch URL validation tests; pair makes no live provider calls | Pass |
| Install/upgrade pin is compatible and rejects unsupported versions | Pair binary exact v0.4.4 commit; CodeGG version gate accepts 0.4.4–0.4.x and rejects 0.4.3 and >=0.5 | Pass |

## 3. Before/after trace

Before the follow-up, a pinned child could return configured local data but there was no proof that those roots represented the current session. The first scoped real-child test also found that modern MCP normalization erased custom `_meta`, causing the child to use startup roots. After the metadata merge fix, accepted roots are selected per call; raw managed-child MCP requests receive authenticated `root: null`, and tests verify they cannot return configured local file data. The real AgentLoop trajectory confirms that the accepted caller path reaches the pinned child with session-specific roots, including subagent mode and warm A/B/A reuse.

## 4. Verification

Passed:

- `cargo test -p codegg --lib tool::codesearch --locked` — 1 test.
- `cargo test -p codegg --lib tool::repo_search --locked` — 2 tests.
- `cargo test -p codegg --lib mcp::protocol::tests::modern_params_preserves_private_request_metadata_and_owns_protocol_fields --locked` — 1 test.
- `scripts/test-eggsearch-pair.sh` with Eggsearch v0.4.4 built from exact commit `32f48bb57a43274d763c7f9ba919eab84d125edc`, SHA-256 `21f01a76d32277fab4346dc1d55452fc55bd024f44456d2d657b63d819adafa2` — focused adapters, fake MCP, child lifecycle, real child, real AgentLoop A/B/A plus subagent, and `scripts/verify.sh quick` passed.
- `cargo fmt --all -- --check` and `git diff --check` — passed.

No live provider call, real model, public release, or SBC benchmark was run or claimed. The real-child fixture uses Eggsearch `mode=off` and no external providers.

## 5. Compatibility, security, and ownership

CodeGG's accepted broker context supplies the canonical root and workspace path-policy identity/revision. Model arguments cannot grant scope. The managed child receives private authenticated request metadata; Eggsearch validates and isolates it. Local repository content remains external-untrusted evidence. Local-required failures stay explicit and do not report remote coverage. The alias discloses and forwards only supported arguments. No new storage, persistent authorization cache, or second index was introduced.

This contract authorizes the accepted workspace root as a whole; it does not introduce file-level ACL semantics. The process-local bounded inventory cache is an optimization and contains no authority state. Standalone Eggsearch configured-root behavior remains supported when host metadata is absent; explicit remote MCP clients do not receive host scope.

## 6. Limitations and severity

No in-scope C002 acceptance gap remains. Fine-grained path ACLs, live-provider routing, performance/endurance, release qualification, and SBC startup cost are not claimed by this milestone and remain separate concerns.

## 7. Unblock audit

C001–C006 and C008 are strictly closed. C002 now strictly closes its capability matrix and discharges the sole hard C007 blocker. C007 is marked ready in `plans/registry.md` and the source addendum. No other registered plan in this corrective line became newly eligible.

Historical C002 and C007 blocker records are unchanged.

## 8. Registry disposition

The implementation plan is marked implemented; this follow-up is the closure record for the resumed C002 work. `plans/registry.md` and the source addendum mark C002 closed and C007 ready.
