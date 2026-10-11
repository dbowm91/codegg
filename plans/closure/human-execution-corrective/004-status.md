# Human Execution Corrective C004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/human-execution-corrective/004-authoritative-human-shell-promotion-staging.md`

Source subsystem roadmap:

- `plans/subsystems/human-execution-corrective-roadmap.md#c001--promotion-redaction-and-config-truth`

Repository baseline reviewed: `2f026f05220f60c62bfc381af3d7479172dfdb50`

Implementation commits: pending commit for this closure batch.

## 1. Executive finding

C004 closes the missing authoritative staging contract for C001. The daemon now owns a bounded, principal/session/project/workspace-bound lifecycle for human-shell projections. The daemon applies the canonical redactor itself before storage, so a direct client cannot bypass the TUI's redaction step. TUI staging requires a typed owner acknowledgement; reconnect lists recover pending evidence; an accepted turn injects each requested projection once and clears the stored payload. `/shell-ask` stages and submits through one idempotent owner request. Remote composer insertion remains an unsubmitted draft.

The C001 dependency is satisfied and C002 is eligible. C002 finite shell execution ownership is not implemented by this closure.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Session-owned, authenticated stage/list | `CoreDaemon::stage_human_shell_promotion`, `list_human_shell_promotions`, authorization routing | pass | Session/project/workspace derive from the bound runtime; principal derives from authenticated authority. Client origin/scope fields are not accepted. |
| Owner-side redaction and output bounds | daemon stage/reconnect test with a raw bearer token | pass | Recovered payload contains the redaction marker and no source secret; payload is capped at 32 KiB. |
| Idempotent stage and conflicting reuse | `codegg-core::session::human_shell` tests; daemon stage/reconnect test | pass | Identical retry returns Duplicate; conflicting identity is rejected. |
| Accepted turn includes evidence once | `accepted_turn_injects_staged_shell_projection_once_and_consumes_it` | pass | Injected `TurnRuntime` captured one redacted provider-facing projection; SQLite state is Consumed and payload bytes are zero. |
| Rejected/busy turn and recovery | reservation/release and restart-recovery storage tests; controller rollback path review | pass | Rejected admission releases the reservation; daemon restart/list recovers orphaned reservations. |
| Reconnect across daemon facade rebuild | `human_shell_promotion_stage_is_idempotent_and_recovers_after_daemon_rebuild` | pass | Same authoritative SQLite pool retains one pending record after rebuild. |
| Expiry and finite retention | `expiration_clears_payload_and_restart_releases_inflight_reservation`; migration/store limits | pass | One-hour pending TTL, 24-hour terminal retention, 16 pending and 64 retained rows per session. Expiry is surfaced on list. |
| Ask uses one owner request | TUI shell-ask request test | pass | Question and stage DTO share one `SessionPromptSubmit`; rejected admission keeps evidence pending. |
| Unsupported daemon and remote composer semantics | TUI unsupported-stage and remote-include tests | pass | No local message or staged claim without daemon acknowledgement; remote composer content is an unsubmitted draft. |
| Existing shell redaction behavior | shell-focused unit suite and shell projection harnesses | pass | Shared TUI projection redaction and bounds remain in place; daemon repeats redaction at the trust boundary. |
| Repository quick verification | `scripts/verify.sh quick` | pass | Formatting, all static guards, workspace all-target check and locked check passed. |

## 3. Production implementation evidence

- Migration v71 adds `human_shell_promotion`, with a session foreign key, principal/project/workspace binding, redacted projection bytes, expiry and lifecycle state.
- `crates/codegg-core/src/session/human_shell.rs` provides bounded idempotent stage, list, reserve, consume, release and orphan-recovery operations.
- `codegg-protocol` adds typed capability, stage, list and combined ask response DTOs. `TurnSubmit` and `SessionPromptSubmit` accept optional promotion identities.
- The daemon derives authority from `RequestAuthorityContext` and the bound session runtime. It re-applies ANSI stripping and the shared model redactor before persistence.
- The TUI keeps only acknowledged promotion IDs for the next turn, reloads pending projections when a session is rebound, and drops stale asynchronous completions.
- Audit records for stage retain only projection digest/byte length. Capability and pending-list reads are explicitly categorized as bounded read operations.

## 4. Verification executed

Commands run:

```bash
cargo fmt --all -- --check
cargo +1.95.0 test -p codegg-core session::human_shell -- --test-threads=1
cargo +1.95.0 test -p codegg --lib human_shell_promotion_stage_is_idempotent_and_recovers_after_daemon_rebuild -- --test-threads=1
cargo +1.95.0 test -p codegg --lib accepted_turn_injects_staged_shell_projection_once_and_consumes_it -- --test-threads=1
cargo +1.95.0 test -p codegg --lib shell_ -- --test-threads=1
cargo +1.95.0 test -p codegg --test shell_projection_harness -- --test-threads=1
cargo +1.95.0 test -p codegg --test shell_projection_phase10 -- --test-threads=1
python3 scripts/check_work_plan_repository_binding.py
python3 scripts/check_project_catalog_invariants.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_audit_coverage.py
scripts/verify.sh quick
```

Results:

- Storage lifecycle/concurrency: 4 passed.
- Daemon idempotent stage/reconnect/redaction: 1 passed.
- Accepted-turn runner capture and consume-once: 1 passed.
- Shell-focused library suite: 111 passed.
- Shell projection harness: 11 passed.
- Shell projection phase 10: 33 passed.
- Repository-binding, catalog, execution-ownership, scheduler, and audit guards: passed.
- `scripts/verify.sh quick`: passed, including workspace/all-target locked check.

## 5. State, cancellation, and recovery semantics

Staged payload remains pending through busy or rejected admission and is restored if the daemon loses a process-local active turn. Once the daemon has accepted the turn runtime and injected the projection into its provider-facing context, the record is consumed and cannot be replayed; a later cancellation does not restore it because the evidence may already have reached the provider. This is the at-most-once boundary. Expired/consumed rows clear their payload; old-daemon requests fail explicitly without a staged UI state.

## 6. Security review

The security-hardening review found and fixed a trust-boundary issue before closure: the daemon initially trusted client-provided redaction. The canonical model redactor now runs again in the session owner and rejects empty, oversized, or unavailable-redaction results. The daemon test sends a bearer token directly in the stage DTO and verifies it is absent from the recovered payload. Raw stdout/stderr are never written to this table or audit records. Session lookups filter by authenticated principal and runtime-derived project/workspace. Stage and list require `AgentInvoke` plus a human/local-owner principal; observers and non-human principals are refused.

## 7. Migration and compatibility review

Migration v71 is additive and leaves existing session/message rows unchanged. Existing clients that omit promotion IDs retain their former turn behavior. Older daemons return an unsupported/unexpected typed operation result; the TUI does not report staged state and keeps remote composer content explicitly draft-only. Database rollback behavior was not exercised; the migration is additive and has no reinterpretation of existing data.

## 8. Documentation and operations

Updated `architecture/human_shell.md`, `architecture/session.md`, `architecture/tui.md`, `architecture/audit.md`, `.opencode/skills/human-shell/SKILL.md`, `docs/tui.md`, `AGENTS.md`, the source roadmap, plan, prior C001 closure addendum, and registry. The repository-binding guard was changed to require storage layout version >= v68 rather than pinning the unrelated current layout to v68.

## 9. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Native macOS and old-daemon binaries were not available in this Linux environment. | Native platform and historical wire-version behavior were not executed here. | Keep old-daemon failures explicit; add host-specific evidence when available. |

## 10. Roadmap disposition

C004 is closed. C001's context/provenance dependency is satisfied. C002 workspace-owned finite human-shell dispatch is eligible and remains the next plan in this line.

## 11. Registry updates

Mark C001 and C004 closed, retain independent C003 closure, and move C002 from blocked to dependency-ready. Sandbox containment C004 remains separately blocked on native Darwin qualification.
