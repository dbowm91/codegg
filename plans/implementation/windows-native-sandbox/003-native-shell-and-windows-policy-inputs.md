# Windows Native Sandbox Milestone 003 — Native Shell and Windows Policy Inputs

Status: blocked (M002 closure required)

Repository baseline: `8e9d8b01e5c229715c8e4dea929e050b391e252c`

Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#7-milestones`

Long-term requirements:
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Applicable ADRs: `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`; `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`

Primary class: infrastructure

## 1. Objective

Make native Windows commands and sandbox policy inputs workable without `sh -c`, Unix directory assumptions, or permissive filesystem path fallbacks. This milestone defines compatible inputs but does not claim enforcement.

## 2. Why this milestone is ready / dependency gate

Hard dependency: M002 closed. M001 contract and process ownership remain stable. Native Windows live runner exists to test shell/argument and filesystem path behavior.

## 3. Current implementation evidence

`src/tool/bash/process.rs` constructs `sh -c` for raw shell and in sandbox launch specs; `src/security/sandbox/policy.rs` hardcodes Unix `/usr`, `/dev` and `HOME`, missing Windows `USERPROFILE`, SystemRoot, Program Files, TEMP, Cargo/rustup and VS installations; `SandboxConfig::launch_spec` currently resolves paths with canonicalization and retains missing-deny best effort.

## 4. Invariants that must not regress

- Explicit executable/argv construction never reparses structured arguments as shell code.
- Tool category and `ApprovalRouter` cannot be bypassed via PowerShell/cmd aliases.
- Sensitive Codegg credentials and daemon endpoints remain outside allowed runtime data.
- No supposed 'canonical path' assertion replaces NTFS ACL enforcement.

## 5. Scope

### In scope

- Add platform-aware shell selection and structured execution for native PowerShell/cmd with Git Bash/WSL as explicit optional executors.
- Enumerate Windows runtime minimum read/toolchain paths, writable caches and isolated per-run TEMP in policy compilation, without granting blanket profile writes.
- Native path identity, UNC/reparse/junction handling, and deny-policy diagnostics before launching.

### Explicitly out of scope

- File-enforcement backend/ACL mutation, firewall, default network blocking, broad terminal PTY changes or auto-installing shells.

## 6. Required production changes

### Runtime/shell
Route executable invocation and shell interpretation separately; `pwsh`, Windows PowerShell and `cmd.exe` use distinct quoting and argument rules; do not build command strings by concatenation. Keep `sh` default only on Unix. `BashTool` name may remain compatibility API, but operational docs must describe native Windows shell selection accurately.

### Policy
Windows-specific toolchain read roots, per-run scratch/temporary and profile-specific write roots; standard OS DLL dependencies; provenance. Do not blanket-allow `%USERPROFILE%`, `%LOCALAPPDATA%` or `C:\` to satisfy a tool. Identify deny-sensitive Codegg config/token/SSH/AWS locations using Windows known-folder APIs and real user profile paths.

### Path security
Validate Windows drive roots, case and separators, `\?` forms, UNC/device paths, junctions/reparse points, ADS and hardlink alias hazards. Produce safely normalized handle-bound roots for M004 and explicitly reject unknown/missing denied-path state where read guarantees depend on them.

### Storage/protocol/docs
No migration. Document shell selection and unsupported runtime incompatibilities.

## 7. Ordered work packages

### WP-A — Native argv/shell semantics
Create opt-in shell kind and distinguish direct argv; unit/integration fixtures for spaces, Unicode, quoting, metacharacters, encoded PowerShell commands and nested invocation.

### WP-B — Host runtime allowance compiler
Resolve Windows known folders, fixed system DLL/exe roots, toolchain and cache roots; isolate writable temp per run and do not modify host root ACLs. Probe Cargo, Rust, Git and Python smoke usage.

### WP-C — Windows path/secret root validation
Negative fixtures for junction, symlink, UNC, volume/device prefix, ADS, case aliases, path traversal, and protected credential roots. Record limits of pre-launch validation.

### WP-D — Route and permission regression
Ensure named shell route still reaches `ManagedProcessService` and full agent tool authorization, including child worktree root ceilings.

## 8. Failure, cancellation, restart, and contention semantics

Invalid PATH entries, ambiguous shell discovery or unresolved roots fail predictably. No untrusted shell is run for policy probes. Per-run scratch is deleted after Job completion, including cancellation where possible; crash recovery collects stale owned scratch without touching user data. Concurrent workspace roots remain separate.

## 9. Compatibility and migration

`FullHost` retains explicit unconstrained shell behavior. Linux/macOS `sh` path unchanged. Config remains readable; optional Windows shell choice defaults to a documented native shell only where available. No global registry/ACL changes.

## 10. Required tests

- Unit: shell quoting, direct argv, PATH spoofing, Unicode and case-insensitive path equivalence.
- Native Windows integration: cmd and PowerShell smoke + representative Rust/Git/Python invocation.
- Security negative: malicious junction/reparse changes, credential path deny, UNC/device prefixes, alternate data stream, two worktrees.
- Regression: unmanaged shell bypass not added, authorization receipts stable.

## 11. Required verification commands

`cargo test -p codegg --lib security::sandbox`
`cargo test -p codegg --lib tool::bash`
`python3 scripts/check_sandbox_contract.py`
`python3 scripts/check_execution_ownership.py`
`cargo fmt --all -- --check`
On Windows: `cargo test -p codegg --test windows_shell_policy -- --nocapture` (new target)
`bash scripts/verify.sh quick` on compatible Unix host.

## 12. Documentation updates

`architecture/tool.md`, `architecture/security.md`, `architecture/testing.md` and operator shell selection docs.

## 13. Acceptance criteria

- Managed raw shell is functional with explicit native shell semantics, no Unix `sh` dependency.
- Produced policy root sets enumerate exact Windows runtime/scratch/credential exceptions without blanket HOME/drive access.
- Windows path adversarial fixtures demonstrate correct rejection or clearly delimited limits. Not `Enforced` yet.

## 14. Stop conditions

Stop if a toolchain needs unconstrained user profile writes by default, if alias-based path checks claim security without ACL protection, or if PowerShell execution requires an unreviewed permission bypass.

## 15. Closure evidence required

Exact Windows versions/shells run, output and exit test evidence, policy path matrix, hostile path test outputs, Unix baseline checks, M004 unblock decision.

## 16. Handoff notes

Plan for different Windows toolchain placements. Do not make success of a ‘powershell -c hello’ smoke test equivalent to sandbox readiness.
