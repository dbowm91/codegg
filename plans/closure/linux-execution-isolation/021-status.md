# Linux Execution Isolation M004 — Constrained PTY Launch Ownership

Status: closed

Source implementation plan:

- `plans/archive/implementation/linux-execution-isolation/021-m004-constrained-pty-launch-ownership.md`

Source subsystem roadmap:

- `plans/subsystems/linux-execution-isolation-roadmap.md#registered-successor-packages`

Repository baseline reviewed: `c997586a9aba4df4f7acb8e30239a5e6bc8dc87b`

Implementation commits or pull requests:

- `bc11fbff` — add supervised constrained PTY launch owner
- `b5e4ef0b`, `23962f1f`, `b37caa9c`, `33766779` — diagnose and correct native lifecycle, foreground process-group, and job-control behavior
- `42d18fe9` — route daemon-authorized constrained PTYs through the M004 owner
- `b681a60c`, `715e7e6f`, `66375f48` — qualify restricted-host refusal and remove the helper-exit observation race
- Exact implementation source: `66375f487c21da76dc8f2bcdda5ec694f32ce7f1`
- Exact-source hosted Linux native workflow: [run 38983005904](https://github.com/dbowm91/codegg/actions/runs/38983005904), success

## 1. Executive finding

C013 is complete. Daemon-authorized constrained PTY creates now route through a
supervised, helper-backed M004 owner for the PTY session's full lifetime. The
profile comes from the transport-bound principal preference; no request DTO can
select or widen it. Hosted Linux proved a usable TTY and foreground job-control
contract, private process view, daemon routing, descendant cleanup, protected
mount/Git regression compatibility, and refusal on a host with user namespaces
disabled. FullHost routing remains on its existing path.

This closes the constrained PTY owner slice only. M004 remains active for
contained LSP process ownership and broader host-policy qualification. M003's
no-admission decision is unchanged; M005 remains production-gated; no Linux
host or enhanced backend is production-admitted by this result.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Only daemon-authorized principal policy can select constrained routing | `src/core/daemon.rs`, `src/interactive_process_attach.rs`; hosted test `m004_daemon_constrained_pty_uses_principal_profile_and_cancels_descendant` | pass | Profile is carried in internal spawn state; it is not caller-selectable. |
| Setup acknowledgment precedes target execution and PTY owner retains the helper receipt | `src/interactive_process.rs`; hosted `m004_live_owner_preserves_tty_and_private_process_view` | pass | Long-lived session tracks helper status and exit. |
| PTY has TTY stdio and a valid foreground process group while host proc remains hidden | Same live-owner qualification; hosted run 38983005904 | pass | Test uses all three TTY streams, `tcgetpgrp`, and checks daemon environment is not exposed through host `/proc`. |
| Daemon route can terminate target and descendants | Hosted daemon route test in run 38983005904 | pass | Descendant marker remained absent after terminate; session was removed. |
| Unsupported user-namespace policy fails before target, handle, or success audit | `m004_daemon_constrained_pty_fails_closed_when_user_namespaces_restricted`; restricted-userns job in run 38983005904 | pass | Target marker absent, no session admitted, event sequence unchanged. |
| Process/IPC denial guarantees hold for helper-backed operations | Hosted M004 denial target in run 38983005904 | pass | 9/9 native tests passed in 616.35 seconds. |
| Protected-mount and trusted-Git behavior remains compatible | Hosted regression steps in run 38983005904 | pass | 11/11 protected-mount tests and 20/20 egggit tests passed. This does not admit M003 or enable trusted Git through the PTY path. |
| FullHost route and scheduler ownership are preserved | FullHost regression coverage in `tests/interactive_process_attach_resume.rs`, `src/interactive_process_attach.rs`; local suite | pass | Existing FullHost behavior remains on its prior route. |
| Ownership and safety contracts are documented and guarded | `docs/execution-ownership.toml`, `architecture/jobs.md`, `architecture/security.md`, `.opencode/skills/tool-execution/SKILL.md`; static guards below | pass | No new direct process-spawn owner was introduced. |

## 3. Production implementation evidence

The daemon resolves the requested profile from its principal-bound preference
and passes that trusted policy across the internal interactive-process
boundary. `SpawnSpec` carries the resulting constrained profile to the live
session owner. The owner starts the trusted sandbox helper, retains the
status/ack receipt, PTY descriptors, cancellation and wait ownership, and
returns a usable PTY only after setup succeeds. The helper pins its executable
and establishes the PID-namespace-visible foreground process group before
execing the target in place.

The existing FullHost path is preserved. A constrained setup failure remains
fail-closed: no direct spawn, target marker, admitted session, or success audit.
The implementation does not modify Git object views, promote the protected
mount candidate, attach LSP launches to this owner, or route M005 guarantees.

## 4. Verification executed

### Commands run

Local on the exact source:

```bash
rtk proxy cargo test --locked --test interactive_process_attach_resume -- --test-threads=1
CODEGG_RUN_M004_RESTRICTED_USERNS=1 rtk proxy cargo test --locked --test interactive_process_attach_resume m004_daemon_constrained_pty_fails_closed_when_user_namespaces_restricted -- --test-threads=1 --nocapture
rtk proxy cargo fmt --all -- --check
rtk proxy scripts/verify.sh quick
rtk proxy python3 scripts/check_execution_ownership.py
rtk proxy python3 scripts/check_sandbox_contract.py
rtk proxy python3 scripts/check_adr_integrity.py
```

Hosted exact-source workflow 38983005904 on `66375f487c21da76dc8f2bcdda5ec694f32ce7f1`:

```bash
cargo test --locked -p codegg --lib interactive_process::tests::m004_live_owner_preserves_tty_and_private_process_view -- --test-threads=1 --nocapture
cargo test --locked --test interactive_process_attach_resume m004_daemon_constrained_pty_uses_principal_profile_and_cancels_descendant -- --nocapture
cargo test --locked -p codegg --lib managed_process::tests::m004_bwrap -- --test-threads=1 --nocapture
cargo test --locked -p egggit --lib process::tests::trusted_git -- --test-threads=1
```

### Results

- Local attach/resume integration suite: 22 passed. The hosted-only positive
  native PTY tests were compiled but not run locally because this host cannot
  establish the required unprivileged namespaces.
- Local restricted-userns PTY negative test: 1 passed. Positive launch was
  unavailable on this local host; no local positive-native claim is made.
- Local formatting, `scripts/verify.sh quick`, execution-ownership guard,
  sandbox contract guard, ADR integrity guard, and `git diff --check`: passed.
- Hosted process/IPC denial target: 9 passed, 0 failed, 616.35 seconds.
- Hosted live PTY owner target: 1 passed, 0 failed.
- Hosted daemon-authorized route and descendant cleanup: 1 passed, 0 failed.
- Hosted protected-mount regression: 11 passed, 0 failed.
- Hosted trusted-Git regression: 20 passed, 0 failed.
- Hosted restricted-userns job passed both existing process-tier refusal and
  the new constrained PTY refusal/no-audit test.
- An earlier hosted attempt exposed a race in the test's observation of async
  helper reaping after target output. The test now waits with a bounded timeout
  for the actual helper exit state; the exact-source rerun passed.

Hosted runner facts from the exact-source job: GitHub-hosted Ubuntu 24.04,
x86_64, kernel `6.17.0-1022-azure`, Bubblewrap `0.9.0`, user namespaces enabled
with `user.max_user_namespaces=63842`, seccomp actions available, and the runner
AppArmor profile `unconfined`. The workflow temporarily disabled
`kernel.apparmor_restrict_unprivileged_userns` only inside its ephemeral VM to
run the positive native tests. A separate job set `user.max_user_namespaces=0`
and verified fail-closed behavior. This qualifies these runner conditions, not
all Linux host configurations.

## 5. Invariant review

- Daemon-resolved profile remains authoritative; client DTOs cannot widen it.
- Setup/identity/ack failure stops before target execution and successful
  session/audit admission; restricted-host probes verified this behavior.
- PTY and private handshake descriptors are held by the owner; unrelated host
  descriptors do not cross the helper boundary.
- The live owner retains terminal I/O, resize, cancellation, termination,
  descendant cleanup, helper wait/reap, and scheduler lifetime.
- FullHost remains on its compatible route; constrained requests never fall
  back to direct spawn.
- PTY routing claims only M004 process/IPC containment. It does not admit M003,
  authorize Git execution, claim M005 network isolation, or claim cgroup limits.

## 6. Failure and recovery review

Helper/setup refusal, missing namespace capability, or invalid setup receipt
fails before exposing a PTY handle. The owner cleans the helper and owned
descendants on termination and closes the PTY. The hosted daemon test confirms
descendant cleanup; the restricted-userns test confirms no-target/no-session
refusal and no success audit. Sessions own independent helper/status resources.
Restart does not reuse a prior helper receipt. No persistent lease or scratch
state is introduced by this slice.

The local host could not run the positive native path. Exact-source hosted
native proof replaces that environmental limitation; it is recorded above and
does not generalize to other kernel/AppArmor/Bubblewrap configurations.

## 7. Migration and compatibility review

No schema, persisted preference, public protocol DTO, or default changed. The
daemon continues to resolve the profile from principal-bound policy. FullHost
interactive process behavior remains compatible. Constrained PTY setup on
unsupported hosts remains an explicit refusal without fallback.

## 8. Security review

Authority remains daemon-owned; workspace roots and the helper are constructed
from trusted policy. The helper setup acknowledgment is bound to the running
session and retained for its lifetime. The tested PTY does not expose host
`/proc`, and the restricted-host route produces no target or successful audit.
Output and terminal bytes remain session-owned. No credentials or private
handshake payloads are added to logs.

## 9. Documentation and operations

- Updated `docs/execution-ownership.toml` for the constrained PTY owner.
- Updated `architecture/jobs.md` and `architecture/security.md` with routing,
  ownership, and qualified guarantees.
- Updated `.opencode/skills/tool-execution/SKILL.md` to match runtime ownership.
- Passed `check_execution_ownership.py`, `check_sandbox_contract.py`, and
  `check_adr_integrity.py`.
- Added exact-source positive and restricted-userns coverage to
  `.github/workflows/linux-m004-native.yml`.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Constrained LSP launch is not owned by an equivalent M004 duplex process service | LSP remains unavailable for constrained turns | Complete the separate daemon-owned LSP owner and qualify profile/workspace/server/generation binding. |
| medium | Native proof currently covers a specific Ubuntu 24.04 hosted runner configuration | Other kernels, AppArmor/SELinux policies, and restricted-userns hosts may refuse | Keep host support qualification open; retain fail-closed behavior. |
| high | M003 protected workspace mount view has no admitted production host due object-view latency/compatibility gates | Constrained PTY does not imply protected Git metadata or M003 admission | Preserve C010 no-admission decision; address with its own corrective and qualification work. |
| medium | M005 network namespace owner coverage is not part of this PTY slice | No offline-network guarantee is made for this route | Continue M005 production-owner and native qualification independently. |

## 11. Roadmap disposition

C013 is closed and the constrained PTY owner route is qualified for the tested
host conditions. M004 remains active: its next owner gap is constrained LSP,
followed by broader supported-host qualification. M005 remains independently
gated on production-owner coverage. M003 remains blocked under C010's
no-admission decision; no Linux host is admitted. The PTY owner result does not
unblock C002/C003 Git execution.

## 12. Registry updates

- Mark C013 closed and point its history to this closure and the archived
  implementation plan.
- Update M004 to show PTY ownership complete while LSP and broader host
  qualification remain active.
- Leave M003/C010, M005, C002/C003, and Linux backend admission unchanged.
- No further registered plan is unblocked by this slice alone.
