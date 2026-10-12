# Human Execution Corrective C004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/human-execution-corrective/004-authoritative-human-shell-promotion-staging.md`

Source subsystem roadmap:

- `plans/subsystems/human-execution-corrective-roadmap.md#c004--authoritative-human-shell-promotion-staging`

Accepted decision:

- `plans/adrs/ADR-0017-human-shell-promotion-admission.md` — Option A,
  consume at durable admission; an accepted turn interrupted by daemon restart
  is not automatically replayed.

Repository baseline reviewed: `6b18122d35e6e05d1f2113c78e77467848769f24`

Implementation commits: `1b355309`, `d560eea1`, `a2adb0cd`, `1482c54e`,
`77fcb326`, `fd9ca6bd`, `00d11fe5`, and `6b18122d`.

Hosted verification:

- Root CI `38251214377` — success on `6b18122d`; all guards, formatting,
  Workspace Clippy, and all 12,778 workspace tests passed (5 skipped).
- Desktop E2E `38251230731` — success on `6b18122d`, including the packaged
  desktop WebDriver live-turn trajectory.

## 1. Executive finding

C004 closes the missing durable turn-admission contract identified by C001.
Human-shell promotion claims and the stable submission receipt now commit with
controller admission before runtime delivery. Same-identity retries return the
existing receipt; changed content or scope conflicts. On restart, incomplete
accepted receipts become interrupted and are not replayed. The promotion is
bound to one accepted daemon turn and remains redacted; raw shell output is not
added to session persistence.

The TUI keeps a submission identity across an uncertain response and accepts
the durable receipt before marking its promotion complete. ACP now consumes the
same receipt and retains its turn ID for cancellation. The WebSocket response
path serializes the typed receipt. A hosted race test exposed two remaining
legacy `Ack` assertions; those test expectations were corrected, and the full
workspace suite then passed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Durable, bounded, principal/session-scoped staging | `crates/codegg-core/tests/human_shell_promotion.rs`: `staging_is_bounded_idempotent_and_principal_scoped`, `pending_queue_cap_rejects_without_adding_an_extra_row` | pass | Authenticated daemon authority supplies principal and session scope. |
| Accepted admission consumes promotions atomically; busy admission preserves them | `turn_admission_consumes_atomically_and_busy_admission_preserves_pending` | pass | Promotion claim and controller lease share the SQLite admission transaction. |
| Stable identity returns prior receipt and restart prevents replay | `durable_submission_identity_returns_receipt_and_restart_marks_it_interrupted` | pass | Digest/scope mismatch conflicts; expired identity may be reused after retention. |
| Concurrent controller submissions yield one winner | `tests/session_family/control_m004_controller_lease.inc` controller lease group | pass | 26 tests passed locally; hosted workspace suite passed. |
| Client state is based on durable acknowledgement | TUI turn/shell dispatch tests; `src/acp.rs` accepted-receipt branch | pass | ACP cancellation is correlated with the receipt's turn ID. |
| Full repository checks | `scripts/verify.sh quick`; hosted CI and Desktop E2E | pass | Exact implementation SHA is `6b18122d`. |

## 3. Implementation and lifecycle contract

Schema migration v72 adds request-digest-bound admission receipts. The
authoritative session store commits the receipt, controller lease, and
promotion consumption together. Admission returns a typed receipt only after
runtime acceptance; duplicate requests with matching identity return the same
receipt and do not create a second runtime turn. A reused identity with a
different digest or binding conflicts. Busy or rejected admission leaves
pending promotions available.

Recovery marks accepted but incomplete receipts `interrupted`, releases the
associated controller lease, and does not replay the turn or promotion. This
is at-most-once daemon admission evidence; it does not claim atomicity with an
external provider's inference endpoint. The user can explicitly submit new
work with a new identity.

## 4. Verification executed

Local verification on the final implementation revision:

```text
cargo fmt --all -- --check                         passed
cargo test --test session_family --locked controller_lease::  26 passed
cargo test --test acp_stdio --locked               1 passed
scripts/verify.sh quick                            passed
```

Hosted root CI `38251214377` ran all 12,778 workspace tests: 12,778 passed,
5 skipped, 0 failed. Formatting, the CI guards, and Workspace Clippy passed.
Desktop E2E `38251230731` passed its production checks and built-app WebDriver
trajectory on the same implementation SHA.

Two earlier exact-SHA CI attempts found stale `CoreResponse::Ack` expectations
in the controller race and atomic-acquisition tests. The tests were updated to
assert accepted durable receipts and contention semantics. The final full
workspace run passed. The Desktop E2E mock was also updated to serve the
provider model-catalog probe required by the durable provider-connection
trajectory.

## 5. Invariant, failure, and security review

- Only redacted promotion projections are persisted; raw shell output remains
  outside this ledger and provider context unless the existing projector
  explicitly admits it.
- The authenticated connection and authorized session establish principal,
  project, and workspace scope; DTO fields do not establish identity.
- An accepted promotion is attached to at most one daemon turn. Unaccepted or
  busy admissions do not consume it.
- Request digest and binding prevent an idempotency key from being reused for
  changed prompt content or scope.
- Interrupted receipts are visible durable history and are never automatically
  replayed. External provider delivery remains outside the SQLite transaction.
- No new process, shell execution, or authorization capability was added.

## 6. Compatibility and documentation

The protocol addition is typed and additive. TUI, desktop, WebSocket, and ACP
turn-submit consumers handle the durable receipt; ACP preserves its turn ID for
cancellation. Updated `architecture/human_shell.md`,
`architecture/session.md`, `architecture/tui.md`, `architecture/audit.md`,
`architecture/authorization.md`, the human-shell skill, `docs/tui.md`, and
`AGENTS.md` describe the authoritative staging and admission boundary. ADR-0017
records the selected restart behavior.

## 7. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| none | No C004 correctness or verification finding remains. | C004 closes. C001's corrective obligation is discharged by this closure. |

C002's workspace-owned finite shell execution remains a separate plan. With
C001's authoritative promotion contract complete, C002 is dependency-ready;
this closure makes no claim about workspace-owned command execution.

## 8. Registry disposition

- Mark C004 closed and record this closure in the human-execution corrective
  roadmap and `plans/registry.md`.
- Record C001's corrective obligation as discharged by C004 while retaining its
  original historical failure evidence.
- Mark C002 ready for handoff; do not claim it implemented.
