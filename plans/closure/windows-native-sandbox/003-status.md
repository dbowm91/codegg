# Windows Native Sandbox Milestone 003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/windows-native-sandbox/003-native-shell-and-windows-policy-inputs.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#7-milestones`

Repository baseline reviewed: `56e34c435b9d1b07f1bfb6f8343e530ae273cbd7`

Implementation and native workflow revision:
`f092a59fc9f2f299b6133a2992540426c55aff36`

Windows workflow: `https://github.com/dbowm91/codegg/actions/runs/38118934632`

## 1. Executive finding

M003 is strictly closed for native shell dispatch, Windows policy inputs, and
pre-launch path validation. The managed shell now selects absolute Windows
executables, carries `cmd.exe` command text through its dedicated raw command
channel, and gives each shell run a private temporary directory. The Windows
policy compiler resolves Windows runtime and credential roots, keeps workspace
write roots distinct, and rejects the tested unsafe path forms. No Windows
filesystem or network enforcement is claimed; constrained Windows launches
remain unavailable pending M004.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Native shell selection does not depend on `sh` or PATH aliases | `src/tool/bash/process.rs`; Windows workflow `38118934632` | pass | PowerShell, cmd, and Git Bash resolve to absolute executables; WSL was path/argv tested only. |
| Raw cmd syntax reaches `cmd.exe` without argv quoting corruption | `configured_cmd_shell_executes_raw_text`; `cmd_shell_text_moves_to_the_unescaped_command_channel` | pass | Native command smoke covers metacharacters, `%COMSPEC%`, `ver`, spaces, and Unicode. |
| PowerShell and common Windows tools execute through the managed route | `native_shell_receives_a_private_per_run_temp_directory`; `native_shell_can_run_common_toolchain_smoke_commands` | pass | Default PowerShell ran; `rustc`, Cargo, Git, and Python smoke commands passed. |
| Git Bash works when explicitly selected | `configured_git_bash_executes_raw_text` | pass | Native output contained the expected marker. |
| Windows policy roots use known folders and preserve credential denies | `windows_roots_use_known_folders_and_retain_credential_denies`; `windows_launch_spec_keeps_workspace_write_roots_distinct` | pass | Policy inputs only; not an access-control result. |
| Unsafe path forms are rejected or bounded | `windows_policy_path_rejects_unc_device_ads_and_traversal`; `windows_policy_path_rejects_junction_components`; `windows_policy_path_normalizes_case_aliases` | pass | Covers UNC/device paths, ADS, traversal, junction components, and case aliases. |
| Shell resolution is not spoofed through PATH | `windows_policy_roots_cannot_be_resolved_through_search_path`; `configured_windows_shells_resolve_outside_path` | pass | Executables are resolved from the intended system/install roots. |
| Windows writable scratch is private and cleaned after completion | `native_shell_receives_a_private_per_run_temp_directory` | pass | `TEMP` and `TMP` matched a per-run `codegg-shell-*` directory, which was removed after exit. |
| Windows native suite and repository verification pass | Workflow `38118934632`; `bash scripts/verify.sh quick` | pass | Exact results below. |

## 3. Native Windows evidence

Hosted workflow run `38118934632` passed on revision
`f092a59fc9f2f299b6133a2992540426c55aff36`:

- OS: Microsoft Windows Server 2025 Datacenter, version `10.0.26100`, build
  `26100`.
- Rust: `rustc 1.89.0 (29483883e 2025-08-04)`, target
  `x86_64-pc-windows-msvc`.
- `cargo test --locked -p codegg --test windows_process_launcher -- --nocapture --test-threads=1`:
  8 passed, 0 failed.
- `cargo test --locked -p codegg --lib managed_process::windows::tests::failed_job_assignment_never_runs_the_suspended_target -- --nocapture`:
  1 passed, 0 failed.
- `cargo test --locked -p codegg --lib windows_policy_tests -- --nocapture`:
  6 passed, 0 failed.
- Raw shell argument, shell resolution, cmd raw-text transport, private temp,
  toolchain smoke, configured cmd, and configured Git Bash tests: each passed.

The Windows run initially exposed cmd command-line failures: slash-form system
paths in `COMSPEC` and cmd's `/C` quote handling. The final revision uses native
path separators and the `/D /S /C` raw-command route. The final workflow proves
the corrected behavior; earlier failed runs are not counted as evidence.

## 4. Unix and repository verification

At the closure revision, `bash scripts/verify.sh quick` passed, including
formatting, sandbox contract, execution ownership, authority and routing
guards, and locked workspace all-target check.

The host default toolchain is x86_64 Homebrew Rust 1.98.1. Focused local test
linking previously failed in native libgit2 dependencies (`zlib`, `iconv`, and
`lzma`); explicitly targeting ARM64 with the installed Rust 1.89 toolchain
could not run because that toolchain lacks the `aarch64-apple-darwin` target.
The hosted Windows focused suites above and Unix-compatible quick verification
are the passing evidence. No local focused Unix unit-test result is claimed.

## 5. Invariant and security review

- Shell selection remains in the existing `BashTool` authorization and
  `ManagedProcessService` execution path; Windows shell choice does not create
  an authorization bypass.
- Structured argv remains separate from shell text. The raw command field is
  restricted to explicitly selected Windows `cmd.exe` calls.
- Per-run shell temp cleanup is tied to managed process completion. It does
  not grant persistent workspace or profile writes.
- Windows path normalization is an input-validation aid, not a substitute for
  NTFS access checks. Hardlinks and path replacement after validation remain
  unproven and are assigned to M004/M006.
- There is no Windows filesystem token, ACL lease, or network enforcement in
  this milestone. Windows constrained launches remain fail-closed/unavailable.
- Unix shell selection and sandbox behavior are unchanged.

## 6. Documentation and operations

Windows shell and policy behavior is described in the updated architecture
and operator documentation. The Windows workflow records OS build and Rust
metadata and runs the native launcher, policy, and shell suites. WSL execution
is not claimed because no provisioned WSL distribution is part of this runner.

## 7. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| informational | Path checks cannot prove hardlink identity or prevent post-validation replacement. | A normalized path is not a filesystem boundary. | M004/M006 must validate effective handle/ACL behavior and reparse races. |
| informational | Windows filesystem and network restrictions are not implemented. | Constrained Windows requests remain unavailable. | M004 owns read/write proof; M005 owns optional offline networking. |
| informational | WSL was only resolved and argv-tested. | No WSL distribution compatibility claim. | Test only on a separately provisioned WSL host if support is later required. |

No unresolved high or medium finding remains in M003 scope.

## 8. Roadmap and dependency disposition

M003 is strictly closed. Its hard dependency on M002 is satisfied, and M004 is
now dependency-ready and active. M004 must prove both read and write
restrictions, credential denial, descendant behavior, and ACL cleanup before
any constrained Windows profile can be reported enforced. The M003 workflow is
not privileged isolated-VM or ACL evidence. M005 remains blocked on M004 plus a
separately provisioned identity/network fixture; M006 remains blocked on M004
(and on M005 only for offline claims). Optional evidence-only M007 remains
available. No constrained Windows enforcement claim is authorized by this
closure.
