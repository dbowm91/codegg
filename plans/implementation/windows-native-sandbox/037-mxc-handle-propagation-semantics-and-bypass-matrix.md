# Windows Native Sandbox C037 — MXC Handle-propagation Semantics and Bypass Matrix

Status: **closed — bounded negative/incomplete; no production admission**
Primary class: security invariant corrective
Baseline: `work/windows-native-sandbox-foundation` at `e8ebeb1df8a9b97907ffebfbaf47d05c13f16f77` after the C036 toolchain correction.
Source: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0024-capture-and-handle-semantics-follow-up`.
Authority: ADR-0026 only; historical C033/C035 closed-negative and C034 no-admission unchanged; ADR-0023 privileged alternatives still rejected.
Canonical: `plans/000-long-term-specification.md` §§4,17,27,29; `plans/001-terminology-and-domain-model.md` §8; `plans/003-planning-process.md`.
Prior: `plans/closure/windows-native-sandbox/035-status.md` and `research/windows-native-sandbox/mxc-c035-harness/native_probe.c`.
Dependency: C035 closed and ADR-0026 accepted; *independent* of C036. Operational Windows 11 25H2 ARM64 or explicitly equivalent eligible serviced host.

## Problem statement and strict threat model

C035's C helper proved an ordinary child can read a synthetic secret via a path and a correctly inherited file handle when parent uses Win32 `CreateProcessW(bInheritHandles=TRUE)` and `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`. MXC's SDK-launched child received only the **numeric value** of the parent's handle; its `GetHandleInformation/GetFileType/ReadFile` route yielded `handle_present=false` and caught `STATUS_INVALID_HANDLE`, while a separate open of the original host path was denied. This proves a **running child did not successfully use that value**; it does not prove a valid handle was transferred then denied, or that deliberate `DuplicateHandle` / intermediary channels are blocked.

A handle value has meaning in an individual process's handle table. The plan explicitly separates *failure to transfer ambient handles*, *policy denial of a valid transferred handle*, *unsupported intentional transfer by the SDK*, *malformed probe*, and *actual read leak*. Do not force or fabricate a "transferred then denied" outcome if MXC's design is to prevent transfers entirely.

## Implementation scope and security invariants

Use only isolated `research/windows-native-sandbox/mxc-c035-harness/` and/or `research/windows-native-sandbox/mxc-c037-harness/`, a small native Win32 helper and optional `.github/workflows/windows-mxc-c037-handle-boundary.yml`. Preserve MXC v1.0.0 pin and isolated Rust toolchain unless a separately documented comparative API test is necessary. No changes to production launcher/backend, host ACLs, security profile, RunStore, tool approval, service/privileged broker, signed helper packaging or guest. Never use real secrets. Do not grant broad read of host roots or preserve host inherited credentials to obtain a positive fixture.

## Ordered work packages

**A — Establish actual handle-transfer contract, before tests.** Inspect pinned MXC v1.0.0 Rust SDK and ProcessContainer Win32 launcher: `bInheritHandles`, standard-handle setup, `STARTUPINFOEX`/`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`, inherited/duplicate handles, blocked handle classes and whether API permits explicit allowlisted host handles. Distinguish SDK wrapper from OS backend and supported tier. Trace which parent PID owns source file and which process invokes `CreateProcessW`; a handle in a grandparent **cannot** be made available merely by setting the inheritable flag on its numeric value. Freeze a matrix of launch edges:
- ordinary direct `CreateProcessW` inherited-file **positive control** (already in C035, repeat exact fixture),
- ordinary `CreateProcessW` with inheritance disabled **negative control**,
- MXC default child, no explicit handle transfer, with child-entry proof and file-table attempt,
- supported explicit transfer path **only if** SDK/backend exposes a safely testable contract,
- standard I/O and named-pipe channels that are intentionally allowed for tool functionality; distinguish permitted streaming output from secret-file read.
If a supported explicit-transferred-handle path does not exist, record *unavailable* and assess "ambient handles not propagated" only.

**B — Build authoritative synthetic access oracle.** Create a disposable synthetic broadly group-readable file with known nonce and SHA, open it for read and verify `GetHandleInformation(HANDLE_FLAG_INHERIT)`. Use a separate synthetic private-DACL control only as a diagnostic; main positive is same broad-readable object. In each child, prove actual entry, PID/nonce, token user/integrity/restricted flags, expected handle **presence type** (`GetHandleInformation`, `GetFileType`, `NtQueryInformationProcess/ProcessHandleInformation` if permitted), and real `ReadFile` result. Avoid converting structured SEH exception `0xC0000008` to a successful "policy deny". Record both a *no-access* assertion and the *mechanism* that led to no access. A copied numeric value, stale handle or missing child entry is never evidence of a valid transfer.

**C — Prove or bound process-tree escape paths.** Run a minimal stage child spawning a second child with supported ordinary flags; probe unexpected inherited file/pipe/stdin/stdout/standard-error, duplicated handles when APIs and permissions permit, and synthetic named-pipe endpoints (C033 baseline denial). Ensure every child, surrogate process and Job cleanup is observed with timeouts/cancellation after one deliberately interrupted synthetic run. Do not create a privileged host handle transfer broker to manufacture a successful adversarial fixture. Observe whether deliberate transfer is rejected by an API before child entry, stripped at launch, or blocked at actual read. Do not claim `network=Enforced`.

**D — Compare exact outcomes and stop.** For ordinary controls require positive known-file read under explicit handle inheritance and negative under disabled inheritance. For MXC require real child entry, same-file denied direct pathname, effective BaseContainer support probe, stage write, safe stdout/stderr functionality, no original host DACL/content change, and no unexpected synthetic handle or IPC exposure. If supported intentional transfer exists, the recipient **must not** read forbidden bytes. If unsupported, classify explicit-transfer dimension `unsupported` rather than "denied" and flag it for C038's narrower no-ambient-handles disposition.

**E — Evidence/closure.** Emit versioned `C037_REPORT`: { `sdk_sha`, `host_tier`, `control_inherit_on_read`, `control_inherit_off_read`, `mxc_ambient_transfer_observed`, `mxc_handle_attempted`, `mxc_probe_result`, `explicit_transfer_supported`, `explicit_transfer_observed`, `path_read_denied`, `stdout_stderr_ok`, `descendant_cleanup`, `fixture_integrity`, `qualified_test_only`, `unavailable_dimensions` }. Do not emit host credential paths in CI summary. Create `plans/closure/windows-native-sandbox/037-status.md` and update roadmap/registry. No historical closure edits.

## Acceptance and security stop

**Bounded strict-positive for the narrow no-ambient-handle claim:** reproducible controlled inheritance-on positive and off negative; MXC child PID/token/entry actual; parent/launcher handle ownership understood, no ambient synthetic handle reaches child or descendants, correct direct-path denial and stage operations, functional permitted stdout/stderr, and zero leaked sensitive file bytes through any observed channel; synthetic metadata stable. If SDK supports explicit handles, qualify an actual attempted transfer separately; if unsupported, the full inherited/duplicate-handle confidentiality claim remains unqualified even with an ambient-negative test. C038 must apply this limit.

Security-negative if any MXC child obtains synthetic prohibited bytes, a handle can be injected by an allowed untrusted path, the test silently turns off security, Job descendants escape, or forbidden host ACLs change. Bounded negative/blocked if the launcher cannot be instrumented safely, child does not enter, or required host is unavailable; no broader grant or indefinite retry.

Focused checks: compile ARM64 helper with pinned MSVC; `cargo fmt --manifest-path research/windows-native-sandbox/mxc-c035-harness/Cargo.toml -- --check` and `cargo test --locked --manifest-path research/windows-native-sandbox/mxc-c035-harness/Cargo.toml` (or C037 manifest), opt-in exact-SHA Windows 11 workflow with genuine native child; `python3 scripts/check_sandbox_contract.py`; `python3 scripts/check_execution_ownership.py`; `bash scripts/verify.sh quick`; `git diff --check`. Record skipped tests accurately.

No production runtime, schema, migration, installer, ACL, firewall or backend status change. Windows `SandboxRequest::Required` remains unavailable/fail-closed. C038 alone compares the C036/C037 outcomes and could recommend a **separate future** production security decision, never admission by this plan.
