# Architecture Convergence M005 — Durable Run Rerun/Replay Completion Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/architecture-convergence-incomplete-verticals/005-durable-run-rerun-replay-completion.md`

Source subsystem roadmap:

- `plans/subsystems/architecture-convergence-incomplete-verticals-roadmap.md`

Repository baseline reviewed: `3c4890035513cd4d74430b6f64523c8be676024e`

Implementation commit:

- `a6a5055` — complete the durable test-run rerun vertical and its scheduler,
  RunStore, TUI, protocol, projection, cancellation, and documentation paths.
- `5dd280a` — harden recorded-base validation, preserve historical subdirectory
  execution context, redact test argv across durable surfaces, and begin the
  child RunStore record before process launch.

## 1. Executive finding

M005 is complete for the bounded supported vertical: a completed, failed, or
timed-out `RunKind::Test` with a credential-free, audit-safe
`test_runner` rerun descriptor can be requested from the TUI, validated by the
daemon against the current bound session and canonical workspace, admitted by
the normal scheduler, and executed by the existing supervised test runner.

The child receives a fresh job/attempt/run identity, persists a parent link,
and publishes `RunRerunLinked` after child persistence. The parent is read
only throughout. The former `ShellRerun { id: 0 }` placeholder is removed.

Git-mutating/worktree-dependent, Python, shell, and other run classes remain
explicitly ineligible; they are not represented as partially safe replay.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Define a bounded rerunnable specification | `src/run_rerun.rs` accepts only `RunKind::Test`, terminal eligible statuses, `test_runner`, non-empty audit-safe argv, no script source reference, and a valid workspace-contained cwd | pass |
| Derive `can_rerun` from reconstructability | `RunCellView::from_manifest` checks kind, status, backend, descriptor base/cwd, argv, script source, and redaction marker; daemon repeats authoritative validation including canonical recorded base and test argv checks | pass |
| Typed daemon/core request | Additive `CoreRequest::RunRerun` and `CoreResponse::RunRerunAccepted` route the operation through `CoreDaemon` | pass |
| Current authority is checked | Daemon requires a currently registered session, verifies its workspace binding, and compares the historical session/workspace identity | pass |
| Scheduler authority is preserved | `RunRerun` creates `NewJob` and calls `JobSubmissionService::submit`; no direct executor or process call is introduced | pass |
| Fresh durable identity and parent immutability | Child is a new scheduler job/attempt; the leased RunStore begins the child before process launch, and `RunDraft.parent_run_id`/`RunOwnership::ChildOf` are set without writing the parent | pass |
| Linkage and projection event | Test completion carries child/parent IDs; `BusEventSink` emits `RunRerunLinked`; safe-publication classification accepts the event | pass |
| Secrets are not replayed | Rerun reconstructs only `AuditSafeArgv`; test reports, invocation metadata, legacy index, and rerun descriptors use the audit form; redacted credential markers return `ineligible_secret_reacquisition_required` | pass |
| Cancellation applies to child | Scheduler token is propagated into test supervision; process group is killed and the persisted report becomes `Cancelled` | pass |
| TUI placeholder is removed | `src/tui/commands/run_rerun.rs` sends the typed request and reports accepted child job IDs or stable denial diagnostics | pass |
| Restart/replay visibility | Child is owned by the workspace RunStore and normal list/get/event replay paths; no in-memory-only linkage was added | pass by storage/event design; runtime restart test is host-limited |

## 3. Supported and ineligible run-class matrix

| Historical class | Rerun disposition | Reason |
|---|---|---|
| `RunKind::Test` with safe `test_runner` argv and valid workspace/cwd | supported | Complete durable reconstruction exists |
| Test run with redacted credential marker | ineligible | Requires current credential reacquisition; no silent expansion |
| Test run with missing/empty descriptor or invalid cwd/workspace | ineligible | `ineligible_missing_spec` or `ineligible_missing_or_invalid_base` |
| Running, cancelled, or incomplete run | ineligible | Historical execution is not a stable rerun base |
| Raw shell / managed process | ineligible | No bounded reconstruction contract |
| Python | ineligible | Existing Python records do not expose a supported rerun contract |
| Git read/mutation or worktree-dependent run | ineligible | M003 base/repository/worktree reconstruction and credential hooks are not part of this bounded slice |
| Native/search/other run | ineligible | No explicit safe durable specification |

## 4. Production implementation evidence

- Added `src/run_rerun.rs` as the daemon-side validation and job-construction
  owner for the supported rerun class.
- The daemon now validates both recorded workspace roots and the historical
  manifest/descriptor cwd before submission; scheduler execution resolves
  relative cwd values against the leased canonical workspace.
- Added additive protocol request/response fields and mapped the existing
  `RunRerunLinked` event through the application event bridge and safe
  projection publication policy.
- Added `parent_run_id` to the test job payload and test request, and made the
  supervised runner persist session, workspace, parent, ownership, and
  audit-safe rerun metadata.
- Passed the scheduler-leased workspace RunStore into `JobExecutionContext`,
  and wired the daemon test executor to the canonical `BusEventSink`, so a
  daemon-created child cannot execute without durable artifacts/events.
- Propagated scheduler cancellation into the test runner and process-group
  cleanup.
- Test execution now confines raw argv to the process-launch boundary. All
  persisted invocation/report/index/descriptor surfaces receive sanitized
  argv, and a RunStore record is opened before child-process launch.
- Replaced the TUI local-store/sentinel handler with a registered async typed
  command and completion message. Normal session test submission also now
  preserves the active session ID.
- Updated RunStore, protocol, workspace-service, and secret-lifecycle
  documentation.

## 5. Storage, protocol, migration, and compatibility

No schema migration is required. Existing `RunManifest` parent/rerun/
ownership fields and existing `RunRerunLinked` protocol event were reused.
Protocol changes are additive. Existing historical manifests remain readable;
legacy records without the new supported descriptor simply evaluate as
ineligible. The RunStore remains the artifact authority and the scheduler
remains the admission authority.

## 6. Security and invariant review

- Historical manifests are never mutated by rerun.
- Current session and workspace authority is checked at request time.
- Rerun argv is sourced from `AuditSafeArgv`; redacted credential markers are
  denied rather than treated as usable credentials.
- No authenticated Git URL, provider key, authorization header, or shell
  secret is added to durable state.
- The scheduler remains the only admission path, and the leased workspace
  services provide the child RunStore.
- Child cancellation kills the supervised process group and follows existing
  RunStore terminal status semantics.
- `RunRerunLinked` is classified safe and requires a non-empty session for
  projection publication.

## 7. Verification executed

Successful:

```text
rtk cargo fmt --all -- --check
rtk cargo check -p codegg --all-targets
rtk cargo check -p codegg-core --all-targets
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk cargo test -p codegg-core rerun_descriptor_no_permission_persistence -- --nocapture
rtk cargo test -p codegg-core projection_replay::safe_publication::tests -- --nocapture
rtk cargo test -p codegg-git sensitive -- --nocapture --test-threads=1
rtk cargo test --test command_routing_execution_ownership test_runner_persists_only_audit_safe_argv -- --nocapture --test-threads=1
rtk scripts/verify.sh quick
rtk git diff --check
```

Focused core results:

- RunStore redaction persistence test: 1 passed.
- Safe-publication suite: 3 passed.
- `codegg-git` sensitive suite: 15 passed. The new root integration test is
  compiled by the quick/all-target checks but cannot link on this host because
  the configured x86_64 target selects incompatible arm64 MacPorts
  `liblzma`/`libiconv`; this is the same environmental linker limitation
  recorded for the root test binary above.
- Quick verification: passed generated-agent checks, core boundary,
  sandbox-contract, execution-ownership, formatting, and locked workspace
  all-target checking.
- All-workspace Clippy passed with no issues after one mechanical cleanup of
  an existing LSP smoke-test late-initialization warning exposed by the
  required `-D warnings` command.

Host-limited:

- `rtk cargo test -p codegg --lib run_rerun -- --nocapture` compiled but could
  not link the root test binary because the configured x86_64 macOS target
  selected incompatible arm64 MacPorts `liblzma`/`libiconv` libraries. The
  linker failure is environmental and occurs after compilation; the quick
  gate's workspace check and all source checks pass.

## 8. Downstream unblock audit

M005 depended on the conditionally closed M003 Git/worktree boundary and is
now closed. The post-hardening audit found no newly registered plan blocked on
M005: M006 remains ready
from M004, M007 remains ready from M002, and M008 remains independently ready
from the session-projection closure. Their statuses therefore require no
change. The unrelated runtime-safety C002 supported-Linux evidence blocker
also remains unchanged.

The registry and subsystem roadmap were updated in the closure change to
remove M005 from dependency-ready work, record M005 as closed, and retain
M006/M007/M008 as ready.

## 9. Residual limitations

The supported slice does not claim Git/worktree replay, credential helper
reacquisition, arbitrary transcript replay, model determinism, or replay of
unsupported historical run classes. Those require a separate explicit
contract and should be planned as a follow-up rather than inferred from this
test-run implementation. Runtime restart execution should be exercised on a
host with a compatible native linker; the durable design uses existing
workspace RunStore and event replay authorities.

Final disposition: **closed**.
