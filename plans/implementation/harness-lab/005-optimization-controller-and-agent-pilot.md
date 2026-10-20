# Developer Harness Lab M005 — Multi-Iteration Controller and Agent Pilot

Status: conditionally closed

Repository baseline: `51555556` (rebased onto current `origin/main`)

Source roadmap: `plans/subsystems/harness-lab-roadmap.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#28-observability`

Applicable ADRs: None required; keep the controller outside the daemon.

Primary class: capability

## 1. Objective

Complete the bounded assess/propose/build/retest loop, make interrupted runs safely resumable, and capture a frozen-baseline live pilot on two explicitly allowed model routes.

## 2. Why this milestone is ready

M001–M003 are closed. M004's candidate worktree, proposer, build gates and paired evaluator are implemented. This plan discharges M004's named operational gaps without changing the daemon or long-term harness boundary.

## 3. Current implementation evidence

`dev/harness_lab/improve.py` supports one proposal and one candidate qualification. The evaluation ledger records no live ACP token or cost usage, and partial candidate trial rows currently prevent resumption. M004 is conditionally closed on those operational proofs.

## 4. Invariants that must not regress

- Baseline revision, suite, expected outcomes and candidate scope are immutable for a run.
- A candidate can be accepted only after complete paired evaluation, expected outcomes, and no correctness regression.
- Provider keys never enter the repository, checkpoint, output, child command arguments, or logs.
- An interrupted trial is neither a pass nor silently repeated; completed task/repeat pairs remain immutable.
- Candidate code cannot alter its evaluator or controller.

## 5. Scope

### In scope

- Outer configurable iteration scheduler, deterministic acceptance receipt, and append-only restartable run checkpoint.
- Resume of incomplete candidate evaluation while validating existing immutable ledger records.
- Frozen baseline pilot with a strong and smaller model selected from Muse Spark 1.3, LongCat 2.5, and MiniMax 3.1, on sustained coding tasks.
- Secret-safe capture of tool selection, rereads, recovery, token usage, costs, and task completion where the providers report them.
- Required documentation and M004 corrective closure evidence.

### Explicitly out of scope

- Automatic merge, push, or production mutation; model training; controller self-modification; unbounded retries; estimates presented as actual usage.

## 6. Required production changes

### Runtime and operator surface

Add a multi-iteration `optimize` command with fixed baseline inputs, per-iteration candidate worktrees, bounded retries/wallclock, stop-on-acceptance, and `--resume` guarded by immutable input digests.

### Security and authorization

Use credentials only through transient provider environment configuration; redact all receipts. Live calls require a user-set total spend ceiling and must stop when actual usage is unavailable or reaches that ceiling.

### Documentation and static guards

Document controller semantics, truthful metrics, resume behavior, the pilot command and evidence. Keep the controller external to production runtime code.

## 7. Ordered work packages

### Work package A — Multi-iteration controller

Implement iteration scheduling, paired acceptance, and crash-safe checkpoints.

Acceptance evidence: deterministic tests for iteration limits, no-regression acceptance, immutable inputs, and interrupted partial trial resume.

### Work package B — Frozen-baseline live pilot

Run two permitted models on the same sustained tasks from identical baseline worktrees before any automatic corrections. Capture observable tool/recovery/usage evidence and verified task outcomes.

Acceptance evidence: redacted run receipts, frozen revision and task hashes, successful/failed task results, and actual token/cost fields or explicit unavailable reasons.

### Work package C — Closure and downstream audit

Update architecture/user docs where the contract changed; write closure evidence and audit this line for newly eligible plans.

## 8. Failure, cancellation, restart, and contention semantics

Persist a hash-linked event before each candidate starts. Resume only when source, suite, policy inputs and checkpoint chain match. Reuse only completed verified trial rows; execute only missing pairs. Preserve incomplete candidate artifacts and refuse ambiguous proposal state. Global elapsed time remains bounded across restarts.

## 9. Compatibility and migration

No production database, wire protocol, or installed dependency changes. Existing single-candidate commands remain compatible. Ledger changes are additive and versioned.

## 10. Required tests

- Unit tests for multi-iteration stop/acceptance and hash-linked checkpoint recovery.
- Recovery test for a candidate ledger with a completed prefix and missing task/repeat rows.
- Python harness suite and ACP integration suites on the rebased baseline.
- Provider-backed pilot is manual and separately labeled; ordinary CI remains credential-free.

## 14. Closure disposition

The multi-iteration controller, append-only run checkpoint, candidate partial-row
validation, and ACP usage/tool-argument telemetry are implemented and locally
verified. This milestone is conditionally closed because the requested live pilot
could not be executed safely: the pinned shared provider profile has no exact
LongCat 2.5 route, and this execution environment has no secret-safe channel for
the chat-supplied temporary credential. The credential was not copied into a
command, file, checkpoint, output, or repository. See
`plans/closure/harness-lab/005-status.md` for the remaining conditions and audit.

## 11. Required verification commands

```bash
python3 -m unittest discover -s dev/harness_lab/tests -v
cargo test -p codegg --test acp_stdio
cargo test -p codegg --lib acp
scripts/verify.sh quick
```

## 12. Documentation updates

- `dev/harness_lab/README.md`
- `plans/subsystems/harness-lab-roadmap.md`
- M004 corrective closure/addendum and M005 closure record

## 13. Acceptance criteria

- The controller can run multiple independent candidates and stops only on verified acceptance, a configured bound, or a recorded blocker.
- Restart resumes without duplicate candidate ledger rows or changed baseline inputs.
- Live pilot compares two allowed model routes against the same frozen task/binary baseline before automatic corrections.
- Tool-choice, reread, recovery, usage, cost and success fields distinguish measured values from unavailable values.
- No key value is committed, checkpointed, printed, or retained in the worktree.
