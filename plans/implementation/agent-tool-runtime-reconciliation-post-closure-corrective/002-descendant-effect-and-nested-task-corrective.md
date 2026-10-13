# Agent Tool Runtime Post-Closure Corrective C002 — Descendant Effect Enforcement and Functional Nested Delegation

Status: ready for handoff

Repository baseline: `a54d18c7e7b92d1c9ac4971d7cdb8cc916418a5d` on `planning/agent-tool-runtime-reconciliation`; rebaseline before editing.

Source roadmap: `plans/subsystems/agent-tool-runtime-reconciliation-post-closure-corrective-addendum.md#4-milestones-and-dependencies`.

Original plan and historical closure:

- `plans/implementation/agent-tool-runtime-reconciliation/003-descendant-tool-authority-ceilings.md`
- `plans/closure/agent-tool-runtime-reconciliation/003-status.md`
- Related M001/M002 closures and existing `architecture/agent-tool-surface.md`, `architecture/tool_broker.md`.

Long-term requirements:

- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md#7-corrective-passes`.

Applicable ADRs: existing delegation, sandbox, job/grant and broker authority decisions; no new ADR if invariant repaired within these owners. If the intended inherited effect policy changes, stop and propose ADR.

Primary class: invariant.

## 1. Objective

Make the inherited parent tool authority executable and non-widening, rather than merely shaping model definitions: verify exact-name/capability/effect/expiry/workspace at broker admission; correctly expose and execute nested `task` where the parent authorized it; reject a forbidden child→grandchild path. Reconcile `ProcessExec` and legacy/system delegations without accidentally breaking authorized shell workflows or permitting wider effects.

## 2. Readiness and dependency

M003's typed snapshot, child transport, registry filtering, effect utility and broker grant machinery are already implemented. M002 provides catalog and external-tool identities. This correction does not depend on C001 retry work; C003 waits for both. Preserve other work on the branch.

## 3. Current-state findings and missed acceptance

1. In `src/agent/worker.rs` the child registry initially contains `TaskTool::default()` (no spawner). A functional nested `TaskTool` is constructed but held in `nested_task_tool` and installed **after** `ResolvedToolSurface::from_registry_with_parent_authority` returns. The resolver explicitly omits `task` if `has_functional_backend=false`. The child eligible set and provider definitions therefore omit legal nested delegation even when the role, parent ceiling and subagent config permit it. Existing `worker_child_withholds_nested_task` only covers denial; add a real successful nested path.
2. `ParentToolAuthoritySnapshot::check_tool` verifies name/effect/time/workspace, but `src/tool/broker.rs::validate_pre_execution` does not call it. Broker `allowed_tools` performs an independent exact-name check against a list derived from the snapshot, and existing `ToolAuthorityGrant` checks effect class against **its own** grant ceiling. Those are valuable layers but do not prove the inherited snapshot's effect ceiling remains binding when a contract changes.
3. `src/agent/tool_batch.rs::build_tool_execution_context` sets `parent_tool_authority` for `task` calls only; other child calls forward `None` to `BrokerInvocationContext`. Broker must receive the validated child snapshot on **every** child call, not only at delegation.
4. `mint_delegation_ceiling` stores `max_effect_class="non_idempotent"` without inspecting the authorized parent's actual eligible contracts. `ToolEffectClass::ProcessExec` ranks higher. A naive inherited effect check would refuse `bash`/`terminal` even when parent and child were independently authorized under their sandboxes. Derive an exact host-owned envelope from the parent-effective contracts and permission/sandbox ceiling; test both preserving permitted process execution and denying a widened effect.
5. `ParentToolAuthoritySnapshot` contains a plain SHA-256 self-fingerprint, not a keyed attestation. It detects accidental changes but is not a cryptographic authentication against a party that can rewrite both the snapshot and hash. Audit the current scheduler/DB/trusted transport boundary before relying on it as a forgery defense. The current model tool-call input has no authority field, which is helpful, but should not be described as cryptographic unforgeability.

These gaps were missed because tests primarily asserted denied names and manually called `check_tool`; they did not carry a normal child tool through the real broker under a changed effect contract, nor an authorized child→grandchild execution.

## 4. Invariants

- Every model-driven descendant execution carries host-authenticated/host-pinned identity, eligible **names**, typed capability ceiling, contract effect ceiling, lifetime, scope, and current grant; all are enforced at broker admission before side effects.
- Child effective tool set must be a subset of the immediate parent's effective set. A newly connected MCP tool, activated plugin tool, alias change, or name collision cannot expand it. No model-authored parent authority or `parent_id` string can mint an execution grant.
- Allowed nested `task` remains callable, registered, in the broker catalog, visible to `tool_search`/the provider and compatible with durable-run ownership; denied nested task is absent and uncallable.
- Effective `ProcessExec` authority is not inferred merely from a write-capability bit, and granting it does not relax the child `SandboxProfile`, path restriction, bash command approval or tool-specific permission.
- `None` is allowed only for reviewed **root/system** origin flows, never an implicitly unrestricted model-spawned descendant. Legacy scheduler payloads must not be silently laundered into model-driven root authority.
- Snapshot expiry, revocation, stale generation/activation and workspace-bound checks must fail closed before execution. Pre-existing approval grants are never silently re-issued.
- Existing ToolBroker and TaskTool remain the sole owners; do not add a second grant database, second delegation route or global authority cache.

## 5. Scope

### In scope

`ParentToolAuthoritySnapshot`, `AgentLoop` execution context, `ToolBroker::validate_pre_execution`, child `ToolRegistry` registration/surface order, `TaskTool`, worker/durable delegation admission; required tests, static guard and docs.

### Explicitly out of scope

Broad parent/child permission redesign; new identity provider or signing service absent a real trust boundary; full AgentRun schema migration without evidence; subagent UX redesign; new capabilities for Plan agents; changing shell regex policy, remote shell, Windows sandbox or Tool Program caller classes. C001 independently owns retries.

## 6. Required production changes

### Core/domain

Implement the effective child identity set as a host-owned validated snapshot (plus current grant). Use a canonical contract/effect lookup to derive the parent-authorized maximum effect rather than a hard-coded `non_idempotent` value. The snapshot's envelope must be specific to its exact named tools or a demonstrably safe parent maximum, not a blanket elevation. `ProcessExec` should be authorized only when the parent surface **and** canonical approval/sandbox context actually authorize a process-executing tool.

Audit the snapshot trust boundary: which actor can write serialized job payloads, at-rest tampering expectations, generation owner and rehydration authorization. Where payloads are trusted only because the scheduler writes them, document that assurance and verify scope/lineage before use. If untrusted cross-process actors can mutate snapshots, replace plain self-hash as an authenticity claim with existing daemon-owned authenticated authority or fail closed pending ADR.

### Storage/migrations

Prefer no schema change. Preserve additive `JobPayload::Subagent{parent_authority}` and `SubagentRun` fields and explicit `None` root/system case. If stronger run identity binding needs a versioned payload field, add migration tests for old JSON and restart reconciliation. No secrets, untrusted remote metadata or model arguments are persisted as grants.

### Protocol/DTOs

No new model argument or tool-name changes. Any new internal snapshot field must be additive to trusted transport, with integrity/fingerprint version handling; reject unknown/invalid effect spelling and stale authority rather than substituting a default grant.

### Runtime and concurrency — functional nested task

Restructure `src/agent/worker.rs` so the effective functional nested `TaskTool` is installed/recognized **before** surface/callability resolution, without carrying a broader snapshot than the validated parent.

One acceptable sequence: validate parent, build the candidate functional tool with already narrowed **parent** ceiling, register it, resolve child surface, then replace its authority with a second narrowed child snapshot **before** broker construction and ensure surface and catalog are recomputed/consistent. Another equivalent implementation is acceptable if test-backed. Do not install an unqualified functional task temporarily into a model-visible or concurrently executable registry. Maintain worker lock/discovery order and nested-run max-depth semantics.

Ensure exact denial when role lacks `task`, parent lacked `task`, no functional spawner/submission exists, depth exhausted, or process/worktree/sandbox grants exclude it. Avoid root/system compatibility bypass. An authorized child must be able to spawn an actual grandchild and await a typed completion in a deterministic fake-provider/spawner integration test.

### Runtime and security — broker enforcement

Thread `self.parent_tool_authority` from the child loop to **all** non-task direct executions, not just `task`. Preserve freshly minted *narrowed child* authority for nested delegation. At broker admission, validate the inherited snapshot's exact name, effect class, scope, expiry and generation where canonical owners can provide them; maintain the existing receipt/grant checks independently and reject if either set denies. Bind it to the current authoritative child/run identity; do not accept a snapshot simply because a caller supplies the string.

Avoid treating `BrokerInvocationContext.allowed_tools` as sufficient evidence of the snapshot's own effect ceiling; it is an exact-name list only. For process tools, compare the contract against the truly authorized effect envelope without changing sandbox containment. Explicitly test contract drift (same name, new effect severity), forged/stale snapshot, wrong workspace, mid-turn revocation, plugin and MCP identity changes and direct manual broker calls.

Use the existing `ToolAuthorityGrant` effect-checking path rather than implementing a second independent effect-ranking algorithm. If it is impossible to prove the origin of `parent_tool_authority` in generic BrokerInvocationContext, constrain validation to the AgentLoop child path with typed host provenance and explain the boundary; do not claim universal broker enforcement prematurely.

### Frontend/operator

Do not change the model tool schema. Diagnostic omission reason `ParentCeiling` must distinguish parent denial from nonfunctional `task`, without leaking the full authority snapshot or passwords.

### Documentation/static guard

Update `architecture/agent-tool-surface.md`, `architecture/tool_broker.md`, `architecture/tool.md`, `architecture/mcp.md` if the runtime contract changes. Strengthen `scripts/check_descendant_tool_ceiling.py` with an explicit guard for missing broker-bound child authority and registration-after-surface paths, plus executable positive/negative fixtures (guards alone are not proof).

## 7. Ordered work packages

### Work package A — Pin failure with a positive nested fixture

Intent: expose the registration-order regression before touching production.

Required changes: fake parent provider allows `task`; child role opts into delegation; parent ceiling contains `task`; functional spawner starts child and grandchild. Test that child provider receives `task`, broker has corresponding contract, grandchild starts, reports outcome, and no child can exceed parent. Also pin denial cases.

Acceptance evidence: failing pre-fix test, passing post-fix same fixture; no deadlock when nesting.

### Work package B — Fix authority/registry assembly

Intent: make callable registry and advertised surface identical.

Required changes: reorder/narrow `TaskTool` registration; re-evaluate broker snapshot creation so no stale default tool contract or inconsistent cache; add static guard.

Acceptance evidence: exact provider definitions, `tool_search`, child effect/category and broker contract identities all agree; no grandchild if any parent/role/depth gate fails.

### Work package C — Enforce parent effect envelope on every call

Intent: bind effective inherited contract authority to execution.

Required changes: thread validated child authority on direct calls, validate in broker or another single canonical admission owner, derive correct envelope from authorized parent effects, retain independent permission/sandbox checks.

Acceptance evidence: synthetic tool same name with `ReadOnly`→`NonIdempotent` or `ProcessExec` drift is denied before execute, even when existing `allowed_tools` includes name; legitimate authorized shell under a real sandbox still works. Report the effect classification of each fixture.

### Work package D — Durable compatibility and adversarial verification

Intent: preserve restart safety and prevent authority injection.

Required changes: validate rehydrated ceiling, wrong identity/workspace, tampering threat model, stale generation and activation; explicit system/legacy paths; update docs, guard, closure matrix.

Acceptance evidence: two-level scheduler payload roundtrip, denied forged child and no unintended tool exposure; no new persistent grant store.

## 8. Failure, cancellation, restart and contention

No child/grandchild is launched if either surface authorization or invocation admission fails; queued durable runs must fail or remain blocked with typed reason, not silently rerun as system/root tasks. Simultaneous siblings get isolated narrowed ceilings. Process cancellation/timeout releases locks and follows C001's no-blind-replay contract. A disconnected MCP server or changed plugin activation invalidates affected calls even with a previously pinned eligible name. A resumed durable child with missing/expired/unauthenticated authority must be handled under an explicit system-delegation rule only if its origin truly is system-owned; otherwise fail closed. A fresh root/user approved delegation mints a new snapshot.

## 9. Compatibility/migration

Current model `task` JSON stays unchanged; parent grant remains host-side. Keep permissions, sandbox worktree roots and user approval behavior. Maintain old payload decoding with explicit origin provenance, not naive null-to-root widening. Record any versioning changes and rollback. Do not relabel an `EffectClass::ProcessExec` grant as less dangerous solely to pass tests.

## 10. Required tests

- **Unit:** `ParentToolAuthoritySnapshot.check_tool` effect/name/scope/expiry/revocation; trusted carrier provenance; legitimate ProcessExec vs denied ProcessExec; snapshot/fingerprint/serialization.
- **Integration:** genuine authorized child→grandchild path, provider `task` advertised and invoked, denied nested counterpart, plan/read-only agents, MCP/plugin external tools, broker manual forged dispatch, resolved ToolCatalog.
- **Restart/recovery:** serialized scheduler `parent_authority` across process restart, absent old payload, revoked run, stale policy/descriptor and child worktree relocation.
- **Contention/cancellation:** nested sibling jobs and simultaneous policy drift; no authority cross-contamination or orphaned tasks.
- **Security negatives:** same name but raised effect class, mismatched parent principal/scope/lineage, name and alias collision, self-hash replacement, model-controlled authority field injection, expired snapshot.
- **Compatibility:** existing `agent_tool_m003_descendant`, `subagent`, `tool_broker_integration`, `agent_loop_harness`, sandbox, plugin and fake MCP tests.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test -p codegg --test agent_tool_m003_descendant
cargo test -p codegg --test subagent
cargo test -p codegg --test tool_broker_integration
cargo test -p codegg --lib -- agent::parent_authority
cargo test -p codegg --lib -- agent::tool_surface
cargo test -p codegg --test agent_loop_harness
python3 scripts/check_descendant_tool_ceiling.py
cargo clippy --workspace --all-targets --locked -- -D warnings
bash scripts/verify.sh quick
```

Add the new positive nested/contract-drift tests to canonical targets above or name a dedicated target and include it explicitly. Record exact executed selectors and failures. Additional `server,plugins,lsp-test-support` feature tests in C003.

## 12. Documentation updates

`architecture/agent-tool-surface.md`, `architecture/tool_broker.md`, `architecture/tool.md`; operational delegate/grant notes if needed. Historical M003 closure stays unchanged.

## 13. Acceptance criteria

1. Positive root→authorized child→authorized grandchild trajectory succeeds, with actual `task` in child model surface, registry, `tool_search` and broker.
2. Unauthorized nested delegation is hidden and uncallable at every layer. Unknown post-launch plugin/MCP names cannot become authorized.
3. A child tool with widened effect compared to its host-attested inherited envelope is denied **at invocation** before its execute method runs, even if its name is eligible.
4. Authorized ProcessExec/shell operations remain available under the unchanged approval and sandbox policy, while process execution without parent effect authority is denied.
5. Genuine forged/stale/cross-scope grant negatives pass and the attestation threat model is precise. Model arguments cannot supply the trusted authority.
6. Test matrix and guards green; durable compatibility and no unexplained snapshot-default widening.

## 14. Stop conditions

Stop for need to broaden parent sandbox, unreviewed `any` effect grant, inability to authenticate cross-process authority, unresolved durable origin semantics, incompatible nested task registration lifecycle, or any requirement for a new permission owner. Seek an ADR or bounded successor rather than weaken fail-closed handling.

## 15. Closure evidence

`plans/closure/agent-tool-runtime-reconciliation-post-closure-corrective/002-status.md` must record implementation SHA(s), before/after positive nested fixture, broker-denial effect fixture, grant/workspace/shell compatibility, trusted-transport review, restart tests, exact commands/guards and outstanding risk. C003 waits on accepted closure.

## 16. Handoff notes

Do not simply set `max_effect_class` to `any`, nor pass `Some(parent_authority)` only for `task`. A snapshot's SHA-256 is an integrity *checksum*, not cryptographic sender authentication absent a trusted store/channel. Avoid relying on a mock root child with no functional spawner as proof of nested delegation. Preserve independent C001 retry modifications.
