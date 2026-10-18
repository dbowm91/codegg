# Agent Workstream Blackboard M005 — Explicit Group-Owned Worktree Placement and Serialized Shared Mutation

Status: active — ADR-0032 and ADR-0035 are accepted; M006 and M007 are strictly closed and M008 proceeds on isolated worktrees. The candidate writer-path inventory is complete. Native Darwin process-lifecycle qualification found that a `setsid()` descendant can write after its managed job reaches `Completed`; complete mediation/native-lifecycle gates therefore remain open. Do not start a writer-permit service until those gates pass. Shared mutation remains unavailable.

Planning baseline: 171ef93b6ba67d50b0412ae23b7697eb02d072eb (main, 2026-10-16)
Source: plans/subsystems/agent-workstream-blackboard-roadmap.md M005
Decision gates: `plans/adrs/ADR-0032-agent-workstream-coordination-and-shared-checkout-ownership.md` was accepted on 2026-10-15 and `plans/adrs/ADR-0035-codegg-mediated-checkout-writer-boundary.md` was accepted on 2026-10-16. Shared-write mode is not qualified or enabled; ADR-0002 isolation remains the effective mutation contract until M005 proves complete CodeGG-writer mediation and passes its acceptance gates.
Canonical: plans/000-long-term-specification.md §§9, 16–17, 27, 29; plans/001-terminology-and-domain-model.md §§5, 7–8.
Primary class: capability / invariant. No work may enable shared mutation before the decision and safety gates close.

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
  created it. The one-shot model `TerminalTool` now carries the normal
  isolated-session sandbox profile through `ManagedProcessService`, but does
  not acquire a generation-fenced shared-worktree writer permit. Human shell,
  scheduled shell/Python, Git subprocesses, Tool
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
per-workspace build admission are strictly closed and did not require this
writer-containment gate.

### M007 boundary update (2026-10-16)

M007 scopes scheduler-owned legacy workspace/worktree mutation keys by the
job's canonical `WorkspaceId` and adds a fixed build-slot quota. This narrows
unrelated workspace contention, but does not satisfy the M005 writer gate:
inline `AgentTurn` tools, persistent PTYs, human shell and other long-lived
writers do not all acquire these job permits. M005 remains independently
active and shared-checkout execution remains unavailable.

### Isolated session allocation boundary evidence (2026-10-16)

M006's managed-worktree launch/fork path is independent of shared writer
containment. `WorktreeService::create_reserved` now uses a narrow filter-free
materializer: it inspects the exact base commit's effective `filter`
attributes through bounded Git queries, refuses active custom filters, and
pins hooks, fsmonitor, and submodule recursion off for the add command. The
generic Git policy still refuses arbitrary `worktree add`. This is a
per-operation checkout materializer profile, not permission to run arbitrary
filters and not a checkout-wide shared-writer permit. It does not make M006 a
dependency of the M005 shared-mutation decision and does not qualify any M005
writer path. Cross-platform M006 qualification passed independently.

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
| Finite scheduled processes (shell, Python, tests, build, formatter, Tool Program) | Scheduler-owned executors receive `JobExecutionContext` and resource permits, but the context has no typed worktree identity. M007 scopes scheduler mutation keys by `WorkspaceId` and adds build quota; these remain capacity locks, not generation-fenced checkout writer authority. | Acquire in the final executor after admission; retain until the process tree is reaped. Explicitly delegate a held permit to nested child jobs rather than reacquiring it. |
| Git mutation and recovery | Typed Git paths in `src/git_mutations.rs`, `src/git_recovery.rs`, and `src/git_network_ops.rs` retain deferred domain executors; repository locks and scheduler keys are not the worktree writer permit. | Carry the generation-fenced scope through the canonical Git mutation boundary or deny these operations in shared mode. Serialize index, branch, commit, recovery, and integration mutations together. |
| Interactive PTY / terminal | `src/interactive_process.rs` owns a long-lived PTY; scheduler admission permits process capacity but the PTY can continue after its initiating tool request. The finite model `TerminalTool` uses `ManagedProcessService` and the isolated-session sandbox profile, but has no worktree-generation permit. | Hold the permit for the PTY lifetime with a supervised stop/reap path, or refuse PTY creation for shared-placement runs. The finite terminal adapter must acquire the common permit at its final process owner or be refused for shared-placement runs. |
| LSP and MCP | `crates/egglsp/src/launch.rs` and `src/mcp/local.rs` own protocol-specialized persistent child lifetimes. A request-scoped broker permit would end while either process could still write. | Keep the permit for the complete service lifetime or deny edit-capable requests / service launch in shared mode. |
| TUI, HTTP, IDE and project-file mutation APIs | Workspace-scoped mutation entry points also exist outside the model broker, including `src/tui/commands/file_tree.rs`, `src/server/routes/file.rs`, `src/ide/`, `src/lsp/mutation.rs`, and `src/document_service.rs`. These run under the daemon or client but can target a checkout while a shared group is active. | Route each project-rooted write through the same service, or deny it for a checkout with an active shared reservation. User-facing edit authority remains distinct from agent membership. |
| Plugins, hooks, and other direct process adapters | `src/plugin/runtime/process.rs`, `src/hooks/`, and `src/tool/formatter.rs` have distinct or deferred owners in `docs/execution-ownership.toml`. | Route each through the same final permit owner and supervised lifetime, otherwise deny it before process start. |
| Unrelated same-user processes | Editors and shell processes outside the daemon do not participate in CodeGG admission or cancellation. | Do not claim these are serialized. If the product requirement includes them, ordinary daemon-owned locks cannot satisfy it; shared mode needs a separately qualified OS identity / filesystem boundary or must remain unavailable. |

This inventory narrows the qualification claim: “universal” means every
CodeGG-mediated writer that can touch a shared-placement checkout. It does not
mean CodeGG can serialize arbitrary external programs running as the same OS
user. External edits must still be detected as conflicts at guarded transaction
boundaries; detection is not protection from a concurrent external writer.

A source-wide scan of direct filesystem mutation APIs was completed on the
integrated branch on 2026-10-17. The scan found 198 Rust source files with
candidate filesystem-write calls across `src/` and `crates/`; most are not
project-rooted writes. Every candidate family is classified below by the
authority and lifetime that reaches its sink. The scan is discovery evidence,
not a line-by-line count of effective writers: filesystem APIs in test code,
database/config stores, temporary artifacts, caches, plugin installation,
and read-only adapters were excluded from the workspace-writer set after
tracing their roots. The command used was:

```text
rg -l -g '*.rs' '(std::fs|tokio::fs|fs::(write|rename|remove|create|OpenOptions)|File::create|OpenOptions::new\\(\\)|create_dir_all|rename\\(|remove_file|remove_dir_all|hard_link)' src crates
```

#### Completed writer-path classification (2026-10-17)

| Effective writer family | Workspace-rooted entry points / owners traced | Shared-placement disposition before a permit exists |
|---|---|---|
| Native agent file edits | `src/tool/{write,edit,replace,apply_patch}.rs`, `src/agent/tool_batch.rs`; validates against the session workspace and executes the transaction/checkpoint path. | Deny for shared placement until the whole transaction obtains a typed, generation-fenced permit. Existing batch overlap serialization is local to one batch. |
| LSP-produced edits | `src/tool/lsp_preview_apply.rs` and `src/lsp/mutation.rs`; applies a returned preview to the selected workspace document. | Deny shared-placement LSP requests until both persistent server lifetime and host-side apply are scoped; a request lock covers only the latter. |
| Document, editor, and file-tree APIs | `src/document_service.rs`, `src/tui/{document_session,editor,file_tree.rs}`, `src/tui/commands/{editor,file_tree}.rs`, `src/server/routes/file.rs`, and `src/ide/mod.rs`; writes are workspace/document scoped and can run outside an AgentRun. | Route through the common permit or refuse while that worktree has a shared reservation. User edits do not inherit agent membership. |
| Project/workspace initialization and updates | `src/core/daemon_project_init.rs`, `src/tui/commands/project_init.rs`, `src/server/routes/{project,workspace}.rs`, and workspace service registration. | Permit or deny any operation whose destination resolves inside the reserved checkout. Catalog-only identity and metadata writes remain outside the checkout. |
| Git/index/worktree mutation | `src/git_mutations.rs`, `src/git_recovery.rs`, `src/git_network_ops.rs`, `src/git_service.rs`, `src/tool/git.rs`, and `crates/codegg-core/src/worktree_service.rs`; Git changes repository metadata/index/refs, while managed worktree creation and cleanup mutate the shared common repository administration area. | Git mutation and cleanup are denied for shared-placement members until they use the same permit and typed ownership. Managed worktree creation is a separate pre-placement lifecycle operation. |
| Finite process-backed workspace writes | `src/tool/bash.rs`, `src/python_script/`, `src/test_runner/`, `src/scheduler/`, `src/tool/formatter.rs`, Tool Program executors, and managed hooks/process plugins; effective write extent is the process tree, not the submitting call. | Deny until final executors retain authority through verified tree quiescence. Escaped descendants quarantine the checkout; output-pipe closure is not proof of quiescence. |
| Interactive or persistent process writers | `src/interactive_process.rs`, `src/tool/terminal.rs`, `crates/egglsp/src/launch.rs`, and `src/mcp/local.rs`; PTY/LSP/MCP lifetimes can outlast the request and current daemon restart accounting does not prove filesystem quiescence. | Deny shared-placement creation/attachment/use until a bounded checkout-scoped lifecycle is qualified. |
| Workspace asset/config writers | Project-scoped asset refresh and project-init paths can target a project root; user agent definitions, project discovery indexes, and config migrations write their configured application/config roots instead. | Permit project-rooted asset writes; classify user/config/catalog roots as global state and exclude them from checkout serialization. Any path that can resolve under the checkout remains denied until covered. |
| Session and run artifacts | `crates/codegg-core/src/run_store.rs`, snapshot/checkpoint modules, context artifact storage, goal/memory stores, and tool-program result/ledger stores use explicit session/data roots rather than the current project root. | Excluded from checkout writers when their daemon-owned root is outside the worktree. Validate the resolved root at runtime; never infer from a relative caller path. |
| Global installation, credential, theme, skill, and cache writes | `src/plugin/install.rs`, `src/install.rs`, credential/config stores, skill/theme registries, provider state, build/cache directories, and security report outputs use explicit user/global/temp roots. | Outside the shared-checkout lock domain. Refuse any configurable destination that resolves inside a reserved checkout unless routed through the permit. |
| External same-user processes | Editors and user shells not launched/owned by CodeGG have no daemon authority or cancellation link. | Outside ADR-0035's serialization guarantee; detect resulting checkout drift as conflict, without claiming exclusion. |

The inventory is complete for the source-level writer families in this branch
and identifies the paths that remain denied. It does not establish universal
containment or authorize Q1: source tracing cannot prove arbitrary dynamic
paths, uninstrumented native extensions, persistent descendants, or platform
filesystem confinement. The next gate is the native process-lifecycle matrix
and a per-platform denial/allow profile, followed by Q1–Q5 only if the
effective writer set is bounded.

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
The writer-path discovery/classification gate is complete as recorded above;
process-lifecycle and platform qualification remain independent open gates.

#### Native process-lifecycle qualification (Darwin, 2026-10-17)

On the integrated branch, the following focused arm64 macOS suites passed:

| Suite | Result | What it establishes |
|---|---:|---|
| `cargo test --test managed_process_descendants --locked` | 6/6 passed in one run on arm64 macOS | Cancellation, timeout, SIGKILL escalation, process-group setup, and bounded output work for ordinary descendants. The adversarial `setsid()` child outlives a terminal managed job and writes afterward, proving current process-group teardown is not a complete effective-writer boundary. |
| `tests/interactive_process_sessions.rs` | 11/11 | PTY attach/input/resize/output, process-group termination, shutdown cleanup, and admission refusal after shutdown behave as tested. This does not make a live PTY safe to release at request completion. |
| `tests/interactive_process_attach_resume.rs` | 19/19 | Daemon attach/resume, cursor, authorization, disconnect, handle invalidation, and termination behavior pass. Handles remain ephemeral across restart. |

The descendant escape test intentionally passes by witnessing the unsafe
property: a scheduled managed job reaches `Completed` while the escaped child
is still live, then that child writes to its temporary workspace. The test's
PID guard reaps the process after observation. This is a hard M005 disqualifier
for arbitrary shell/process writers: a future permit must be quarantined until
quiescence is proven, or these process surfaces must be denied. It is not
shared-write qualification. The native worktree, delegation, project-picker,
and identity suites passed earlier on the integrated branch; rerun them at the
final head before closure. Windows/Linux process-owner qualification remains pending the
exact-head hosted platform jobs.

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

ADR-0035 resolves the product boundary by excluding non-CodeGG same-user
writers from the serialization guarantee. Before a permit service is exposed,
M005 must close the CodeGG writer inventory, define a per-capability allow/deny
profile, and prove each allowed writer remains contained through quiescence.
The initial profile must deny PTY and persistent LSP/MCP/plugin paths until
they have checkout-scoped lifecycle contracts; finite processes need separate
descendant containment evidence. Only then is Q1 useful, and Q1 storage must
never imply that a caller has permission to use a capability excluded by its
profile.

This review does not qualify shared mutation and does not change ADR-0032 or
ADR-0002. ADR-0035 now accepts the external-writer boundary described below;
that decision records why scheduler exclusivity keys and request-scoped locks
are insufficient, but it does not prove complete mediation or persistent
process ownership.

#### Accepted external-writer boundary (2026-10-16)

Accepted ADR-0035 records Option B: shared-placement
serialization covers CodeGG-mediated writers only. Unrelated same-user
processes are outside the serialization guarantee; guarded CodeGG writes must
detect external checkout drift as conflict, without claiming protection from
that writer. Any CodeGG mutation surface without a typed permit through
verified effective-writer quiescence remains denied. ADR acceptance authorizes
no service or shared-write path; M005 remains active pending per-platform
feasibility evidence.

#### Architecture options and decision

| Option | Feasibility | Decision |
|---|---|---|
| Daemon mutex held around each tool/job request | Fails for detached descendants, idle PTYs, persistent LSP/MCP processes, and other CodeGG file APIs that bypass the permit. A request lifetime is not the writer lifetime. | Reject as a universal checkout guarantee. A mutex may remain an internal scheduling primitive only after all effect owners are mediated. |
| OS identity/filesystem boundary around the shared checkout | This is the only plausible route if the guarantee must exclude arbitrary same-user writers. It requires separate native designs and adversarial lifecycle evidence on Linux, macOS, and Windows; current process-group and Job Object ownership alone is not filesystem isolation. | No implementation is qualified. Treat this as a new platform-security project, not a permit-store extension. |
| Restricted CodeGG shared-placement capability profile | Potentially usable if the product boundary excludes external same-user writers and each CodeGG effect either receives a typed checkout permit through verified quiescence or is denied. Initial profile would deny PTY and persistent LSP/MCP/plugin paths and must prove descendant containment for finite shell/build/test jobs per platform. | Selected by ADR-0035, with ADR-0002 isolated placement remaining the default. Do not start Q1 storage until the writer inventory and per-platform capability matrix are complete. |

The feasibility result is therefore conditional, not a blanket “add a lock.”
If users require protection against arbitrary processes running as the same
OS identity, the restricted CodeGG profile cannot satisfy the requirement and
shared checkout placement is a no-go until a qualified OS identity/filesystem
boundary exists. If those external writers are explicitly outside the
contract, a constrained profile may be pursued, but every enabled process and
file-write route still needs native containment evidence.

ADR-0035 accepted the restricted external-writer boundary on 2026-10-16. This
unblocks the M005 feasibility/inventory work only. Q1 remains gated until the
CodeGG-owned writer inventory is closed with a complete per-capability
allow/deny matrix and the effective lifetime of every allowed writer can be
attested. The broad-source-scan caveat above is still open; accepted ADR text
does not upgrade it to a complete inventory.

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
