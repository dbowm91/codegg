# Tool Programs Milestone 015 — Independent Review Attestation

Status: accepted

Reviewed implementation head:

- `247ef5015d79bdd834bffca15c76ebb2426beb40`

Review relationship:

- The reviewer did not author the M015 implementation commits.
- The closing-document refresh at `2969c2f75b40220313ca05dbc9d63edffa98873d`
  was outside the reviewed implementation head and contains no production code.

## Disposition

APPROVE. No unresolved high or medium findings.

The reviewer first identified two closure blockers: arbitrary public
`JobSubmit` payloads could fabricate Tool Program authority, and notification
state was marked injected before a durable parent-session insertion. Both were
corrected at the reviewed implementation head and independently re-reviewed.

## Evidence reviewed

- Ordinary public `JobSubmit` rejects `JobKind::ToolProgram`; the process
  recovery fixture is process-owner enabled, debug-only, and compiled to
  disabled in release builds.
- The public-protocol rejection regression proves a fabricated authority
  payload creates no Tool Program job.
- Parent notification injection appends an idempotent session event before
  marking the notification injected and before acknowledgement.
- The session append uses the stable injection key as event identity, accepts
  identical retries, and rejects identity/content collisions.

## Independent commands and results

```text
cargo test -p codegg --test tool_program_m015_authority_contract -- --test-threads=1
  5 passed
cargo test -p codegg --test tool_program_m015_recovery -- --test-threads=1
  5 passed
cargo test -p codegg --test tool_program_m015_notification_recovery -- --test-threads=1
  9 passed
cargo test -p codegg --test tool_program_m015_artifact_pipeline -- --test-threads=1
  4 passed
cargo test -p codegg --test tool_program_m015_descendant_convergence -- --test-threads=1
  8 passed
cargo test -p codegg --test tool_program_m015_daemon_failpoints -- --test-threads=1
  8 passed
```

The independent review relied on the implementation pass's separately
recorded static-guard and broader-suite evidence for repository-wide checks.
That limitation does not leave an unresolved M015 high or medium finding.
