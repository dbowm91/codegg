# Identity / Audit Live Execution Corrective M004 — Qualification and Coverage Guard Closure

Status: ready

Hard dependencies: M002 and M003 closed.

Repository baseline: `a996e20060a0103a152a80fb62463241c1fd1162`

Source roadmap: `plans/subsystems/identity-audit-live-execution-post-closure-corrective-addendum.md`.

Primary class: qualification / guard closure.

## 1. Objective

Prove the new live executor hooks across real trajectories and make the executable audit coverage guard reject regressions where required single-host actions fall back to builder-only/non-live status.

## 2. Guard requirements

Extend the audit coverage model so it distinguishes:

- daemon request operation mappings;
- executor-owned live hooks (`command_execute`, `git_operation`, `job_complete`);
- intentionally future/distributed actions (`node_enrollment`, `remote_execute`);
- intentionally content-free/high-volume domains.

Do not satisfy the guard by merely adding operation names to `UNINSTRUMENTED_OPERATIONS`. The three corrected actions must have executable evidence of a live owner.

Prefer a small declarative executor-hook table consumed by tests/guard rather than source-grepping arbitrary call text. If source-level checks are used, pin them with self-tests and fail closed if the owner moves.

## 3. Qualification trajectory

Build one deterministic single-host trajectory:

1. LocalOwner or authenticated project member submits a turn/job.
2. Model/tool path executes a command through ToolBroker.
3. A Git mutation occurs.
4. An interactive process is created.
5. A scheduler job reaches terminal success or failure.
6. Project Owner queries audit.

Assert ordered/correlated structural events, trusted actor, decision ids, project/session/turn/run/job linkage, no bodies/secrets, and no duplicates after restart/replay.

Add negatives for unauthorized project actor, credential-like Git URL, command containing secret-looking text, and terminal input content. The secret material must not be present in metadata or body and must not leak through error strings.

## 4. Documentation

Update `architecture/audit.md`:

- mark `command_execute`, `git_operation`, and real `job_complete` live;
- describe interactive-process create treatment;
- remove the corresponding M005 low deferred-hook notes from current-state text while preserving historical closure records;
- keep `node_enrollment`/`remote_execute` explicitly future.

Update skills/AGENTS only where current ownership pointers change.

## 5. Verification

```bash
python3 scripts/check_audit_coverage.py --verbose
python3 scripts/check_audit_invariants.py --verbose
python3 scripts/check_authorization_matrix.py --verbose
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_git_forbidden_patterns.py
cargo test --test identity_m005_audit_instrumentation -- --test-threads=1
cargo test --test identity_live_execution_audit -- --test-threads=1
cargo test --workspace --locked -- --test-threads=1
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

Hosted `CI / verify` should be green before final closure if available.

## 6. Acceptance criteria

- Coverage guard fails if any of the three live hooks is removed.
- End-to-end query returns correct command/Git/job terminal events.
- Interactive command execution is present without input/body retention.
- No secret, duplicate, attribution, scheduler, or Git-ownership regression.
- Original M005 low single-host live-hook findings are closed.
- No new high/medium/low single-host execution-audit finding remains.

## 7. Closure evidence

`plans/closure/identity-audit-live-execution-post-closure-corrective/004-status.md` with requirement-to-evidence matrix, guard negative proof, end-to-end event chain, secret census, full workspace/CI evidence, and registry closure audit.
