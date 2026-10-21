# Managed Agent Scratch Spaces Roadmap

Status: conditionally closed — M004 session/agent-private reclamation is closed, and M005 (the temp-root redirect offer) is closed on both hosted CI and a live observation with a real model. M006 (the scratch usage census) is closed and is measurement only: it reclaims nothing. M003 remains conditionally closed pending **Windows** native sandbox qualification; the **Linux** half is now closed by the post-closure corrective addendum's C001.

Post-closure qualification successor: `plans/subsystems/managed-agent-scratch-native-qualification-corrective-addendum.md` (C001 Linux native Required **closed** with native evidence; C002 Windows native Required **blocked** with a recorded stop condition). This addendum does not reopen M001/M002/M004, Windows shared cleanup, or already-green PR #128 CI.

Planning baseline: `c0d6ff2d324ee37005e828a5bf269feebfabff79` (main inspected 2026-10-16).

Long-term references:
- `plans/000-long-term-specification.md` §§4.2–4.4, 9, 16–18, 24, 26–29 (ownership, paths as locators, daemon authority, reliability and security).
- `plans/001-terminology-and-domain-model.md` §§5–8 (execution context, session/turn, agent run, job/attempt).
- `plans/002-long-term-roadmap.md` Phases 9–11 and 19.
- `plans/003-planning-process.md`.

Applicable accepted decisions: `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`, `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`, and ADR-0028 command authorization (inspect exact filename and authority before editing policy).

Related owners: `architecture/process-tool-execution-ownership.md`, `docs/execution-ownership.toml`, `architecture/jobs.md`, `architecture/agent.md`, `architecture/permission.md`, `architecture/security.md`, and `plans/subsystems/agent-workstream-blackboard-roadmap.md`.

## 1. Purpose and ownership boundary

Give every daemon-owned model session a stable, private, disposable scratch path, permit optionally distinct scratch for each delegated agent run, and let orchestrators intentionally share selected scratch resources with named children/jobs. Scratch is a **managed resource** identified by a CodeGG-generated ID; paths are non-authoritative locators. Avoid unauthorized out-of-workspace writes, needless human approvals, global /tmp pollution and accumulation. This is a portable cooperative convention coupled to **existing** tool/OS authorization, not a new containment backend.

Daemon session/run resource ownership determines allocation and reclamation. Existing ToolBroker, JobSubmissionService/JobScheduler, RunStore, delegated agent/run service, ManagedProcessService and OS sandbox remain the only existing authorities for their respective domains. No separate scheduler, permission engine, or filesystem virtualizer.

## 2. Work classification

**Invariants:** grants are scoped to authenticated session/agent/job owners and are non-transferable without explicit bounded delegation; children receive no broader inherited rights; sandbox-required execution never weakens or silently falls back; no path alone grants authority; workspace and isolated-child-worktree ceilings are not redefined.

**Capabilities:** private scratch exposed in prompts and process environments; optional share by scratch-resource ID at child spawn; permission recovery hints for unauthorized /tmp use.

**Infrastructure:** collision-resistant directory provisioning; manifest/lease registry; process-launch environment and OS backend grant wiring; recovery-safe garbage collection.

**Polish:** usage reporting, diagnostics, retention policy, documentation and optional operator inspection. These do not count as complete until user-visible paths are exercised.

## 3. Non-goals

No dedicated model-facing `scratch` tool initially; no mount namespace, /tmp spoofing, LD_PRELOAD, symlink hack, Windows junction trick, parser/regex rewrite, command semantic classifier or implicit approval; no global shared /tmp allowlist; no automatic Git/Cargo target/cache relocation; no new artifact transfer service, remote-node scratch replication, arbitrary cross-session share, general-purpose quota filesystem, or global sandbox expansion.

## 4. Current state and source evidence

- `src/managed_process.rs` already creates a **fresh private 0700 TempDir per Required-sandbox launch**, adds it to sandbox read/write paths, sets TMPDIR, and drops it after process reaping. This must **not** simply be deleted or confused with persistent session resources.
- `src/python_script/executor.rs` creates independent per-run Python script scratch with its own sandbox paths; `src/tool/bash/process.rs` creates short-lived Windows shell temp directories. These are compatibility seams needing reconciliation, not evidence that durable shared scratch already exists.
- `src/security/sandbox/policy.rs` explicitly omits shared scratch and global package caches from Unix write allowances; Windows TEMP/TMP authority is not a qualified Required isolation guarantee.
- `src/agent/worker.rs`, `src/tool/task.rs`, `src/agent/loop.rs` and `crates/codegg-core/src/agent_run` own delegation and run identity; `src/scheduler/` owns admitted background jobs. `ToolBroker` is the sole model tool-call boundary.
- `src/managed_process.rs::EnvironmentPolicy` clears and rebuilds a sanitized environment, so setting vars in the daemon parent process is insufficient. Explicit context propagation is required.
- Relative filesystem-tool paths are workspace-root-relative; shell `workspace_root` and `child_workspace_ceiling` are independent. Structured filesystem tools validate paths in `src/tool/util.rs`; unsandboxed subprocess writes are not observable through those checks.

No verified session/agent-scoped managed scratch service, durable share registry, or unified tool/agent prompt contract exists at this baseline.

## 5. Target architecture

A daemon-owned `ScratchManager` (name illustrative) allocates owner-private directories under a CodeGG-controlled per-user **persistent managed data/state root**, not the repository, shared host /tmp, or an unreliable process cwd. Use secure per-user parent permissions and opaque unpredictable IDs; choose canonical platform-appropriate paths. The persisted lookup identity is `ScratchId`; `session_id` / optional `AgentRunId` / authorized consumer records determine access. Resource records may be in existing session storage or an owner-only sidecar manifest if that suffices for robust restart reconciliation—do not assume a new database migration is necessary.

Ownership example (schematic, not a required on-disk schema):

```text
<codegg-owned>/scratch/
  <session-id>/
    <private-id>/
    <agent-run-id>/
    <shared-id>/
```

Every top-level model session obtains stable private scratch. Every delegated agent run obtains its own private scratch by default; it may optionally accept **explicit, capability-checked** shared `ScratchId` references assigned by its orchestrator. A shared resource is not made global or implicitly granted to all siblings. `CODEGG_SCRATCH_DIR` and `TMPDIR`/`TMP`/`TEMP` identify the **private** scratch root for managed model subprocesses; shared resources receive bounded aliases or typed spawn metadata, never override standard temp defaults. Prompt supplies the private path and small list of authorized shared aliases, with no per-call retrieval requirement. Maintain native-path semantics; path escaping/quoting and Windows non-Unicode/long-path cases require tests.

An OS-constrained attempt may retain a narrower **launch-local subordinate scratch** for security/mutability compatibility, even if its model-visible stable scratch is shared across invocations. Contract must identify which variables and filesystem roots are actually usable within that attempt. Native sandbox path grants must reflect authorized CodeGG resources only (and preserve existing explicit denies); if a platform cannot represent a necessary grant, the Required launch fails rather than claiming scratch support. Unconstrained executions have convention/structured-tool grants, **not** OS-enforced isolation.

Temporary file lifetime is session/agent-run scoped, not tool-call scoped. Background attempts hold leases until reaped or durably terminal; shared resources remain while any authorized active consumer/owner lease needs them. Restart reconciles retained resources against authoritative session/run/job state and a bounded retention grace period. Cleanup is safe against symlinks, junctions, path replacement, concurrent cleanup, root confusion, and stale instances; no background cargo build can lose scratch because the foreground call returned.

Separate disposable scratch from `target/`, Cargo caches and persisted RunStore artifacts. An orchestrator can place exchangeable build outputs/logs there intentionally; scratch is **not** a build cache or a source of durable results.

## 6. Dependency graph and ordering

```text
Existing session/run, ManagedProcessService, ToolBroker, scheduler and
accepted authorization boundaries [interface/closed]
       |
       v
M001 — private managed session scratch, prompt/env, safe lifecycle [closed]
       |
       v
M002 — delegated private scratch + explicit shared ScratchId grants [closed]
       |
       v
M003 — actionable permission hints, cleanup hardening/qualification [conditionally closed: `plans/closure/managed-agent-scratch/003-status.md`]
       |
       v
M004 — session and agent-private scratch reclamation [closed: `plans/closure/managed-agent-scratch/004-status.md`]
```

M001 is an end-to-end single-agent vertical slice, not merely a directory allocation type. M002 is a separate durable delegation contract with direct-parent and ownership checks. M003 qualifies security/restart/diagnostic behavior and closes the workstream; correctness blockers found earlier must be corrected *in that milestone*, not deferred as polish.

## 7. Milestones

### M001 — Private session scratch and canonical launch wiring

Class: capability (with infrastructure). Status: **closed**.
Plan: `plans/implementation/managed-agent-scratch/001-private-session-scratch-and-launch-wiring.md`.
Dependencies: existing daemon session ownership + ManagedProcessService/ToolBroker interfaces (closed/interface).
Exit: one session uses the **same** scratch across two independent managed shell calls and a compatible Python call, another session uses a different root, a model prompt exposes it, conventional temp vars point there, existing sandbox rules and human-originated command semantics are unchanged, and deterministic identity plus bounded owner-record recovery survive daemon/process restart. M001 does not reclaim directories; scratch is disposable cache data and host cache maintenance may purge it. See `plans/closure/managed-agent-scratch/001-status.md` for exact runtime and platform qualification.

### M002 — Delegated scratch and explicitly shared resources

Class: capability. Status: **closed**.
Plan: `plans/implementation/managed-agent-scratch/002-delegated-shared-scratch-and-job-leases.md`.
Closure: `plans/closure/managed-agent-scratch/002-status.md` (implementation `2e0aa549`).
Exit: two child runs get distinct private roots, a parent can create/grant one shared scratch resource by ID to selected children, unauthorized children cannot obtain tool/OS grants, and background job leases prevent premature cleanup. No implicit cross-session sharing. Linux/Windows native scratch containment remains unqualified; M003 records available platform evidence.

### M003 — Permission recovery, cleanup and cross-platform qualification

Class: polish / invariant hardening. Status: **conditionally closed**.
Plan: `plans/implementation/managed-agent-scratch/003-permission-guidance-and-lifecycle-qualification.md`.
Exit: structured path denial / approval-needed results suggest authorized scratch but never rewrite a required path, explicit operator Deny remains authoritative, garbage collection is demonstrably safe under crash/race/symlink cases, Linux/macOS/Windows support claims are evidence-qualified.
Closure: `plans/closure/managed-agent-scratch/003-status.md`. The Linux native sandbox qualification is closed by the post-closure corrective addendum's C001 (`plans/closure/managed-agent-scratch-native-qualification-corrective/001-status.md`, native Landlock evidence, CI run `39298041771`, 14,813/14,813 green); the **Windows** native sandbox qualification remains outstanding and keeps M003 conditional.

### M004 — Session and agent-private scratch reclamation

Class: invariant / lifecycle correctness. Status: **closed**.
Plan: `plans/implementation/managed-agent-scratch/004-private-scratch-reclamation.md`.
Closure: `plans/closure/managed-agent-scratch/004-status.md`.
Exit: terminal delegated runs reclaim only their private scratch; deleted sessions reclaim their full scratch tree only after all durable runs/jobs are terminal; busy cleanup retries and restored sessions retain their data. Daemon startup resumes cleanup for deleted sessions. Durable transcripts, run results, and Git worktrees remain separate. M003's **Windows** native sandbox qualification condition remains open; its Linux half is closed by C001.

### M006 — Scratch usage census and operator visibility

Class: infrastructure (usage accounting) with an operator-visible diagnostic surface. Status: **closed**.
Plan: `plans/implementation/managed-agent-scratch/006-scratch-usage-census-and-operator-visibility.md`.
Closure: `plans/closure/managed-agent-scratch/006-status.md`.
Dependencies: M001/M002/M004/M005 closed, and the C001 Linux native qualification closed (interface).
Exit: an operator can obtain a measured, symlink-safe census of the per-user scratch tree — total bytes, tree count, age range, and the reclaimability verdict the existing validator would reach for each tree — through `codegg daemon scratch-usage` and one line of `codegg daemon status`. The census is **read-only**: it creates, renames, and removes nothing, follows no link, and reads no content, and a before/after manifest of the root is identical across a census. It adds no configuration key, no durable state, and no storage migration, and it claims no Windows containment. **It reclaims nothing**: the growth of a session tree that is never soft-deleted remains unbounded, and the absence of a sweep and of a retention policy is unchanged by this milestone. Its purpose is to produce the measurement that the successor retention milestone needs before a policy can be chosen, and to make a tree that reclamation can never accept visible instead of silent. The successor — a retention policy, a session scratch sweep, a session-scratch lease against a live turn's `TMPDIR`, and a disposition for an unreclaimable tree — is deliberately not registered here: its policy cannot be specified until the census has produced real numbers.

## 8. Cross-cutting requirements

**Storage/migrations:** manifest is not an authorization credential; schema/migration and `STORAGE_LAYOUT_VERSION` change only if actual persistent DB contract changes. Preserve old sessions and client serialization using optional/default fields.

**Protocol:** scratch requests, if exposed over daemon protocol, derive caller/owner from authenticated scope. Use typed `ScratchId`; reject forged paths, owner IDs, retries with altered grants, stale generation/unknown IDs.

**Security:** initial no-approval scratch grants must be bounded to actual resource roots, with race-resistant symlink-safe path traversal at file-tool boundaries and OS-enforced rules where available. Never lexical-prefix-check untrusted paths as security. No broad HOME/tmp/credential or sibling grants. Existing explicit Deny and child worktree mutation constraints win.

**Concurrency/recovery:** idempotent allocation; two frontends attaching same session agree on one root; restart fence/lease reconciliation; cancellation and task retry preserve ownership; no cleanup before all affected process groups/child jobs are terminal.

**Observability:** bounded path-safe operator diagnostics, usage and stale lease accounting; don't project file contents, secrets, transcript bodies, or unbounded paths to unauthorized peers.

**Performance:** no global scan per tool call; no size traversal on agent hot path; lazy creation and batched or periodic cleanup. SBC compilation, dedicated benchmark lane, network-dependent CI not required.

## 9. Verification strategy

Focused manager/owner/grant unit tests; executable model shell and Python env/path trajectories; background continuation and daemon restart; multi-frontend session reuse; sibling read/write negative tests; symlink/junction and rename-race rejection; Windows/macOS compile and native runtime verification where available. Preserve `scripts/check_execution_ownership.py`, `scripts/check_scheduler_bypass.py`, `scripts/check_sandbox_policy_wiring.py`, `scripts/check_daemon_cwd_usage.py`, `scripts/check-core-boundary.sh`, `scripts/verify.sh quick` and existing no-regression suites.

## 10. Risks and decisions

Stable session temp paths vs per-launch Required private temp have different lifetimes; verify actual backend writable roots before promising sameness. Existing Windows constrained sandbox remains unqualified; scratch doesn't solve that. A shared path is not OS-user isolation, especially among same-UID unsandboxed agents. Shared scratch is an **artifact transport convention**, not a concurrent Rust `target/` cache, security capability encoded by path, or blackboard replacement. If a true cross-domain policy decision arises, propose an ADR before broadening authority.

## 11. Completion definition

All three milestones have accepted `plans/closure/managed-agent-scratch/00N-status.md` records with exact-head tests, integration and negative evidence; registry is reconciled. No closure may be claimed because directories exist or Rust compiles.

## 12. Milestone status

| Milestone | Status | Plan | Closure | Blocker |
|---|---|---|---|---|
| M001 | closed | `plans/implementation/managed-agent-scratch/001-private-session-scratch-and-launch-wiring.md` | `plans/closure/managed-agent-scratch/001-status.md` | none |
| M002 | closed | `plans/implementation/managed-agent-scratch/002-delegated-shared-scratch-and-job-leases.md` | `plans/closure/managed-agent-scratch/002-status.md` | none |
| M003 | conditionally closed | `plans/implementation/managed-agent-scratch/003-permission-guidance-and-lifecycle-qualification.md` | `plans/closure/managed-agent-scratch/003-status.md` | Authenticated workspace/private/shared-scratch mutations bypass only the implicit default Ask; explicit tool/path rules remain authoritative. Unix and Windows shared cleanup implementations are qualified by platform tests; **Linux** native sandbox containment is closed by C001 and only the **Windows** native Required-sandbox containment condition remains unqualified. M004 was the corrective successor and is closed; M005 and M006 followed and are recorded above. |
| M004 | closed | `plans/implementation/managed-agent-scratch/004-private-scratch-reclamation.md` | `plans/closure/managed-agent-scratch/004-status.md` | Exact-head cleanup, full CI, Windows lifecycle, and desktop evidence recorded. No successor plan in this line is currently eligible or blocked. |
| M005 | closed | `plans/implementation/managed-agent-scratch/005-temp-root-redirect-offer-before-human-approval.md` | `plans/closure/managed-agent-scratch/005-status.md` | Corrective pass on M003's advisory temp-write recovery: the recovery hint lives inside the write tool, so under the default policy the model never received it — the human was asked first. M005 answers the model first with the same actionable message and lets the human decide only when the model repeats the restricted path. Hosted CI run `39374727100` passed on the pull-request head and run `39378897301` passed on the merged head (`e52695b9`), and the live observation with a real model was obtained on 2026-10-20 under the default policy (closure §4 and §13). No successor milestone in this line is currently registered. |
| M006 | closed | `plans/implementation/managed-agent-scratch/006-scratch-usage-census-and-operator-visibility.md` | `plans/closure/managed-agent-scratch/006-status.md` | Depends on M001/M002/M004/M005 and the C001 Linux native qualification, all closed. The read-only census is implemented: the daemon measures its own scratch tree on an internal cadence and serves the report through `codegg daemon scratch-usage` (table and `--json`) and one line of `codegg daemon status`. The reclaim verdict is the reclaim validator's **own** verdict — the rule is shared, not restated — and a truncated walk never claims a tree is reclaimable. It reclaims nothing, adds no configuration key, no durable state, and no migration, and claims no Windows containment. The retention/sweep successor is still not registered: its policy needs real numbers from this census. |
