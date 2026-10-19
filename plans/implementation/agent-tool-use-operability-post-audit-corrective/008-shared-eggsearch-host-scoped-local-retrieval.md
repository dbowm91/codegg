# Agent Tool-Use Operability Corrective C008 — Shared Eggsearch with Host-Scoped Local Retrieval

Status: active implementation (2026-10-19)
Repository baseline: CodeGG `d44da50a66f9774e0c9c794b388deae05cb8ac48` on `plans/agent-tool-use-operability-20261019` (2026-10-19); re-check both repositories before editing. Eggsearch source inspected at pinned v0.4.3 `5e7b853e7eebf29c58f84959805c8774279d7ea4`.
Source roadmap: `plans/subsystems/agent-tool-use-operability-post-audit-corrective-addendum.md#7-milestones`.
Predecessor: C002 `plans/implementation/agent-tool-use-operability-post-audit-corrective/002-live-workspace-repository-search.md` and immutable blocker evidence `plans/closure/agent-tool-use-operability-post-audit-corrective/002-status.md`; downstream C007 `007-real-agent-tool-trajectory-qualification.md` remains blocked.
Long-term requirements: `plans/000-long-term-specification.md#26-reliability-and-recovery`, `#28-observability`, `#29-system-invariants`; `plans/001-terminology-and-domain-model.md`; `plans/003-planning-process.md`.
Applicable ADRs: ADR-0001 (programmatic authority), ADR-0004 (approval routing), ADR-0028 (execution authority) as already applicable. This handoff keeps authorization ownership unchanged. Stop and propose a separate ADR if work requires changing public authentication/authorization semantics, adopting shared remote MCP authority, or a durable public protocol rather than the bounded private CodeGG/Eggsearch adapter extension described here.
Primary class: infrastructure.

## 1. Objective

Discharge the **architectural interface blocker** of C002 with **one daemon-managed Eggsearch process** that performs local retrieval strictly within the workspace scope resolved and authorized by CodeGG *for each invocation*. Reuse Eggsearch's existing `LocalWorkspaceBackend`, inventory construction/cache, bounded parsing and safe file-opening; do not launch one Eggsearch instance per session/worktree and do not build a competing index or a second permission engine. Deliver executable normal-path evidence of A/B workspace isolation so C002 can resume its remaining capability tests.

## 2. Why this milestone is ready

C001 broker accepted authority is strictly closed; C003/C004/C005/C006 are also closed. C002's 2026-10-19 blocker audit established the missing session-to-local-roots contract, rather than a defect in `include_local` forwarding. User-selected direction is the shared-process, host-authorized per-request scope; implementation mechanics may be chosen against current code. Interface order: freeze the small CodeGG↔Eggsearch private transport contract first, then integrate in both repositories, then run pair and AgentLoop qualification. No Eggsearch M002/M004 semantic/evidence-roadmap closure is a hard prerequisite. **C008 is an implementation-ready successor; C002 and C007 are not themselves unblocked by the existence of this plan.**

## 3. Current implementation evidence

- CodeGG `src/tool/repo_search.rs` has `execute_structured(input, _ctx)` and uses an immutable `SearchRuntimeContext`; its current adapter `src/search_backend/eggsearch.rs::call_repo_search_structured` forwards `include_local` but has no trusted invocation workspace. `SearchRuntimeContext` in `src/search_backend/context.rs` carries search config and a shared daemon-owned `McpService` handle, not an authorized workspace selection.
- CodeGG's broker execution context already carries session, cwd, effective accepted permission, and workspace authority; C001 fixed the Tool Program propagation. **A caller-provided cwd is not itself authorization**. Resolve through the daemon workspace/project/session service and applicable `file.read` / tool authority at the same trusted boundary used by native workspace tools.
- Eggsearch `src/mcp/tools/repo_search.rs` routes to `ServerState.local_backend`; `src/mcp/state.rs` keeps a shared local backend/inventory cache constructed from startup `[local].roots`. `src/meta/local_backend.rs::LocalWorkspaceBackend::new` canonicalizes roots and owns an `Arc<RwLock<Option<WorkspaceInventory>>>`. Inventory building is Git-aware, cached and bounded; `src/meta/safe_open.rs` performs race-resistant relative file opening. `src/meta/local_inventory.rs` and local `repo_fetch`/`repo_map` use the configured inventory.
- Pinned v0.4.3 offline `scripts/test-eggsearch-pair.sh` proves one configured local root, no remote calls and an excluded out-of-root decoy. It does **not** demonstrate session switching, parent/subagent scope, cross-root isolation, or dynamic retrieval context.
- A local search is read-only in its outward contract, but may expose confidential bytes. Eggsearch's path-containment protections therefore remain necessary. They are not a second user authorization system.

## 4. Invariants that must not regress

1. **CodeGG alone authorizes callers and selects scope.** Permission identity, project/session/workspace membership, subagent inheritance and per-tool rights derive from daemon/broker-owned data. No model-provided path, locator, `cwd`, `workspace_id` or argument can mint read permission.
2. **Eggsearch enforces the supplied scope**, without independently deciding user/project ACLs: path traversal, symlink escape, hidden/ignored-file exclusions, file-size, read/count/deadline bounds and safe opening still apply. A root grants search coverage only if CodeGG's accepted policy allows that *entire* root; finer file-read restrictions require enforced filtering or explicit unavailability, never silent widening.
3. A host-scoped request selects one immutable effective root/config view per call. No mutable process-global `active_workspace` and no cross-request root swap; concurrent parent, child and different projects cannot influence each other's selection or results.
4. `include_local=false` never searches local. Explicit local-only/remote-denied operations never escalate to network providers; a missing/expired/unauthorized root yields a bounded, truthful error/status, not an empty "searched locally" success or remote substitute.
5. Only **trusted CodeGG-managed stdio/explicitly private transport** may set host scope. `schemars(skip)`, a hidden JSON field, or an opaque string **alone is not authentication**. A raw MCP tool exposed to a model or an arbitrary client must not acquire filesystem authority by supplying the field.
6. Search snippets remain data, not instructions. Preserve provenance, caps, cancellation, and identity/permission audit boundaries. "One process" is not OS isolation between mutually hostile principals; stronger tenant isolation is a separate OS execution policy.
7. Keep C001/C003/C004/C005/C006 strict closures and the immutable C002/C007 blocker records intact.

## 5. Scope

In: the trusted session→workspace root resolution on CodeGG's invocation path; a small private CodeGG-to-pinned-Eggsearch scope contract; Eggsearch's per-request local backend selection and bounded per-root cache; local scope propagation to repository search and any local fetch/map paths; truthful diagnostics; focused offline + real broker/AgentLoop qualification; docs and registry.

Out: general ACL/RBAC, OAuth, tenant provisioning, filesystem grants, multi-client network MCP authorization, externally routable remote Eggsearch, a sidecar for every subagent, new persistent search index/database, new LSP/symbol engine, Eggsearch E-M002/E-M004 general ranking/projection, new daemon scheduler, broad model benchmark loop, live web CI or SBC compile tests.

## 6. Required production changes

**CodeGG core/domain:** At the trusted tool admission or authorized workspace-service boundary, derive a typed, immutable `AuthorizedLocalSearchScope` (conceptual type: canonical root + workspace association + effective read-policy constraints + request trace). Verify the caller may use local repository search and read the *whole* root covered by the backend; otherwise return a typed scope-denied/unavailable outcome. Session and subagent selection must follow their actual effective workspace, not a process bootstrap directory or model field. Model JSON must never contain authoritative scope.

**CodeGG tool/adapter:** Pass the trusted selection through `RepoSearchTool::execute_structured`, the search adapter and the same daemon-owned `McpService`. Separate caller filters (`query`, `path`, `language`, `include_local`) from host metadata. Support "local required" explicitly for offline qualification; never silently mix wrong roots. Ensure `codesearch` claims only its supported semantics. Preserve existing broker/permission ordering and supported remote-only behavior.

**Eggsearch core:** Provide a per-invocation local scope selection for the existing backend, using a bounded in-memory map of `LocalWorkspaceBackend`/inventory instances keyed by canonical root **and indexing configuration** (include policy/config fingerprint where material). Initialize lazily, share work safely for the same root, prevent cache collisions and cross-workspace source-card contamination. Add simple bounded eviction (e.g. explicit max entries plus idle eviction), not a new indexing subsystem; measure before introducing complex invalidation. Select local scope for `repo_search` and the local branches of `repo_fetch` and `repo_map` so discovery-to-follow-up cannot drift to a differently configured root. Standalone Eggsearch without host scope retains its existing `[local].roots` behavior.

**Private wire contract:** Freeze an additive, version-gated, host-injected request field or private adapter method (e.g. `host_local_scope` carrying a canonical root plus optional effective policy descriptor). This example is **not** a model-visible input-schema proposal. Decide whether the request field is permitted only on a trusted locally spawned child connection; raw/public/direct MCP entry points must reject or ignore untrusted attempts to select an arbitrary root. Do not claim this is enforced by hiding a schema key. If one shared Eggsearch service must accept mutually untrusted clients, STOP: require authenticated caller binding/capability-based scope registration and an ADR before broadening this milestone.

**Storage / migration:** No schema migration, durable authority registry, new persisted credential or per-root on-disk index. Existing in-process bounded inventory/cache remains ephemeral.

**Operator surface:** Local-disabled, no index, no match, denied/missing scope and backend unavailable must be observably distinct; disclose local root only to an authorized diagnostic caller. Preserve remote routing/provenance status.

**Docs/guards:** Update CodeGG `architecture/search_backend.md`, `architecture/tool.md` and `docs/tools.md` as appropriate; Eggsearch `architecture/local-workspace.md` and integration guide. Add focused contract/security regressions rather than broad new scanners.

## 7. Ordered work packages

### A — Freeze authority and transport ownership
Intent: small, testable internal interface.
Required changes: trace the actual AgentLoop→broker→`RepoSearchTool` path, daemon workspace lookup, delegated subagent authority, raw MCP exposure paths and shared stdio ownership. Choose and document how the host-only local scope is delivered, including absence/denial behavior and public/standalone compatibility. Prove model args cannot override trusted scope; root is canonicalized under CodeGG's existing accepted path/read policy.
Evidence: table of trusted/untrusted ingress paths; specific test showing an injected model `host_local_scope` is ignored or rejected. Stop on unresolved authorization or authenticated shared-HTTP requirement.

### B — Implement scoped backend selection in Eggsearch
Intent: one process serving concurrent roots.
Required changes: add a bounded cache/factory around existing local backend, selecting an immutable view for each invocation; avoid cloning the entire server state, swapping globals or one process per root. Key index/cache by effective root/config and honor existing invalidation/freshness. Thread request-local inventory through `repo_search` plus local `repo_fetch`/`repo_map` branches; use existing safe-opening functions.
Evidence: two roots with disjoint sentinels; concurrent calls return only matching root's sentinel, including warm-cache inversion and eviction/restart.

### C — Wire through CodeGG's real accepted caller
Intent: authoritative normal agent execution.
Required changes: carry the daemon-authorized root through the tool execution context to the private Eggsearch request without widening model schema. Exercise session switching, inherited/restricted subagent execution and local-only mode. Preserve remote-only and standalone behavior.
Evidence: scripted AgentLoop/broker calls backed by **one** pinned real Eggsearch child; outputs and identities asserted on model-visible results. A model-provided absolute path/`cwd` cannot retarget local retrieval.

### D — Qualify status, failure, and closure gate
Intent: resolve architectural blocker with evidence.
Required changes: test absent scope, denied `file.read`, root removal, symlink escape, stale session/workspace, disabled local, no match, child death, cancellation, timeout and simultaneous A/B/root-reuse. Confirm explicit remote mode routing never leaks private query to web under local-required policy. Update integration compatibility pin only after independently testing upstream Eggsearch revision.
Evidence: focused passing commands and exact CodeGG/Eggsearch SHAs; closure at `plans/closure/agent-tool-use-operability-post-audit-corrective/008-status.md`. Once C008 closes, resume C002's *remaining* original acceptance matrix and create a **new** C002 follow-up closure/disposition record under planning governance (do not rewrite historical `002-status.md`); only C002 strict closure unblocks C007.

## 8. Failure, cancellation, restart, and contention semantics

Request-local root selection is immutable after admission; an active call may complete only under its accepted authority and inherited deadline. If current CodeGG policy requires revocation at execution time, honor that policy instead of caching authorization with the inventory. Cache holds search data/index only, never transferable read permission. Session switch/root replacement invalidates the *selection for future calls*, not by mutating another caller's active state. Root disappearance, changed canonicalization, missing grants or policy mismatch fail explicitly; do not silently search a default root. Child restart rebuilds transient caches and renegotiates compatibility without preserving authority in Eggsearch. Bounded admission/eviction prevents per-root unbounded memory or repeated cold-index storms. Cancellation must not turn into a later search on a different root. Preserve returned partial/truncation telemetry where supported.

## 9. Compatibility and migration

One managed stdio Eggsearch child remains canonical for CodeGG. Standalone Eggsearch and existing static `[local].roots` requests remain compatible; remote-only queries remain available per existing policy. CodeGG host-scoped calls require a confirmed sidecar protocol version/capability; on older v0.4.3 without scope support, report `local_scope_unsupported` / local unavailable rather than fall back to global roots. No hidden security-by-schema assumption, no externally exposed new path-selection API, no persistent cache migration. Defer opaque handles: if lifecycle, multi-client authorization, or transport requirements justify them, specify binding/authentication and a separate ADR instead of smuggling them into this implementation.

## 10. Required tests

**Focused:** Eggsearch two-root backend selection, key isolation under config differences, warm/cold/eviction, stale/missing root, unsafe symlink and traversal, untrusted host-field rejection; CodeGG authorized scope extraction, denied narrow `file.read`, subagent inheritance and model argument override negatives.
**Integration:** One real pinned Eggsearch process, two CodeGG workspaces with unique fixture symbols/decoys, concurrent parent+child and workspace switching, positive `repo_search` followed by **local** `repo_fetch` and map where supported; model-visible provenance and result bounds.
**Failure/recovery:** Child crash/restart, missing index, local-disabled, no-match, cancel/deadline, stale session, root delete/recreate, cache invalidation; zero remote calls in local-required mode.
**Compatibility:** Standalone static-root behavior and old pinned binary explicit unsupported; no changes to existing v0.4.3 provider semantics unless versioned.
**Security:** Model spoofing attempts, raw/direct MCP access check, cross-project A/B no-leak; prove read-only scope cannot be widened by a source-card locator, symbol/path filter or follow-up fetch.

## 11. Required verification commands

At implementation, discover actual feature/target names and run the narrow equivalent of:
```bash
# eggstack/eggsearch (on implementation revision; no optional global feature sweep)
cargo test --locked --lib meta::local_backend
cargo test --locked --lib mcp::tools::repo_search
# codegg
cargo test --locked -p codegg --lib tool::repo_search
cargo test --locked -p codegg --test eggsearch_pair_compat
cargo test --locked -p codegg --test agent_loop_harness
scripts/test-eggsearch-pair.sh
scripts/verify.sh quick
```
Commands are handoff targets, **not executed in planning**. If crate/test names differ, record the corrected exact commands. The paired test must use the **new** explicitly built Eggsearch revision; do not claim the old pinned v0.4.3 demonstrates request scoping. No `--all-features` workspace sweep, public network CI or SBC startup benchmark.

## 12. Documentation updates

Update ownership and protocol truth in CodeGG `architecture/search_backend.md`, `architecture/tool.md`, `docs/tools.md` and Eggsearch `architecture/local-workspace.md` / `docs/codegg-integration.md` (actual files/paths verified at implementation). Update pinned compatibility/version docs if upgraded. Keep Eggsearch general evidence M002/M004 independently owned. If an ADR gate becomes necessary, link it explicitly before implementing the changed security boundary.

## 13. Acceptance criteria

1. With **one** Eggsearch process and two authorized CodeGG workspace sessions, concurrently searching distinct unique markers returns only results under each caller's root, including warm-cache and later session-switch cases.
2. Caller lacks root-wide read rights, provides a spoofed root, uses stale authority or bypasses the CodeGG host path: never obtains another workspace's bytes; no model-visible raw path parameter grants authority.
3. Local search followed by local fetch/map honors the same scope; absence/disabled/no-match/transport failure are explicit and cannot masquerade as other-root coverage or web fallback.
4. Existing bounded-safe read, Git inventory, no-follow symlink and remote-provider policies remain; standalone configured-root operation still works.
5. CodeGG and Eggsearch implementation SHAs, real-child model-call evidence, version gate, focused and quick verification recorded. No claim of full C007 closure or real-model endurance.

## 14. Stop conditions

Stop and report if: a proposed request field is publicly callable without a trusted transport/authorization guard; a root cannot represent CodeGG's effective read policy and filters cannot faithfully narrow it; separate tenants must share unauthenticated HTTP; implementation requires changing accepted ADRs, introducing a general Eggsearch authorization service, or ignoring C002's privacy concern; a needed Eggsearch revision cannot be built/tested; code diverges from the inspected source contract. Prefer a narrow alternative with an explicit maintainer decision rather than inventing hidden credentials, weak tokens or speculative persistence.

## 15. Closure evidence required

Create `plans/closure/agent-tool-use-operability-post-audit-corrective/008-status.md` with exact code commits/PRs in **both repositories**, requirement-to-evidence matrix, original failure vs fixed normal-path trace, all commands run (pass/fail/unrun), limits, migration/protocol compatibility, no-index and cross-workspace tests, authority/raw MCP threat assessment, cancellation/restart/concurrency, residual severity and registry/roadmap unblock audit. Do not edit C002's or C007's historical blocker records. C008 closure makes C002 resumption eligible; C007 remains blocked until C002's remaining capability tests strictly close.

## 16. Handoff notes

Implementation is a coordinated two-repository vertical slice, not a request to create multiple sidecars. Limit cross-repo edits to Eggsearch's existing local-search machinery and CodeGG's existing accepted execution path. Avoid creating a generic multi-tenant policy engine inside Eggsearch. Root-keyed inventory caching is an implementation detail, not an authorization cache. Coordinate changes to `plans/registry.md` with other active worktrees; pin the Eggsearch revision before declaring CodeGG pair compatibility. Prefer honest partial closure with one named blocker over overstating the production privacy contract.
