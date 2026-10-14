# Windows Native Sandbox C052 — Two-hop native launcher differential against C044

Status: ready — strictly test-only diagnostic; independent of C049/C050
Class: infrastructure (bounded corrective research)
Baseline: main aa6cbcb1e154a2676325fb654c5b68dc068edfc3 (refresh before implementation).
Source roadmap: plans/subsystems/windows-native-sandbox-roadmap.md (post-C043/C044 successor).
Authority: ADR-0016 and ADR-0029 bounded test-only; historical C028/C032/C044 negative closures. No installation/provisioning approval, no ADR-0030 acceptance.
Dependencies: C044 negative evidence hard (closed); disposable consented Windows runner operational. C042 contract interface only.
Closure target: plans/closure/windows-native-sandbox/052-status.md.

## Objective

Determine **which launch-context difference prevents Codegg's C044 statically linked restricted child from reaching its first instruction**, using Anthropic sandbox-runtime's native Rust srt-win topology and Codex's public Windows launcher as technical comparators. This is not permission to copy any complete third-party sandbox, repeat C044 without a discriminating control, or equate a running process with proved filesystem confidentiality.

## Work packages

A. Freeze GitHub source revision and license for Anthropic's Apache-2.0 srt-win (vendor/srt-win-src/src/{logon,launch,token,winsta,job,runner,self_protect}.rs), Codex's Windows token/launch routines and Codegg's C044 exact SHA/log. Build a line-item contract comparison: first-hop CreateProcessWithLogonW flags/profile, second-hop CreateRestrictedToken/primary duplication/CreateProcessAsUserW, current/session/logon SID membership, disabled admin groups, token IL/default DACL, parent/child desktop and window station, nested Job/breakaway rights, explicit PROC_THREAD_ATTRIBUTE_HANDLE_LIST, environment block/stdio, process mitigation attributes and service vs interactive session. Record each source's tested release and unverified claims. Respect third-party licenses; do not vendor full upstream runtime into Codegg in this milestone.

B. Run **paired experiments** on the same disposable host and same first-instruction marker binary, with an independently executing same-account unrestricted control. Reuse C044 fixture identity and numeric suspend-count checks. Instrument native entry using a raw/no-CRT control where feasible, process/thread start addresses, exit codes, startup module/loader status, desktop access, parent and child Job membership, token group and privilege snapshots; do not call a 30-second STILL_ACTIVE case success. Change exactly one launch-context variable per controlled variant after identifying a falsifiable hypothesis. Host one baseline+one changed variant per test with bounded deadlines, not an open-ended permutations loop.

C. If an upstream-style control reaches entry, isolate the minimal necessary difference and add a test-only fix to Codegg's existing managed-process launcher contract, never a parallel production command runner. Verify ordinary child and restricted child entry, all inherited stdio/safe handle rules, Job ownership, nested descendants and deterministic termination. Refuse startup if broker self-protection or privilege-boundary invariants fail; do not inherit upstream warning-only self-protection behavior blindly.

D. Only **after** first-instruction entry, run same-file normal-account broad-read and restricted-child read probes, allowed stage write and denied external write, synthetic direct/junction/hardlink/ADS and file/pipe/process-token handle tests. Record what is and is not restricted. Test one representative cmd, PowerShell, Git and Python invocation. If any restricted read succeeds, report broad-read (not strict). Persistent identities, provisioning, WFP and original-worktree ACLs remain prohibited and require independent C045/provisioning ADR.

## Acceptance / bounded stop

Within one bounded comparative revision and one bounded correction, either (1) show repeatable restricted child entry and a minimal verified launch fix, or (2) close with an explicit negative/unsupported reason including exact C044 control results. No first-instruction marker -> no filesystem claim, no C045 unlock. Success establishes native launch feasibility only, not read confinement, host-secret protection, offline network, original-host-mutation safety or production admission. Record exact source pins, Windows build/UBR/arch, child/job/token IDs, flags, error/wait state, process exit, cleanup and unrun dimensions. If testing on a shared hosted runner is insufficient, qualify only those dimensions actually observed and request a consented isolated VM; do not grant host-wide rights.

## Expected changes / verification / compatibility

Only test-only native probes, feature-gated launcher correction if proven and focused regression tests. Preserve production Required fail-closed, MSRV 1.89, no new service/accounts stored persistently, no public IPC/config/schema changes. Run cargo fmt --all -- --check; git diff --check; python3 scripts/check_adr_integrity.py; python3 scripts/check_sandbox_contract.py; python3 scripts/check_execution_ownership.py; bash scripts/verify.sh quick; exact-SHA hosted Windows launcher/adversarial tests with child-entry and cleanup proofs. C045, C047 integration and C048 remain separately blocked.
