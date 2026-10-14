# Windows Native Sandbox C053 — MXC API delta and strict re-entry gate

Status: ready for source/ABI comparison only; native retry blocked unless documented capability change passes gate
Class: infrastructure (bounded feasibility disposition)
Baseline: main aa6cbcb1e154a2676325fb654c5b68dc068edfc3 (refresh exact head).
Source roadmap: plans/subsystems/windows-native-sandbox-roadmap.md (post-C043/C044 successor).
Authority: ADR-0029 bounded test-only; C033/C035/C036/C037/C038/C043 closures immutable; no new production ADR.
Dependencies: C043 and C038 negative closures hard (closed); eligible Windows 11 BaseContainer host operational ONLY for newly qualified API candidate.
Closure target: plans/closure/windows-native-sandbox/053-status.md.

## Objective

Establish whether an *actual new MXC SDK/ABI/runtime capability* changes either of Codegg's prior stop conditions: inability of pinned mxc-sdk 1.0.0 ProcessContainerConfig to explicitly prohibit DACL-mutation fallback, and incomplete helper-entry/handle/descendant/teardown proof. Avoid recreating C033–C043 on unchanged SDK or host evidence.

## Work packages

A. Pin and compare the previously tested 1.0.0 Rust SDK and hosted Win11 build against the current tagged Microsoft MXC release, API sources and schema/CLI surface. Identify exactly where fallback.allowDaclMutation (or equivalent) is exposed and how it is honored by the Rust adapter in practice; a feature shown only in documentation or a distinct CLI is not proof of an SDK control. Record effective BaseContainer tier, host edition/UBR/arch and required PSEC functionality. Check MSRV and third-party license; no production Rust workspace toolchain uplift for this spike.

B. Write an explicit **delta gate**: native re-probe is permitted only if the pinned candidate (1) programmatically and verifiably enforces no original DACL/SACL/MIC/owner change and refuses silent fallback, AND (2) supplies a plausible documented/testable way to resolve native helper startup and required handle-transfer/child-lifecycle gaps. A mere version bump, updated docs, or a successful path-read denial is insufficient. If either criterion fails, close negative on source analysis with no Windows rerun.

C. If both gates pass, execute one Windows 11 disposable-fixture native test suite: compare effective tier, real helper entry, same-file normal-vs-contained read, direct/junction/hardlink/ADS/device/UNC if applicable, writable stage and denied external mutation, numeric as well as actual transferred handles, child-correlated events only when claimed, Job descendant, timeout/cancellation and independent OS process-exit proof. Compare original descriptor/content snapshots before/after; no real profile or repository mutations. Run Git, cmd/PowerShell, Python, offline Cargo positive controls. Distinguish not-run, unsupported, setup failed, denied, completed.

D. Produce a formal backend re-entry/no-reentry recommendation with exact compatible host matrix and SDK/config contract. Positive feasibility remains test-only; any production candidate requires a new accepted ADR, C047 process surface control, C048 independent review and a separate launch adapter plan.

## Acceptance / stop

Any silent DACL fallback, missing effective-tier proof, unentered child, unsafe inherited handle, uncontrolled descendant, unknown cleanup or host-object mutation is a hard negative. Do not re-run old C043 under identical conditions or treat missing denial telemetry as a secret leak. Unchanged SDK/ABI is a valid negative documentary closure, not a reason to open more probes. On a new native test run, execute cargo fmt/diff/ADR/sandbox/ownership/verify checks and exact-SHA Windows evidence; on documentary-only closure, mark native testing not run. No production registration, schema change, installer, network enforcement or historical status modification.
