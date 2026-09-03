# Agent Convergence M002 — Closure Status

Status: closed

## 1. Scope and decision

M002 is strictly closed. CodeGG now has a complete single-cycle
produce -> independent read-only verify -> explicit owner decision vertical
slice. The implementation consumes the accepted M001 durable convergence
contract and does not add repair/replan execution, automatic Git integration,
goal authority, or a second scheduler.

## 2. Implementation revisions and dependency review

- Planning activation: `a8066b65` (`plans: activate convergence verifier milestone`).
- Implementation: `28008ddd` (`feat: implement independent convergence verifier`).
- Reviewed M001 dependency: `46a7e5ba`, with accepted implementation revisions
  `18397ab1` and `ffc3847c`; M001 closure is
  `plans/closure/agent-convergence/001-status.md`.
- The implementation plan was moved to `closing` when production work landed
  and is moved to `implemented` with this accepted closure record.

## 3. Producer, verifier, and owner state-machine evidence

- `TaskTool` is the only model-facing submission boundary. `converge` creates
  one M001 record and cycle, then submits exactly one normal durable
  `SubagentRun` through `JobSubmissionService`.
- The accepted call identity is the convergence idempotency key. Store-level
  fingerprint conflicts fail closed; persisted producer/verifier references
  prevent duplicate child model work under repeated notifications or status
  reconciliation.
- Producer terminal state is read from `AgentRunStore`. Only a completed run
  with a successful structured `AgentRunResult` can transition to `Verifying`.
- The verifier is submitted only after the producer evidence packet is
  persisted/available. Its terminal result is parsed as a bounded marked
  `Pass | Revise | Inconclusive` verdict; malformed output becomes
  `Inconclusive`, never `Pass`.
- `convergence_decide` is authorized against the exact persisted turn/run
  owner and accepts only M002 `accept`, `stop`, or `escalate`. `repair` and
  `replan` return an explicit M003 availability error. Accept completes only
  the convergence record; it does not merge or integrate a worktree.
- `convergence_cancel` uses run control only for the convergence's active
  producer/verifier references and persists a revision-checked cancellation.

## 4. Verifier effective-permission matrix

The built-in `verifier` asset allows only bounded inspection (`read`, `grep`,
`glob`, `list`, read-only `diff`/`lsp`, and `evidence_bundle`). The host adds an
unconditional denied-tool ceiling before child execution covering:

| Authority class | Host-enforced result |
|---|---|
| File mutation and patching | `write`, `edit`, `replace`, `multiedit`, `apply_patch` denied |
| Shell, scripting, and Git mutation | `bash`, `terminal`, `python`, `python_script`, `git`, `commit` denied |
| Delegation and orchestration | `task` denied |
| Permission and goal authority | `permission`, `goal_get`, `goal_update_progress`, `goal_request_completion` denied |
| External execution/research | `test`, `repo_fetch`, `research`, `webfetch`, `websearch` denied |

The deny list is added to the concrete child request, so selecting a custom or
overridden verifier agent cannot widen the effective ceiling. The verifier is
created as a fresh child run and receives no producer message history.

## 5. Durable specification, restart, and evidence isolation

Objective and criteria are validated and digest-bound by `ConvergenceSpec`
before persistence. Restart/status reconciliation reads the durable record,
cycle references, authoritative run status, and structured result; it never
scrapes a transcript or resubmits a child whose reference is already stored.
Terminal event subscription is only a wake-up path, and repeated advancement is
revision checked.

`assemble_verifier_evidence` accepts only bounded `AgentRunResult` values and
projects summary, commits, changed paths, validation, findings, artifacts, and
repository state into `VerifierEvidencePacket`. Transcript, hidden reasoning,
tool arguments, environment, and credentials have no input path. Packet and
verdict envelopes are size bounded before the verifier prompt or store boundary.

## 6. Duplicate, restart, cancellation, and contention evidence

- In-memory and SQLite M001 stores retain exact invocation identity,
  fingerprint conflict detection, cycle reference idempotency, and revision
  checks.
- Coordinator advancement is event-driven and repeat-safe: a producer
  notification can cause at most one successful `Producing -> Verifying`
  transition and one verifier reference; a verifier notification can cause at
  most one persisted verdict and `Verifying -> AwaitingDecision` transition.
- Status/reconnect performs one bounded reconciliation pass for a durable
  nonterminal record, covering restart between producer completion and verifier
  submission and restart after verifier completion.
- Cancellation filters to referenced active runs, and a decision/cancel race
  resolves through the M001 revision/state transition guard rather than
  applying both outcomes.
- No polling loop is required for correctness, and waiting consumes no
  scheduler process slot.

## 7. Goal-authority regression

The goal verification regression keeps host evidence authoritative: a failed
host-recorded test yields `GoalVerificationVerdict::NotMet`, regardless of
model/semantic claims. The existing path still requires deterministic accepted
host evidence for `Met`; convergence does not add a semantic-verifier source or
mutate `GoalStatus`.

## 8. Projection and TUI evidence

`ConvergenceUpdated`/`ConvergenceUpserted` is an additive session projection
event. Its bounded DTO contains only id, owner summary, lifecycle/cycle,
producer counts/handles, verifier handle, verdict summary/kind, decision state,
and terminal reason. Reducer replay and resync use the durable event stream;
unknown older clients retain compatibility through the additive field. The TUI
sidebar renders compact convergence rows and leaves detailed evidence in the
existing run/result surfaces.

## 9. Documentation and compatibility

Updated `architecture/agent.md`, `architecture/tool.md`, `architecture/goal.md`,
`architecture/overview.md`, and built-in-agent documentation. Legacy
`task spawn`/group behavior remains compatible; the new optional job model
field defaults when decoding historical payloads. No legacy team inbox/outbox,
scheduler resource algorithm, or deterministic goal acceptance rule was
changed.

## 10. Verification executed

Successful local checks:

- `cargo fmt --all -- --check`
- `git diff --check`
- `cargo check --workspace --all-targets --locked`
- `cargo check -p codegg --tests --locked`
- `cargo clippy -p codegg --lib --locked -- -D warnings`
- `cargo test -p codegg-core agent_convergence --locked` — 8 passed
- `cargo test -p codegg-core goal::verification --locked` — 7 passed
- `cargo test -p codegg-protocol --locked` — 163 passed
- `python3 scripts/generate_builtin_agents.py --check`
- `bash scripts/check-core-boundary.sh`
- `bash scripts/verify.sh quick` — passed

The full `cargo test -p codegg-core --locked` pass initially hit one unrelated
flaky checkpoint fixture assertion (`goal::checkpoint::tests::test_append_checkpoint_update`);
the exact test passed on immediate isolated rerun. The convergence-focused
core suite remained green.

The required root test invocations were attempted. `cargo test -p codegg
--lib convergence --locked` and `cargo test --test agent_convergence --locked`
compile their targets but cannot link on this host: the x86_64 macOS toolchain
is given arm64 `/opt/local/lib/liblzma.dylib` and `libiconv.dylib`, leaving
`_lzma_*` undefined. This is an environment linker defect, not a Rust compile
or test assertion failure; workspace all-target compilation passed. Workspace
Clippy also reports one pre-existing unrelated `src/tool/review.rs` item-order
warning; the changed root library clippy target is clean.

## 11. Unresolved findings

- Critical: none.
- High: none.
- Medium: none.
- Low: the two root test binaries need a correctly-architected x86_64 macOS
  liblzma/libiconv toolchain (or CI) to execute; their source compilation and
  all-target check passed.

## 12. Roadmap and registry disposition

The blocked-work audit searched the registry and plan dependency graph for
references to agent-convergence M002. Only M003 has M002 as a hard dependency.
M003's M001/M002 interfaces are now stable and its plan is moved from
`blocked on M002` to `ready` in this same closure change. Memory-to-skill M002
and M003 remain blocked on their own predecessor milestones; no other plan is
unblocked by this closure.

The agent-convergence roadmap remains active at M003. M002 is recorded under
recently closed work, removed from active/dependency-ready work, and the
implementation plan is marked `implemented`. No corrective pass is required.
