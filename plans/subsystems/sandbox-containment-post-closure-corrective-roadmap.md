# Sandbox Containment Post-Closure Corrective Roadmap

Status: active integration-recovery corrective (C001-C003 closed; C004 conditionally closed; C005 conditionally closed; C006 ready)

Repository baseline reviewed: `main` at `8e9d8b01e5c229715c8e4dea929e050b391e252c` (2026-10-10).

Predecessors: `plans/subsystems/runtime-safety-resource-footprint-roadmap.md` (conditionally closed), `plans/implementation/runtime-safety-resource-footprint/001-landlock-and-sandbox-contract-correction.md`, `009-sandbox-helper-trust-channel-corrective-unblock.md`, and `plans/closure/runtime-safety-resource-footprint/001-status.md`, `009-status.md`, `010-status.md`. Historical closure/evidence is not overwritten by new findings.

Long-term references:
- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

Related accepted decision: `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md` (approval authority cannot silently grant broader execution).

## 1. Purpose and ownership

Close reproducible filesystem-containment and truthful-enforcement gaps on Linux and macOS. The `src/security/sandbox/{policy,backend,landlock,seatbelt}.rs` policy/backend separation and `codegg-sandbox-helper` remain canonical. `ManagedProcessService` remains the sole owner of child launch, supervision, cleanup, and private status. Bash/Python and any governed native/scheduler path must consume the same requested-vs-obtained authority. A backend name is not proof that all desired permissions are enforced.

This is a **new post-closure security corrective**, not a retrospective claim that the previously conditional runtime-safety workstream was strictly closed. Predecessor evidence conditions stay in place and are referenced, not silently discharged.

## 2. Work classification

**Invariants:** (a) a constrained request never silently runs with host authority; (b) a required policy must be applied *before* target code executes and incomplete enforcement cannot be reported as success; (c) declared credential exclusions actually resist reads/writes, including when overlapping allow roots; (d) `ReadOnly` prevents persistent content mutation including truncation; (e) a child never acquires a broader profile than the parent/job; (f) unconfined network/IPC remain explicitly disclosed, not described as isolated.

**Capabilities:** macOS Python Analyze/Transform/Verify receive actual Seatbelt containment; both hosts offer predictable workspace-write/read-only behavior; operators see backend, effective rights/ABI, reason for denial or degraded mode.

**Infrastructure:** corrected backend capability/ABI negotiation, negative-proof fixtures, normalized read/write/deny precedence, narrow scratch/cache ownership, and shared real-helper tests.

**Polish:** macOS availability diagnostics, accurate architecture/CLI documentation, minimal maintained guards.

## 3. Non-goals

No sandbox daemon, replacement process supervisor, containers, VMs, namespace/seccomp/bubblewrap framework, Windows backend, broad scheduler/authorization rewrite, new CI matrix, external-service test, dependency sweep, or implied strong network/IPC isolation. No forcing tool-breaking strict macOS filesystem read allowlisting absent empirical viability. No rewrite of accepted predecessor closure records.

## 4. Current source evidence and defects

- C001 is closed at `plans/closure/sandbox-containment-post-closure-corrective/001-status.md`: Python selection is platform-neutral, required constrained launches fail closed, and native Darwin helper/Python mode qualification passed on Apple Silicon.
- C002 is closed at `plans/closure/sandbox-containment-post-closure-corrective/002-status.md`: Linux kernel `6.8.0-146-generic`, effective Landlock ABI 4, real-helper mutation probes passed.
- C003 is closed at `plans/closure/sandbox-containment-post-closure-corrective/003-status.md`: overlapping Linux Landlock grants are carved around credential denials, Seatbelt emits final denials after re-allowances, broad persistent write roots are removed, and private 0700 per-launch scratch is reaped after the process tree exits. Native Darwin helper and lifecycle probes passed.
- Real-helper Linux probes cover nested and future-created credentials, aliases, workspace writes, truncation, rename/link, scoped scratch, and offline Cargo metadata. Native Darwin C001/C003 qualification passed in hosted run `38166386985` on `macos-15-arm64`; C004's residual IPC/privilege probes remain named and unqualified.

Source references:
- `src/security/sandbox.rs`, `src/security/sandbox/policy.rs`, `src/security/sandbox/landlock.rs`, `src/security/sandbox/seatbelt.rs`
- `src/python_script/{sandbox,executor}.rs`, `src/tool/bash/process.rs`, `src/managed_process.rs`
- `tests/sandbox_containment.rs`, `tests/sandbox_policy_wiring.rs`, `architecture/security.md`
- Kernel reference: https://docs.kernel.org/userspace-api/landlock.html (ABI 3 `TRUNCATE`, ABI 2 `REFER`).

## 5. Target contract

Input = one authenticated execution policy snapshot: `SandboxProfile`, per-command capability requirement (strict/explicitly approved degraded/disabled), normalized workspace and optional scoped scratch/cache roots, and declared sensitive exclusions. Rendering is backend-specific; **effective grant must be a subset of authorized grant**. Do not claim identical MAC/LSM capabilities; use a backend-neutral minimum guarantee for filesystem mutation and credential confidentiality, plus backend-specific limitations. Classify runtime capability *and* actually obtained enforcement, not just presence of `sandbox-exec` or Landlock ruleset creation. A missing helper or failed application never runs a required target.

Sandboxed child network access, direct IPC, inherited file descriptors, setuid behavior, and tool-provided exfiltration paths remain separately threat-modeled. Any unsupported guarantee is visible.

## 6. Dependency graph and order

```text
C001 (Python Seatbelt + required-failure semantics) ────────────┐
       |                                                        |
       └────────────────────────────────────────────────────┐   |
C002 (Landlock ABI/truncate correction; independent) ──> C003  |
                                     (allow/deny/cache rights)  |
                                                          |     |
                                                          └──┬──┘
                                                             v
                                                       C004 (macOS read/IPC
                                                       boundary + qualification)
```

- C001: closed; backend-neutral selection and required-failure contract are qualified on Linux and native Apple Silicon.
- C002: closed; ABI >=3 is the required guarantee tier for preventing truncation; older kernels are rejected for strict requests.
- C003: closed; Linux ABI 4 and native Apple Silicon effective deny/write/scratch evidence passed.
- C004: conditionally closed; effective read/write/network/FD contract and tool compatibility are measured and documented. Apple Events/Launch Services reachability, XPC/Mach reachability, and setuid transitions remain unqualified; no claim of full parity or complete IPC isolation.
- C005: conditionally closed; ownership-safe sandbox candidate is PR #112; local focused tests are blocked by the configured x86_64 macOS linker and final hosted qualification transfers to C006.
- C006: ready; qualify PR #112's final head through normal hosted CI and Desktop E2E; retain C004's residual limitations.

All four plan documents may be read in parallel, but hand off only dependency-ready ones for production changes.

## 7. Milestones and user/operator exits

| Milestone | Class | Implementation plan | Status | Exit |
|---|---|---|---|---|
| C001 | invariant | `plans/implementation/sandbox-containment-post-closure-corrective/001-macos-python-and-strict-launch-contract.md` | closed | strict constrained launch and Python mode behavior qualified on Linux and native Apple Silicon |
| C002 | invariant | `plans/implementation/sandbox-containment-post-closure-corrective/002-landlock-abi-and-mutation-rights.md` | closed | Linux ABI>=3 protects truncation; older kernels receive truthful and safe disposition |
| C003 | invariant | `plans/implementation/sandbox-containment-post-closure-corrective/003-policy-deny-precedence-and-scoped-tool-state.md` | closed | Linux ABI 4 and native Apple Silicon credential deny, scoped write, and scratch lifecycle evidence passed |
| C004 | capability/invariant | `plans/implementation/sandbox-containment-post-closure-corrective/004-macos-confidentiality-and-cross-platform-qualification.md` | conditionally closed | actual macOS read/write/network/FD limits and tooling compatibility qualified; IPC and privilege residuals remain explicit |
| C005 | invariant/infrastructure | `plans/implementation/sandbox-containment-post-closure-corrective/005-integration-branch-ownership-and-base-reconciliation.md` | conditionally closed | ownership-safe PR #112; local test linking blocked, with hosted evidence assigned to C006 |
| C006 | invariant/infrastructure | `plans/implementation/sandbox-containment-post-closure-corrective/006-hosted-integration-qualification-and-closure.md` | ready | qualify final PR #112 head through hosted CI and Desktop E2E; C004 residual limits remain unchanged |

Closure record for each: `plans/closure/sandbox-containment-post-closure-corrective/00N-status.md`.

## 8. Cross-cutting requirements

Storage: no schema migration; ephemeral isolated scratch directories must be lifecycle-bound and cleaned on cancellation/crash (with bounded stale cleanup on restart if persistent per-run scratch is introduced). No durable policy change without explicit configuration migration.

Protocol/configuration: retain existing `ReadOnly`, `WorkspaceWrite`, `FullHost` and audit tokens where possible. An already-uncontained legacy mode cannot be rebranded `FullHost`; rejecting a previously allowed degraded operation is a documented safety tightening. Use a new explicit authorization surface only if existing approval semantics cannot represent it; do not smuggle best-effort via `Yolo`.

Concurrency: never sandbox daemon; inherited helper descriptor must close on exec; concurrent per-job roots cannot grant writes into one another. All path normalization and policy preparation before child starts; cancellation and timeouts own the entire process tree.

Observability: report requested profile, applied backend, effective ABI/rights, explicit limits, failure stage, and whether *any* user code ran. Do not log credential values, unsanitized command text, private paths in routine telemetry, or generated SBPL containing secrets.

Qualification: reuse `tests/sandbox_containment.rs`, `tests/sandbox_landlock.rs` and existing quick/static guards. Capture at least one supported Linux and one macOS real-helper result; kernel/ABI/version, assertion outcomes and skips must be explicit. Do not add an always-on CI matrix solely for this line.

## 9. Risks and decision points

- Tool compatibility versus read secrecy: `git`, `cargo`, `rustup`, developer CLIs and macOS runtime caches legitimately read outside the workspace. C004 must select a **measured** policy, not assert full Landlock-style parity by renaming a profile.
- Seatbelt is deprecated and permission semantics can vary by macOS release; record OS version and test profile, not merely the presence of `sandbox-exec`.
- Broad temp and cache roots create same-user cross-job and credential integrity hazards; C003 must isolate scratch correctly without breaking real compilers.
- Landlock older ABIs cannot express every modern mutation right. Treat unsupported essential protections as unavailable in strict mode; do not claim `FullyEnforced` means rights absent from the chosen ABI have been covered.
- If a network/IPC boundary change or new public sandbox mode requires durable authority redesign, stop and create an ADR rather than hide the security contract change inside a renderer.

## 10. Integration recovery corrective and observed red CI (2026-10-12)

The C001-C003 production security corrections are **closed**, C004 stays **conditionally closed**. None is reopened merely because a later mixed integration branch failed unrelated whole-workspace gates. This new line corrects merge/ownership and CI handoff without weakening or silently expanding the sandbox guarantee.

At the reconciled baseline `7317a67ad604fe39fd5282ddca07496255faa4d1`, the preserved planning branch `planning/sandbox-containment-post-closure-corrective` remains an unchanged 41-commit mixed provenance branch. The focused candidate was created from that `main` base at PR [#112](https://github.com/dbowm91/codegg/pull/112), head `e9ea2078718130229ef639af0658cab96fed191c`; its review diff contains sandbox C001–C004 files plus two narrow CI lint repairs. PR [#110](https://github.com/dbowm91/codegg/pull/110) remains open as preserved provenance. Human-shell, LSP/editor and desktop ownership is separately reviewable in PR [#111](https://github.com/dbowm91/codegg/pull/111). Earlier C005/C006 handoff documents registered at commit `baf9f228` disappeared during prior history reconciliation; they were restored on the planning branch and are retained there.

The mixed-head hosted `CI / verify` [38177378810](https://github.com/dbowm91/codegg/actions/runs/38177378810) failed eight Clippy diagnostics in `src/core/daemon_turns.rs`, `src/tui/commands/editor.rs`, and `src/core/daemon_lsp.rs`; hosted `Desktop E2E` [38177378815](https://github.com/dbowm91/codegg/actions/runs/38177378815) failed duplicate `input_tokens`/`output_tokens` fields in `apps/desktop/src-tauri/src/present.rs`. Those failures were real and are now excluded from sandbox PR #112; PR #111 carries the sibling owners and its hosted CI and Desktop E2E passed. The first focused-candidate CI run `38276126795` passed formatting and guards, then found a current Clippy 1.99 redundant-wildcard error in `tests/session_family/projection_consumer.inc`; C006 fixed the semantics-preserving test pattern and a non-Linux unused import at `e9ea207`.

**C005 — conditionally closed:** `plans/implementation/sandbox-containment-post-closure-corrective/005-integration-branch-ownership-and-base-reconciliation.md`. The ownership matrix and focused PR #112 are recorded at `plans/closure/sandbox-containment-post-closure-corrective/005-status.md`. Local focused Rust tests could not link with the configured x86_64 macOS target; C006 owns exact-head hosted test evidence.

**C006 — ready:** `plans/implementation/sandbox-containment-post-closure-corrective/006-hosted-integration-qualification-and-closure.md`. The prior human-shell/editor/desktop diagnostics are excluded from #112 and retained on PR #111. A newly surfaced session-family Clippy warning received a semantics-preserving cleanup; obtain normal hosted CI and Desktop E2E on #112's final SHA and record the local linker limitation with hosted test evidence. Required closure: `plans/closure/sandbox-containment-post-closure-corrective/006-status.md`.

Do not start another broad test campaign, change C004 conditional status, create new sandbox IPC claims, or merge an unattributed human-shell/editor diff under a sandbox PR. Native Linux 6.8/ABI 4 and macOS 15.7.9 evidence remain valid only to the extent the tested sandbox implementation semantics are unchanged.

## 11. Completion rule

All four closure records must establish real enforcement tests, fail-closed required behavior, scoped write and deny precedence, macOS Python correctness, and truthful residual limits. Historical predecessor conditional evidence stays independently unresolved until its own concrete evidence is supplied. Do not mark this workstream closed from unit-profile string assertions or an unsupported-host skip alone.
