# Agent Workstream Blackboard M005 — Explicit Group-Owned Worktree Placement and Serialized Shared Mutation

Status: active — M001/M004 and ADR-0032 gates are satisfied; project-root writer sink inventory and the currently available native process lifecycle evidence are recorded. Q1–Q5 and shared mutation remain unqualified; unresolved writer containment and platform gaps keep the capability unavailable.

Planning baseline: 171ef93b6ba67d50b0412ae23b7697eb02d072eb (main, 2026-10-16)
Source: plans/subsystems/agent-workstream-blackboard-roadmap.md M005
Decision gate: `plans/adrs/ADR-0032-agent-workstream-coordination-and-shared-checkout-ownership.md` was accepted on 2026-10-15. Its shared-write mode is not qualified or enabled; ADR-0002 isolation remains the effective mutation contract until M005 proves complete writer containment and passes its acceptance gates.
Canonical: plans/000-long-term-specification.md §§9, 16–17, 27, 29; plans/001-terminology-and-domain-model.md §§5, 7–8.
Primary class: capability / invariant. No work may enable shared mutation before the decision and safety gates close.

## Corrective handoff registered 2026-10-19

The source audit below remains authoritative evidence, not production
qualification. Its unresolved product-boundary and writer-lifetime issues now
have separate corrective handoffs in
plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md
(C001–C006), with executable plans under
plans/implementation/agent-workstream-m005-writer-containment-corrective/.
ADR-0044 option B was **accepted on 2026-10-19**, which unblocked C002
only and enabled no shared mutation; C001 is closed with the restricted
CodeGG-mediated writer profile and denial matrix, and C002 is closed with
the durable generation-fenced checkout writer permit. C003 is
dependency-ready; C004–C006 are blocked. **No writer is routed through the
permit yet**, so nothing is fenced. Do **not** enable SerializedShared from this audit or treat
current global scheduler keys, request-scoped guards, or process-group
cleanup as the missing checkout writer permit. M005 remains active; parent
strict closure requires positively qualified Q1–Q5 and the corrective gates.
M006 isolated session/worktree launch and M007 per-workspace build
admission remain independent.

## Objective

Allow explicit, authorized workspace placement for task spawn and spawn_many without equating agent concurrency with filesystem concurrency. Preserve current default per-mutating-child isolated worktree, read-only inherited checkout, and daemon-owned lease safety. Introduce an opt-in serialized shared checkout for a bounded group of mutation-capable children only where every write path can be governed; no concurrent unreserved writes.

## Existing production boundary

SubAgentPool::requires_isolation in src/agent/worker.rs identifies write-capable tool surfaces; src/scheduler/executors.rs allocates WorktreeService::CreateWorktreeRequest and replaces child workspace_root/allowed_paths with owned checkout. WorktreeService currently creates an owner_run_id lease with one active owner. src/tool/task.rs spawn_many permits at most 16 children and has no placement choice. WorkOrders have a distinct WorkspacePolicy corrected in M001. Nested allocation uses canonical common Git root and explicitly selected parent/base state.

### Writer-path gate audit (2026-10-16)

M001 and strict M004 are closed and ADR-0032 is accepted, but the required
common writer reservation is not present:

- `ToolBatchExecutor` is the model-facing native file-mutation boundary. Its
  affected-path checkpoint/overlap serialization applies within one batch;
  it does not coordinate separate `AgentTurn` jobs or other writer surfaces.
- `ResourceRequest::for_kind` gives a global `exclusive:workspace-mutation`
  key to selected Build/Format work and a global
  `exclusive:worktree-mutation` key to Git mutation. These are not keyed by a
  `WorktreeId`, and `AgentTurn` has no writer exclusivity key. The WorkOrder
  M001 corrective therefore correctly keeps shared mutation fail-closed.
- `src/interactive_process.rs` explicitly admits PTYs without workspace
  mutation exclusivity; a persistent terminal can outlive the request that
  created it. Human shell, scheduled shell/Python, Git subprocesses, Tool
  Programs, and process plugins each have different owners/lifetimes in
  `docs/execution-ownership.toml` and
  `architecture/process-tool-execution-ownership.md`.
- LSP/MCP servers have protocol-specialized persistent process lifecycles.
  A broker-call lock alone cannot cover work that continues after a request or
  escapes the managed process lifetime. File/path claims and command parsing
  do not prove the write extent of arbitrary code.

Until a typed `WorktreeId`/lease-generation writer permit has one owner and is
carried through direct ToolBroker calls, scheduled executors, Git mutations,
and persistent process lifetimes (or unsupported paths fail closed), the
serialized placement contract is not enforceable. The design must also prove
restart/cancellation release, approval/lock ordering, platform-specific process
containment, and denial of unmediated mutation surfaces. Do not call the
existing scheduler keys a substitute: they are global and omit ordinary turns.

Disposition: M005 is active for architecture and qualification because its
decision and strict M004 prerequisites are satisfied. Shared-checkout
execution remains unavailable. M006 isolated session/worktree UX and M007
per-workspace build admission are separately eligible and do not wait for this
writer-containment gate.

### Checkout-scoped writer-containment design (qualification candidate, 2026-10-16)

This is the implementation design being qualified in M005; it is not
implemented or platform-qualified and does not enable shared mutation.

1. **Authority identity:** define the writer scope as the typed pair
   `(WorktreeId, lease_generation)`. The daemon resolves this from the durable
   AgentRun/group and WorktreeService records. Caller strings, a path, a
   `WorkspaceId`, or knowledge of a WorktreeId cannot mint authority. Each
   permit is additionally bound to the owning group, run, and effective job
   attempt; stale generations and non-member runs are refused.
2. **One permit owner:** add a daemon-owned `WorktreeWriterPermitService`
   backed by a compare-and-swap durable lease and a per-worktree waiter. The
   state is `Available -> Held(owner, generation, attempt) -> Releasing`;
   uncertain restart/cancellation cleanup moves to `Quarantined`, which
   refuses new writers until the old execution is proven quiescent. Lease
   expiry alone never releases a permit while a process may still write.
   This is a mutation lock, not a second job queue or scheduler resource
   budget.
3. **Acquire at the effective writer:** direct native file transactions
   acquire around their complete transaction; Git, shell/Python, formatter,
   Tool Program, and other finite process jobs acquire in their final
   scheduler executor and hold through process-tree completion/reaping.
   Child-job delegation is an explicit daemon-owned owner transfer bound to
   parent/run lineage; the submitting caller must not hold a permit while
   awaiting a child that needs the same permit. Reads remain concurrent.
4. **Trusted context seam:** add an optional typed worktree execution scope
   to the daemon-built tool/job execution context. `BrokerInvocationContext`
   currently carries workspace/run/job strings but no WorktreeId or lease
   generation; the typed ProjectBinding seam already has an optional
   WorktreeId. Bridge those through authoritative store resolution, not a
   client or model payload. `ToolEffectClass` can conservatively identify
   mutating/process calls, but it is not itself a path-write proof and must
   not authorize a writer.
5. **Complete mediation or fail closed:** every effectful broker call and
   every direct execution owner must either take the same permit or be
   unavailable to shared-placement members. This includes native file edits,
   Git mutation/recovery, scheduled jobs, human shell, PTYs, hooks, plugins,
   LSP/MCP edit-capable requests, and Tool Programs. Long-lived services must
   hold the permit for their effective lifetime or be denied in shared mode.
   Do not add a generic `exclusive:worktree:<id>` scheduler string as the
   security mechanism: persisted job admission is not the final writer for
   inline tools, and a string key does not cover unmanaged process lifetimes.
6. **Lock ordering and cleanup:** resolve authority and obtain approval before
   waiting for the writer permit; do not hold a writer permit while waiting
   for approval or unrelated scheduler capacity. A scheduled effect acquires
   its permit only after it is admitted, then revalidates the worktree lease
   generation before mutation. Cancellation and timeout must stop/reap the
   effective writer before release. On daemon restart, reconcile job/process
   ownership first; ambiguous active writers quarantine that checkout.
7. **Platform boundary:** in-process permit checks serialize CodeGG-mediated
   actions but cannot stop an unrelated same-user process from editing the
   directory. Shared mode therefore cannot claim foreign-process exclusion
   from a mutex, advisory file lock, Landlock/Seatbelt policy, or Windows Job
   Object. Any execution surface that can escape CodeGG's supervised lifetime
   must be denied unless a platform backend supplies and attests the required
   stronger containment. Cross-node and network filesystems remain denied.

#### Concrete enforcement seam inventory

The source audit found no single existing seam that can provide this permit
today. The following ownership boundaries must converge before shared mode can
be qualified:

| Mutation surface | Current owner / evidence | Required M005 disposition |
|---|---|---|
| Native model-facing file transactions | `ToolBatchExecutor` in `src/agent/tool_batch.rs`, reached through `ToolBroker`; checkpoints and overlap handling are batch-local. `BrokerInvocationContext` has session/workspace/job/attempt identity but no typed worktree generation. | Resolve daemon-owned worktree scope and acquire the common permit over the whole transaction. Reject the mutation before any filesystem effect when scope is missing or stale. |
| Finite scheduled processes (shell, Python, tests, build, formatter, Tool Program) | Scheduler-owned executors receive `JobExecutionContext` and resource permits, but the context has no typed worktree identity. Existing `ResourceRequest` workspace/worktree keys are global capacity keys, not checkout-scoped writer authority. | Acquire in the final executor after admission; retain until the process tree is reaped. Explicitly delegate a held permit to nested child jobs rather than reacquiring it. |
| Git mutation and recovery | Typed Git paths in `src/git_mutations.rs`, `src/git_recovery.rs`, and `src/git_network_ops.rs` retain deferred domain executors; repository locks and scheduler keys are not the worktree writer permit. | Carry the generation-fenced scope through the canonical Git mutation boundary or deny these operations in shared mode. Serialize index, branch, commit, recovery, and integration mutations together. |
| Interactive PTY / terminal | `src/interactive_process.rs` owns a long-lived PTY; scheduler admission permits process capacity but the PTY can continue after its initiating tool request. | Hold the permit for the PTY lifetime with a supervised stop/reap path, or refuse PTY creation for shared-placement runs. |
| LSP and MCP | `crates/egglsp/src/launch.rs` and `src/mcp/local.rs` own protocol-specialized persistent child lifetimes. A request-scoped broker permit would end while either process could still write. | Keep the permit for the complete service lifetime or deny edit-capable requests / service launch in shared mode. |
| TUI, HTTP, IDE and project-file mutation APIs | Workspace-scoped mutation entry points also exist outside the model broker, including `src/tui/commands/file_tree.rs`, `src/server/routes/file.rs`, `src/ide/`, `src/lsp/mutation.rs`, and `src/document_service.rs`. These run under the daemon or client but can target a checkout while a shared group is active. | Route each project-rooted write through the same service, or deny it for a checkout with an active shared reservation. User-facing edit authority remains distinct from agent membership. |
| Plugins, hooks, and other direct process adapters | `src/plugin/runtime/process.rs`, `src/hooks/`, and `src/tool/formatter.rs` have distinct or deferred owners in `docs/execution-ownership.toml`. | Route each through the same final permit owner and supervised lifetime, otherwise deny it before process start. |
| Unrelated same-user processes | Editors and shell processes outside the daemon do not participate in CodeGG admission or cancellation. | Do not claim these are serialized. If the product requirement includes them, ordinary daemon-owned locks cannot satisfy it; shared mode needs a separately qualified OS identity / filesystem boundary or must remain unavailable. |

The source audit now classifies the production CodeGG write paths by their
effective root and owner. This closes the earlier gap where the seam table was
only a control-flow starting point. “Complete” here means CodeGG-mediated
operations that can mutate a registered project/worktree; arbitrary command
contents and external processes cannot be reduced to a finite path list.

#### Project-root writer sink inventory (source audit 2026-10-19)

| Entry surface | Effective sink and authority | Existing coordination | Shared-placement disposition |
|---|---|---|---|
| Agent native file tools | `write`, `edit`, `replace`, `apply_patch`, and `lsp_preview_apply` resolve against the execution workspace or explicitly authorized scratch. `ToolBatchExecutor` is the model-facing dispatch owner. | Path validation, authorization, per-batch affected-path checkpoints and overlap handling. None spans another run or non-broker writer. | Carry the typed checkout generation and hold one permit for the full file transaction, or refuse these tools for shared members. Scratch roots stay under their separate owner/lease contract. |
| Agent Git tools and recovery | `GitMutationService::execute`, Git network operations, recovery, commit/stage and worktree administration mutate the worktree, index, refs, or the shared repository Git directory. | Typed Git policy, operation audit, repository/worktree checks and existing Git locks; these do not serialize shell, file tools, or other APIs under one checkout identity. | Use the same permit across all Git/index/ref/integration mutations, including recovery; otherwise deny them. A worktree lease is checkout ownership, not a writer permit. |
| Finite command and job surfaces | Bash/shell, Python, test/build/format, Tool Program child jobs, hooks, and process-backed plugins can write anywhere their effective process authority permits, including via subprocesses. Human `!` shell is a separate authenticated user-originated path. | Durable heavy work enters `JobSubmissionService`/`JobScheduler`; finite execution reaches `ManagedProcessService`. Admission/resource keys are global capacity controls, not checkout writer authority. Human shell also uses the finite-process owner. | Acquire only in the final executor and retain through verified writer-tree quiescence; bind nested-job handoff. Refuse any command/profile whose writer effects or descendants cannot be contained. Human-originated activity also needs explicit conflict policy. |
| Persistent terminal | `src/interactive_process.rs` owns a PTY that can continue writing after its creation request returns. | Process-slot admission plus a live session handle; no worktree-scoped reservation. | Keep the checkout isolated or deny PTY creation/attachment for shared placement. Releasing a permit on tool return is unsafe; holding it for an indefinite PTY is not a usable shared mode. |
| Editor documents and LSP preview | `CoreDaemon` document create/delete/save and reviewed LSP preview application write through `src/core/daemon_documents.rs` and `src/lsp/mutation.rs`. TUI editor commands submit these daemon requests. | Document writer/revision leases, clean-buffer checks and repository locks protect their own checked transaction. They do not cover AgentRuns, Git, shell, HTTP, or snapshot restore. | Route the final checked transaction through the common permit. LSP server process access remains separately denied unless its full lifetime is contained or read-only. |
| HTTP workspace file API | `src/server/routes/file.rs` reads, writes, and deletes project-rooted files after route authorization. It is outside `ToolBroker`. | Root/path validation and route authorization; no common worktree writer gate. | Route mutations through the common permit or reject while a shared checkout is reserved. |
| Project initialization and snapshot restore | `src/core/daemon_project_init.rs` publishes the root `AGENTS.md`; `codegg-core` snapshot restore applies checked file states into the project root. | Digest/identity validation, atomic replacement and restore rollback protect each operation, but do not coordinate all writer classes. Bootstrap analysis itself is read-only and returns a draft. | Include both at the final mutation boundary or refuse while a shared reservation is active. |
| Project-local CodeGG state | Workspace service construction/run storage writes under `<workspace>/.codegg/runs`; legacy project-store migration can write `<project>/.codegg/sessions.db`. These are physically inside the checkout even when ignored by Git. | Run-store identity/atomic-write rules and migration locking are scoped to their own stores. | Either include these sinks in checkout conflict policy or move/resolve them outside the checkout before claiming all CodeGG writes are serialized. |
| LSP, local MCP, plugin and hook lifetimes | LSP/MCP servers and some plugin adapters retain child processes or capabilities beyond one broker request; a tool response is not proof that an extension stopped writing. | Protocol-specific lifecycle, cancellation and direct-child cleanup; no shared worktree permit and no uniform descendant-quiescence attestation. | Deny in the initial shared profile unless a typed checkout scope and full supervised lifetime are proven. |
| Non-writers and out-of-scope writers | File-tree requests are listing/read-only. `src/ide/mod.rs` writes temporary diff copies rather than the checkout. Agent bootstrap analysis and project-agent discovery read the checkout. User config, daemon/session databases, skill stores, scratch roots and test fixtures use managed data/cache/temp locations. Editors and shells outside CodeGG are external same-user writers. | Read-only operations need no permit. Managed private state uses its own storage/scratch owners. External same-user processes do not participate in daemon admission. | Keep read-only surfaces concurrent. Do not count temp/private-state writes as checkout mutations. External same-user writers are outside a CodeGG-only contract; if they must be excluded, shared placement is a no-go without a separately qualified OS identity/filesystem boundary. |

No common checkout-scoped writer permit currently spans these sinks. Per-batch
file coordination, document/repository locks, scheduler capacity keys, Git
operation checks, and filesystem path validation solve different problems and
cannot be composed into a checkout-wide reservation by assumption. In
particular, path validation does not constrain arbitrary process effects, and
detecting an external edit at a later checkpoint does not prevent a concurrent
write.

#### Qualification sequence

- **Q1 — identity/state machine:** durable compare-and-swap contention across
  separate run IDs; only one permit per `(WorktreeId, generation)`; independent
  worktrees do not block; wrong group, stale generation, expiry, restart,
  cancellation, panic and quarantine all have typed outcomes.
- **Q2 — direct native writers:** two production-shaped AgentRuns in one
  managed group checkout overlap at a barrier. Readers overlap; mutating
  broker operations never overlap. Verify permit denial/refusal is returned
  before filesystem effects and transaction rollback releases the permit.
- **Q3 — every enabled writer owner:** run the same two-writer race through
  each enabled Git/process/tool surface and audit that each disabled surface
  is refused before spawn or mutation. Include child-job delegation without
  reacquisition deadlock, cancellation, timeout, daemon restart and descendant
  escape attempts.
- **Q4 — native platform matrix:** qualify the exact supported backend on
  Linux, macOS, and Windows separately. Where process-tree or filesystem
  containment is not evidenced, mark that surface/platform unavailable; do
  not infer support from mocked permits or a successful compile.
- **Q5 — integration and recovery:** nested worktree base, shared-group
  lifecycle, concurrent readers, successive writer handoff, Git state and
  explicit parent integration, dirty/conflicted retention, and crash recovery
  all pass against a real temporary Git repository. No platform enters
  `SerializedShared` until every active write path for that platform passes
  Q1–Q5 or is denied.

Current qualification result: Q1–Q5 have not been run for this candidate.

**Q0 — safe-default posture (pass; negative evidence only).** On merged head
`c19a6c3c2ae1f77e5298cb3390035f0420140929`, the spawn surface still has no
placement selector; `SubAgentPool::requires_isolation` treats write/edit,
terminal, shell, Git and commit capability as requiring isolation, and the
scheduler executor allocates the managed worktree before constructing the
child. The M001 WorkOrder policy test proves serialized mutation reaches
`NeedsAttention` with no session or job
(`trigger_fire_wakes_coordinator_and_honors_closed_shared_mutation`). Hosted
CI run `38833977755` passed the full suite on that exact source head. This
qualifies preservation of the current safe posture only; it does not qualify a
permit, shared placement, or any Q1–Q5 capability.

Existing M004 two-run fake-provider tests qualify peer-observation refusal,
not writer locking; existing WorktreeService lease generations do not qualify
the proposed writer permit. ADR-0002's isolated mutation default remains
effective. The next slice is the process-lifetime feasibility decision below,
not a permit-store implementation. Q1 should begin only if that decision
identifies a bounded capability profile whose effective writers can all be
contained or denied.

#### Persistent-process and universal-writer feasibility review (2026-10-16)

This review narrows what a future shared-write implementation can claim. A
short scheduler/tool request is not the lifetime of its effective writer.

| Owner | Lifetime evidence | Feasibility result for a checkout permit |
|---|---|---|
| ManagedProcessService finite command | Unix starts a new session and signals its process group. Its own join_output comment records that a descendant calling setsid can escape the group and hold a pipe open indefinitely; the implementation bounds the join and reports an error. Windows Job Object ownership is stronger for descendants but remains process supervision, not filesystem containment. | A permit can be held through the direct job only if the process owner reports positively that the whole effective tree is quiescent. Current bounded output completion/error is not such proof. Arbitrary shell stays unavailable until escaped descendants are contained or detected and the permit remains quarantined. |
| Interactive PTY | InteractiveProcessService retains an ephemeral live session and has TERM/KILL process-group teardown. A PTY can remain open and idle after its creating tool request returns; handles do not survive daemon restart. | Holding a writer permit for the PTY's full interactive lifetime serializes the group behind an arbitrarily long user session. Releasing on tool return is unsafe. Initial shared placement must refuse PTY creation/attachment or keep the run isolated. |
| LSP process and edit application | egglsp owns a long-lived child with JSON-RPC framing and restart state; launch::terminate calls child.kill(). The protocol server rejects server-originated workspace/applyEdit requests, while CodeGG applies returned previews through src/lsp/mutation.rs. However, the child itself has filesystem access and no checkout-scoped read-only sandbox contract. | The host apply path can acquire a checkout permit around the checked transaction, but that does not constrain the persistent server process. Shared mode must deny LSP server use unless launch is qualified read-only on that platform or the full service lifetime is supervised and reconciled. A request-scoped lock is insufficient. |
| Local MCP process | LocalClient retains a persistent process and protocol state. Shutdown kills and waits for the direct child; this is not the common process owner and does not attest descendant quiescence. An MCP server may execute arbitrary tools independently of a single broker call. | Shared mode must deny local MCP launch/use until capability and descendant lifetimes are bound to the checkout permit. |
| Hooks, plugins and Tool Programs | Finite hooks/process plugins use managed execution in some routes; plugin protocol/lifecycle integration is deferred. Tool Programs may delegate through scheduler jobs. | Each final effect owner must inherit a typed permit for its whole effect. Unsupported or detached/delegated surfaces must be denied. |
| TUI, IDE, HTTP and external same-user tools | Daemon/client file APIs can write through independent services; editors and shells outside CodeGG never acquire daemon permits. | Every CodeGG project-rooted sink must route through the permit or refuse while shared placement is reserved. A daemon lock cannot exclude an unrelated process under the same OS identity. If “universal” includes those processes, shared mode is infeasible without a separately qualified OS identity/filesystem boundary. |

The central feasibility condition is stronger than adding a durable mutex:
every mutation-capable capability must be either (a) attached to a typed
checkout scope and held through verified effective-writer quiescence, or
(b) unavailable to a run in shared placement. Current persistent-process
owners do not provide that uniform contract. Indefinite PTY ownership is
operationally unusable, and current LSP/MCP lifecycle adapters cannot prove
checkout-wide process quiescence. This is a design gate, not an implementation
task for a generic worktree lock.

Before a permit service is exposed, the next architecture decision must state
whether M005's product boundary excludes non-CodeGG same-user writers. If it
does not, stop: the current daemon architecture cannot provide universal
writer protection. If it does, define a shared-placement capability profile
that denies PTY and persistent LSP/MCP/plugin paths until they have a
checkout-scoped lifecycle contract; separately prove descendant containment
for finite processes. Only then is Q1 useful, and Q1 storage must never imply
that a caller has permission to use a capability excluded by its profile.

This review does not qualify shared mutation and does not change ADR-0032 or
ADR-0002. It records why scheduler exclusivity keys and request-scoped locks
are insufficient and why persistent process ownership must be resolved before
broad implementation.

#### Native process-lifecycle qualification record (2026-10-19)

The production process owners were checked against their actual lifetime
contracts, with focused native tests where available. The results qualify
current cleanup behavior for the tested cases; they do not establish a
checkout-scoped writer permit or prove that arbitrary descendants are
quiescent.

| Native owner | Evidence | Result and limit |
|---|---|---|
| ManagedProcessService on macOS | Rust 1.89, Apple Silicon; `cargo test -p codegg --lib managed_process --locked` passed 20/20 after building `codegg-sandbox-helper`. Includes sandbox temporary-directory cleanup and timeout/cancellation process-group termination. | Qualified for these tested child/process-group cases. Unix descendants that call `setsid` can escape; bounded output-join failure is not positive quiescence evidence. |
| InteractiveProcessService on macOS | `cargo test -p codegg --lib interactive_process --locked` passed 22/22. Added `terminate_kills_session_descendant_before_releasing_process_slot`; the focused rerun passed 1/1. It terminates a shell with a background delayed writer, checks the process slot is released, then verifies the marker was never written. | Qualified for this ordinary same-session descendant case. A descendant that escapes the process group is not covered by this test. PTYs remain long-lived and therefore unavailable to a request-scoped shared-write permit. Non-Unix builds refuse PTY operation. |
| Local MCP child on macOS | `cargo test -p codegg --lib mcp::local --locked` passed 5/5, including hung discovery/tool-call timeout and direct-child reap cases. | Direct child cleanup is tested; descendant quiescence and checkout-scoped authority are not. Keep local MCP unavailable in a shared profile. |
| ManagedProcessService on Windows | Hosted Windows process-launcher workflow [39262822248](https://github.com/dbowm91/codegg/actions/runs/39262822248) passed on the integrated source head. Source starts suspended, assigns a kill-on-close Job Object, resumes, then closes the job for timeout/cancellation cleanup and reaps the direct child. | Confirms the hosted lifecycle suite on that head; Job Object supervision is not filesystem containment and does not qualify every writer route. |
| ManagedProcessService on Linux | Hosted full CI [39262822255](https://github.com/dbowm91/codegg/actions/runs/39262822255) passed on the integrated source head. | Full-suite evidence only; no separate adversarial escaped-descendant qualification. Linux shared-write capability remains unavailable. |
| LSP launch and persistent runtime | `crates/egglsp/src/launch.rs` configures `kill_on_drop`; termination kills the direct child. The runtime/restart tests exercise owned runtime teardown, but there is no native test proving descendant-tree quiescence or read-only filesystem access. | Not qualified for shared checkout writers. Keep LSP server use unavailable until its full lifetime is contained or its filesystem access is independently restricted. |

The PTY test is committed only with this M005 audit branch; it is not part of
the M004 PR. The source audit classifies project-root writes from native file
tools, Git and recovery, finite commands/jobs, PTYs, editor/document/LSP
preview, HTTP file routes, project initialization/snapshot restore, and
project-local CodeGG state. It also identifies persistent LSP/MCP/plugin/hook
capabilities and external same-user processes as containment gaps. The updated
source inventory in `docs/execution-ownership.toml` has 58 process entries;
`scripts/check_execution_ownership.py` passes. This makes the writer inventory
complete as a classification of the checked-in CodeGG paths, while the
unbounded effects of arbitrary commands and external processes remain explicit
limitations rather than claimed coverage.

**Disposition:** native cleanup is qualified only for the named tests and
platform evidence above. Escaped Unix descendants, persistent process writers,
independent file APIs, and external same-user writers prevent a universal
checkout-scoped guarantee. Do not implement or enable SerializedShared from
this evidence. Q1–Q5 remain unrun; M005 stays active pending an accepted
product boundary and a restricted capability profile or a separate native
containment design.

#### Architecture options and recommendation

| Option | Feasibility | Decision |
|---|---|---|
| Daemon mutex held around each tool/job request | Fails for detached descendants, idle PTYs, persistent LSP/MCP processes, and other CodeGG file APIs that bypass the permit. A request lifetime is not the writer lifetime. | Reject as a universal checkout guarantee. A mutex may remain an internal scheduling primitive only after all effect owners are mediated. |
| OS identity/filesystem boundary around the shared checkout | This is the only plausible route if the guarantee must exclude arbitrary same-user writers. It requires separate native designs and adversarial lifecycle evidence on Linux, macOS, and Windows; current process-group and Job Object ownership alone is not filesystem isolation. | No implementation is qualified. Treat this as a new platform-security project, not a permit-store extension. |
| Restricted CodeGG shared-placement capability profile | Potentially usable if the product boundary excludes external same-user writers and each CodeGG effect either receives a typed checkout permit through verified quiescence or is denied. Initial profile would deny PTY and persistent LSP/MCP/plugin paths and must prove descendant containment for finite shell/build/test jobs per platform. | Recommended candidate for a future ADR/implementation gate, with ADR-0002 isolated placement remaining the default. Do not start Q1 storage until the product boundary and per-platform capability matrix are accepted. |

The feasibility result is therefore conditional, not a blanket “add a lock.”
If users require protection against arbitrary processes running as the same
OS identity, the restricted CodeGG profile cannot satisfy the requirement and
shared checkout placement is a no-go until a qualified OS identity/filesystem
boundary exists. If those external writers are explicitly outside the
contract, a constrained profile may be pursued, but every enabled process and
file-write route still needs native containment evidence.

## Work packages

A. Consolidate placement resolution. Define one core domain policy (avoid duplicating WorkOrder WorkspacePolicy and task-spawn policy semantics): Auto, IsolatedNew, InheritReadOnly, and SerializedShared (group or exact worktree ID). Separate selected placement from effective mutation capability and sandbox/approval ceiling. Auto remains existing M004-era isolated default for write-capable delegated runs. Group placement is an explicit field on spawn_many and inherited by nested children only within authorized ceilings; single spawn may opt in if supported. Reject unsupported placement with typed diagnostic, never silently share or weaken capabilities.

B. Separate checkout ownership from active execution access. Worktree owner may be a group/session/work order under bounded reservation, but individual execution permits remain run-owned/generation-fenced. Do not turn a worktree lease into a bearer token. Establish typed scope for ProjectId/RepositoryId/WorkspaceId/WorktreeId/group/run; align persistence, restart reconciliation and safe cleanup. A group terminal does not force-remove dirty or conflicted checkout. External manually created worktrees are never silently claimed.

C. Implement defensible serialized mutation. Shared group members may analyze/read concurrently. All filesystem-mutating actions, Bash/terminal/process execution that can mutate, Git index/branch/commit, LSP edits, tools/plugins, Tool Programs and Python execution require a common checkout-scoped writer reservation held for the effective mutation lifetime. Disallow escape paths or unsupported backends; for arbitrary shells whose write extent cannot be proved safe, reserve the entire process lifetime or fail closed. Enforce this on daemon/tool execution boundary, not on a prompt hint or regex command classifier. Ensure approval waits do not hold incompatible locks indefinitely; define ordering vs scheduler/Git repository locks to avoid deadlock. Claim/path intents from M003 are advisory and cannot replace this writer reservation.

D. Git changes/integration. One group checkout has one coherent Git index/branch; commits must be serialized and attributed to active run or designated group owner. Explicit parent integration remains authorized and typed; no child push/rebase/merge by default. Qualify nested parent checkout, same-repository base/HEAD, successive child-result integration on an advanced target, conflicts/recovery and cancelled writers. If typed integration cannot prove a moving parent safe, reject with actionable recovery, not silent rewrites.

E. Negative platform and remote behavior. Check Linux/macOS/Windows effective backends independently; do not treat a scheduler key as OS filesystem containment. Non-Git mutable work cannot claim Git worktree isolation. Cross-node/shared directory and network filesystem modes are out of scope. Deny SerializedShared if any active tool path bypasses the common guard or is outside the effective sandbox.

## Acceptance tests

- Two write-capable members use one group checkout with exclusive writer operations; two isolated members still receive distinct managed worktrees; read-only inherited member cannot write.
- Two contending writers: precisely one runs, second waits or receives typed conflict; cancellation, restart, process kill, permission wait, exception and stale lease never admit two writers.
- Tools outside ordinary file edit paths (shell, terminal, Git, plugin, Python, LSP, Tool Program, native process) cannot bypass the effective guard; adversarial renamed files, symlink escapes, overlapping path claims and foreign-process edits treated conservatively.
- Separate repository/workspace groups do not accidentally serialize one another; another principal/group cannot acquire an active group lease merely by knowing IDs.
- Descendant inherits only narrowed authority and placement ceilings, never arbitrary group membership or Git push/force.
- Dirty/conflicted cleanup retained; branch and untracked data never destroyed for apparent success.
- Existing SubAgentExecutor auto-isolation and M001 WorkOrder behavior regressions remain green; exact hosted platform evidence where claimed.

## Verification / stop conditions

Run focused worktree, scheduler contention/restart, task/run control, session authorization, agent integration, Git security and process execution tests, scripts/verify.sh quick, Clippy, source guards for scheduler/exec and sandbox, and at least one production-shaped multi-run shared-mutation trajectory under supported native containment. Mark unsupported platforms explicitly; no 'supported' labels based only on mocked reservations.

Stop shared-placement enablement if accepted ADR is missing, if any enabled mutation surface can evade the reservation, if the implementation requires a second scheduler, or if arbitrary shell requires weakening a sandbox. Retain Auto isolated and report negative feasibility. M006 isolated session/worktree UX and M007 per-workspace build admission proceed independently; shared target/cache work remains out of scope.

## Closure

plans/closure/agent-workstream-blackboard/005-status.md must include accepted-ADR identity and approval, pre/post architecture, mutex/lease state machine, negative bypass and race matrix, native containment evidence by platform, nested/advancing integration results, performance cost, exact commits/commands, residual risks and unblock audit.
