# Windows Native Sandbox C022 — MXC Versus Dedicated Principal Disposition and Successor Gate

Status: conditionally closed (insufficient evidence for production admission; bounded comparison and explicit successor gate recorded)
Date: 2026-10-12
Primary class: infrastructure / security feasibility
Objective: Compare both native candidate experiments against identical security/compatibility criteria, choose a next integration planning direction or explicitly retain Windows constrained unavailability.

**Authority:** ADR-0016 (cross-platform launch authority), ADR-0019 (historical experimental record), ADR-0020 (accepted feasibility comparison), `plans/subsystems/windows-native-sandbox-roadmap.md`, and `plans/003-planning-process.md`.

**Baseline:** `b1024ba719c627f8a294cb50170d48ec08ce62f4` on `work/windows-native-sandbox-foundation` (2026-10-12), refresh HEAD before implementation.

**Security invariant for every plan:** Production Windows `SandboxRequest::Required` stays unavailable; no production backend registration, no `SandboxExecutionOutcome::Enforced`, no silent FullHost downgrade, no network confinement assertion without packet/socket evidence. Existing Linux/macOS backends, canonical `ManagedProcessService` ownership, identity/worktree ceilings and approval/audit remain unchanged. Fixture-only behavior is not release behavior. Preserve unrelated branch changes and keep Windows sandbox scope independent from the other diff.

**Closure convention:** report exact code SHA, Windows edition/build/installed KB, host privilege, target tool versions, child token/identity/backend, selected nonzero test count, logs proving *running-child* allowed and prohibited operations, skipped tests and reasons, provenance of isolated VM evidence, and before/after host security descriptors where applicable. A missing native/privileged host is **blocked**, an observed incompatibility or privilege escape is **closed negative**, and a passing compile or preflight is never positive security evidence. Close with `plans/closure/windows-native-sandbox/022-status.md`; update roadmap and registry on actual evidence only.


Long-term references: `plans/000-long-term-specification.md` (sections 4, 17, 27 and 29) and `plans/001-terminology-and-domain-model.md` (section 8).

## Scope

This is a **decision/documentation closure**, not permission to enable a Windows sandbox. Inputs are the accepted C019 shared oracle and exact-SHA C020/C021 native closure records. C020/C021 may each close positive, negative or blocked. Missing host evidence remains blocked and cannot be laundered into success. The earlier C007/011/013/014 negative evidence remains in force. No production code, protocol, enrollment, or user-profile change is authorized by this plan.

## Work packages

1. **Normalize facts:** produce one matrix of effective OS backend/tier and build, chosen principal/token/restricting SIDs, direct and aliased original-host denied read/write results, allowed stage, shell/Git/Cargo/Python, descendants/cancel/restart, inherited handles, IPC/registry/device read exceptions, host ACL/MIC drift, network actual/unclaimed, rollback, provisioning/consent, MSRV/install footprint and latency sample details. Cite actual fixture log lines/commit/run for every security cell.
2. **Uncertainty and independent review:** distinguish vendor-documentation conjecture, test setup failure, negative child result, native OS evidence and risk accepted explicitly. Review high/medium findings with independent Windows security reader. Confirm no new trust edge from a tool-side helper to daemon authority and no C011 invariant waived.
3. **Decision:** if **one or both** strict-positive, recommend the least-mutating usable backend, supported build matrix and explicit caveats; draft a *new* production integration ADR/roadmap sequence for maintainer acceptance, still leaving Windows `Required` unavailable. If both negative, record retain-unavailable and optionally propose separate VM/WSL architecture. If either is blocked and no strict-positive path exists, record incomplete comparison and conditional follow-up—not a fabricated winner.
4. **Plan disposition:** do not auto-unblock old AppContainer-specific C015–C018. Determine which bounded input snapshot/C017 promotion concepts are reusable and which must be redesigned for selected runtime. If dedicated-user chosen, move identity foundation ahead of M004 and retain M005 firewall/offline qualification afterward. Make a new minimal integration series only after explicit acceptance; no independent production action in C022.
5. **Registry and audit:** update `plans/subsystems/windows-native-sandbox-roadmap.md`, `plans/registry.md` and new `research/windows-native-sandbox/adr-0020-harness-comparison.md` with final *positive/negative/blocked* dispositions. Keep C011/M004–M006 unresolved until a separately verified end-to-end integration and independent review.

## Exit criteria

Every candidate has exact evidence or explicitly blocked operational prerequisite; differences in Windows 11 versus Windows Server eligibility are not conflated. No high/medium unreviewed security finding or missing negative probe is reclassified as positive. A clear **winner / neither / insufficient evidence** disposition is recorded, with concrete follow-on admission conditions and risk/OS matrix. Other agents can continue without revisiting the same inconclusive experiments. No user-visible claim of Windows enforcement is made.

## Verification and closure

Review file/registry references and status graph; `git diff --check` and existing planning/contract guards where available, `cargo fmt --all -- --check` / `bash scripts/verify.sh quick` only if code touched. Do not invent a new permanent CI policy lint just for documentation. Native security tests are inherited from C020/C021 exact traces, not re-run unnecessarily for a planning-only decision. Write `plans/closure/windows-native-sandbox/022-status.md` and update registry. Maintainer approval still required for any production ADR or backend enrollment.
