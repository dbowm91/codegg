# Windows Native Sandbox C030 — Consented Privileged Broker Security Feasibility

Status: blocked (terminal — ADR-0021 rejected; broker route unselected)
Primary class: security/infrastructure
Repository baseline: `7a27bc7e6566616fc1b2b8a69637f12050d42843` on `work/windows-native-sandbox-foundation` (2026-10-13).
Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0021-launch-authority-decision-gate`.
Architecture decision: `plans/adrs/ADR-0021-windows-privileged-launch-vs-guest-isolation.md` (rejected; no admission); research: `research/windows-native-sandbox/adr-0021-authority-options.md`.
Canonical references: `plans/000-long-term-specification.md` sections 4, 17, 27, 29; `plans/001-terminology-and-domain-model.md` section 8; `plans/003-planning-process.md`; ADR-0004, ADR-0016, ADR-0020.
Immutable evidence: C024, C026 and C027 status records.

## 1. Objective

Test whether a minimal privilege-separated Windows broker can launch a **lower-privilege, true restricting-SID child** and deny broad-group-readable host data without making the broker an elevated agent shell.

## 2. Readiness and dependencies

C029 rejected route selection under ADR-0021. This plan is terminally blocked and is not eligible to run. Reconsideration requires a new explicit architecture decision, fresh qualified evidence, and a disposable administrator-consented VM. Hosted runner alone is inadequate.

## 3. Current repo evidence

Win32 `CreateProcessAsUserW` privilege requirement does not establish read isolation. Default named pipe DACL is broad and failed `ImpersonateNamedPipeClient` may leave privileged server context. C027 never launched restricted child.

## 4. Security invariants

Trusted local SCM service only, no automatic install or agent/TUI direct endpoint; fixed launcher digest and versioned authorized manifest, account SID/workspace binding, nonce, verified client token/logon SID and checked `RevertToSelf`. Deny arbitrary EXE/command, raw handle, token or ACL instructions. Unprivileged child SID/Job checked before resume.

## 5. In-scope / out-of-scope

Disposable VM research service and synthetic fixtures only; excludes product daemon integration, real user ACLs, persistent service deployment, firewall or general privileged command execution.

## 6. Required changes

Build test-only narrow broker executable/pipe with explicit restrictive pipe descriptor, client token validation, immutable fixed target and stage file identity; no daemon production dispatch. Document single canonical managed-process owner for later integration.

## 7. Ordered work packages

WP-A: privileged service setup under operator consent, SCM/token/pipe DACL inspection. WP-B: authenticate actual client, reject impersonation failure/replay/forged SID/manifest and arbitrary commands. WP-C: restricted-token child, Job and actual unchanged broad-file denied read, allowed staged shell/Git. WP-D: crash, cancel, cleanup, independent attacker pipe test, remove test service/account.

## 8. Failure, cancellation, restart and contention

On crash or IPC drop, terminate or reconcile Job by protected owner lease; no orphan privileged child. Unique account and nonce per run, quarantine foreign SID/service instead of deleting. No silent self-install.

## 9. Compatibility and migration

No product installer/settings/schema/protocol. Service runs only isolated VM; host OS support and future credential lifecycle explicitly separate.

## 10. Required tests

Attacker local/remote pipe connection, impersonation failure, replay, wrong owner SID, arbitrary command attempts, inherited handles, IPC, hardlinks/ADS, direct broad-host read, Job descendants, force-stop and cleanup.

## 11. Verification commands

New VM harness `pwsh -NoProfile -File research/windows-native-sandbox/run-consented-broker-feasibility.ps1` plus `cargo test --locked -p codegg --test windows_privileged_broker_contract -- --nocapture --test-threads=1` if Rust fixture, and C019 native oracle. Cross-host `git diff --check`, `cargo fmt --all -- --check`, `bash scripts/verify.sh quick`.

## 12. Documentation updates

`research/windows-native-sandbox/c030-broker-threat-model.md`, SCM/pipe installation and teardown, exact C030 closure and security reviewer notes.

## 13. Acceptance criteria

Actual restricted child denied unchanged broad-group readable file while shell/Git and stage work; authorized pipe only, no arbitrary elevated command or persistent resources; all negative tests green.

## 14. Stop conditions

Stop on client impersonation error continuing privileged, access to denied host file, agent-controlled elevated launch, original ACL mutation, absent consented VM, broken cleanup.

## 15. Closure evidence

VM SHA, service privilege/token audit, pipe DACL/auth results, forged calls, restricting SID/Job, actual reads, cleanup, reviewer risk assessment.

## 16. Handoff

Even positive C030 authorizes only a later production broker ADR/implementation plan and independent C011/M004 qualification.

Write `plans/closure/windows-native-sandbox/030-status.md` with exact native evidence or a blocked/negative disposition; never rewrite historical closures.
