# Desktop Frontend/IDE Post-Closure Corrective C003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation-post-closure-corrective/003-diagnostics-not-ready-contract.md`

Source subsystem roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-post-closure-corrective-addendum.md`
- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-006-tui-ide-vertical-slice`

Repository baseline reviewed: `1c4a30563755056c9d16152e48b5bb56612de992`

Implementation commits:

- `87ae2d4ac` — add fail-closed diagnostics readiness to protocol, daemon result, and client reconciliation.
- `3fa233146` — derive the `NotReady` default required by workspace Clippy.
- `2967e899f` — enable manual root-CI dispatch on feature refs so the branch can receive hosted verification.
- `d31714a35` — resolve all findings from the second hosted Clippy run.
- `1c4a30563` — isolate Unix timeout-bound Git children into process groups, preventing timeout cleanup from killing the caller; record the hosted test failure and correction.

Hosted root CI: `38231581158` — success on the exact reviewed baseline.

## 1. Executive finding

C003 is complete. `LspDiagnosticsGet` now distinguishes an unavailable service or
missing authoritative snapshot (`NotReady`) from an authoritative complete empty
set (`Ready` with no files). Older responses without the additive readiness field
fail closed to `NotReady`, and the client preserves its last known diagnostics
until it receives an authoritative `Ready` result. Hosted root CI passed all
12,767 tests, including both daemon readiness regressions. M006-C is unblocked;
it remains explicit-pull based and makes no live-publisher claim.

The hosted run also exposed an unrelated but severe Unix Git timeout-cleanup bug:
the child had not been placed in its own process group, so Linux cleanup could
signal the test runner's group. C003's final verified revision includes the
process isolation fix and regression coverage. The accepted Git process contract
and skill documentation now state this requirement.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Unavailable service or absent snapshot is distinct from authoritative empty | `diagnostics_result`; `unavailable_or_unobserved_diagnostics_are_not_ready`; `an_observed_empty_set_is_ready_and_authoritative` | pass | Both readiness outcomes passed in hosted root CI. |
| Readiness is additive and missing fields fail closed | `LspDiagnosticsStatus`, serde default, `diagnostics_without_readiness_field_fail_closed_to_not_ready` | pass | An older daemon payload cannot clear the displayed set. |
| Reconciler preserves its baseline on `NotReady` and clears on `Ready` + empty | `not_ready_pull_preserves_the_last_complete_diagnostics_baseline`; `ready_empty_pull_is_authoritative_and_clears_the_baseline` | pass | Hosted workspace tests passed. |
| `file.read`, request family, and warm-only LSP ownership remain unchanged | daemon authorization and read-surface trajectory tests; hosted root CI guards/tests | pass | No implicit startup or new capability was introduced. |
| DTO bounds and disclosure rules remain unchanged | existing LSP DTO caps and read-surface regression suite | pass | Only the readiness status field was added. |
| Workspace timeout cleanup cannot kill its caller | `isolate_child_process_group`; 84 `egggit` tests; hosted `Workspace tests` | pass | Final hosted run completed the formerly SIGKILLed timeout tests and all remaining tests. |

## 3. Production implementation evidence

`LspDiagnosticsResultDto` carries a serialized `Ready`/`NotReady` status whose
default is `NotReady`. The daemon returns `Ready` only when the service is
available and the diagnostics store supplies an authoritative snapshot; a
complete snapshot with zero files remains `Ready`. Invalid or unavailable
requests do not fabricate an empty clean result. Client reconciliation ignores
`NotReady` payloads and retains the previous complete baseline.

The C003 implementation also fixes Unix child cleanup in `egggit`: commands
that may be terminated as a tree are process-group leaders before spawn. This
keeps timeout cleanup scoped to the child and its helpers.

## 4. Verification executed

### Commands run locally

```bash
cargo fmt --check --all
cargo test -p codegg-protocol diagnostics_without_readiness_field_fail_closed_to_not_ready
cargo test -p codegg-client --lib not_ready_pull_preserves_the_last_complete_diagnostics_baseline
cargo test -p codegg-client --lib ready_empty_pull_is_authoritative_and_clears_the_baseline
cargo clippy -p codegg-protocol --all-targets --locked -- -D warnings
cargo check -p codegg --features server --locked
cargo test -p egggit --locked
cargo clippy -p egggit --all-targets --locked -- -D warnings
python3 scripts/check_git_forbidden_patterns.py
scripts/verify.sh quick
```

Results: all listed local commands passed. The `egggit` package run passed 84
tests. A local attempt to link `cargo test -p codegg --lib
diagnostics_readiness_tests --locked` could not resolve system zlib/deflate,
iconv, and lzma libraries on this macOS host; hosted workspace tests ran and
passed the same daemon tests.

The optional `scripts/check_work_plan_repository_binding.py` was also run; it
reported five pre-existing dependency-boundary findings in ignored `.worktrees/`
checkouts. Those unrelated checkouts were not part of C003 and were not changed.

### Hosted result

Workflow dispatches were spaced and each run was checked for active progress.
Attempts `38228419611` and `38228801296` reached Workspace Clippy and failed on
new lint findings; the findings were corrected. Attempt `38229457314` passed
Formatting and Workspace Clippy but exposed the Unix Git timeout process-group
bug: two timeout test processes received `SIGKILL`. The corrective revision
passed hosted root CI `38231581158` on SHA
`1c4a30563755056c9d16152e48b5bb56612de992`: all guards and formatting passed,
Workspace Clippy passed, and Workspace tests reported 12,767 passed, 5 skipped,
0 failed.

## 5. Invariant review

1. `NotReady` has no file payload and does not represent a complete result.
2. `Ready` with no files remains authoritative emptiness.
3. An omitted readiness field defaults to `NotReady`.
4. Client state only changes from a complete `Ready` response; failed and
   unavailable pulls preserve the last known diagnostics as stale.
5. `file.read` authorization, request routing, payload caps, and warm-server-only
   behavior are unchanged.
6. Unix process-group cleanup now targets a private child process group.

## 6. Failure and recovery review

An unavailable service or missing snapshot returns `NotReady`; the client keeps
the last complete diagnostics set. A rejected request remains its existing
distinct error and likewise does not clear the set. No new durable state,
recovery path, or push-delivery mechanism was added. Git timeout cleanup kills
the child group and reaps the direct child, avoiding collateral termination of
the caller.

## 7. Migration and compatibility review

No database schema or migration changed. The readiness field is additive. Older
responses deserialize to `NotReady`, which is conservative and cannot falsely
clear diagnostics. No protocol negotiation or configuration migration is
required.

## 8. Security review

The existing project-scoped `file.read` authority is preserved. Diagnostics
payload bounds and disclosure policy are unchanged. The process-group fix
prevents timeout cleanup from signaling unrelated caller processes; it does not
change Git command authorization or environment policy.

## 9. Documentation and operations

- Updated `architecture/lsp.md` and `.opencode/skills/lsp-ide/SKILL.md` for the
  diagnostics readiness contract.
- Updated `architecture/git.md` and `.opencode/skills/git/SKILL.md` for
  Unix process-group timeout cleanup.
- Added `workflow_dispatch` to `.github/workflows/ci.yml` to verify this feature
  branch without opening a pull request against its large commit history.
- Registry and roadmap now mark C003 closed and M006-C ready.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| none | No C003 correctness finding remains | The unavailable-versus-empty contract is restored and hosted verification is green. | None. |

The M006-B change publisher remains undriven, as already recorded by C002 and
the immutable M006-B closure. M006-C is explicitly pull-based and does not
depend on or claim publisher behavior.

## 11. Roadmap disposition

C003 is closed. M006-C handoff 009 is dependency-ready and may proceed with
explicit diagnostics pulls. No broader desktop capability is claimed by this
corrective closure.

## 12. Registry updates

- Mark C003 closed in the corrective addendum and registry.
- Mark M006-C ready in `plans/registry.md` and update handoff 009 to the verified
  repository baseline.
- Keep the post-closure corrective roadmap active for any separately registered
  work; no publisher-driving work is added by this closure.
