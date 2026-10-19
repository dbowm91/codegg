# Agent Tool-Use Operability Corrective C002 — Restore Truthful Live-Workspace Repository Search

Status: blocked — Eggsearch has no session workspace binding contract; see `plans/closure/agent-tool-use-operability-post-audit-corrective/002-status.md`.
Repository baseline: `4c86bac2a608dd717be085247838fd2f37d81ab6` (2026-10-19 main)
Source roadmap: `plans/subsystems/agent-tool-use-operability-post-audit-corrective-addendum.md#7-milestones`
Long-term requirements: `plans/000-long-term-specification.md#26-reliability-and-recovery`; `#28-observability`; `#29-system-invariants`; `plans/001-terminology-and-domain-model.md`.
Applicable ADRs: ADR-0001 programmatic authority; ADR-0004 approval routing; ADR-0028 execution authority (only where applicable). No new ADR required if existing ownership remains unchanged.
Primary class: capability

## 1. Objective
Restore Truthful Live-Workspace Repository Search. Close only the specified real-model operating contract; do not conflate source review with successful runtime reproduction.

## 2. Why this milestone is ready
No unresolved hard dependency: existing tool, authorization and test seams are present. Interface dependencies are current repository contracts; re-check their SHAs at implementation.

## 3. Current implementation evidence
- `src/tool/repo_search.rs` takes `include_local`; `src/search_backend/eggsearch.rs::call_repo_search_structured` copies it. Real-child offline private fixture in `tests/eggsearch_pair_compat.rs` proves local `local_workspace` retrieval for pinned eggsearch v0.4.3. Production report found no local hit for existing `canonicalize_workspace_root`, instead large remote hits. `src/tool/codesearch.rs::request_input` reconstructs query/profile/max_results only, dropping any local controls. Exact runtime child config/root/coverage remains unverified.

## 4. Invariants that must not regress
- Search does not silently broaden from local-required to remote. Local index failure, no match, policy-off and remote fallback are distinguishable. Eggsearch owns provider routing/index; CodeGG owns runtime workspace binding, argument translation, provenance and model-facing disclosure. Untrusted remote content remains framed and bounded.

## 5. Scope
In: source paths and bounded tool-contract behavior listed below, focused regressions, model-facing diagnostics, and affected docs.
Out: unrelated runtime refactors, new global scheduler, network CI, public release and SBC compilation/startup benchmarks; do not weaken authorization or workspace containment.

## 6. Required production changes
Runtime: verify the effective daemon-owned `SearchRuntimeContext`, child version/env/local root, and workspace-binding behavior for each session and subagent; check repository locator vs local-root interaction. Adapter: preserve exact semantics of `include_local` rather than reimplementing index/search. Alias: either explicitly accept/forward supported structured filters in `codesearch` or narrow its description/schema/visibility so unsupported `include_local` is never implied. Result framing: show bounded local-coverage and remote-routing reason; coordinate relevance-budget work with eggsearch evidence M004, do not reproduce its full structured-projection milestone.
Storage: no new schema or migration expected; any proposed persistence change requires a new documented contract.
Protocol: preserve existing names/fields except the explicit compatibility corrections below; no remote protocol migration.
Security: no model-supplied source may grant privilege; fail closed on an absent required authority.
Documentation/static guards: update only owning source docs and focused tests/guards; do not duplicate existing cross-cutting scanners.

## 7. Ordered work packages
### A — Reproduce production-local mismatch
Spawn the real pinned sidecar using runtime startup configuration, one unique in-workspace symbol and one out-of-root decoy. Capture effective root, providers_queried, routing decision and truncated metadata with no secrets. Compare with the existing private-child fixture.
### B — Correct root/filters/alias
Repair only proven CodeGG wiring/validation issues; require an explicit local-only policy/error if no local engine can run; avoid unexpected public search. Distinguish `repo: owner/name` remote locator from attached workspace without silently retargeting searches.
### C — Compact output contract
Preserve structured results and error/coverage diagnostics under output cap. Avoid byte-prefix-only dumping for this immediate search path, but defer generalized scoring and next-action projection to evidence M004.

## 8. Failure, cancellation, restart, and contention semantics
Sidecar disappearance/disablement returns typed unavailability, not an empty successful local result. Cancellation/timeouts bound child call; stale workspaces must not leak old local roots after session switch. Under no circumstances perform live web provider network calls in offline qualification.

## 9. Compatibility and migration
Existing native repo_search schema remains; `codesearch` is documented as compatibility alias, and either forwards supported args or rejects/does not advertise them explicitly. If changing publicly visible semantics/fields requires new upstream contract, stop and coordinate eggsearch M002/M004 rather than guessing.

## 10. Required tests
- Real pinned v0.4.3 private-child positive local symbol, negative out-of-root decoy, no-index/no-match/disabled-local and zero-remote local-only tests.
- Runtime-startup session/workspace binding, subagent root, owner/repo filter and providers_queried matrix.
- Codesearch alias schema/forwarding test and 15k cap result coverage with relevant hit preserved where available.
- Security: SSRF/untrusted provenance/no implicit remote escalation.

## 11. Required verification commands
```bash
cargo test -p codegg --lib tool::codesearch
cargo test -p codegg --lib tool::repo_search
cargo test -p codegg --test eggsearch_pair_compat
scripts/test-eggsearch-pair.sh
scripts/verify.sh quick
```
These are required for implementation closure when applicable, not claims about work run while authoring the plan. Use `--locked` and no `--all-features` unless explicitly justified for a narrow target; no required live-provider or SBC run.

## 12. Documentation updates
`architecture/search_backend.md`, `docs/tools.md`; link active eggsearch evidence M002/M004 and do not claim their closure.

## 13. Acceptance criteria
- A genuine attached-workspace search finds a unique newly created symbol with a correct path; no previous read required.
- A disabled or missing local index surfaces an explicit status; remote fallback cannot masquerade as local coverage.
- The model receives bounded, actionable results and truthful alias capabilities.

## 14. Stop conditions
If local-provider defect is upstream of CodeGG v0.4.3 and cannot be repaired without a separate eggsearch revision or the required offline fixture would initiate remote network traffic; write an upstream blocking record.
In all cases stop rather than silently changing accepted ADRs, inventing authority, dropping scope requirements, rewriting historical closure or creating unbounded work.

## 15. Closure evidence required
Exact implementation commit and branch; before/after failure trace; requirement-to-test matrix; tests/guards actually run (and explicitly unrun); negative security cases; contention/restart treatment; known limitations with severity; `plans/closure/agent-tool-use-operability-post-audit-corrective/002-status.md` and registry/roadmap status update and unblock audit.

## 16. Handoff notes
Treat the audited production miss as reproducible symptom, not proof that `include_local` is discarded by `repo_search`. Do not add a second local index or persistent cache. Keep E-M004 semantic projection ownership untouched.
