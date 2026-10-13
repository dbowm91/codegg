# Windows Native Sandbox C038 — Post-C035 MXC Security Disposition and Stop/Proceed Gate

Status: **closed — no candidate; no production admission**
Primary class: security/architecture review
Baseline: `work/windows-native-sandbox-foundation` at `84faa0bd585e7161361bdb255f29e70cdf7ab619` (refresh before handoff).
Roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0024-capture-and-handle-semantics-follow-up`.
Authority: ADR-0024 comparison only; ADR-0022/C034 no-admission; ADR-0023/C035 negative preserved.
Canonical: `plans/000-long-term-specification.md` §§4,17,27,29; `plans/001-terminology-and-domain-model.md` §8; `plans/003-planning-process.md`.

## Objective and decision contract

Issue one evidence-based review after the final **two bounded diagnostic correctives**. Decide whether the pinned MXC candidate has enough validated *test-only* filesystem and ambient handle evidence to warrant a separate future production ADR/security-review plan or whether the current native Windows exploration should stop. This plan intentionally produces **no production feature** and is not another experiment or unlimited replay loop.

## Hard dependencies and scope

C036 `plans/closure/windows-native-sandbox/036-status.md` and C037 `plans/closure/windows-native-sandbox/037-status.md` must exist with exact code SHA, supported host build/UBR/tier, URLs for Windows native runs, machine-readable per-dimension outcomes and any not-run cases. A negative or blocked candidate still satisfies the *documentation dependency* and permits a no-admission decision. Do not reclassify C020/C032/C033/C034/C035 historical evidence.

Read-only research review against C019 actual-child oracle, C033 baseline, C035 positive child entry, C036 capture and C037 handle mechanism. No sandbox code or host ACL changes, no new service/install/network permission and no expansion of agent execution authority.

## Work packages

1. **Evidence chain and identity.** Independently inspect the raw hosted logs/JSON and links, including selected *effective* MXC backend/tier, requested policy, mode=block, actual child and descendant PID/token, stage marker, same-file ordinary positive versus MXC negative, per-operation denied/allowed vs unavailable, actual event pointer/status, and host object integrity. Missing logs are unverified, not passing.
2. **Separate three handle claims.** (A) no ambient parent handle visible to child; (B) explicitly inherited handle blocked by launcher; (C) already-valid handle cannot be used to read forbidden content. If C037 can prove only A, state A only. A numeric handle with `STATUS_INVALID_HANDLE` is not proof of B or C. Include standard handles, named pipes, `DuplicateHandle`/cross-process authority, inherited process/token handles, descendant process and IPC escape surfaces with `pass/fail/not_run/unavailable`.
3. **Capture semantics.** Determine if C035 missing JSON was a client path mismatch versus actual capture unavailability, based on pinned SDK/source and structured post-exit metadata. Capture `allow` mode cannot count. A data file from a different process/run must invalidate the claim.
4. **Developer-tool and lifecycle qualification.** Preserve proven C033 Git, PowerShell/cmd, Python descendant and offline Cargo smoke and C035 native helper entry. Explicitly itemize **not yet established**: Windows edition/tier matrix, cancellation/interruption/crash/restart, concurrent worktrees, credential/IPC boundaries beyond synthetic probes, network/WFP offline egress and stage-to-workspace promotion. Positive bounded findings never imply M005/M006 closure.
5. **Record one final decision:** (A) **research-eligible only** for a future separately accepted production ADR and independent security review, with precise prerequisite checklist; (B) **no candidate** due to observed leak or missing mandatory boundary; (C) **operationally blocked** because host/API is unavailable. A positive review does not admit MXC or reopen blocked M004/C011 automatically.

## Verification, acceptance and stopping

Document `research/windows-native-sandbox/c038-mxc-evidence-and-disposition.md` plus `plans/closure/windows-native-sandbox/038-status.md`. Verify references are real, source SHA matches native workflow, report fields match actual child evidence, no contradictions against C033/C035, and no omitted high findings. Run `git diff --check`, `python3 scripts/check_sandbox_contract.py`, `python3 scripts/check_execution_ownership.py`, `bash scripts/verify.sh quick` when applicable; disclose unavailable tests. Independent Windows security reviewer required **for a positive recommendation**, otherwise document that no positive review was claimed.

Acceptance is an exact, reproducible comparison and final proceed/stop decision with no false `Enforced`/`Required` claim or speculative reopening. Stop on missing prerequisites, any synthetic host secret leak, ambiguous token/handle provenance, stale capture document, broad original host grant, or attempts to treat a green evidence-collection job as security admission.

No production binary/Windows helper, schema, public API, installer, owner, account, privileged service, host ACL, firewall or network enforcement changes. C011/M004–M006/C015–C018 stay blocked, C030/C031 remain terminally blocked. Future production work needs another independently accepted decision.
