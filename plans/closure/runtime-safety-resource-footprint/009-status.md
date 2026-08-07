# Runtime Safety, Resource Control, and Footprint Corrective C001 — Closure Status

Status: conditionally closed

Source implementation plan: `plans/implementation/runtime-safety-resource-footprint/009-sandbox-helper-trust-channel-corrective-unblock.md`

Source subsystem roadmap: `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`

Repository baseline reviewed: `013f157639b82d16a38aca2764c819b3d63bd355`

Implementation commit: `013f157639b82d16a38aca2764c819b3d63bd355` — isolate sandbox helper trust channel and move corrective plumbing outside target cwd

Pull request context: PR #72, branch `planning/runtime-safety-resource-footprint`

## 1. Executive finding

C001's production corrective implementation is complete and locally verified.
The parent now resolves the helper from the canonical executable sibling,
creates the bounded specification in the system temporary directory, and
observes setup/exec state through a private typed status pipe. Target stdout and
stderr are ordinary bounded output and cannot forge sandbox state. The helper
requires complete Landlock enforcement and `no_new_privs`, then closes the
status writer on successful target `exec`.

The independent Codex Security diff review found no reportable candidate and no
unresolved critical, high, or medium trust-channel finding. Strict closure is
conditional because this macOS host cannot provide the required supported-Linux
Landlock run, and GitHub did not schedule a new hosted check for the pushed
revision. The Linux requirement is a substantive evidence condition, not a
reason to create another corrective plan.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Production helper identity is installation-owned | `sandbox_helper_path()` canonicalizes the running executable and its sibling; inherited helper environment, PATH, and target cwd are not consulted | pass |
| Test-only injection does not ship an arbitrary override | `run_inner(..., helper_override)` is `cfg(test)`; resolver unit test uses an internal fixture and an inherited-variable negative check | pass |
| Missing/non-regular/non-executable helper fails before target launch | strict resolver canonicalizes, checks regular-file metadata, and checks Unix executable bits | pass |
| Private bounded status transport | Unix pipe, fixed helper fd, 16 KiB stream cap, versioned length-prefixed JSON frames | pass |
| Setup/exec sequencing and writer isolation | helper emits `Enforced`, sets `FD_CLOEXEC`, then `exec`s; `ExecError` is a separate terminal frame; Linux fixture covers writer write denial | pass in source review; Linux runtime pending |
| Target output cannot forge status or lose marker text | managed service no longer scans stderr; marker-like target stderr is preserved byte-for-byte in focused test | pass |
| Missing/malformed/duplicate/oversized status fails closed | decoder unit tests plus missing-status fake-helper test | pass |
| Specification is outside target cwd and bounded | owner-only `NamedTempFile::new()` outside cwd; serialized spec capped at 64 KiB; helper verifies regular owner-only file | pass |
| Cleanup and read-only cwd | temp file is held through launch and dropped on every return path; read-only-cwd integration test is present and Linux-gated | source/focused pass; Linux runtime pending |
| Timeout/cancellation/process group behavior remains intact | managed-process and descendant suites; `scripts/verify.sh quick` | pass |
| Static regression guard | `check_sandbox_contract.py` regular run and negative self-test | pass |
| Supported-Linux enforcement | existing `sandbox_landlock` fixture covers allowed read, read-only denial, workspace write, outside-root denial, setup failure, and parent nonrestriction | not run on Darwin; required evidence remains |

## 3. Production implementation evidence

- `src/security/sandbox.rs` now defines the bounded local status frame and
  state machine, trusted sibling-helper resolution, and 64 KiB spec bound.
- `src/managed_process.rs` owns creation of the system-temp spec and Unix
  status pipe, passes only the helper writer through a child pre-exec fd
  duplication, waits for bounded status EOF, and preserves target stderr.
- `src/bin/codegg-sandbox-helper.rs` validates owner-only spec metadata,
  applies the complete policy, reports typed setup/exec status, and marks the
  status writer close-on-exec before replacing itself with the target.
- `src/python_script/executor.rs` consumes managed typed outcomes; its former
  stderr marker parser and marker-stripping tests are removed.
- `tests/sandbox_landlock.rs` now uses a private test pipe and covers enforcement,
  setup failure, and post-exec status-writer isolation on Linux.
- `scripts/check_sandbox_contract.py` rejects helper environment selection,
  marker parsing, cwd-rooted specs, and status-bypass patterns; `--self-test`
  proves each negative fixture is detected.

## 4. Verification executed

All commands below were run locally on macOS unless explicitly marked Linux.

```text
cargo fmt --all -- --check                                      pass
cargo check -p codegg --all-targets --locked                    pass
cargo clippy -p codegg --all-targets --locked -- -D warnings    pass
cargo test -p codegg managed_process --lib -- --test-threads=1  12 passed
cargo test -p codegg sandbox --lib -- --test-threads=1          56 passed
cargo test --test managed_process_descendants -- --test-threads=1 pass
cargo test --test sandbox_landlock -- --test-threads=1          1 passed (non-Linux unavailable path)
cargo test --test python_sandbox_adversarial -- --test-threads=1 57 passed
python3 scripts/check_sandbox_contract.py --self-test           pass
python3 scripts/check_sandbox_contract.py                       pass
python3 scripts/check_execution_ownership.py --self-test        pass
python3 scripts/check_execution_ownership.py                    pass
python3 scripts/check_daemon_cwd_usage.py                       pass
python3 scripts/check_git_forbidden_patterns.py                 pass
scripts/verify.sh quick                                          pass
```

The independent security diff scan completed with zero reportable findings:

- scan ID: `9bdd39dd-c457-4fd3-812c-8bf15116580c`;
- coverage: all five changed source rows reviewed in full;
- report: `/private/var/folders/2j/dlwhrpps66scv9bw8f7vdfg40000gq/T/codex-security-scans-poxVKR/codegg/a36b164a202fd1d7e94aa949b56a7f94fa993391_20260805T184651Z_uxz1jexq/report.md`;
- measured scan usage: 66,518 total tokens, 4,368,980 input tokens, and
  4,312,576 cached input tokens.

The scan reported only that delegated workers were unavailable; the parent
completed the bounded review directly. Its generated coverage receipt paths
were not retained by the temporary workbench because they referenced the
scan-local work ledger with an invalid receipt projection; this did not affect
the full-file review count or no-findings result.

The existing PR #72 hosted run is for the previous revision and is not evidence
for `013f157`. GitHub did not create a new run after the push, and no new CI
lane or workflow was added.

## 5. Invariant review

- Explicit sandbox requests fail closed when helper resolution, spec transport,
  status framing, Landlock setup, or target exec fails.
- Helper identity is independent of inherited environment, PATH, target cwd,
  and target-provided arguments.
- Target stdout/stderr are not inspected for setup state and marker-like text is
  retained as ordinary output.
- The target receives no status writer after successful helper `exec`.
- The local frame and spec are bounded at 16 KiB and 64 KiB respectively.
- The daemon remains unsandboxed; only the one-shot helper applies Landlock.
- Existing timeout, cancellation, bounded output, stdin, environment,
  process-group, and provenance ownership remains in `ManagedProcessService`.
- No public daemon/frontend/provider protocol, storage schema, CI lane, or
  persistent helper service was introduced.

## 6. Failure and recovery review

Setup failure, helper unavailability, target exec failure, target nonzero exit,
timeout, cancellation, output-limit termination, and cleanup diagnostics remain
distinct. Missing, malformed, oversized, duplicate, and contradictory status
streams are rejected before a required sandbox result is returned. The temp
spec is held only for the launch lifetime and is dropped on spawn, wait,
timeout, cancellation, status-read, and interpretation failures. Existing
descendant tests remain green.

## 7. Migration and compatibility review

No database, RunStore, memory, job, configuration, daemon protocol, or frontend
migration is required. Unsandboxed execution is unchanged. Explicit sandbox
execution remains fail-closed on unsupported hosts. The helper packaging rule
is the existing sibling-binary layout; no arbitrary helper configuration was
added.

## 8. Security review

The independent diff review examined helper path canonicalization and metadata,
test-only injection, status descriptor ownership and close-on-exec sequencing,
bounded frame decoding, spec permissions/lifecycle, target output handling,
and fail-closed outcomes. It found no critical, high, or medium issue and no
reportable candidate. The remaining conditional item is runtime evidence on a
supported Linux kernel, not an identified code defect.

## 9. Documentation and operations

Updated `architecture/security.md`, `architecture/jobs.md`, and
`docs/execution-ownership.md` to describe trusted helper resolution, bounded
spec transport, and the private status channel. No separate hosted rerun per
predecessor milestone and no new CI lane or workflow were added.

## 10. Unresolved findings (severity: critical/high/medium/low)

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium evidence gap | Supported-Linux enforcement was not runnable on this Darwin host; GitHub did not schedule a new run for the pushed revision | Strict sandbox correctness evidence remains incomplete; no local source or focused-test defect was found | Run `cargo test --test sandbox_landlock -- --test-threads=1` on one Landlock-capable Linux host and record kernel, effective ABI, and all fixture outcomes |

No critical, high, or product-correctness medium finding remains open.

## 11. Roadmap disposition

C001 is conditionally closed: production correctness, focused local evidence,
static guards, documentation, and independent security review are accepted;
the named supported-Linux evidence condition remains.

- M001 and M002 retain their historical conditional closure records and now
  link this corrective disposition.
- M003 remains blocked because its substantive sandbox dependency is not yet
  strictly closed. Its executable/argv interface is stable, but promotion is
  intentionally deferred until the Linux result exists.
- M005/M006 hosted runner/cache conditions remain operational evidence and are
  not independent M007 implementation blockers.
- M007 remains blocked on M003; M008 remains blocked on C001 plus M001–M007.
- No new corrective plan is created for unavailable hosted dispatch.

## 12. Registry updates

- Marked the C001 implementation plan `implemented`.
- Added this closure record with `conditionally closed` status.
- Linked C001 from the M001 and M002 historical closure records.
- Removed C001 from the dependency-ready table and recorded it in the
  runtime-safety disposition as conditional.
- Audited all future registered plans: no downstream plan is ready because the
  supported-Linux evidence condition remains on C001/M001/M002; M003, M007,
  and M008 stay blocked with their existing dependency descriptions.
- Preserved the existing operational classification for M005/M006 and did not
  add a per-milestone hosted rerun or CI lane.

Final recommendation: conditionally closed; promote C001 and M001/M002 to
strict `closed` and M003 to `ready` after the one required supported-Linux
enforcement result is recorded.

## C002 final-integration addendum

C002 reconciled PR #72 with remote `main` and retained C001's accepted helper
trust-channel implementation. The final default CI job is not a substitute for
the missing kernel/ABI and enforcement-versus-skip record, so C001 remains
conditionally closed pending the one named supported-Linux fixture execution.
