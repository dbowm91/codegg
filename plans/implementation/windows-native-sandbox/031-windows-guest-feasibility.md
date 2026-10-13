# Windows Native Sandbox C031 — Native Windows Guest Isolation Feasibility

Status: blocked (terminal — ADR-0021 rejected; guest route unselected)
Primary class: security/infrastructure
Repository baseline: `7a27bc7e6566616fc1b2b8a69637f12050d42843` on `work/windows-native-sandbox-foundation` (2026-10-13).
Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0021-launch-authority-decision-gate`.
Architecture decision: `plans/adrs/ADR-0021-windows-privileged-launch-vs-guest-isolation.md` (rejected; no admission); research: `research/windows-native-sandbox/adr-0021-authority-options.md`.
Canonical references: `plans/000-long-term-specification.md` sections 4, 17, 27, 29; `plans/001-terminology-and-domain-model.md` section 8; `plans/003-planning-process.md`; ADR-0004, ADR-0016, ADR-0020.
Immutable evidence: C024, C026 and C027 status records.

## 1. Objective

Evaluate Windows Sandbox or Hyper-V guest executing **native Windows** tools with only approved copied inputs and reviewed outputs, without host credential/workspace/IPC exposure.

## 2. Readiness and dependencies

C029 rejected route selection under ADR-0021. This plan is terminally blocked and is not eligible to run. Reconsideration requires a new explicit architecture decision, fresh qualified evidence, and a supported virtualization-capable operator host with an isolated disposable VM; hosted Windows GitHub CI eligibility cannot be assumed.

## 3. Current repo evidence

Windows Sandbox requires supported non-Home editions, has one instance and defaults clipboard/network enabled; host folders may leak data. WSL2 is Linux tooling, not native Windows parity.

## 4. Security invariants

No mapping original host worktree/secrets, no default clipboard/device/network bridge, no guest direct host write, no implied network `Enforced`. Explicit input manifest and output approval; preserve `ManagedProcessService` authority later.

## 5. In-scope / out-of-scope

In: test-only supported guest launch, native Git/shell, synthetic data transfer, adversarial host boundary and cleanup/cost matrix. Out: product backend, installer, persistent VM inventory, WSL native claim.

## 6. Required changes

Develop test-only Windows Sandbox `.wsb` or Hyper-V guest configuration with bridges off, signed/verified guest image, bounded manifest file injection, untrusted result archive validation. Do not expose daemon pipes or host original roots.

## 7. Ordered work packages

WP-A: OS/edition/virtualization, guest count, network/clipboard/share config. WP-B: execute cmd/PowerShell/Git with copied synthetic project, record optional Python/Cargo. WP-C: host secret/IPC/device/UNC, malicious output/ADS/reparse/zip bomb probes. WP-D: boot latency, RAM/disk, contention, cancel/restart, guest cleanup and edition matrix.

## 8. Failure, cancellation, restart and contention

Cancel/timeout kills guest; restart finds only owned guest; stale or foreign guest never deleted by name prefix. Multi-worktree contention measured, not falsely claimed supported for Windows Sandbox.

## 9. Compatibility and migration

No product schema/installer change; Windows Home unsupported for Sandbox; separate Hyper-V guest licensing and image chain; WSL2 explicitly non-native.

## 10. Required tests

Actual guest-native Git/shell success, host denied-resource/IPC probes, network/clipboard disabled in observed config, output traversal/alias/replay rejection, cleanup and instance count.

## 11. Verification commands

New `pwsh -NoProfile -File research/windows-native-sandbox/run-windows-guest-feasibility.ps1` and optional `cargo test --locked -p codegg --test windows_guest_exchange_contract -- --nocapture --test-threads=1`; cross-host `git diff --check`, `python3 scripts/check_sandbox_contract.py`, `bash scripts/verify.sh quick`.

## 12. Documentation updates

Guest build/edition/licensing/virtualization, config and host bridge threat model, output manifest, performance/instance and C031 closure.

## 13. Acceptance criteria

Native guest runs useful tools, never reads original host secrets or daemon pipe, has safe input/output and measured cleanup/resource bounds. No production claim.

## 14. Stop conditions

Stop without eligible host, when share/clipboard/network cannot be contained, unexpected host read/write, unsupported concurrency or unmanageable guest toolchain cost; no broker fallback.

## 15. Closure evidence

OS/image/SHA, observed denied host resources, Git/shell results, guest bridges, output attacks, latency/resources, cleanup and risk review.

## 16. Handoff

Positive C031 permits a new guest-backend architecture plan, not closing native-process M004 or enabling `Required`.

Write `plans/closure/windows-native-sandbox/031-status.md` with exact native evidence or a blocked/negative disposition; never rewrite historical closures.
