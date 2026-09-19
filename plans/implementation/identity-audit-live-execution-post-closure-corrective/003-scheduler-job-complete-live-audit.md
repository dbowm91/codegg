# Identity / Audit Live Execution Corrective M003 — Scheduler Job Completion Live Audit

Status: blocked

Hard dependency: M001 trusted execution audit context/emitter closed.

Repository baseline: `a996e20060a0103a152a80fb62463241c1fd1162`

Source roadmap: `plans/subsystems/identity-audit-live-execution-post-closure-corrective-addendum.md`.

Primary class: invariant / scheduler audit corrective.

## 1. Objective

Emit the existing `job_complete` audit action at the real durable scheduler terminal attempt transition for success, failure, cancellation/interruption, and other bounded terminal outcomes. Stop treating `job_retry` as the representative live terminal once the true terminal owner is instrumented.

## 2. Ownership

The canonical scheduler/job store transition that persists the terminal attempt/job state is the emission owner. Do not emit from TUI polling, projection/event observers, retry requests, or completion consumers.

Use attribution persisted with the job/source/attempt plus the trusted M001 emitter/context. If required attribution is absent on a legacy row, use the established explicit legacy-local/service attribution rule; never manufacture a current human principal.

## 3. Semantics

- Emit after the terminal state is durably accepted so the event describes a real transition.
- One terminal transition -> one deterministic event id.
- Replayed completion/restart reconciliation of an already-terminal attempt emits no duplicate.
- Retry creates/uses the next attempt according to existing scheduler semantics; the failed prior attempt remains terminal and auditable.
- Correlate project/session/turn/run/job/attempt where available.
- Metadata is limited to ids, bounded outcome/state labels, and decision outcome; no job payload/tool output.

## 4. Required tests

- success terminal;
- failure terminal;
- cancellation/interrupted terminal;
- retry prior-attempt terminal + next attempt without duplicate;
- restart reconciliation/idempotent replay;
- legacy attribution fallback;
- audit store failure does not corrupt scheduler terminalization and increments existing failure counters.

## 5. Verification

```bash
cargo test -p codegg --lib scheduler -- --test-threads=1
cargo test --workspace job --no-fail-fast
cargo test --test identity_m005_audit_instrumentation -- --test-threads=1
python3 scripts/check_scheduler_bypass.py --self-test
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_audit_coverage.py --verbose
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

## 6. Acceptance criteria

- Actual scheduler terminal transitions emit `job_complete` with truthful outcome.
- `job_retry` remains an audit event only for the retry request/action, not a surrogate completion.
- Restart/retry races produce no duplicate terminal audit rows.
- Scheduler authority/admission semantics are unchanged.

## 7. Closure evidence

`plans/closure/identity-audit-live-execution-post-closure-corrective/003-status.md` with terminal transition matrix, duplicate/restart evidence, attribution fallback evidence, and M004 unblock audit.
