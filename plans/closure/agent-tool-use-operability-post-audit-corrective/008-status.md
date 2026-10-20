# Agent Tool-Use Operability Corrective C008 — Closure Status

Status: closed
Source plan: `plans/implementation/agent-tool-use-operability-post-audit-corrective/008-shared-eggsearch-host-scoped-local-retrieval.md`
Source subsystem roadmap: `plans/subsystems/agent-tool-use-operability-post-audit-corrective-addendum.md#7-milestones`
CodeGG implementation commit: `c23951db96e93d9ddf9cc022f98d03b336df93ac` (`plans/agent-tool-use-operability-20261019`)
Eggsearch implementation commits: `0ed5c1dd8a42ea69db8c8de60d9815a030b9fc9a` and qualification follow-up `32f48bb57a43274d763c7f9ba919eab84d125edc` (`agent-tool-use-c008`, pushed)
Recommendation: strict closure; C002 becomes ready for its remaining capability qualification. C007 remains blocked on strict C002 closure.

## 1. Finding and disposition

C008 establishes the missing private contract between CodeGG's accepted workspace authority and one shared Eggsearch stdio process. CodeGG selects a canonical workspace root from the accepted `ToolExecutionContext`; a model argument cannot select it. The managed child receives a private process token, and repository calls carry the root (or explicit `null`) in authenticated MCP request metadata. Eggsearch validates the token and creates a bounded, root-keyed local retrieval view with an isolated inventory cache. A present null scope disables local retrieval and never falls back to startup-configured roots.

The real process-pair test exposed an additional wire defect: CodeGG's modern MCP normalizer replaced custom `_meta` with protocol negotiation metadata. This silently removed host scope, and the real child fell back to static local roots. `modern_params` now merges custom metadata while retaining CodeGG-owned protocol fields; the regression test and the real child no-leak assertion pass. A separate AgentLoop fixture initially omitted Eggsearch's provider table and exited before initialization; correcting the fixture allowed the real AgentLoop qualification to pass.

## 2. Changes landed

- Eggsearch 0.4.4 adds authenticated `codegg/host-local-scope` metadata to `repo_search`, `repo_fetch`, and `repo_map`, rejects malformed/forged scopes, canonicalizes one requested root, fails closed for missing roots, and maintains isolated bounded local backend/inventory caches per root/configuration. Existing startup-configured roots remain available to standalone Eggsearch when host metadata is absent.
- CodeGG creates and privately retains a per-child token for its managed Eggsearch stdio connection, injects per-call root metadata only for accepted host-scoped repository calls, and sends explicit disabled scope for ordinary raw MCP calls on that child. Explicit remote MCP clients remain unscoped.
- Native `repo_search`, `repo_fetch`, `repo_map`, and `codesearch` derive scope from accepted execution context. `codesearch` forwards through the same scoped repository search adapter.
- Modern MCP parameter normalization now preserves custom request metadata while overwriting reserved protocol fields with CodeGG's negotiated values.
- Managed-install and upgrade pins now require Eggsearch 0.4.4; compatibility tests reject 0.4.3 and versions outside the supported 0.4.x range.
- Architecture, user docs, tool-execution and MCP skills, release pin tests, and the paired sidecar script describe and verify the boundary.

## 3. Requirement-to-evidence matrix

| Requirement | Evidence | Status |
|---|---|---|
| One shared process serves concurrently authorized roots A and B without cross-workspace results | `pinned_eggsearch_agent_loop_uses_each_accepted_workspace_root`; `pinned_eggsearch_process_pair_is_compatible_offline` | Pass |
| Warm inventory is isolated by root and switching A→B→A does not reuse another root's view | Eggsearch `host_scope` unit tests; real AgentLoop A/B/A trajectory | Pass |
| Host scope is private and model-supplied scope cannot authorize access | Public schemas omit host scope; paired native call includes spoofed `host_local_scope` and returns only the accepted root; protocol metadata merge regression | Pass |
| Raw MCP on the managed child cannot inherit configured local roots | Real pair call without CodeGG workspace authority; no local file path/result; this test caught and verified the MCP `_meta` replacement fix | Pass |
| Missing, malformed, forged, null, or removed roots fail closed without startup-root fallback | Eggsearch `host_scope` unit tests; real child no-authority and local-disabled cases | Pass |
| Local-required no-match and disabled behavior do not become remote coverage | Real pair no-match, `include_local=false`, and disabled-local cases under `mode=off`; zero remote calls | Pass |
| Safe root/file containment is preserved | Existing Eggsearch local workspace traversal/symlink tests; scoped pair includes an out-of-root decoy and an out-of-scope decoy | Pass |
| Pinned version and real managed-child compatibility | Exact Eggsearch 0.4.4 binary built from commit `32f48bb57a43274d763c7f9ba919eab84d125edc`; version gate covers below-minimum, supported, and unsupported versions | Pass |
| Cancellation and child lifecycle remain bounded | Existing real MCP child timeout/reap tests, run by the paired qualification | Pass |
| Standalone Eggsearch and explicit remote MCP compatibility | Eggsearch absent-host-scope unit path retains configured roots; CodeGG remote path does not mint host scope | Pass, source and focused tests |

## 4. Verification

Passed on the implementation tree:

- `cargo test -p codegg --lib mcp::protocol::tests::modern_params_preserves_private_request_metadata_and_owns_protocol_fields --locked` — 1 test.
- Eggsearch `cargo test --locked --lib host_scope` — 2 tests, on its pushed implementation branch.
- `CODEGG_EGGSEARCH_BIN=<exact 0.4.4 binary> scripts/test-eggsearch-pair.sh` — passed provider/bootstrap/research/search adapter tests, fake MCP and mapping tests, child lifecycle tests, real pinned-child privacy/compatibility test, real AgentLoop A/B/A test, and `scripts/verify.sh quick`.
- `cargo fmt --all -- --check` and `git diff --check` — passed.

The first pair run failed on a stale 0.4.3 diagnostic assertion; corrected to 0.4.4. The next run caught the `_meta` loss and the final run passed after the normalizer fix. The first AgentLoop run exposed a missing provider fixture table; the corrected test passed. No live provider calls, real model, release publication, or SBC benchmark was run or claimed.

## 5. Authority, compatibility, and limitations

CodeGG remains the sole authority that selects a root for each accepted invocation. The process token is private to the managed child and is not rendered in logs, schemas, or model arguments. Eggsearch treats the host scope as transport metadata, authenticates it in constant time, and uses it only to construct a local retrieval view; its root-keyed cache carries no authorization state. Local search remains untrusted evidence. This does not add per-file ACL semantics: accepted workspace path policy authorizes the selected workspace root as a whole, matching the existing CodeGG workspace authority contract.

Eggsearch 0.4.4 is a compatibility floor for managed CodeGG installations. Standalone Eggsearch with no host metadata keeps its configured-root behavior. Raw tools on a CodeGG-managed child receive disabled local scope. Explicit remote MCP Eggsearch cannot supply filesystem scope. No schema, persistent authorization state, opaque-handle registry, or per-workspace child process was introduced.

## 6. Residual findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| None in C008 scope | Workspace authority is root-wide; CodeGG has no per-file read ACL in this contract | A caller accepted for a workspace receives that workspace's local retrieval scope | Keep the scope tied to the accepted path-policy identity/revision; any finer-grained ACL contract is separate work |

## 7. Unblock audit

C008 discharges the architectural blocker recorded in historical `002-status.md`: one managed Eggsearch process can now receive a trusted, per-invocation CodeGG workspace scope. It makes C002 eligible to resume its remaining acceptance matrix; it does not satisfy C002's owner/repository, alias, output-cap, and integration disposition by itself. C002 is therefore marked ready below. C007 remains blocked until C002 strictly closes. No other plan in this corrective line is newly eligible.

Historical C002 and C007 blocker records remain unchanged.

## 8. Registry and roadmap disposition

`plans/implementation/agent-tool-use-operability-post-audit-corrective/008-shared-eggsearch-host-scoped-local-retrieval.md` is implemented and strictly closed. `plans/registry.md` and the source addendum mark C008 closed and C002 ready; C007 remains blocked on C002. No C007 acceptance claim is made.
