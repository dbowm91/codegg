# ADR-0026 — Bounded MXC Denial-capture and Handle-propagation Semantics Review

Status: **accepted solely for bounded test-only C036/C037 research and C038 comparative closure; no production backend selection**
Date: 2026-10-13
Branch baseline: `work/windows-native-sandbox-foundation`, commit `84faa0bd585e7161361bdb255f29e70cdf7ab619` (refresh before implementation).
Long-term authority: `plans/000-long-term-specification.md` §§4,17,27,29; `plans/001-terminology-and-domain-model.md` §8; `plans/003-planning-process.md`; ADR-0004/0016/0019–0023.

## Decision background and why further work is justified

The earlier C033 and C035 closures remain **negative/no-admission**. On Windows 11 25H2 ARM64, C035 run `38417084880` (exact SHA `3fbe620134ee3fff31d5c6f195a8573ab6941b2f`) proved actual minimal native child entry under MXC v1.0.0 BaseContainer, a normal child's positive read of the synthetic broadly readable host secret by pathname *and explicitly inherited file handle*, an MXC child's `ERROR_ACCESS_DENIED` host-path open, child-created stage proof, and low-integrity token identity. The MXC child received only a numeric handle value, **not an independently verified handle-transfer contract**, and reported absent handle / caught `STATUS_INVALID_HANDLE (0xC0000008)`. Its per-request denial document was absent. C035 itself emitted `qualified=false`. C033's independent baseline captured 21 block-mode denials under that run, so MXC capture is not categorically broken.

There are **two narrow, falsifiable explanations** worth testing before another architecture change:

1. **Capture-path mismatch:** in C035, `src/main.rs` constructs `capture/.../denials.json` then calls `fs::read(&capture_path)` on the literal filename. MXC's documented capture contract may insert a unique per-invocation stem and expose the actual path through return output metadata. This is strongly suggested by MXC upstream documentation, but the source and exact output-metadata API of pinned `mxc-sdk = "=1.0.0"` must be verified first. Missing a literal file does **not** establish ETW failure or capture unavailability.
2. **Non-transferred handle:** Windows handle numbers are *process-relative*. C035's unsandboxed positive control uses `CreateProcessW(bInheritHandles=TRUE)` and `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`, whereas the MXC child is spawned through `mxc_sdk::v1::run` without a corresponding explicit inherited-handle configuration. Passing `handle_value` as a command-line number does not grant access. An invalid value in the MXC child may mean **no ambient transfer**, not a denial of an already transferred handle. That is a meaningful, narrower isolation property if independently proved, but must not be presented as proof of rejection of an explicitly duplicated/injected handle.

The C035 closure says ADR-0025's one corrective authorization was exhausted. This new ADR authorizes only this bounded follow-up, and leaves ADR-0023's broker/guest rejection, C034 no-admission and C035 negative historical closure unchanged.

## Decision and dependency graph

- **C036 — ready, test-only:** inspect exactly pinned MXC v1.0.0 capture output contract, fix actual output path/metadata consumption, attest invocation provenance and negative-control classification. No vendor version upgrade without an explicit versioned test variant. A documented unsupported metadata path closes blocked, rather than guessing.
- **C037 — ready, test-only:** establish the actual MXC launcher's handle inheritance policy using proven Win32 controls, real child handle-table/standard-handle probes and a capped ambient versus explicit-transfer matrix. Separate "not transferred" from "transferred but access denied" and "unexpectedly leaked".
- **C038 — blocked until both dispositions:** decide whether C036 and C037 yield a strictly positive *bounded* candidate worth later independent security review, or whether MXC remains negative/blocked and this research line stops. Do not reopen production C011/M004–M006 based on C036/C037.

C036 and C037 have no hard dependencies on each other, each consumes C035. C038 has hard dependencies on exact-SHA C036 and C037 closures, **including** negative or operationally blocked conclusions. All three are research/implementation plans, not release milestones.

## Invariants and explicit non-goals

- Windows production constrained `SandboxRequest::Required` remains **unavailable and fail closed**. No filesystem or network `Enforced`, FullHost downgrade, backend registration, privileged account/service/guest, stage-to-host output promotion, installer, original host ACL/SACL/MIC mutation, or broad user profile/toolchain read grant follows.
- Only disposable synthetic secrets, same-file broad-read positive controls, and a serviced eligible Windows 11 ARM64 test host. The initially readable synthetic host file, stage and output artifacts must preserve stable file identities and descriptor/hash integrity; no real credentials or original workspace/host file mutations.
- Separate native API creation/resume, actual child entry, token, namespace/handle visibility, read result, captured audit event, Job containment and teardown. Unsupported/no transfer ≠ access denied; timeout ≠ read denied; successful workflow/report ≠ strict-positive candidate.
- Do not use MXC permissive `CaptureDenialsMode::Allow` for any security admission; mode `Block` must be demonstrated. Retained ETL may contain sensitive paths and is permissible **only** for disposable synthetic fixtures, with bounded retention and explicit destruction.
- The inherited-handle question is defined by which handles the launcher passes. An MXC API that *never* accepts arbitrary host handles may qualify the narrower "no ambient host handles" property only after test and review; it does not automatically qualify behavior for explicit handle passing, `DuplicateHandle`, broker-mediated pipes or inherited stdout/stderr.
- Tests must tolerate no available host by reporting blocked, not a positive result. Do not conflate build 26200/UBR 9457 ARM64 with all Windows editions/builds or alternative tiers. Runtime SDK and OS tier remain pinned and disclosed.
- Shell command policy test failures are a separate existing CI issue, not a reason to weaken Windows security tests; an independent shell CI corrective is registered alongside this work.

## Stop rules and future promotion boundary

Bound the first two experiments: once each has a trustworthy uncontained positive/control, the enforcing child observation, a diagnosed outcome and at most one justified narrowly scoped correction, close it. Do **not** open another series of regex, ACL, platform flag, and timeout variations. If the v1 API cannot express an explicit foreign-handle test, **record that limitation** instead of equating a missing handle with denial; an architectural design supporting safe explicit transfer would require a new ADR.

C038 may recommend **future** production architecture research only with independent review conditions and remaining lifecycle/cancellation/concurrency/network/host-matrix gaps enumerated. C038 itself cannot authorize a production runtime, and C030/C031 remain terminally blocked.

## Research references

- C035: `plans/closure/windows-native-sandbox/035-status.md`, `research/windows-native-sandbox/mxc-c035-harness/{src/main.rs,native_probe.c}`; run https://github.com/dbowm91/codegg/actions/runs/38417084880.
- C033: `plans/closure/windows-native-sandbox/033-status.md`.
- MXC current upstream capture contract (must recheck pinned 1.0.0 source): https://github.com/microsoft/mxc/blob/main/docs/logging-access-denied.md and https://github.com/microsoft/mxc/blob/main/src/core/mxc-sdk/README.md .
- Win32 handle inheritance semantics: https://learn.microsoft.com/en-us/windows/win32/sysinfo/handle-inheritance .
