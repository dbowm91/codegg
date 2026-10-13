# Windows Native Sandbox C033 — MXC Denial Telemetry and Native Toolchain Requalification

Status: **implemented (negative bounded candidate feasibility)**
Primary class: infrastructure/security
Baseline: `work/windows-native-sandbox-foundation` at `920aa08ea484a9230e1508e9bbaad58fbaefd498` (refreshed after rebase).
Roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0022-bounded-successor-feasibility`.
Authority: ADR-0024 bounded feasibility, ADR-0020 comparison; historical C020 closed-negative status is immutable.
Canonical: `plans/000-long-term-specification.md` §§4,17,27,29; `plans/001-terminology-and-domain-model.md` §8; `plans/003-planning-process.md`.
Hard prior: C019 common actual-child oracle and C020 negative/partial trace. Parallel to C032; no dependence on C032 success.

## 1. Objective and stop boundary

Determine whether a pinned, explicitly supported MXC Windows ProcessContainer tier can **both** enforce real direct/alias/IPC/handle restrictions and initialize the minimum native Windows developer toolchain (PowerShell/cmd + Git; separately Python script, native helper, Cargo) without modifying original host tree ACLs. Diagnose previous C020 Python/native startup failures with *deny-and-record* telemetry when the selected SDK/runtime/OS tier supports it. If not supported, report capability unavailable and stop rather than substituting permissive or AppContainer/DACL fallbacks.

No production `ManagedProcessService` integration, backend enumeration, account/install step, firewall, declared OS support, tool execution authority or final security admission.

## 2. Current evidence

C020 run `38335783458` selected native ProcessContainer/BaseContainer on eligible Windows 11 Enterprise 25H2 ARM64, observed direct/alias denials and descendant cleanup, but Python script and native helper failed initialization and no inherited-handle probe result was obtained. SDK `v1.0.0` was pinned in `research/windows-native-sandbox/mxc-v1-harness/`. A subsequent MXC upstream schema/doc revision describes denial capture, audit modes and OS-dependent tiers; **do not assume these capabilities exist in the previously pinned binary**. The C020 result stays negative for minimum compatibility and incomplete for handles; this plan is a fresh candidate with distinct closure.

## 3. Threat, contracts and forbidden fallbacks

- Use the C019 actual running-child test schema and C032's **same-file broad-group read baseline** distinction, not merely "cannot reach path" after startup failure.
- Never grant original host workspace/profile/credentials/DACL/SACL/MIC/reparse objects to make toolchain work. For staging, create independent synthetic files without copying descriptors, hardlinks, junctions or ADS. Requested root/deny policy MUST be compared to the *actually selected tier and honored operations*; partial operations are unavailable, never silently enforced.
- Denial capture in **block** mode preserves access-denial semantics and may supplement child tests. Permissive/audit **allow** mode can run only with fully synthetic nonsecret workloads on a disposable machine for diagnostics and cannot count toward any containment acceptance.
- Runtime fallback to a weaker AppContainer mode, tier lacking read/deny controls, outside owned Job, generic FullHost, or missing process initialization closes negative/unavailable, not positive.
- Network unknown; `network=Enforced` forbidden without separately tested network policy; preserve strict production Windows `SandboxRequest::Required` unavailable.

## 4. Ordered work packages

**A — Versioned capability and host inventory.** Inspect current upstream release tag, exact SDK and CLI binary digest, license and Rust MSRV. Decide whether denial capture exists in the pinned C020 v1.0.0 revision or requires a new pinned experimental release; preserve the historical pinned fixture. Build a *separate* isolated harness/lockfile/toolchain (do not raise Codegg Rust 1.89 MSRV). Check Windows edition, architecture, build, UBR, necessary `processmodel.dll` exports, PSEC capabilities, actual tier, documented support matrix and secure OS update eligibility. If unsupported, record blocked and stop; no attempt to grant system-wide privilege or install a preview OS on a user host.

**B — Minimal trusted startup and diagnostics.** On supported host, launch a tiny native first-instruction marker, `cmd /c echo`, PowerShell no-profile and Git init/status. Record launch API, initialization exit/error, stderr, CRT/no-CRT markers, DLL/registry/device/resource denials, status and Job ordering. Run deny-and-record (if supported) and normalize events to denied resource identity/type, requested access, effective tier, scope, child identity and error. An event absent due to unsupported capture is `not_run`, not proof no denial. Use permissive audit only to triage trusted synthetic test if useful, separately labeled not security evidence.

**C — Essential and representative toolchains.** Re-run real Python script (not `python --version` only), native compiled helper, Cargo bounded offline smoke, Git initialization/status and shell commands; qualify each independently as pass/fail/not_run. Determine if compatibility grants involve normal OS DLL/registry/device facilities or dangerously broad user-data objects; reject latter. Test `\\.\NUL` and MSYS `/dev/null` separately with Win32/device error capture rather than guessing cause from exception wrapper.

**D — Strict security matrix.** Against unchanged synthetic `Users`/`Authenticated Users`-readable host secret, run ordinary unconfined child **positive read baseline**, then actual MXC child attempt. Assert denied content read through direct, junction, hardlink, case, ADS, extended path and sibling; no write/create/move/delete outside stage; named pipe/daemon synthetic endpoint and passed/inheritable handles rejected; descendants terminated after cancellation and Job close. Include token/tier/Job facts and environmental exceptions. Test same-file ID and descriptor/hashes before/after. A broad-reader access observed under MXC is security negative even if other denies pass. Do not treat inapplicable namespaced probes as pass.

**E — Closure and review.** Record exact source SHA, SDK/binary digest, OS edition/build/UBR, capability probe/tier selection, requested-vs-honored rules, all access-denial telemetry modes, child-access matrix, runtime test results, process/Job cleanup, time/resource costs, original descriptor/hash integrity and omitted cases. Write `plans/closure/windows-native-sandbox/033-status.md` and update roadmap/registry. C034 consumes positive, negative or blocked closure independently from C032.

## 5. Verification

Primary pinned harness: `cargo run --locked --manifest-path research/windows-native-sandbox/mxc-v1-harness/Cargo.toml` for historical baseline only; use newly pinned successor manifest when introducing capabilities not in v1.0.0. Run Windows C019 oracle via `cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_native_probe_contract -- --nocapture --test-threads=1` and new exact-harness test/driver on supported serviced Windows 11. If CI runner is unsupported, record blocked with host inventory; no green-host-probe-as-containment claim.

Cross-host: `git diff --check`; `cargo fmt --all -- --check` when Rust changes; `python3 scripts/check_sandbox_contract.py`; `python3 scripts/check_execution_ownership.py`; `bash scripts/verify.sh quick`. Do not claim Windows Server 2025 coverage based on Windows 11 tests or vice versa.

## 6. Failure, contention and migration

No product schema/API/protocol/installer change. Sandbox/Job created and verified before tool execution; timeouts kill descendants. No cross-run shared profiles, unknown global system changes, user credential propagation or host ACL modifications. Include startup and cleanup latency and provisional CPU/memory cost; no positive multi-worktree/restart claim without actual fault injection. An unsupported SDK feature or host edition is an operational blocker, not an invitation to weaken the policy.

## 7. Acceptance and stop conditions

Bounded strict positive: selected documented tier with enforceable explicit read/write/deny controls; actual running native tool + Git/shell with verified Job, denied same broad-group host reads through alias probes, stage allowed, no unqualified inherited handle/IPC exposure, unchanged original metadata, all relevant requested policy operations actually honored, and successful cleanup. Actual Python/script/helper failure (if declared minimum) or unrun inherited-handle test prevents a fully qualified result. Separate optional tool support truthfully.

Close negative on read leak, unsupported effective security operations, inability to execute minimum Git/shell, uncontrolled fallback, unsafe user-host ACL grant or leaked descendant. Close blocked if no supported host/runtime/consent. Never reinterpret C020 or admit production Windows `Required`, M004–M006, C011 or C015–C018 from this experiment.
