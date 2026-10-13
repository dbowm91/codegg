# C023–C026 Windows sandbox corrective assessment

Status: closed evidence and no-admission disposition, 2026-10-13. Baseline `4d61f9622ee037313ce4781eb5b5666142d95a5d`.

## Factual triggers

- **CI:** `.github/workflows/windows-process-launcher.yml` still runs the closed-negative C014 projection/Git test as a mandatory early step. [Run 38336513511](https://github.com/dbowm91/codegg/actions/runs/38336513511) failed C014 and skipped subsequent positive launcher/Job/ACL/status/shell regressions; workflow also repeats several steps. C023 separates negative research and required tests without suppressing genuine red tests.
- **C021 read boundary:** [Run 38334807679](https://github.com/dbowm91/codegg/actions/runs/38334807679) passed on Windows Server 2025 using a newly created *standard* local user plus Job and private synthetic host DACLs. The script uses `CreateProcessWithLogonW` but does **not** create a restricted token. That evidence does not show denying reads of host objects granted to `Users`, `Authenticated Users`, or `Everyone`. C024 tests real access checks and stops negative rather than modifying existing user ACLs.
- **C021 lifecycle:** normal `finally` cleanup and Job child termination passed; orphan/reboot recovery, colliding owners, concurrent workspaces, cancellation races and host loss were not tested. C025 is blocked until strict-positive C024 and C023, then requires a disposable reboot-capable VM.
- **Decision:** C022 [closure](https://github.com/dbowm91/codegg/blob/work/windows-native-sandbox-foundation/plans/closure/windows-native-sandbox/022-status.md) is conditionally closed with *insufficient evidence*. C026 requires independent Windows security review for any positive proposal, or records no-admission immediately if C024 fails.
- **MXC:** C020 selected native ProcessContainer and denied observed host reads, but Python/native helper initialization and inherited-handle qualification failed; bounded negative, not demonstrated MXC escape. Do not launch another automatic MXC corrective loop.

## Corrective line closure — 2026-10-13

- **C023 closed:** Required launcher CI passed at exact implementation SHA `68113300c68961a57faa6902ef6307b5e5e59db9` in run `38359288381`; the full process-launcher job also passed in the manual C014 research run `38359300786`. The opt-in C014 job in `38359300786` reproduced the closed-negative Git `/dev/null` denial and failed as expected, while its separate launcher job passed. The metadata step in follow-up run `38362180227` at SHA `40de0257b1fde353c2b2b3f9c71a51fd7000bb7f` records UBR `33438`; its repeat suite was still running when closure was prepared and is not used for acceptance. Closure: `plans/closure/windows-native-sandbox/023-status.md`.
- **C024 closed negative for the tested candidate:** Exact SHA `645f24a1ff35e7b38db5580f1b09346d95af00b8`, run `38362309506`, Windows Server 2025 Datacenter build `26100`, UBR `33438`. The ordinary disposable-account child read the broad-group-readable synthetic host object and wrote its stage. The restricted-token child did not launch: `CreateProcessAsUserW` returned Win32 2 in the final candidate attempts, the unrestricted `CreateProcessWithTokenW` control timed out, and the restricted `CreateProcessWithTokenW` attempt returned Win32 5. Fixture DACL fingerprints and secret hashes were unchanged; the principal was removed. This is a negative admission result, not a demonstrated restricted-child read denial.
- **C025 closed without execution:** Its required C024 strict-positive gate failed, so no recovery/VM/concurrency work was started. Closure: `plans/closure/windows-native-sandbox/025-status.md`.
- **C026 closed no-admission:** No candidate passed C024, so no production proposal or positive security review is warranted. This is not an independent positive review. Closure: `plans/closure/windows-native-sandbox/026-status.md`.

The C023–C026 corrective line is complete. Windows production `SandboxRequest::Required` remains unavailable; filesystem/network `Enforced` remain unclaimed; C011/M004–M006 and C015–C018 remain blocked. Any new candidate requires a separately registered plan and a working native-token control before it may claim access-denial evidence.

## Security and scheduling rule

C023 and C024 **ready in parallel**. C025 **blocked on C024 strict-positive and C023 strict closure**; stop it if C024 negative. C026 needs C024 disposition; if C024 positive, also C025 and an independent reviewer. Successful C026 permits *only* proposing a later, explicitly approved production integration ADR. No existing workspace ACL lease, network `Enforced`, automatic account provisioning, C011/M004 requalification or C015–C018 unlock.

## Follow-up handoff clarification — 2026-10-13

C023–C026 already exist and are registered, so no parallel C027 is warranted at this stage. The next risk is not just a separately named account, but **read isolation when the original host file is permitted to broad normal groups**. C024 must show that a normal sandbox account *can* read a synthetic broad-group-readable source file, then that a true **read-restricted primary-token child of that same account cannot**, while preserving allowed stage and Git/shell operations. The baseline fixture's ACL and file identity must remain unchanged between tests.

The Windows `CreateRestrictedToken` contract performs two access checks for restricting SIDs except under `WRITE_RESTRICTED`, which restricts only writes. The current C021 `CreateProcessWithLogonW` cannot directly launch a prebuilt restricted token, so the experiment must prove its actual `CreateProcessAsUserW` or qualified equivalent token-launch and privilege boundary. Failed privilege acquisition or a fallback to unrestricted account invalidates the gate. Suspended child/Job lifetime and inherited handles remain mandatory.

This supplements C024/C025/C026 acceptance details, **not** their dependency order: C023/C024 ready in parallel; C025 only after C023 closed and C024 strict positive on broad-readable originals plus an isolated reboot-capable VM; C026 may issue a negative no-admission after C024 fails. Independent review precedes any production integration ADR. No production Windows `Required`, filesystem/network `Enforced` or original source ACL mutation is authorized.

Official reference contracts: https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createrestrictedtoken ; https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessasuserw ; https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects .
