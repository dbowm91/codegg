# Windows Native Sandbox C019 — Common Running-Child Adversarial Matrix and C014 Diagnosis

Status: active (ADR-0020 accepted; Windows Server 2025 fixture sufficient for baseline)
Date: 2026-10-12
Primary class: infrastructure / security feasibility
Objective: Freeze a reusable, backend-neutral, actual-child security/compatibility oracle and diagnose the C014 Git /dev/null failure without re-opening failed AppContainer implementation.

**Authority:** ADR-0016 (cross-platform launch authority), ADR-0019 (historical experimental record), ADR-0020 (accepted feasibility comparison), `plans/subsystems/windows-native-sandbox-roadmap.md`, and `plans/003-planning-process.md`.

**Baseline:** `b1024ba719c627f8a294cb50170d48ec08ce62f4` on `work/windows-native-sandbox-foundation` (2026-10-12), refresh HEAD before implementation.

**Security invariant for every plan:** Production Windows `SandboxRequest::Required` stays unavailable; no production backend registration, no `SandboxExecutionOutcome::Enforced`, no silent FullHost downgrade, no network confinement assertion without packet/socket evidence. Existing Linux/macOS backends, canonical `ManagedProcessService` ownership, identity/worktree ceilings and approval/audit remain unchanged. Fixture-only behavior is not release behavior. Preserve unrelated branch changes and keep Windows sandbox scope independent from the other diff.

**Closure convention:** report exact code SHA, Windows edition/build/installed KB, host privilege, target tool versions, child token/identity/backend, selected nonzero test count, logs proving *running-child* allowed and prohibited operations, skipped tests and reasons, provenance of isolated VM evidence, and before/after host security descriptors where applicable. A missing native/privileged host is **blocked**, an observed incompatibility or privilege escape is **closed negative**, and a passing compile or preflight is never positive security evidence. Close with `plans/closure/windows-native-sandbox/019-status.md`; update roadmap and registry on actual evidence only.


Long-term references: `plans/000-long-term-specification.md` (sections 4, 17, 27 and 29) and `plans/001-terminology-and-domain-model.md` (section 8).
 
## Present evidence and scope

C011 host-secret alias reads and C013 populated ACL-lease failures are real native regressions. C014's actual child wrote/read projection but Git `init` failed on `/dev/null` **before** the rest of its denial probes. Current `tests/windows_projection_feasibility.rs` mixes toolchain smoke and later security checks into one sequential PowerShell script. Separate probes and collect per-probe outcomes so one compatibility fault never masks unrelated security behavior.

In scope: test-only fixture/probe library with strict typed observation/reporting, portable test manifest schema and native Windows executable probes; C014 `\Device\Null` handle/ACL/device-object diagnostics in an isolated test VM where needed. Out of scope: a new sandbox backend, modifying real host ACLs, account provisioning, production status changes, a permanent elevated CI action or retrying C014 until green.

## Ordered work packages

1. **Baseline inventory:** extract C011/C013/C014 source/test names, fixtures and precise failures; capture parent/child token groups/integrity, handle inheritance, effective child identity and Job membership. Define typed per-action record: {backend, host version, token, action, resource identity, expected, observed, error, child evidence, executed?, provenance}. No result synthesized from exit code alone.
2. **Synthetic resources:** allocate per-run authorized stage and original-workspace host trees, protected synthetic credential, authorized regular file, unauthorized sibling, synthetic pipe and inherited hostile handle. Include direct/reparse/junction/hardlink/ADS/UNC/device path variants with real file IDs/link counts and case/alias swaps. Deny fixtures must be actual objects, not merely request strings. No non-synthetic secrets.
3. **Decouple execution:** run minimal shell, `git init`/`status`, `python --version`/bounded script and `cargo --version`/bounded no-network operation when installed, independently from the direct/alias host-read/write, named-pipe, handle, child/grandchild, cancellation and restart probes. Failure of Git does not skip security probes when a child can run. Explicitly record unavailable SDK/tools and missing tested actions.
4. **Device hypothesis:** on disposable Windows VM identify Git distribution and failure system call/resource; compare normal child and AppContainer test seam with `\Device\Null` access, token and device descriptor. Permitted experiments with security descriptor manipulation are *isolated, explicit, measured and reverted*, not merged into normal host/CI setup. No global-device workaround in production.
5. **Test oracle and quality:** enforce fixed expected deny/allow and separate `not-run`/`blocked` outcomes, checksum/identity before-after fixture data and DACL/SACL/control flags, deterministic timeout and completed Job cleanup. Attach a machine-readable result and human matrix to closure.

## Acceptance / negative / blocker

- Test harness on Windows Server 2025 executes at least one actual child operation and reports every selected probe individually, with independent failure classifications, no alias-permission false positives and nonzero test selection.
- Known C011 synthetic credential bypass fixture remains observable as a regression **without** making CI unexpectedly require a failing legacy test; mark explicit expected historical weakness when invoking unqualified existing mechanism.
- C014 Git `/dev/null` failure is either root-caused by observed kernel object/access evidence or explicitly remains unconfirmed with exact traces; lack of privileged diagnostic host is separately blocked.
- Test matrix covers direct, junction, hardlink, ADS, sibling, denied create/write, pipe, pre-opened/inherited handle, descendant, cancellation, and allowed stage operations; unsupported probes are recorded not silently discarded.
- No original workspace ACL/SACL/MIC change, no production `Required` enablement. If a purportedly backend-independent test invokes privileged host mutation, stop and correct fixture.
- C020/C021 may proceed when common *security oracle/fixture* is strictly accepted, even if null-device root-cause subprobe remains operationally blocked (track separately).

## Verification

On an actual Windows runner, focused newly added `cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_native_probe_contract -- --nocapture --test-threads=1` and existing `windows_projection_feasibility` with explicit known-negative disposition; verify selection counts. On other hosts `cargo fmt --all -- --check`, `python3 scripts/check_sandbox_contract.py`, `python3 scripts/check_execution_ownership.py`, `git diff --check` and `bash scripts/verify.sh quick`. New command is target-to-create, not already runnable at planning time. Record test-only feature compile and missing host coverage.

## Concurrency / storage / docs

Disposable per-run fixtures with bounded depth/bytes/time and unique workspace/job identity; cleanup after Job closure, quarantine unexpected files rather than deleting foreign paths. No public schema/protocol; machine-readable fixture result is local evidence only. Update `research/windows-native-sandbox/adr-0020-harness-comparison.md` with exact diagnosis and `architecture/testing.md` only for verified new test mode. Write closure `plans/closure/windows-native-sandbox/019-status.md`, then unblock C020/C021 independently.
