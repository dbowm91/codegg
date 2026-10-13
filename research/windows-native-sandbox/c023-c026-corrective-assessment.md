# C023–C026 Windows sandbox corrective assessment

Status: research/planning input, 2026-10-13. Baseline `4d61f9622ee037313ce4781eb5b5666142d95a5d`.

## Factual triggers

- **CI:** `.github/workflows/windows-process-launcher.yml` still runs the closed-negative C014 projection/Git test as a mandatory early step. [Run 38336513511](https://github.com/dbowm91/codegg/actions/runs/38336513511) failed C014 and skipped subsequent positive launcher/Job/ACL/status/shell regressions; workflow also repeats several steps. C023 separates negative research and required tests without suppressing genuine red tests.
- **C021 read boundary:** [Run 38334807679](https://github.com/dbowm91/codegg/actions/runs/38334807679) passed on Windows Server 2025 using a newly created *standard* local user plus Job and private synthetic host DACLs. The script uses `CreateProcessWithLogonW` but does **not** create a restricted token. That evidence does not show denying reads of host objects granted to `Users`, `Authenticated Users`, or `Everyone`. C024 tests real access checks and stops negative rather than modifying existing user ACLs.
- **C021 lifecycle:** normal `finally` cleanup and Job child termination passed; orphan/reboot recovery, colliding owners, concurrent workspaces, cancellation races and host loss were not tested. C025 is blocked until strict-positive C024 and C023, then requires a disposable reboot-capable VM.
- **Decision:** C022 [closure](https://github.com/dbowm91/codegg/blob/work/windows-native-sandbox-foundation/plans/closure/windows-native-sandbox/022-status.md) is conditionally closed with *insufficient evidence*. C026 requires independent Windows security review for any positive proposal, or records no-admission immediately if C024 fails.
- **MXC:** C020 selected native ProcessContainer and denied observed host reads, but Python/native helper initialization and inherited-handle qualification failed; bounded negative, not demonstrated MXC escape. Do not launch another automatic MXC corrective loop.

## Security and scheduling rule

C023 and C024 **ready in parallel**. C025 **blocked on C024 strict-positive and C023 strict closure**; stop it if C024 negative. C026 needs C024 disposition; if C024 positive, also C025 and an independent reviewer. Successful C026 permits *only* proposing a later, explicitly approved production integration ADR. No existing workspace ACL lease, network `Enforced`, automatic account provisioning, C011/M004 requalification or C015–C018 unlock.

## Follow-up handoff clarification — 2026-10-13

C023–C026 already exist and are registered, so no parallel C027 is warranted at this stage. The next risk is not just a separately named account, but **read isolation when the original host file is permitted to broad normal groups**. C024 must show that a normal sandbox account *can* read a synthetic broad-group-readable source file, then that a true **read-restricted primary-token child of that same account cannot**, while preserving allowed stage and Git/shell operations. The baseline fixture's ACL and file identity must remain unchanged between tests.

The Windows `CreateRestrictedToken` contract performs two access checks for restricting SIDs except under `WRITE_RESTRICTED`, which restricts only writes. The current C021 `CreateProcessWithLogonW` cannot directly launch a prebuilt restricted token, so the experiment must prove its actual `CreateProcessAsUserW` or qualified equivalent token-launch and privilege boundary. Failed privilege acquisition or a fallback to unrestricted account invalidates the gate. Suspended child/Job lifetime and inherited handles remain mandatory.

This supplements C024/C025/C026 acceptance details, **not** their dependency order: C023/C024 ready in parallel; C025 only after C023 closed and C024 strict positive on broad-readable originals plus an isolated reboot-capable VM; C026 may issue a negative no-admission after C024 fails. Independent review precedes any production integration ADR. No production Windows `Required`, filesystem/network `Enforced` or original source ACL mutation is authorized.

Official reference contracts: https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createrestrictedtoken ; https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessasuserw ; https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects .
