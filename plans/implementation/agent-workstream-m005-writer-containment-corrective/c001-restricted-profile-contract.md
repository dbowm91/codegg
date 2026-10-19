# C001 — Restricted Profile Decision and Writer-Path Matrix

Milestone: **M005 writer-containment corrective C001**
Status: **decision/negative-test handoff only — creates no permit and enables no writer**
Baseline SHA: `e78497d2` (worktree `work/agent-workstream-m005-writer-audit`)
ADR: `plans/adrs/ADR-0042-restricted-agent-shared-checkout-writer-contract.md` — **`proposed`, NOT accepted** at C001 close. **Accepted 2026-10-19 (option B)** — see the ADR's "Acceptance record" section. Acceptance unblocks C002 and enables nothing.

## 1. What this document is, and what it is not

This is the C001 **contract**: the complete writer-root inventory for CodeGG, the
restricted-profile policy each root receives, the user-visible guarantee that
follows, the lock order the successor milestone must implement, and the guard
that keeps the inventory honest.

It is **not** authority. C001 enables nothing:

- No writer permit, fence, lease, lock or reservation is created here.
- `fenced-native` marks a **candidate** for C002/C003 to consider. It does not
  fence anything today, and it must not be read as "now safe".
- ADR-0042 remained `proposed` at C001 close, and C002–C006 stayed blocked
  until a maintainer recorded an explicit accept/reject decision. **C001 closed
  with the decision outcome "still proposed"**, which is an honest outcome, not
  a blocked one: C001's own scope is the decision package, and it is complete.
  *(Subsequently: the maintainers accepted option B on 2026-10-19 — see
  §10.1. Acceptance unblocked C002 and enabled nothing.)*

The one thing this document *does* do is make an undecided writer visible. A
new model-facing writer tool, or a new daemon/HTTP module that touches the
filesystem, now fails `scripts/check_writer_disposition.py` until someone
declares its disposition. That is the durable C001 output.

## 2. Vocabulary

The C001 policy vocabulary (from the milestone plan, work package B), with the
audit vocabulary mapped onto it:

| C001 disposition | Audit vocabulary | Meaning under the restricted profile |
|---|---|---|
| `allowed-read` | `read-proven` | Proven read-only. Nothing to restrict. |
| `fenced-native` | `fenced-native-candidate` | Native file/Git writer. **Candidate** for the C003 fenced transaction. Not fenced today. |
| `denied-process` | `denied-process` | Process-backed or persistent. Denied before launch or before effect. |
| `fenced-or-denied` | `fenced-or-denied` | CodeGG non-broker writer. Fence it, or refuse it. |
| `unsupported` | — | Platform / node / NonGit: not supported here. |
| `out-of-contract` | `out-of-contract`, `private-scratch` | Outside the restricted profile's checkout guarantee. |

A second axis records **scope**: `checkout-writer`, `non-checkout`,
`read-only`, `external`. The two axes are independent on purpose: a root can
be a checkout writer whose disposition is `denied-process`, and a root can
write managed state that is simply outside the checkout guarantee
(`non-checkout` / `out-of-contract`).

## 3. Scope boundary — what the guarantee covers, and what it does not

ADR-0042 option B is a **restricted CodeGG-managed** contract. It coordinates
CodeGG-initiated mutations. It does **not** exclude unrelated same-UID
programs.

| In scope | Out of scope |
|---|---|
| Every CodeGG-initiated mutation: agent turns, jobs, daemon APIs, and authenticated human CodeGG operations | An editor the user runs, a shell they open, another agent, a build script outside CodeGG |
| Every CodeGG writer: model tools, non-broker daemon/HTTP sinks, project-local state | The OS itself: no filesystem exclusion, no process-group containment claim |
| Project-local state that lives physically inside the checkout | Independent same-UID writers sharing the same checkout |

**The user-facing label must say "CodeGG-mediated serialized checkout" and
list the exact unsupported operations.** It must never say *exclusive*,
*sandboxed*, or *filesystem isolated* — CodeGG cannot make any of those true
about a shared checkout.

The audit confirmed the boundary is real, not rhetorical: 13 distinct paths
under `<project>/.codegg/` are **physically inside** the checkout, and the
legacy project database is keyed to the **process cwd**. A shared-checkout
contract that ignored project-local state would be incomplete.

## 4. The writer matrix

48 writer roots. Every row is backed by a source audit with `file:line`
citations; the full evidence, including the adversarial bypass worked
examples, is in `c001-evidence/`.

| # | Writer root | Final effect owner | Checkout locator | Authority path | Lifetime | Disposition | Existing coordination (limits) | Adversarial bypass |
|---|---|---|---|---|---|---|---|---|
| 1 | `write` (tool/write.rs) | `WriteTool::execute` — a `tokio::task::spawn_blockin | the tool's registry-supplied `allowed_ro | broker pre-execution validation plus the | request-scoped. The blocking closure i | `fenced-native-candidate` | batch-local only. The batch executor takes a per-repository | session A's agent calls `write` on `src/lib.rs` while session B's agen |
| 2 | `edit` (tool/edit.rs) | `EditTool::execute` — a `spawn_blocking` closure tha | registry-supplied `allowed_root`; the mo | broker validation + permission registry; | request-scoped; the blocking closure i | `fenced-native-candidate` | none at the sink. No repository lock, no atomic replace, no | a child run in a worktree and the parent session both `edit` the same |
| 3 | `replace` (tool/replace.rs) | `ReplaceTool::execute` — `spawn_blocking` closure th | registry-supplied `allowed_root`, valida | broker validation + permission registry | request-scoped; the blocking closure i | `fenced-native-candidate` | the batch treats `replace` as restorable for checkpointing ( | a `replace` call writes a path the operator never saw in the approval |
| 4 | `apply_patch` (tool/apply_patch.rs, tool/pat | `ApplyPatchTool` — parses a unified diff (`patch_uti | registry-supplied `allowed_root`; model- | broker validation + permission registry. | request-scoped; applied inline within | `fenced-native-candidate` | `apply_patch` is restorable, so it participates in the batch | an `apply_patch` in session A and a `write` in session B target the sa |
| 5 | `lsp_preview_apply` (tool/lsp_preview_apply. | `crate::lsp::mutation::apply_preview` — the only nat | the tool's registry-supplied `workspace_ | the strongest in this cluster. A preview | request-scoped, but the effect is dura | `fenced-native-candidate` | repository lock + digest + per-file hash + atomic write + ro | a `write` in another session lands between the LSP preview's hash chec |
| 6 | `git` typed local mutation (tool/git.rs → sr | `GitMutationExecutor::execute` — renders argv and ru | model-supplied `workdir`, unvalidated ag | broker validation + permission registry, | the `git` child process outlives the r | `fenced-or-denied` | no lock at all. `GitMutationExecutor::execute` takes no repo | the model supplies `{"mutation":"stage_paths","paths":["x"],"workdir": |
| 7 | `git` network operation (tool/git.rs → src/g | `crate::git_network_ops::{fetch, pull, push, remote_ | the same unvalidated, model-supplied `wo | broker validation + permission registry | the `git` child process is awaited; th | `fenced-or-denied` | env hardening, audit emission, and RunStore persistence. `sr | two sessions both call `{"mutation":"push","remote":"origin"}` against |
| 8 | `git` recovery (continue / abort / skip) (to | `crate::git_recovery::continue_in_progress` and sibl | the model-supplied `workdir`. `src/tool/ | broker validation + permission registry | the `git` child is awaited; the result | `fenced-or-denied` | state pre-detection prevents mis-targeted recovery. `src/git | session A runs `{"recover":"continue"}` on a conflicted merge while se |
| 9 | `git` raw-subcommand fallback (tool/git.rs) | the `git` binary itself, invoked as `git <model-supp | model-supplied `workdir`, existence-chec | the read-only gate (`read_only_only`) an | the `git` child is awaited; the effect | `denied-process` | the read-only gate and env hardening only. `src/tool/git.rs: | `{"subcommand":"-c","args":["core.pager=cat"],"workdir":"/etc"}` — or |
| 10 | `commit` (tool/commit.rs) | `CommitTool::execute_with_context` → `commit_with_se | the registry-supplied `self.workdir` — N | broker validation + permission registry; | the `git` child is awaited; the commit | `fenced-or-denied` | env hardening, snapshot/state-delta via the executor, and Ru | session A stages and commits while session B's `write` modifies a trac |
| 11 | `formatter` (tool/formatter.rs) | `rustfmt`, spawned as a managed process — the rewrit | the paths handed to `rustfmt`. The tool | broker validation + the managed-process | the `rustfmt` process is a child; it i | `denied-process` | `formatter` is classified as a *write* by the risk classifie | session A runs `formatter` on `src/lib.rs` while session B runs `edit` |
| 12 | `task` — delegated child run admission (tool | not this tool. `TaskTool` performs no file, index, o | the child's root is bound by the schedul | job admission through `JobSubmissionServ | outlives the request. The child is a d | `out-of-contract` | a managed worktree with a durable lease when the child has a | a parent turn spawns a `task` child whose workspace has no repository |
| 13 | worktree create / lease / release / cleanup | `WorktreeService::{reserve, create_reserved, acquire | the managed worktree path is computed fr | a durable, SQLite-backed record with an | durable across the request; the record | `fenced-native-candidate` | the strongest existing coordination in this cluster — durabl | a child run's worktree is cleaned up (removed from disk) while a nativ |
| 14 | HTTP `POST /api/file/write` and `DELETE /api | `routes::write_file` calls `tokio::fs::write` direct | `sanitize_path_from_root` canonicalizes | `authz::authorize_project` with `Capabil | request-scoped; the write is awaited b | `fenced-or-denied` | project capability authorization and path containment only. | a frontend client issues `POST /api/file/write` for `src/lib.rs` while |
| 15 | `git_query` / `git_read` (src/tool/git_read. | none — genuinely read-only. `GitReadTool` / `GitQuer | the registry-supplied workdir / broker-s | operation allow-list plus `deny_unknown_ | request-scoped, read-only. | `read-proven` | the read surface is genuinely read-only — it is a true read- | none established — this surface cannot write. It is listed here so the |
| 16 | `bash` — model-originated finite command (fo | `ManagedProcessService::run` — the single managed-pr | `canonical_workdir`, resolved from the t | `request.execution_authorization = autho | The child is made a session leader via | `denied-process` | Scheduler admission gates process slots and CPU/IO/memory (` | `bash` runs `sh -c 'setsid sh -c "while :; do printf x >> $PWD/shared. |
| 17 | `bash` background / scheduler `JobKind::Shel | `ManagedArgvExecutor` (`src/scheduler/executors.rs:2 | the durable job payload's `cwd` string ( | durable acceptance through `JobSubmissio | The job is durable and runs after the | `denied-process` | Scheduler queue + admission (`src/scheduler/admission.rs:164 | submit a background `bash` job that writes `shared.txt` in a loop, the |
| 18 | `terminal` — one-shot managed shell (model-o | `ManagedProcessService::run` (`src/tool/terminal.rs: | the tool's `workdir`; a terminal constru | `check_command_security` pattern check p | `sh -c <command>` under the shared man | `denied-process` | Permission prompt for `Mutating`; pattern-based command chec | `terminal` runs `sh -c 'setsid sh -c "printf x >> $PWD/shared.txt" & s |
| 19 | `test` / build / lint / format scheduler job | `ManagedArgvExecutor` for `Build`/`Lint`/`Format`/`M | the job payload `cwd` (`src/scheduler/ex | scheduler admission permit must be live | Durable scheduler jobs; the process ou | `denied-process` | `JobKind::Build` and `JobKind::Format` hold `exclusive:works | a `test` job (`JobKind::Test`, no exclusivity key) runs a test fixture |
| 20 | Python script tool (`JobKind::Python`) | `PythonJobExecutor` (`src/scheduler/executors.rs:146 | the request's `cwd`, validated against a | sandbox spec when present → `SandboxRequ | The interpreter is launched as a manag | `denied-process` | Scheduler admission; `JobKind::Python` carries no exclusivit | a Python script calls `os.setsid()` then `os.fork()`, and the child ap |
| 21 | PTY attach / interactive process (long-lived | `InteractiveProcessService` — declared the "single p | `ExecutionContext::resolve_relative_cwd( | Scheduler admission before any PTY or ch | Long-lived by design and explicitly no | `denied-process` | One scheduler process slot per PTY (`src/interactive_process | a human opens the TUI interactive terminal and runs `setsid sh -c 'whi |
| 22 | local MCP stdio child (persistent) | `LocalClient` in `src/mcp/local.rs`; the manager tha | none. `LocalClient::new` spawns with `Co | operator config only. `connect_from_conf | Persistent by construction — the child | `denied-process` | MCP server configuration and a per-call timeout (`src/mcp/lo | an operator-configured local MCP server that, on `initialize`, forks a |
| 23 | LSP server child (persistent) | `egglsp`'s authoritative runtime — `crates/egglsp/sr | the LSP root path, passed as `Some(root) | LSP server configuration and the daemon- | Persistent across the daemon's life. ` | `fenced-native-candidate` | The LSP *edit* itself is applied by CodeGG natively, not by | an LSP server that, on `initialize`, writes or appends directly to a f |
| 24 | process-backed plugin runtime | `ProcessRuntime` in `src/plugin/runtime/process.rs`; | `self.spec.cwd` if set, otherwise `std:: | plugin trust/policy class — `PluginRunti | Bounded by the managed-process scheme | `denied-process` | plugin trust class + lifecycle policy; a timeout (`src/plugi | an installed process plugin whose command is `sh` with args `-c 'setsi |
| 25 | hook shell adapter (`ShellCommandHook`) | `ShellCommandHook::execute` (`src/hooks/mod.rs:115-1 | `std::env::current_dir()` (`src/hooks/mo | operator hook configuration (`src/hooks/ | Bounded by the managed-process timeout | `denied-process` | hook timeout and output limits (`src/hooks/mod.rs:132-134`). | a configured `pre_tool_execute` hook running `sh -c 'setsid sh -c "pri |
| 26 | formatter (`Formatter::format_file`) | `Formatter::format_file` (`src/tool/formatter.rs:22` | the parent directory of the file being f | none. The request is built with `Managed | Bounded by a 120 s timeout (`src/tool/ | `denied-process` | timeout and output cap (`src/tool/formatter.rs:72-73`). Limi | a formatter rule whose command is `sh -c 'setsid sh -c "printf x >> $P |
| 27 | Tool Program / `task` job delegation (schedu | `ToolProgramExecutor` for Tool Programs and `Subagen | the Tool Program's workspace root is tak | the Tool Program is bound to a frozen ru | The Tool Program can outlive the reque | `denied-process` | frozen contract snapshot, path-policy fingerprint, backgroun | a tool whose `contract().effect_class` is declared `ReadOnly` but whos |
| 28 | human `!` shell (human-originated — not mode | `ShellRuntime::spawn` (`src/shell/runtime.rs:60-64`) | the daemon-owned workspace host's `works | the human principal (`src/core/daemon_hu | Streaming, cancellable, bounded by the | `denied-process` | command policy, sandbox profile, workspace-scoped cwd, and a | a human runs `! sh -c 'setsid sh -c "printf x >> $PWD/shared.txt" &'` |
| 29 | native git dispatch from `bash` (process-bac | `ManagedProcessService::run` (`src/tool/bash/process | `canonical_workdir`, else `input_workdir | none at the process level. The request i | Bounded by the managed-process timeout | `denied-process` | routing metric and executor record (`src/tool/bash/process.r | a repository whose `.git/hooks/post-checkout` or a configured `core.fs |
| 30 | HTTP file write | the axum handler itself — `tokio::fs::write(&full, & | `resolve_context` (`src/server/scope.rs: | route disposition `SharedAuthz` / operat | request-scoped. The write completes in | `fenced-or-denied` | project-level capability authorization only | a TUI editor save of the same file and an HTTP write of the same |
| 31 | HTTP file delete | the axum handler itself — `tokio::fs::remove_file(&f | identical to HTTP file write — `resolve_ | `authz::authorize_project(... Capability | request-scoped. | `fenced-or-denied` | capability authorization + path containment. Does | the TUI editor has the file open and holds a writer lease, and a |
| 32 | daemon document save (human-originated edito | `crate::lsp::mutation::checked_workspace_text_write` | `self.workspaces.resolve(&workspace_id)` | three layers. | request-scoped. The write completes in | `fenced-native-candidate` | writer lease, per-document operation lock, revision | an external IDE (not CodeGG) saves the same file after the daemon |
| 33 | daemon document delete | `crate::lsp::mutation::checked_workspace_file_delete | `self.workspaces.resolve(&workspace_id)` | daemon request authorization maps `CoreR | request-scoped. | `fenced-native-candidate` | project/workspace binding check, attached-document | the same file is concurrently deleted over HTTP |
| 34 | daemon document create | `crate::lsp::mutation::checked_workspace_file_create | `self.workspaces.resolve(&workspace_id)` | daemon request authorization maps `CoreR | request-scoped. | `fenced-native-candidate` | capability authorization, workspace verification, | an HTTP write creates `foo.rs` at |
| 35 | LSP preview apply (host applyEdit path) | `crate::lsp::mutation::apply_preview` | `workspace_root.canonicalize()` (`src/ls | three gates before the write. | request-scoped for the write; the chec | `fenced-native-candidate` | reviewed-digest revalidation, per-file original-hash | an HTTP write to one of the preview's target files lands after |
| 36 | project init AGENTS.md publish | `publish_checked` (`src/core/daemon_project_init.rs: | `resolve_project_workspace` (`src/core/d | a single-use, scope-bound, expiring draf | request-scoped for the write. The draf | `fenced-native-candidate` | single-use scope-bound draft token, stale-root | an HTTP write replaces `AGENTS.md` between the draft's digest |
| 37 | edit-checkpoint undo / reapply (the live sna | `EditCheckpointManager::checked_restore` | the manager is constructed with `record. | the checkpoint must match the caller's w | deferred and persistent in the sense t | `fenced-native-candidate` | workspace + session binding checks, per-path | the HTTP write changes a checkpointed file to content that happens |
| 38 | legacy SnapshotManager restore (codegg-core | `SnapshotManager::restore` | the manager is constructed with a root p | none established. The module's own resto | request-scoped when called. | `out-of-contract` | traversal rejection on `restore_to_path` | not applicable today, because there is no production caller. If |
| 39 | TUI editor save (frontend entry point) | not a separate final effect owner. `save_editor` | the controller does not resolve a root; | the client-side controller holds the dae | request-scoped, with the draft held cl | `fenced-native-candidate` | everything the daemon document save provides, plus a | the human edits a file in the TUI while an HTTP write or an |
| 40 | IDE temporary diff copies | `tempfile::Builder` temp files — `src/ide/mod.rs:136 | none. The module never takes a workspace | none required — the write target is not | ephemeral. The temp files are removed | `private-scratch` | the diff tool is launched with content copied into | none available — this sink cannot touch the checkout. |
| 41 | Workspace run storage — `<workspace>/.codegg | `codegg_core::run_store::FsRunStore` | INSIDE the checkout tree. | per-run identity is `RunId::new()` minte | Before any model turn, and without a r | `fenced-or-denied` | Per-instance serialization only. `FsRunStore.lock` is a `tok | the rename-vs-append lost update. Instance A |
| 42 | Legacy project session database — `<project> | `codegg_core::storage::init_migrated_legacy_project_ | INSIDE the checkout tree. | the file path is derived from cwd, not f | Before any model turn, and without a r | `fenced-or-denied` | SQLite's own file locking plus `busy_timeout` on the connect | two `codegg sessions` invocations started concurrently in the |
| 43 | Project-local `.codegg/` state family (remai | 12 distinct owners: context::artifact::FileArtifactStore, research::service::ResearchService, tool::tool_program_ledger::ToolProgramLedger, and others (c001-evidence/04) | INSIDE the checkout tree. | identity is the *workspace/project root | mostly during a model turn, not before | `fenced-or-denied` | Atomic temp-file + rename for context artifacts (`src/contex | a model turn in group B, inside a shared checkout that group A |
| 44 | Managed private scratch root | `codegg::scratch::session_scratch_dir` / `agent_run_ | OUTSIDE the checkout — and | identity is host-supplied, never model-s | created on demand, per session and per | `private-scratch` | Ownership + permission enforcement: `ensure_private_parent` | a scratch leaf is created under the user cache root, so a |
| 45 | User config root (and global skill / agent / | `codegg_core::storage::DaemonPaths` | OUTSIDE the checkout, with | `CODEGG_DATA_HOME` overrides the data ro | created at config/catalog initializati | `out-of-contract` | per-user filesystem ownership. It does not | a process started with `HOME` unset and no `XDG_*` variables |
| 46 | Daemon catalog / session database and daemon | `codegg_core::storage::init_migrated_daemon_catalog` | OUTSIDE the checkout. | the daemon is a user-scoped singleton: t | strictly before any model turn — the d | `out-of-contract` | singleton `flock` on `daemon.lock` (the daemon home | an operator exports `CODEGG_DATA_HOME=/shared/checkout/.codegg-data` |
| 47 | Plugin store and plugin data | `codegg::plugin::install::plugins_dir` | OUTSIDE the checkout. | plugin id. `validate_uninstall_target` | at plugin install/uninstall time — a h | `out-of-contract` | install/uninstall target containment relative to | with no `XDG_DATA_HOME`/`HOME` resolvable, `plugins_dir()` |
| 48 | Managed worktree root and worktree metadata | `codegg_core::worktree_service::WorktreeService` | OUTSIDE the checkout in the | `WorktreeId::new()` + a `codegg/worktree | worktree creation is a user/agent-init | `fenced-native-candidate` | Native fence: the "outside the repository root" invariant | the daemon starts with no injected `worktree_service`, so |

### 4.1 Disposition totals

| Disposition | Count | What it means for the restricted profile |
|---|---|---|
| `fenced-native-candidate` | 15 | Native file/Git writers, and the non-broker writers that already carry a native fence in-tree. **Candidate** for the C003 fenced transaction. |
| `denied-process` | 15 | Process-backed or persistent writers. Denied before launch or effect. |
| `fenced-or-denied` | 10 | CodeGG non-broker writers, including the project-local `.codegg/` state family. Fence them, or refuse them. |
| `out-of-contract` | 5 | Outside the checkout guarantee: external same-UID programs, dormant APIs, and managed state outside every checkout. |
| `private-scratch` | 2 | The managed scratch root and temporary IDE diff copies. Not checkout writers. |
| `read-proven` | 1 | `git_read` / `git_query`: allow-listed subcommands, `deny_unknown_fields`, no `workdir` field. |

Two entries deserve emphasis because they are easy to misread:

- **`tool_program` is `ReadOnly`-labelled, and that label is enforced.** A Tool
  Program's delegated tools must be program-callable *and* carry a read-only
  effect class (`src/tool/tool_program_context.rs:200-242`). The default
  contract is `NonIdempotent`, so the palette is **fail-closed**: a tool must
  explicitly opt in. But the *background* job surface is a separate matter —
  a program may be submitted in `background` mode and outlive its request, so
  the program surface is `denied-process` while its delegated palette is
  `allowed-read`.
- **`lsp_read` is `ReadOnly`, but it is backed by a persistent server.** The
  read-only tool is not the risk; the LSP *server* is a long-lived process
  that can write. A read-only tool backed by a writer-capable server is
  exactly the distinction work package B demands.

## 5. Policy matrix (work package B)

The profile is a **two-axis** decision. Axis one is whether the root can
mutate a checkout. Axis two is what the restricted profile does about it.

| Scope | Disposition | Rule |
|---|---|---|
| `checkout-writer` | `fenced-native` | Allowed only through the C003 fenced native transaction, under a C002 permit. Until C002 lands, the restricted profile must **refuse** it. |
| `checkout-writer` | `denied-process` | Refuse before launch or before effect. Not a candidate for v1: the profile excludes arbitrary compilation and test commands inside a shared checkout. |
| `checkout-writer` | `fenced-or-denied` | Fence it, or refuse it. A non-broker writer may not be left unfenced. |
| `checkout-writer` | `unsupported` | Non-Git mutable shared execution is not supported. |
| `read-only` | `allowed-read` | Reads and model reasoning may overlap freely. A read-only call must not start an unqualified persistent process. |
| `non-checkout` | `out-of-contract` | Managed state proven outside every checkout is not governed by the checkout guarantee. |
| `external` | `out-of-contract` | Independent same-UID programs are not CodeGG's to fence. |

**Effective permission ceilings.** A restricted-profile call is bounded by the
*intersection* of the effective ancestor tool permissions, the sandbox
profile, the approval decision, and the principal ceiling. A profile never
widens an existing permission; it can only narrow it. A run that holds a
read-only tool permission cannot gain write authority through the profile.

**A read-only category is not a capability claim.** `ToolCategory` (which
drives permission prompting) and `ToolEffectClass` (which drives
program-callability) are independent fields with no cross-validation
(`src/tool/mod.rs:136-145`, `src/tool/contract.rs:48-69`). The matrix
therefore records the *declared* effect and the *observed* effect separately,
and the guard treats a writer-category tool that declares a read-only effect
class as a finding rather than trusting either field alone.

## 6. The user-visible guarantee and the exact unsupported-capability errors

**The guarantee, in words a user can be held to:**

> While a checkout is reserved by the CodeGG-mediated restricted profile,
> CodeGG's own writers are serialized against each other. Reads and model
> reasoning are not blocked. Independent programs you run yourself are not
> blocked, and are not fenced.

**What must never be said:** *exclusive*, *sandboxed*, *filesystem isolated*,
*all writers prevented*, *safe*, *isolated from other agents*. None of those
is true of a shared checkout under option B.

**The error contract.** C002 must introduce one typed refusal, distinct from
the existing variants, so that the refusal is machine-readable rather than a
free-text `Permission` string:

| Existing variant | Meaning | Why it is not enough |
|---|---|---|
| `ToolError::Permission` | The call was not allowed | Carries no reason code; the user cannot tell "not permitted" from "not supported in this profile". |
| `ToolError::Disabled` | Operator configuration disabled the tool | Means configuration, not profile. |
| `ToolError::MissingWorkspaceRoot` | No root was resolved | Correct and already fail-closed, but says nothing about the profile. |

The C002 refusal must carry, at minimum: the **capability** (which writer
root), the **profile** (which restricted profile was active), the **reason**
(denied process / unsupported platform / no permit / quarantine), and the
**remediation** (use an isolated worktree, or wait for the reservation to
clear). It must refuse **before** the effect, and it must not leak a path
claim, a permit id, or a sibling run's identity.

**Operational fallback.** When the restricted profile refuses an operation,
the supported fallback is the **existing isolated worktree** (ADR-0002),
which remains the default placement for mutating agents. There is no
"retry without the profile" path: the profile is not a best-effort filter.

## 7. Lock order and ownership (work package E)

### 7.1 Lock order

Strict, and never inverted:

```
1. permission / approval decision        (human wait happens HERE, and only here)
2. scheduler admission                    (job accepted, resource keys acquired)
3. effective permit                      (CAS/fenced writer permit acquired)
4. writer effect                          (the mutation itself)
```

**Never** hold a writer permit while waiting for children, a human, or a
provider. **Never** acquire the permit before the approval decision, because
that would pin a checkout against a human's thinking time. **Never** release
the permit on a timeout, a failed output join, or a cancelled request: those
are not proof of quiescence. An ambiguous restart or cancellation
**quarantines** the checkout until it is resolved; there is no automatic
release while a writer may still be running.

### 7.2 Ownership

One daemon-owned typed writer authority coordinates the final effects, and it
is **not** a scheduler and **not** a second Git service. The permit resolves
a typed tuple: `WorktreeId`, worktree lease generation, group reservation,
authenticated actor/run, and execution attempt. No model-supplied path,
`WorktreeId`, generation or group id is a bearer credential. Missing or
ambiguous scope **refuses before an effect**.

Every writer root in the matrix names its **final effect owner** — the module
that actually performs the filesystem or index effect. Two consequences
follow:

- A route that reaches a writer without passing the final owner is a
  **second writer on the same paths**, and must join the same permit. The
  HTTP file route is exactly this today.
- A coordinator that reaches a writer through another module does not need
  its own fence. `daemon_documents` is a coordinator; `document_service` and
  `lsp::mutation` are the effect owners.

### 7.3 Protocol shape for C002

C002 owns the durable permit, the typed id tuple, the state machine, and the
quarantine semantics. C003 owns the native file transaction and typed Git
interposition. C004 owns non-broker sink mediation. C005 owns the process
denial and lifecycle gating. C006 owns the restricted end-to-end
qualification.

The interface C002 must expose to C003/C004/C005 is exactly one thing: a
**final-effect authority** that a writer can call immediately before it
mutates, and which either returns a live permit or refuses. Every writer
root in the matrix maps onto that call, and the matrix is the checklist.

## 8. Holes the audit found (these are C002+ work, not C001 fixes)

These are recorded so that C002–C005 inherit a concrete list. C001 does not
fix them; it cannot, because the fence does not exist yet.

1. **The `git` tool's `workdir` is model-supplied and unvalidated.**
   `src/tool/git.rs:342-345` takes `workdir` from the tool input, and
   `resolve_repo_root` (`src/git_mutations.rs:343-364`) only checks that the
   path exists, is a directory and has a `.git` entry. The broker's
   `workspace_path_policy_id` is a *revision match* against the grant
   (`src/tool/broker.rs:370-374`), not a path-scoping check on the
   parameter. **A model can point `git` at any repository on the
   filesystem.** This is the single most important input to C003.
2. **No native file or Git sink takes the repository lock.** Only the LSP
   preview-apply path and the worktree service do — and they use **disjoint
   lock tables**: `WorkspaceLockTable::new()` is instantiated separately in
   `src/lsp/mutation.rs:750` and again in `src/core/runtime_deps.rs:261,310,374`.
   A lock taken on one path is invisible to the other.
3. **Batch coordination is eligibility- and path-overlap-gated.** A batch
   containing `git`, `bash` or `formatter` takes **no lease at all**, and
   `replace` and `commit` are path-invisible to the permission layer.
4. **The HTTP file route is a second, non-broker writer** on the same paths:
   it truncates in place, takes no repository lock, no digest, no atomic
   replace, and emits no `FileChanged` event.
5. **Every digest check in the document path is check-then-act.**
   `daemon_documents` is well fenced — writer lease, revision, base-disk
   digest, atomic rename, repository lock — but the digest is read and then
   acted on without an atomic compare-and-swap.
6. **A linked worktree does not collapse to one identity.** The lock table
   is path-keyed, so a path *alias* collapses to one identity, but a linked
   worktree does not. No writer sink consults a git common-dir mapping.
7. **The run-store index has a provable lost update.** Three independent
   `FsRunStore` instances share one root with only per-instance mutexes, so a
   rename-versus-append race can drop an index entry.
8. **Relative-path fallbacks defeat the "outside the repository root"
   fence.** `crates/codegg-core/src/storage/paths.rs:47,55`,
   `src/plugin/install.rs:35` and `src/core/daemon_construct.rs:220-225`
   can fall back to `.` or a relative root, and the last of these defeats
   the native fence at `worktree_service.rs:832-836`.
9. **The scheduler's exclusivity keys are empty for the process kinds that
   matter.** `JobKind::Shell`, `ManagedProcess`, `Test`, `Lint`, `Python` and
   `ToolProgram` carry empty exclusivity-key sets
   (`crates/codegg-core/src/jobs/mod.rs:755-762`), so `workspace-mutation`
   and `worktree-mutation` never conflict.
10. **`JobExecutionContext.workspace_root` is not honoured by the argv
    executor**, which uses the durable payload `cwd`
    (`src/scheduler/executors.rs:321-326`).
11. **The daemon-cwd guard does not cover the two process surfaces that need
    it.** `scripts/check_daemon_cwd_usage.py` scans `src/tool`,
    `src/scheduler`, `src/core` and five files, so the process-global
    `current_dir()` in `src/hooks/mod.rs:117` and
    `src/plugin/runtime/process.rs:100` is unguarded, and the local MCP
    child sets **no** `current_dir` at all, inheriting the daemon's launch
    directory.
12. **No sink has proven descendant-tree quiescence on Unix.** The managed
    process group kill is explicitly escaped by a `setsid` descendant
    (`src/managed_process.rs:1666-1669`); the PTY, MCP child and LSP child
    are direct-child-kill only, with no group signal at all. Only the
    Windows `ManagedProcess` Job Object gives a real tree kill, and that
    bounds *lifetime*, not *write authority*.

## 9. The guard (work package D)

`scripts/check_writer_disposition.py`, with the disposition manifest at
`docs/writer-disposition.toml`.

The guard enumerates **code sites** and requires each to have a declared
disposition. It fails when a model-facing writer tool or a daemon/HTTP/
project-state module that performs a filesystem effect has no entry, when the
vocabulary is invalid, when a declared path no longer exists, when a checkout
writer is classified as a harmless read, or when a non-checkout root claims a
checkout disposition.

**What the guard deliberately does not do.** It does not attempt to enumerate
what a shell command, script, plugin, MCP server, LSP server or PTY writes.
Dynamic command contents are not statically enumerable, and the C001 contract
forbids claiming otherwise. The guard reasons about call sites, owners and
declared metadata — never about effects inferred from command text. It is
also not a universal command regex blocker.

**Proven non-vacuous.** `--self-test` injects violations and asserts the
checks fire; and a real injection into the tree — a new `ToolCategory::Mutating`
tool with no manifest entry — was confirmed to fail the guard, and to pass
again once the probe was removed.

**Coverage limit, stated honestly.** The guard detects a writer module by the
presence of a filesystem-effect primitive. A writer spelled differently would
be missed, and the guard does not claim otherwise: it is a *narrow* static
inventory guard, and the C001 source audit is what establishes the full set.
The audit's own sweep is the backstop; the guard is what keeps the audited
set from silently drifting.

## 10. Decision request

**At C001 close, ADR-0042 was `proposed` and remained `proposed`.** C001's
decision outcome was **"still proposed"** — a complete decision package
awaiting an independent maintainer accept/reject decision.

- **If accepted:** C002 becomes eligible; C003–C006 follow the chain.
- **If rejected:** C002–C006 remain blocked, and M005 shared mutation stays
  unavailable. Per the milestone's stop condition, a rejection that still
  wants independent same-user writer exclusion requires a separate OS
  isolation ADR.
- **Either way:** the existing isolated worktree placement (ADR-0002) remains
  the default for mutating agents, and nothing in this document changes
  runtime behaviour.

### 10.1 Outcome — accepted 2026-10-19 (option B)

The maintainers accepted **option B** after reviewing this package, and the
decision is recorded in the ADR's own "Acceptance record" section. The
"if accepted" branch above is the one that was taken:

- **C002 is unblocked** and is the active milestone. C003–C006 remain blocked
  on their own predecessor closures.
- **Option A** (universal filesystem exclusion, including unrelated same-user
  processes) is not selected and stays deferred to a separate OS-isolation
  project. **Option C** (advisory path claims plus prompt instructions)
  remains rejected.
- **Nothing is enabled.** There is still no permit, no fence, no shared-write
  mode and no user-visible shared-mutation selector. The existing isolated
  worktree placement (ADR-0002) remains the default for mutating agents.
- The two-axis policy matrix in §5, the user-visible guarantee and refusal
  contract in §6, and the lock order in §7 are now the governing contract
  that C002–C006 implement against — not an aspiration.

Nothing in this document, in `docs/writer-disposition.toml`, or in the guard
grants, enables, permits or fences any writer.
