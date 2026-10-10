# Human Execution Corrective C001 — Closure Status

Status: corrective pass required

Source implementation plan:

- `plans/implementation/human-execution-corrective/001-human-shell-promotion-and-redaction.md`

Source subsystem roadmap:

- `plans/subsystems/human-execution-corrective-roadmap.md#c001--promotion-redaction-and-config-truth`

Repository baseline reviewed: `2f026f05220f60c62bfc381af3d7479172dfdb50`

Implementation commits:

- `77f5ba44bb7f2da49de18ffa4c44758c631a9484` — provider-facing bounded/redacted shell promotion, PTY/VT fidelity, closure evidence, and C004 corrective plan.

## 1. Executive finding

The local shell promotion implementation now routes approved output through a bounded, redacted model projection and submits it through the selected session's actual turn or ask request. Provider-facing fake-client tests demonstrate accepted output and keep `!` output private. C001 does not meet its full canonical staging contract: the pending `!!`/include promotion is held in frontend TUI memory until a turn is submitted, rather than acknowledged and owned by the authoritative session/context owner. It therefore cannot prove pending promotion survives frontend reconnect/restart or that a reconnect cannot lose or duplicate it. C001 requires a corrective pass; C002 remains blocked.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| `!` remains private from provider context | `shell_dispatch_tests` fake-provider context capture | pass | All `ShellCell` entries are excluded from provider-facing context. |
| Include/ask output is bounded and redacted | `cargo test -p codegg --lib shell` | pass | Includes token redaction and a secret crossing the model-output truncation boundary. |
| Accepted include appears once in intended local session | fake `CoreClient` captures `TurnSubmit` and `SessionPromptSubmit` | pass | Marked consumed only on canonical request acknowledgement. |
| Observer/unbound/stale session or workspace cannot promote | shell dispatch tests and source checks | pass | Refusal retains output privately. |
| Effective enabled, timeout and auto-promote config | shell configuration/dispatch tests | pass | Execution is disabled when configured off. |
| Acknowledged pending staging survives reconnect/restart | No authoritative staging API or durable owner exists in this implementation | fail | Pending staging is local TUI state until turn submission. |
| Reconnect/retry idempotency under authoritative promotion identity | No daemon-owned staging identity exists | partial | Turn-local acknowledgement tokens prevent consuming newer local entries but do not establish reconnect idempotency. |
| Canonical projection across remote frontend paths | Remote explicit include moves evidence into composer for normal turn acceptance | partial | No remote staged-promotion acknowledgement; acceptance is only the subsequent normal turn. |
| Focused projection harnesses | `cargo test --test shell_projection_harness`; `cargo test --test shell_projection_phase10` | pass | 11 and 33 tests passed. |
| Canonical repository verification | `scripts/verify.sh quick` | pass | Completed successfully. |

## 3. Production implementation evidence

The TUI filters shell cells from model-facing context, redacts and bounds promoted output, binds shell runs to the originating session/workspace, and marks local entries promoted only after the submitted request is acknowledged. `/shell-ask` uses one `SessionPromptSubmit`. Remote explicit include uses the normal composer submission path. Configuration controls are wired into production execution.

The authoritative owner does not accept a pending promotion before turn submission. No storage or reconnect identity was added. This is the remaining correctness gap, not an external evidence limitation.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg --lib shell
cargo test --test shell_projection_harness
cargo test --test shell_projection_phase10
cargo fmt --all -- --check
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_skill_guides.py
scripts/verify.sh quick
```

### Results

- Shell library tests: 509 passed.
- Projection harness: 11 passed.
- Projection phase 10: 33 passed.
- Formatting, execution ownership, scheduler bypass, and skill-guide checks: passed.
- Quick verification: passed.
- `python3 scripts/check_work_plan_repository_binding.py` was also attempted and failed on a pre-existing stale guard expectation that `STORAGE_LAYOUT_VERSION` is 68 while the repository schema and constant are at version 69. This guard is outside C001/C003 scope and is not part of quick verification.

## 5. Invariant review

- Shell commands and output are not agent Bash calls; shell cells do not enter provider context by default.
- Promoted content is treated as user evidence, redacted and byte-bounded.
- Local session/workspace binding prevents stale results from being attached to another context.
- `promoted` is not reported from a local bubble; acknowledgement is tied to turn/ask submission.
- Durable acknowledged staging and reconnect idempotency remain unproven and require corrective implementation.

## 6. Failure and recovery review

Rejected turn submission preserves local pending promotion state. Newer local entries are protected from an earlier turn acknowledgement by token identity. A frontend restart or loss of its in-memory state can lose a pending promotion; there is no authoritative staged record to recover. The corrective plan must cover duplicate retry, reconnect, daemon restart, stale session/workspace, and bounded expiry.

## 7. Migration and compatibility review

No database or protocol migration was introduced. The corrective pass must first audit session/context storage and protocol mechanisms, then add only a narrow additive capability if existing canonical owner APIs cannot provide durable staging. Older daemon behavior must fail explicitly.

## 8. Security review

Projection redaction is enforced for human promotion regardless of the general model-output redaction toggle. The current local path fences observer/unbound and stale session/workspace contexts. The corrective pass must preserve authenticated human authority and prevent client-supplied identity or workspace data from becoming trusted daemon authority.

## 9. Documentation and operations

Updated `architecture/human_shell.md`, `architecture/tui.md`, `architecture/process-tool-execution-ownership.md`, `.opencode/skills/human-shell/SKILL.md`, `.opencode/skills/tui/SKILL.md`, `docs/tui.md`, and `AGENTS.md`. The corrective plan will define the authoritative staged-state and operator-visible expiry/refusal behavior.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | Pending promotion is owned only by frontend memory until it is included in an accepted turn. | Reconnect/restart can lose pending evidence; retries cannot be deduplicated by the authoritative owner. | Implement the C001 corrective plan and add durable/session-owner acceptance and recovery tests. |
| medium | Remote include uses composer insertion and has no distinct stage acknowledgement. | User can submit the evidence, but status is not a canonical staged state. | Define and test a typed remote acceptance result or make the composer handoff semantics explicit in the authoritative UX contract. |

## 11. Roadmap disposition

Corrective implementation plan required. Keep C001 active until the authoritative session/context owner acknowledges bounded staged promotions and restart/reconnect behavior is verified. C002 remains blocked until C001 closes.

## 12. Registry updates

Register the C001 corrective implementation plan, retain C001 as active/corrective, leave C002 blocked, and mark independent C003 closed. Do not claim that the C002 dependency has been discharged.
