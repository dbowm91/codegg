# Windows Native Sandbox C035 — MXC Native-helper Startup and Inherited-handle Boundary Corrective

Status: **implemented; conditionally closed as a bounded negative test-only candidate**
Primary class: corrective / security-infrastructure
Planning baseline: `80139df351e9a77f46f7b09f904a0d6e854939b4` on `work/windows-native-sandbox-foundation`; implementation agent MUST refresh head/evidence before edits.
Source subsystem roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0023-mxc-helper-diagnostic-corrective`.
Architecture authority: **ADR-0025 (accepted for test-only C035 only)**; ADR-0024 prior bounded feasibility, ADR-0020 C019 oracle, ADR-0016 process authority, ADR-0004 approval/security policy. ADR-0023 rejection and C034 no-admission remain effective.
Canonical: `plans/000-long-term-specification.md` §§4,17,27,29; `plans/001-terminology-and-domain-model.md` §8; `plans/003-planning-process.md`.
Corrective predecessor: `plans/implementation/windows-native-sandbox/033-mxc-denial-telemetry-and-toolchain-requalification.md` and `plans/closure/windows-native-sandbox/033-status.md`; C034 comparison at `plans/closure/windows-native-sandbox/034-status.md`.

## 1. Objective

Close, with independently observable native results, the **four unclosed C033 findings** rather than rerun another broad feasibility loop: (A) MXC native-helper startup `STATUS_DLL_INIT_FAILED`, (B) actual inherited-handle attempt not performed, (C) stage write not independently asserted and child identity/groups not observed, (D) ambiguous per-invocation denial telemetry. Obtain strict-negative or strict-positive test-only closure without changing Windows product sandbox availability.

C033's hosted run `38404451372` at SHA `ff0443ca` (Windows 11 Enterprise 25H2 ARM64 build 26200 UBR 9457) positively demonstrated normal same-file read baseline; MXC BaseContainer denied direct, junction, hardlink, ADS, case, extended-path and sibling reads, outside writes and synthetic pipe; shell/Git/Python/Cargo worked and 13 block-mode denied events were reported. A staged **copy of the complete C033 Rust/MXC harness executable**, launched as a second MXC request, exited `-1073741502` / `0xC0000142` (DLL initialization failure) before reporting its inherited-handle observation. This is a native entry/loader **hypothesis**, not proof of a particular missing DLL or MXC defect.

### Why prior verification missed it

- The copied executable itself carries the full MXC SDK/Rust dependencies. The original C033 helper has no independent native **first-user-entry marker** or positive launch control under equivalent executable/cwd/env assumptions. A nonzero process return was recorded, but its cause was not isolated.
- The parent called `SetHandleInformation(..., HANDLE_FLAG_INHERIT)` for the file but did not establish a successful uncontained child inheritance/read under an explicit `CreateProcessW` `bInheritHandles=TRUE` control. A missing output file was marked `unavailable`, correctly not denied.
- The batch performed `echo stage-ok > stage-write.txt` and `whoami /user`/`/groups`, but the report did not independently read back a nonce-bearing stage file and did not obtain reliable token-group records.
- The main MXC invocation and copied helper reuse the same `capture.output_path` (`denials.json`), allowing ambiguous attribution/overwriting; a 13-event count alone does not establish which invocation produced each event.

## 2. Hard security/ownership constraints

- Test-only fixtures under `research/windows-native-sandbox/mxc-c033-harness/` or a clearly named successor `mxc-c035-harness/`, plus isolated Windows 11 ARM64 workflow/scripts and evidence documentation. Preserve Codegg Rust 1.89 MSRV; MXC 1.0.0 and Rust 1.93.1 remain pinned and isolated unless a documented diagnostic variant is necessary.
- No product backend registration, agent-exposed tool, `src/managed_process.rs` or `src/security/sandbox/backend.rs` changes, service/broker, Windows guest, administrator installer, firewall/WFP, real user profile/credential/ACL mutation, or production `Required` path. No broad user directory or registry blanket grants to make the probe start.
- Continue using the same synthetic original host secret readable by `Everyone`/`Users`/`Authenticated Users` for the **normal-child positive** and MXC denied-read contrast; record its file ID, descriptor hash/control bits and content hash before/after. Never mutate original host objects or alias-sharing DACLs.
- Use block-mode denial capture only for security decisions. Diagnostic unconfined tests must be strictly synthetic and labeled; unsafe read results are negative, not workarounds. Never count a loader failure as denied handle or filesystem access.
- Distinguish launch API outcome, DLL/PE loader failure, first marker, actual child token, actual handle operation, stage file, timeout, and Job cleanup as **separate** fields. Typed outcomes: `passed` / `failed` / `not_run` / `unavailable` / `blocked`; an empty/missing result is not `passed`.
- Evidence collection may be green when a candidate fails, but the report and GitHub step summary must explicitly say `qualified=false` and which gates failed. Never represent CI job success as security admission.

## 3. Scope and exact code surfaces

- Main candidate: `research/windows-native-sandbox/mxc-c033-harness/src/main.rs`. Existing `inherited_handle_probe()` at roughly lines 140–180, staged self-copy at roughly 292–310, stage/write and whoami lines 314–328, MXC policy/request/environment at 418–508, and denial/report collection at 548–635 are the investigation entry points; line numbers are advisory only.
- Workflow: `.github/workflows/windows-mxc-c033-requalification.yml` may be reused only with separate C035 labeled reporting, or add `.github/workflows/windows-mxc-c035-helper-corrective.yml`. Do not change C033's historical SHA/run/closure. Prefer a dedicated workflow to distinguish old negative vs new evidence.
- A minimal trusted native ARM64 Win32 child fixture is permitted (C/MSVC with explicitly controlled CRT/imports or a minimal Rust executable with pinned target), placed separately from the MXC SDK-bearing parent harness. Capture compiler/linker version, target machine, imported DLL table, executable SHA-256, and early entry marker.
- Common C019 oracle under `tests/windows_native_probe_contract.rs` is the adversarial reference. It need not be expanded into an unrelated workspace test refactor.

## 4. Ordered work packages and hard gates

### WP-A — Fix fixture provenance and build reliable startup controls

1. Keep a **no-MXC, identical executable path/cwd/env** launch control for existing staged copied helper; add separately compiled minimal Win32 ARM64 helper that records an entry marker **from the child** to a fresh, nonce-bound stage path and exits with a distinct known code. Optionally contrast CRT/no-CRT and original-location/staged-location cases, but cap variations to those that discriminate loader vs runtime/policy. Include trusted binary hash, machine type, import table and full `GetLastError` / exit NTSTATUS evidence.
2. Run three core cases: (i) uncontained original/staged helper control, (ii) MXC minimum native helper with same environment/cwd, (iii) MXC staged copy of existing Rust harness. Record actual process entry, imports, DLL/registry/device denial events if observed, plus stdout/stderr, exit and timeout. If even uncontained staged control cannot reach entry, classify **fixture defect** and repair only that fixture; do not widen MXC allowlists. If only MXC fails, attempt **one** measured minimal-runtime-dependency correction and re-test. A second unexplained failure is a bounded **negative/blocked** result, not an invitation for repeated speculative policy relaxations.
3. Assert architecture/target parity (ARM64 versus host) and explicit PE entry-point/early marker provenance. Distinguish `STATUS_DLL_INIT_FAILED` from wrong-architecture/command-quoting errors; do not diagnose exact DLL from the NTSTATUS alone.

**Gate A:** trusted native helper reaches actual first user instruction outside and inside selected enforcing MXC BaseContainer, or C035 closes explicitly negative with precise first-known divergence. Do not continue to inherited-handle security gate without this.

### WP-B — Genuine inherited handle boundary with a meaningful control

1. Parent creates a **unique synthetic host secret** and opens a readable native file handle; verify `GetHandleInformation` confirms `HANDLE_FLAG_INHERIT`. Verify the regular unconfined control child is created with **actual handle inheritance enabled** via `CreateProcessW` (or an equivalent explicitly inspected native launch), receives the intended numerical handle value and **successfully reads exactly the synthetic bytes**. Record child PID, handle value, actual inherited handle type, `GetLastError` and content digest. If the control cannot read, the setup is invalid; stop or repair the fixture, not mark MXC success.
2. Run the **same compiled child probe** under an MXC request with the same synthetic host secret (normal-child positive read control retained), report file path for output in owned stage, nonce, requested/effective BaseContainer tier and block capture. Child MUST reach entry and report whether `GetHandleInformation`/`GetFileType` and actual `ReadFile` succeeded; collect exact Win32 result and byte/content hash. Record whether the MXC API allows inherited-handle propagation or deliberately strips handles. If the SDK offers no way to exercise an inheritable handle under the same semantics, classify this test **unsupported/unavailable**, not `denied`; a blocked transfer may support only a narrowly worded no-ambient-inheritance claim.
3. Include a separate explicit handle transfer/control (where supported) to check against a child bypass through standard handles or other permitted inheritance channels; DO NOT create an unreviewed command-controlled duplication or elevate source privilege. A successful forbidden content read under MXC is a **security failure / stop**. If handles are absent, check and attest that the child actually attempted the specified handle, not a skipped environment branch.

**Gate B:** uncontained child undeniably reads synthetic inherited handle while actual MXC child runs and cannot obtain its contents, with transfer semantics demonstrably comparable and no leaked inherited/IPC capability; otherwise negative, unavailable or blocked with reason. A missing marker or status file is never a pass.

### WP-C — Stage-write and child identity are independent observations

1. Generate per-invocation unpredictable synthetic nonce and require an actual sandbox child to write a newly named `stage/child-proof-<nonce>.json` containing the nonce, PID, tool result and fixed content marker. Parent verifies **after MXC return**: file exists; exact nonce/PID/content; was absent beforehand; owner/stage path and hash; no outside writes; unique file identity. Remove this marker on cleanup. The parent MUST NOT pre-create/fake a success marker or accept `echo allowed` alone.
2. Record child identity using direct `OpenProcessToken` + `GetTokenInformation` for `TokenUser`, `TokenGroups`, `TokenIntegrityLevel`, `TokenType` and relevant restricted/AppContainer capability fields where applicable. Prefer structured native helper output in the owned stage; record failure codes and mark unavailable, not fabricate identities from process stdout. Correlate PID/nonce and selected MXC tier; if parent can query child before termination, report that independently and note which context observed it.
3. Preserve C033's useful Git init/status, PowerShell/cmd, Python script/descendant and offline Cargo smoke. Verify **true stage read/write**, ordinary same-file broad read, MXC denied original file/aliases and outside write again after any helper/runtime grant correction; ensure no new runtime read allowlist accidentally encompasses synthetic host secret or a user profile.

**Gate C:** child-created permitted stage data verified on disk with correct nonce, structured token identity (or explicit unavailable), and no regression in baseline/read/alias/write/tool observations. A mandatory token/identity omission prevents strict positive.

### WP-D — Isolate denial telemetry, enforce negative provenance and cleanup

1. Give each MXC request an independent fresh capture directory/output filename (e.g. `main-<nonce>/denials.json` and `handle-<nonce>/denials.json`); never arbitrarily pick the first JSON in a shared directory. Report per-invocation process/run ID, selected backend/tier/capabilities, capture mode, child PID where available, actual event count, truncation flags and phase. Store original per-run evidence in restricted CI artifacts with redacted/hashed sensitive strings; summary reports should not expose real absolute user paths.
2. Report actual exit for every invocation. Do **not** allow a nonzero helper exit, pre-entry status or missing inherited-handle file to map to `not_transferred`. Enforce explicit expected/observed assertions and an overall machine-readable `qualified=false/true` with named failed gates; successful report collection is separately labeled. Add a regression that intentionally makes the helper exit before entry and verifies the harness refuses to call it a denial.
3. Assert no leftover Job-owned descendant after normal completion and after a single forced failure/timeout within a capped, synthetic-run budget. Check original secret contents, file ID, owner/DACL/control bits, ACL hash, stage and account/runner cleanup. If Job or MXC cleanup semantics are inaccessible, state exact coverage; do not infer crash/reboot recovery or concurrency.
4. Write C035 closure under `plans/closure/windows-native-sandbox/035-status.md` containing a requirement-to-evidence matrix, tested branch SHA, workflow run ID, host OS build/UBR/edition/architecture, MXC and toolchain versions, helper build/import SHA, all control and child results, gate outcome and remaining unknowns. Update roadmap and registry only to reflect **actual** result; preserve C033/C034 closures verbatim.

**Gate D:** independently attributable deny records, no false-positive classifications, original host fixture integrity, bounded cleanup and exact-SHA replayable evidence.

## 5. Testing, static guards and commands

Use an opt-in test-only native job on `windows-11-arm`, pin the isolated Rust toolchain to 1.93.1 and MXC SDK v1.0.0. Native run explicitly builds the ARM64 entry probe and invokes the C035 harness/driver with a real child (not a JSON mock). Include a no-container control before enforcing MXC; print `C035_HOST`, `C035_CONTROLS`, `C035_MXC`, `C035_HANDLES`, `C035_STAGE`, `C035_IDENTITY` and `C035_REPORT` with a versioned report schema. The workflow summary MUST state whether native *security qualification* succeeded independently of the Actions job conclusion; permit evidence collection to finish on a negative candidate but never mark the candidate green.

Focused checks (adapt only paths if the corrective creates a separate manifest):

- `cargo fmt --manifest-path research/windows-native-sandbox/mxc-c033-harness/Cargo.toml -- --check`.
- `cargo check --locked --manifest-path research/windows-native-sandbox/mxc-c033-harness/Cargo.toml` using its isolated toolchain.
- `cargo run --locked --manifest-path research/windows-native-sandbox/mxc-c033-harness/Cargo.toml` on an eligible Windows 11 ARM64 test host, or the new C035 driver when introduced.
- `cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_native_probe_contract -- --nocapture --test-threads=1` for shared-oracle compatibility if fixtures or contract code change.
- `git diff --check`; `cargo fmt --all -- --check` if workspace Rust touched; `python3 scripts/check_sandbox_contract.py`; `python3 scripts/check_execution_ownership.py`; `bash scripts/verify.sh quick`.

Run focused negative regression cases for control-not-inheritable, missing child entry, helper wrong-architecture, preexisting stage marker, mismatched nonce/PID, absent JSON, denial capture missing/overwritten, leaked handle and unexpectedly readable host object. Explicitly assert nonzero case selection rather than a workflow-only green check.

**Branch CI caveat:** preceding Linux CI run `38405432003` reported failures in `tests/tool_execution.rs` for blocked `$(whoami)` and `sudo su`. They are not caused by the isolated C033/C035 fixture based on current diff, but should be tracked/reconciled by an independent execution-policy corrective; do not loosen these security expectations or claim full-branch merge readiness from this plan. Record current-head CI status separately.

## 6. Migration, compatibility, latency and cleanup

No product migration, schema, protocol, feature exposure, installer, dependency, MSRV or runtime permission change. Keep test executables tied to a fixed hash/architecture; generated artifacts stay in unique owned disposable roots. All file, pipe, Job, token and process handles close on ordinary exit and failure. Unique random run roots prevent parallel test collision; they do not establish production multi-worktree isolation. Bound every child step and collect p50/p95 startup measurements only if convenient; do not trade security for benchmark success. Continue to report network `not tested`.

## 7. Acceptance, stop and closure disposition

**Bounded strict-positive C035 corrective** requires *all* of:

1. Trusted minimal ARM64 native probe reaches actual child entry uncontained and under enforcing MXC with comparable bounded environment and no broad original-host read grants.
2. Real, validated uncontained inherited synthetic-handle read and actual running MXC child denied/stripped equivalent handle access with provenance and no secret exposure; the case is not silently skipped/unsupported.
3. Independently verified unique child-written stage file and structured child token SID/groups/integrity evidence, tied to child PID, nonce and active MXC backend/tier.
4. C033's originally observed synthetic broad-file direct and alias denials, outside-write/pipe denials, normal positive control and Git/shell/Python/Cargo smoke do not regress; source ACL/file ID/hashes unchanged.
5. Per-invocation block-mode denial events are separately correlated, no false pass on missing results, and bounded Job/process cleanup passes.

**Negative** if helper still fails to enter after bounded diagnostic, MXC actually reads forbidden file or inherited secret content, same-file aliases escape, identity/provenance cannot be obtained under required policy, or minimum toolchain breaks unless unsafe grants are added. **Blocked** if eligible host/toolchain or required inheritance API is inaccessible, with explicit reason and no pretend pass. Stop immediately on real-data/host ACL mutation, new administrator service/broker/guest, policy weakening, unsafe host runtime/profile grants, descendant breakaway, or leaked secret.

Create only the C035 closure and corresponding registry/roadmap updates upon implementation; do not modify C033/C034 historical closure. **A positive C035 is still not production admission**. A new explicit production ADR plus independently reviewed broad security, lifecycle, concurrency, supported OS, installer/packaging and network qualification would be required before `src/managed_process.rs` or a production backend is enabled. C011/M004–M006/C015–C018 and C030/C031 remain blocked.
