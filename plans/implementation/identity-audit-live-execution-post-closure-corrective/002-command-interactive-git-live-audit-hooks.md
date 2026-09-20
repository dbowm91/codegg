# Identity / Audit Live Execution Corrective M002 — Command, Interactive, and Git Live Audit Hooks

Status: ready

Hard dependency: M001 trusted execution audit context/emitter closed (`plans/closure/identity-audit-live-execution-post-closure-corrective/001-status.md`).

Repository baseline: `a996e20060a0103a152a80fb62463241c1fd1162`

Source roadmap: `plans/subsystems/identity-audit-live-execution-post-closure-corrective-addendum.md`.

Primary class: invariant / audit instrumentation corrective.

## 1. Objective

Close the M005 live `command_execute` and `git_operation` gaps at canonical execution owners, including interactive process creation, using the trusted M001 context/emitter. Emit structural digests and labels only.

## 2. Command execution ownership

The canonical model/tool path is `ToolBroker::execute/execute_with_retry`. Emit one `command_execute` terminal structural event for real process/command execution, not for every read-only tool. Prefer the narrowest owner that knows an actual command/process was dispatched and its terminal outcome.

Requirements:

- digest normalized command/argv bytes with the existing `structural_digest` helper or equivalent; never store argv text;
- family label is bounded (e.g. shell/test/process/interactive);
- preserve project/session/turn/run/job causation from M001 context;
- if dispatch never occurs, do not emit a successful command execution;
- uncertain/timeout/cancel outcomes remain structurally distinguishable without inventing success;
- retries of one logical idempotent invocation use deterministic event identity when the underlying execution is the same committed action.

Do not emit duplicate command events at both ToolBroker and Bash/managed-process layers. Select one canonical owner per execution family and test the ownership matrix.

## 3. Interactive process

`InteractiveProcessCreate` is a real command execution surface. Emit `command_execute` on successful dispatch/create using the transport-bound daemon principal/provenance and a digest of command+args. Do not audit terminal input bodies/keystrokes. Attach/detach/resize/list remain authorization/audit-decision surfaces, not command-content events.

Terminate/remove should not fabricate another command execution; if a lifecycle action is needed, use an existing appropriate structural action only if already defined. Do not add a new AuditAction in this corrective without a separate design decision.

## 4. Git operation ownership

`GitMutationExecutor` is the canonical local mutation owner shared by native Git and routed Bash->Git. Git network/recovery code also composes around that executor family.

Emit `git_operation` exactly once for mutation/network/recovery state transitions that execute:

- bounded operation label (`stage`, `commit`, `branch_create`, `merge`, `rebase`, `push`, `pull`, `fetch`, recovery actions, etc.);
- digest of target ref/remote/refspec or an empty structural digest where no ref exists;
- terminal outcome;
- trusted audit chain from M001.

Never store remote URLs containing credentials, commit messages, path lists, patch bodies, or subprocess output.

Read-only status/diff/log/blame do not need `git_operation` unless long-term policy is explicitly changed.

## 5. Regression matrix

Cover at minimum:

- shell/process command success + failure/cancel;
- one direct native tool process path;
- interactive process create;
- Git stage/commit or branch mutation;
- Git network operation with credential-redaction negative;
- Git recovery transition;
- Bash-routed Git mutation proving no double event;
- unauthorized/denied operation produces authorization denial but no execution event;
- retry/replay does not duplicate one committed logical event.

## 6. Verification

```bash
cargo test --test identity_m005_audit_instrumentation -- --test-threads=1
cargo test --test git_execution_origin_matrix -- --test-threads=1
cargo test --test git_credential_cross_path -- --test-threads=1
cargo test --test git_network_integration -- --test-threads=1
cargo test --test interactive_process_m001 -- --test-threads=1
cargo test -p codegg --lib tool::broker -- --test-threads=1
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_audit_coverage.py --verbose
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

Use the actual current interactive-process test target if its name differs; record substitutions in closure evidence.

## 7. Acceptance criteria

- Real command/process dispatches emit `command_execute` once with trusted attribution and no command text.
- Interactive process creation is live-audited without terminal input retention.
- Mutating/network/recovery Git operations emit `git_operation` once with no secrets.
- Native Git and Bash-routed Git converge on one audit event, not duplicates.
- Existing execution/security behavior is unchanged apart from structural audit records.

## 8. Closure evidence

`plans/closure/identity-audit-live-execution-post-closure-corrective/002-status.md` with event samples reduced to structural metadata, duplicate/secret negatives, ownership matrix, and M004 unblock audit.
