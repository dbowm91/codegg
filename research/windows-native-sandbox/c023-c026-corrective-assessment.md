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
