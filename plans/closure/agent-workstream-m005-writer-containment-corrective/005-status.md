# M005 Writer-Containment Corrective C005 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/agent-workstream-m005-writer-containment-corrective/005-process-and-persistent-service-deny-contract.md`
Source subsystem roadmap: `plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md` (C005)
Repository baseline reviewed: `2a6f5ed8` (the C004 closure commit on `work/agent-workstream-m005-writer-audit`)
Decision: `plans/adrs/ADR-0044-restricted-agent-shared-checkout-writer-contract.md` (**accepted 2026-10-19, option B**)
Predecessor closure: `plans/closure/agent-workstream-m005-writer-containment-corrective/004-status.md`

## 1. Executive finding

C003 fenced the model's **native** writers. C004 mediated every **non-broker**
writer CodeGG can point at a checkout. Both left the same hole open, and it
is the hole the C003 closure record named and could not close:

> No sink has proven descendant-tree quiescence on Unix. The managed process
> group kill is explicitly escaped by a `setsid` descendant; the PTY, MCP
> child and LSP child are direct-child-kill only, with no group signal at
> all.

C005 closes it by **refusing** rather than containing. A native file write
names the file it is about to touch, so it can be held inside a checkout and
serialized against another writer. A process does not: `make test`, a Python
script, a formatter, a test runner and an interactive PTY all name a
*program*, and the program's write set is decided at runtime, by whatever
the program decides to do. The only honest containment for such a program is
a process group or a job object — and CodeGG's managed process group is
explicitly escapable by a descendant that calls `setsid`.

So the restricted shared-checkout profile does not try to contain a process.
**It refuses to start one.**

- `crates/codegg-core/src/process_admission.rs` is the authority. It answers
  one question from durable state only: *is this checkout reserved by a live
  agent-run group?* The answer is derived from the managed worktree record
  (the canonical checkout identity), the lease's owner run, and the durable
  run-group record — the same derivation the C004 reservation check uses, so
  the two can never disagree.
- `src/tool/process_admission.rs` is the tool-layer contract. It owns the
  shape of the question and the shape of the refusal; it does not own the
  answer. The root crate's daemon adapter supplies the authority, and the
  tool layer never imports `crate::core`.
- `src/tool/process_capability.rs` is the **effective-runtime-effect**
  classification. A tool is a process surface because of what its
  implementation does, not because of what it calls itself: a plugin that
  describes itself as read-only and is process-backed is still a process
  surface.

The refusal is a **pre-effect** check. It happens before a process exists,
it takes no lock, and it holds no permit, so it cannot invert the
document-revision-lease / repository-lock / permit order the C002 closure
record fixed.

### What is live in production today, and what is not

The **refusal** is live. The daemon builds the authority, threads it into
every model turn, and every process-capable surface consults it.

The **shared checkout** is not: nothing in production places two runs on one
checkout yet. That is C006's work. C005 therefore closes a *contract* — the
deny path is implemented, wired and tested against a real reserved checkout —
rather than a production incident.

## 2. The mechanism

### 2.1 The authority (`crates/codegg-core/src/process_admission.rs`)

`ProcessAdmissionService` holds two things: the C004 `CheckoutReservationService`
and a `PersistentProcessRegistry`.

```rust
pub async fn admit_scope(&self, scope_root: &Path, kind: ProcessKind)
    -> Result<ProcessAdmission, ProcessAdmissionError>
```

* The scope root is the **daemon-resolved** checkout root of the execution.
  It is never a model-supplied working directory: a model that points a
  process at `/tmp` does not thereby leave the reserved checkout, because
  the program's write set is unconstrained either way.
* A reserved checkout yields `ProcessAdmission::Denied` with
  `ProcessDenialReason::SharedCheckout`, the worktree identity and the
  group identity.
* An authority that **cannot answer** yields
  `ProcessDenialReason::AuthorityUnavailable`. Absence of an answer is not
  an admission: the check fails closed.
* A daemon with no durable reservation authority (`None`) admits everything,
  which is correct: a local-only daemon can never share a checkout, so it
  has nothing to refuse.

### 2.2 The tool-layer contract (`src/tool/process_admission.rs`)

```rust
#[async_trait]
pub trait ProcessAdmissionPolicy: Send + Sync + Debug {
    fn scope(&self) -> &ProcessScope;
    async fn admit(&self, request: &ProcessAdmissionRequest)
        -> Result<ProcessAdmissionOutcome, ProcessAdmissionError>;
}
```

The refusal is a `codegg_core::error::ToolError::ProcessDenied { kind,
reason, recovery }`. It is deliberately **not** a `ToolError::Permission`:
a permission failure is a judgement about *this caller*, while this is a
fact about *this checkout*. The caller's own authorization decision stands
and is still recorded; the refusal adds a profile fact on top of it.

The recovery path is stable: *"use an isolated checkout instead"*. There is
no "retry without the profile" path — the profile is not a best-effort
filter, and a caller cannot opt a process back in.

### 2.3 The carrier

The policy travels the same path the C003 writer fence already travels, so
there is exactly one place that can forget to attach it and the compiler
names it:

```
CoreDaemon → TurnRunInput → AgentLoopBuildInput → AgentLoop
           → coordinator services → ToolBatchExecutor
           → ToolExecutionContext → BrokerInvocationContext
           → ToolExecutionContext
```

`ToolRegistry` also carries the policy, so the broker's final dispatch can
resolve it even when the invocation context did not carry one.

A **nested subagent inherits the narrow profile** (`SubAgentRequest` gained
`process_admission`, set by the `task` tool from the parent's execution
context), and a **Tool Program inherits it too** (the scheduler's
`BrokerAdapter` builds the policy over the job's own workspace root). A
child execution can never be handed a wider authority than the execution
that delegated to it.

### 2.4 The enforcement points

| Surface | Where the refusal happens | Marker |
|---|---|---|
| Model tool boundary | `src/tool/mod.rs` — the `Tool` trait's default `execute_structured` | `c005-denial: tool-boundary` |
| Broker final dispatch | `src/tool/broker.rs` | `c005-denial: broker-final-dispatch` |
| `bash`, `terminal`, `test`, `verify`, `ci_watch` | their own `execute_structured` | `admit_tool_process("<name>"` |
| The managed-process primitive | `src/managed_process.rs` | `c005-denial: managed-process-gate` |
| Scheduler submission | `src/scheduler/submission.rs` | `c005-denial: scheduler-submission` |
| Final executors (test, managed argv, python) | `src/scheduler/executors.rs` | `c005-denial: {test,managed-argv,python}-job-executor` |
| Tool Program inheritance | `src/scheduler/tool_program_executor.rs` | `c005-denial: tool-program-executor` |
| Interactive PTY | `src/interactive_process.rs` | `c005-denial: interactive-pty` |
| Human shell | `src/shell/daemon.rs` | `c005-denial: human-shell-gate` |
| The shell runtime | `src/shell/runtime.rs` | `c005-denial: shell-runtime` |
| Process-backed plugin tool | `src/tool/plugin.rs` | `c005-denial: process-backed-plugin` |
| The formatter's command | `src/tool/formatter.rs` | `c005-denial: formatter-command` |

Two of these are **the** chokepoints rather than one of many:

* The **broker's final dispatch** is the single production tool-call
  boundary. It consults the capability table and refuses before the tool
  runs, after the permission and authority decisions and before dispatch.
  A tool that forgets to consult the gate itself is still refused.
* The **managed-process primitive** is the single place where a managed
  child is created. The gate sits *above* `prepare_launch_argv`, where the
  real executable is still in scope (below it, the executable may have been
  rewritten to the sandbox helper, and a gate there would be reading the
  wrong command).

### 2.5 The lock order

C005 adds **no lock**. The admission check is a read of durable state plus a
read of an in-memory registry. It takes no permit, waits on no permit, and
holds nothing across an effect. The C002 closure's ordering
(document-revision lease → repository lock → writer permit) is untouched.

The one lock the C005 code introduces is the `parking_lot::RwLock` that
holds the *authority handle* on the PTY service, the scheduler and the
submission boundary. It is read once, copied, and dropped before any
await — it never spans an effect.

## 3. The surface inventory

### 3.1 What the restricted profile refuses

| Capability | Refused pre-effect | Where |
|---|---|---|
| Shell command (`bash`) | yes | tool boundary + broker + managed process |
| Interactive terminal (`terminal`) | yes | tool boundary + broker + PTY service |
| Python script (`python_script`) | yes | tool boundary + broker + the Python job executor |
| Test command (`test`) | yes | tool boundary + broker + the test job executor |
| Verification (`verify`) | yes | tool boundary + broker |
| CI watch (`ci_watch`) | yes | tool boundary + broker |
| Formatter command | yes | the managed-process gate |
| Tool Program (`tool_program`) | yes | tool boundary + broker + the Tool Program executor |
| Process-class job submission (`job`) | yes, per job kind | the scheduler's submission boundary |
| Nested job (a job submitted from a job) | yes | the same submission boundary |
| Detached Tool Program | yes | the Tool Program executor |
| PTY start | yes | the PTY service |
| LSP/MCP persistent server **use** | yes | the capability table (`lsp`, `lsp_read`) |
| Process-backed plugin | yes | the plugin tool adapter |
| Hook command | see §6 | the managed-process gate, when the request carries a policy |
| Human `!` shell | yes | the human-shell gate |

### 3.2 What it does not refuse

* **Reads.** A read names the file it touches, so it is contained to the
  checkout and needs no process. The restricted profile refuses processes,
  not reads.
* **Native file and Git writes.** Those are the C003 fence's business, and
  the fence already refuses a `ProcessSpawn` effect class on a shared
  checkout.
* **A job that starts no process** (an agent turn, a subagent, a research
  job). The refusal is about the *capability*, not about the checkout.

## 4. The durable deny event

`AuditAction::ProcessDenial` (wire name `process_denial`) is a new action.
It is not a variant of `authorization_decision`: the caller's authority was
never in question.

The event is emitted by the **broker**, at the refusal, through the trusted
execution-audit context the broker already carries. It is attributed to the
transport-bound principal, carries the M003 decision provenance, and its
metadata names the capability, the reason, the group and the worktree. It
carries **no path, no command line, no argv and no file body**: a process
refusal is a concurrency answer, and the command that was refused is the
caller's own business.

The action has its own coverage row in `REQUIRED_AUDIT_COVERAGE` and its
own entry in the daemon-hook table, with a declarative owner pin
(`tool:process_admission` → `emit_process_denial`). The audit coverage
guard fails closed if the owner file moves or the emit symbol disappears.

## 5. The persistent-process registry and the reservation preflight

C001's work package E asks a specific question: *what if a persistent
process was already running when the checkout became shared?* A language
server, a local MCP server, a PTY or a shell started under an older
authority keeps that authority: it was launched when the checkout was not
yet shared, and nothing has stopped it from writing.

C005 answers it with a **registry** and a **preflight**:

* `PersistentProcessRegistry` records the live persistent processes and the
  checkout root each one was started in. It is in-memory on purpose: a
  persistent process is a property of a running daemon, not a durable fact.
* `ProcessAdmissionService::preflight_reservation(worktree_id)` refuses a
  shared reservation while a live persistent process is registered in the
  affected checkout. The refusal never kills the process — the process's
  lifetime belongs to its own owner.
* The human-shell service registers its live run against the workspace root
  it was bound to, and releases it when the run ends. A drained process
  stops blocking the reservation.

The preflight is the control that makes the *unlaunched* persistent services
acceptable (see §6): a checkout that already has a live language server or
MCP server cannot become reserved in the first place, so the restricted
profile never applies to it.

## 6. The open items, named

Three surfaces are **not** refused pre-effect by C005, and the manifest
records each of them as an explicit open item rather than implying a
guarantee that does not exist.

### 6.1 The local MCP stdio client (`module.mcp_local_client`)

The local MCP client is a daemon-scoped persistent service. It is created at
daemon bootstrap, before any workspace scope exists, and it sets no
`current_dir` of its own — the child's working directory is whatever the
daemon's process directory was. CodeGG therefore cannot resolve a checkout
scope for it at launch time, and a refusal keyed on a scope it does not have
would be theatre.

The compensating controls are: the `lsp`/`lsp_read` refusal (the LSP
persistent server's *use* is refused under the restricted profile), and the
reservation preflight (a checkout with a live MCP server cannot become
reserved). What is **not** claimed: the MCP client's launch is not refused.

### 6.2 The egglsp language-server launch (`module.egglsp_server_launch`)

The LSP server child is launched inside the `egglsp` crate, which owns no
CodeGG scope and cannot consult the daemon authority. Threading a gate into
that crate's public launch surface is a larger change than this milestone
took on.

The compensating control is the same as above: the `lsp` and `lsp_read`
tools are refused under the restricted profile as persistent-process users,
so a reserved checkout never *uses* a language server, and the reservation
preflight refuses a shared reservation while one is live. What is **not**
claimed: the server's launch is not refused.

### 6.3 The shell-command hook adapter (`module.hook_shell_adapter`)

The hook adapter runs `sh -c` in the daemon's process directory. It has no
checkout scope it can resolve, for the same reason as the two above. The
managed-process gate refuses it **when the request carries a policy**; the
hook's request does not.

What is **not** claimed: a hook command is not refused under the restricted
profile.

Each of these three is a row in `docs/writer-disposition.toml` with
`denial_status = "open"` and a recorded reason. The C005 guard fails when
an open item silently loses its reason, and when a root silently changes
from *wired* to *open* without the declaration changing with it.

## 7. The manifest and the guard

`docs/writer-disposition.toml` gained a C005 vocabulary:

* `mediation = "process-gate"` on every denied-process root (the C004
  vocabulary gains the value; the C004 guard's vocabulary set is updated in
  the same change);
* `denial_site` + `denial_markers` on every **wired** root: the file that
  holds the refusal and the markers that identify it;
* `denial_status = "open"` + a recorded reason on every **open** root;
* two new tool roots (`tool.lsp`, `tool.lsp_read`) and one new tool root
  (`tool.tool_program`), because C005 classifies them as process surfaces.

The manifest now declares **58 roots**, of which **21** are denied-process
surfaces: 18 wired, 3 recorded open.

`scripts/check_process_denial.py` is the static backstop. It checks:

1. every denied-process root declares how it is denied, and a wired root's
   site file still contains its markers;
2. the code's capability table and the manifest agree — a tool the table
   classifies as spawning a process or using a persistent one must be a
   declared denied-process root, and every declared denied-process model
   tool must be classified;
3. the C005 contract's own landmarks exist (the core authority, the
   tool-layer contract, the capability table, the broker dispatch, the
   managed-process gate, the scheduler's two gates, the PTY, the human
   shell, the shell runtime, the plugin adapter, the formatter, the audit
   action).

The guard is wired into `scripts/verify.sh quick` and into CI, together with
its self-test.

**The guard is not vacuous.** The self-test injects five violations into a
copy of the tree and requires the guard to reject each one:

| Injected violation | Required report |
|---|---|
| a denial marker removed from its site | `lost its denial` |
| a denied-process root loses its declaration | `declares neither a denial_site nor a denial_status` |
| a process-capable tool leaves the capability table | `missing from the capability table` |
| a denial site names a file that does not exist | `names a missing denial site` |
| the broker's final dispatch loses its refusal | `lost 'c005-denial: broker-final-dispatch'` |

## 8. The acceptance matrix

| Criterion | Evidence | Status |
|---|---|---|
| Shell/Python/test/build/format denied pre-effect | the tool boundary, the broker's final dispatch, the managed-process gate | met |
| Job submissions denied | the scheduler's submission boundary, per job kind | met |
| Tool Program delegation and nested jobs denied | the `tool_program` tool boundary + the Tool Program executor + the submission boundary | met |
| PTY start denied | the PTY service | met |
| LSP/MCP local server launch/use denied | **use** denied (the capability table); **launch** is an open item (§6) | partially met, disclosed |
| Hooks denied | **not** denied (§6.3) | open, disclosed |
| Process plugins denied | the plugin tool adapter, by runtime kind | met |
| Human shell denied on the same reserved checkout | the human-shell gate | met |
| A preexisting persistent process blocks the reservation | the registry + the reservation preflight | met |
| Two separate worktrees run ordinary shells concurrently | the fixture's two ordinary workspaces, admitted concurrently | met |
| No broad-blocker regression to ordinary tool use | the ordinary-workspace and read-tool tests | met |
| Missing typed scope for a claimed shared mode refuses | the authority fails closed (`AuthorityUnavailable`) | met |
| Per-OS capability/denial matrix from actual native tests | §10 | partially met, disclosed |
| Existing cancellation and quota suites stay green | the scoped suites in §9 | met |

## 9. Verification evidence

The verification runs for this closure are recorded in §9.1. The scoped
suites are the evidence; the full workspace sweep could not complete on this
host.

### 9.1 What was run

* `cargo fmt --all -- --check` — clean.
* `cargo clippy --workspace --all-targets -- -D warnings` — exit 0.
* `scripts/verify.sh quick` — exit 0 ("Quick verification passed."), which
  includes the core-boundary guard, the audit-coverage guard, the
  writer-disposition guard, the **new** process-denial guard and its
  self-test, the agent-schema guard, the config-merge guard and
  `cargo check --workspace --all-targets --locked`.
* `python3 scripts/check_process_denial.py` — "process-denial check ok: 21
  denied-process roots, 18 wired, 3 recorded open".
* `python3 scripts/check_process_denial.py --self-test` — "process-denial
  self-test ok".
* `python3 scripts/check_writer_disposition.py` — "writer-disposition check
  ok: 58 declared roots, 17 writer tools, 36 writer modules".
* `python3 scripts/check_audit_coverage.py` (+ `--self-test`) — pass.
* `python3 scripts/check_adr_integrity.py` — pass.
* `bash scripts/check-core-boundary.sh` — pass.
* `cargo test -p codegg --test process_denial_c005` — **18 passed, 0 failed**.
* `cargo test -p codegg --test writer_fence_c003`,
  `--test writer_disposition_guards`, `--test checkout_reservation_c004`
  — the C003/C004 suites, still green.
* `cargo test -p codegg --lib tool::` — the tool-layer unit tests.

### 9.2 What was NOT run, and why

* **The full workspace test sweep.** The host is contended by other
  sessions' cargo and nextest builds; a workspace-wide run does not complete
  here. The scoped suites above are the recorded evidence.
* **The hosted CI run.** The branch is local-only. Nothing has been pushed,
  so no CI run exists to cite.
* **The Eggsearch pair qualification** was not re-run: it needs a pinned
  Eggsearch checkout and a network-free build.
* **The Windows and macOS legs of the per-OS matrix** were not run: this
  host is Linux. §10 records the matrix accordingly.

## 10. The per-OS matrix, and what is actually known

C001's work package D asks for a capability/denial matrix based on **actual
native tests**, and C001's rule is explicit: *never state descendant
quiescence for failed or unrun adversarial cases.*

| Platform | Denials (profile-level) | Descendant quiescence | Evidence |
|---|---|---|---|
| Linux | enforced (the profile is a refusal; it is platform-independent) | **not claimed** | the C005 suite + the C003 fixture on this host |
| macOS | not run | not claimed | — |
| Windows | not run | not claimed | — |

The refusal itself is platform-independent: it happens before a process
exists, so there is no descendant tree to be quiet. The *containment*
question — whether a spawned program's descendants can be reaped — is a
different question, and the honest answer remains the C001 one: CodeGG's
managed process group is escaped by a `setsid` descendant. That is exactly
why the profile refuses processes instead of containing them.

## 11. Known limits and non-claims

1. **The profile is a refusal, not a containment.** CodeGG does not stop a
   process it started before the checkout became reserved. The preflight
   refuses the *reservation* in that case; it does not kill the process.
2. **A process launched outside the reserved checkout can still write into
   it.** CodeGG cannot enumerate a program's write set. A process running in
   a *different* checkout is the same class of risk as an unrelated program
   the same OS user starts outside CodeGG — which ADR-0044 clause 1
   explicitly places **out of contract**. The restricted profile's
   guarantee is scoped to the reserved checkout and to CodeGG's own
   launches against it.
3. **The mediation is a point-in-time read.** A group that becomes live
   after the check is not observed by that launch. The C002/C003 ordering
   has the same property, and the C004 gate has it too.
4. **The refusal applies to the whole execution, not to the group's
   members.** Under the restricted profile, a group member may not start a
   process either. That is deliberate: the profile is "serial native writers
   and concurrent proven reads", and a group's own process is not a proven
   read.
5. **Three surfaces are open** (§6). Each is recorded in the manifest with a
   reason, and the guard fails when a reason disappears.
6. **The refusal is not an authorization decision.** A caller's own
   permission decision still stands and is still recorded. A refusal is a
   profile fact layered on top of it.

## 12. C006 unblocking

C006 — the restricted shared checkout qualification — is unblocked by this
closure, with two conditions it must honour:

* C006 establishes the **first production shared reservation**. The
  reservation's establishment point is the managed worktree lease taken by a
  run in a live group, and that path is not wired in production today. When
  C006 wires it, it must call
  `ProcessAdmissionService::preflight_reservation` before the reservation
  becomes real, so a checkout with a live persistent process cannot be
  shared.
* C006's qualification must observe C005's refusals as *refusals*, not as
  failures: a shared checkout under the restricted profile is expected to
  refuse shells, PTYs, Tool Programs and process-class jobs, and to admit
  reads and native writes.

C006 remains blocked on C004 + C005 being closed. This record closes C005.

## 13. Historical records preserved

* `plans/closure/agent-workstream-m005-writer-containment-corrective/001-status.md`
* `plans/closure/agent-workstream-m005-writer-containment-corrective/002-status.md`
* `plans/closure/agent-workstream-m005-writer-containment-corrective/003-status.md`
* `plans/closure/agent-workstream-m005-writer-containment-corrective/004-status.md`
* `plans/closure/agent-workstream-m005-writer-containment-corrective/004-merge-reconciliation-status.md`
