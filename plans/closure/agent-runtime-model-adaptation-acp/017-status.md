# Agent Runtime, Model Adaptation, and ACP Milestone 017 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-runtime-model-adaptation-acp/017-corrective-integration-evidence-and-closure.md`

Source subsystem roadmap: `plans/subsystems/agent-runtime-model-adaptation-acp-corrective-closure-addendum.md`

Accepted executable revision: `c85980e2a570a47669c54b23dd02ef388e30fd3b`

## Executive finding

The independent M017 production-path audit is complete. M012–M016 strict
predecessor records, representative production call sites, ACP lifecycle and
protocol purity, specialized finalizers, prompt/context convergence,
adapter-driven reasoning, descendant admission, cancellation, and explicit
workspace ownership were reconciled on the accepted revision. No critical,
high, or medium finding remains.

## Focused evidence

```text
cargo test --test acp_stdio -- --test-threads=1                 1 passed
cargo test --test agent_loop_harness -- --test-threads=1       40 passed
cargo test --test context_plan_convergence -- --test-threads=1  4 passed
cargo test --test provider_transcripts -- --test-threads=1     21 passed
cargo test --test subagent -- --test-threads=1                 22 passed
python3 scripts/check_daemon_cwd_usage.py                      passed
python3 scripts/check_execution_ownership.py                   passed
```

The accepted review also reconciles the stale model-profile helper removal,
workspace Clippy diagnostics, frontier profile defaults, and the research
agent permission assertion. These are narrow verification-contract
corrections; no new runtime capability or authority was introduced.

Shared broad evidence:

- `scripts/verify.sh quick`: passed once on the accepted executable revision;
- hosted GitHub Actions `verify`: run `30931979689`, job `92084050226`, passed
  on attempt 3 on the exact accepted revision;
- generated-agent checks and the focused model-profile/agent tests passed;
- no duplicate full local workspace run was required under DVR M007.

## Disposition

This review pass did not author the M012–M016 implementation records or their
strict predecessor closures. M017 is now strictly closed and no Agent Runtime
follow-up plan is registered absent a newly demonstrated defect.
