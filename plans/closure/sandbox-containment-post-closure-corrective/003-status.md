# Sandbox Containment Post-Closure Corrective C003 — Closure

Status: closed

Source plan: `plans/implementation/sandbox-containment-post-closure-corrective/003-policy-deny-precedence-and-scoped-tool-state.md`

Implementation commit: `be7537cc`

Predecessors: C001 is closed at `001-status.md`; C002 is closed at `002-status.md`. Historical runtime-safety records remain unchanged.

## 1. Executive finding

The effective Linux Landlock grant removes credential-denied descendants from broader read and write roots. A directory needed only to reach a denied descendant receives listing-only rights; the denied file itself is not granted. Future-created deny paths are retained through nearest-existing-parent normalization. Policies that Landlock cannot express, including a denied path under a recursively writable root, fail before target execution. Existing denied regular files with hardlink aliases also fail preflight.

Seatbelt emits credential read/write denials after generic write denials and all writable-root re-allowances. Shared `/tmp`, user caches, package-manager state, and toolchain directories are no longer generic writable roots. A required managed launch receives a collision-resistant scratch directory with mode `0700`; `TMPDIR` points to it, and cleanup occurs after the supervised process tree is reaped. Python scripts and their temporary state use private run directories, while Transform persistence remains limited to the workspace. Native macOS qualification confirms these deny/write behaviors on the same Seatbelt backend used by Python.

## 2. Host and real-helper evidence

Linux host: `Linux deadpool 6.8.0-146-generic #146-Ubuntu SMP PREEMPT_DYNAMIC Thu Sep 3 16:12:30 UTC 2026 x86_64 GNU/Linux`.

The real helper reported `Enforced`, backend `landlock`, effective ABI 4, and guarantees for deny-first path allowlisting, ABI 3+ write/truncate control, ABI 2+ cross-directory refer handling, supported additional rights, `no_new_privs`, and exec inheritance. It reported no network isolation and the limits around inherited pre-restriction descriptors, host filesystem permissions, and rights newer than the crate supports.

| Real-helper probe | Result |
|---|---|
| Credential nested under a broad workspace root; read and write attempt | Denied; Landlock rejects the unrepresentable writable overlap before target exec, and no marker is created |
| Credential alias under workspace | Denied |
| Explicit deny outside workspace | Denied |
| Deny absent during spec preparation, created before helper apply | Denied |
| ReadOnly workspace mutation | Denied |
| WorkspaceWrite inside-workspace mutation | Allowed |
| Outside write and truncation | Denied; original data retained |
| Rename and hardlink from workspace to outside | Denied |
| Explicit scoped scratch write and sibling scratch write | First allowed; sibling denied |
| Offline `cargo metadata --no-deps` from scoped scratch | Passed with a scoped empty Cargo config and read-only toolchain |

The Linux suite also checks symlink aliases, external symlink targets, denied hardlinks, future-path carving, special file classification, and the no-shared-cache-write policy. Hosted native macOS run `38166386985` on commit `98f96a99` ran the real helper suite: 15 passed, including nested credential read/write denial under a workspace allowance, future-created denied paths, sibling scratch denial, malformed/rejected profile no-marker behavior, and mode-specific workspace rights. A separate native lifecycle test confirmed private temp root placement, mode `0700`, and removal after reap.

## 3. Scratch lifecycle and compatibility

- `ManagedProcessService` owns a collision-resistant `codegg-run-*` directory, explicitly secures it to `0700` on Unix before launch, exports it as `TMPDIR`, and holds ownership through process-tree reap. The focused lifecycle test confirmed mode `0700` and removal afterward.
- Python execution puts its script and private temporary state under a per-run directory. Persistent Python output is workspace-only in Transform mode; Analyze and Verify receive no persistent write root.
- A real shell/tool compatibility probe can read system configuration, resolve a command, and report its workspace. The Cargo metadata smoke starts from scoped scratch so Cargo does not discover the host's parent `.cargo/config.toml`; no host Cargo configuration or credentials are granted.
- No schema, configuration, or protocol migration was added. Workflows requiring globally writable package caches must use approved scoped state or receive a clear denial.
- No performance benchmark or cold-cache comparison was available. Toolchain reads remain read-only and are not copied per launch.

## 4. Verification

- `rtk proxy cargo test -p codegg --lib sandbox` — 99 passed.
- `rtk proxy cargo test -p codegg --lib constrained_launch_gets_private_tmpdir_removed_after_reap` — passed.
- `rtk proxy cargo test -p codegg --lib python_persistent_writes_are_workspace_only_with_private_scratch` — passed.
- `rtk proxy cargo test -p codegg --test sandbox_containment -- --test-threads=1 --nocapture` — 13 passed; helper identity and ABI recorded above.
- `rtk proxy scripts/verify.sh quick` — passed, including formatting, maintained guards, and locked workspace all-target check.
- `rtk proxy python3 scripts/check_sandbox_contract.py` and `rtk proxy python3 scripts/check_sandbox_policy_wiring.py` — passed.
- `rtk proxy git diff --check` — passed.

## 5. Conditional evidence and residual risk

Native Seatbelt policy evidence: macOS `15.7.9` build `24G830`, Darwin `24.6.0`, ARM64 M1 hosted runner. The helper suite reported `Enforced`, backend `seatbelt`, no ABI, and the declared write-confinement and credential-path-denial guarantees. The six read-only `--version` probes for `git`, `cargo`, `rustc`, `python3`, `brew`, and `gh` passed. Python Analyze/Verify/Transform tests passed 22/22. This does not establish Linux/macOS security parity: Seatbelt broadly allows reads while Landlock is an allowlist, and C004 records that difference explicitly.

Other limits remain explicit: no network isolation is claimed; loopback reachability was confirmed on macOS; inherited open descriptors remain usable; Landlock denies an unrepresentable overlap rather than weakening it. Apple Events/Launch Services remain documented cross-process channels; no safe fixture service was used to qualify their reachability, XPC/Mach access, or setuid privilege transitions. Those residuals are tracked by C004. Earlier runtime-safety closure conditions remain separate.

## 6. Roadmap and registry disposition

C003 is closed on Linux Landlock ABI 4 and native Apple Silicon Seatbelt evidence. C004's native C001/C003 prerequisites are satisfied; its residual IPC and privilege evidence is conditionally closed and documented separately. No security parity between the backends is claimed.
