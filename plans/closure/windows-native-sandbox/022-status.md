# Windows Native Sandbox C022 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/windows-native-sandbox/022-feasibility-comparison-and-successor-admission-decision.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0020-independent-successor-feasibility-2026-10-12`

Repository baseline reviewed: `cfc5466a78afdb3bd00c090b15d617ebbe40e09d` (C020 evidence SHA; rebased branch retains the evidence harness)

Implementation commits or pull requests:

- C019 evidence: `2a39eb6eba4234ff865e8a92282797203dab681f`, Windows run `38330137971`.
- C020 evidence: `cfc5466a78afdb3bd00c090b15d617ebbe40e09d`, Windows run `38335783458`.
- C021 evidence: `7fb637030f63a3857c1e2afcf84ea566bb6661a1`, Windows run `38334807679`.
- This milestone changes planning, research and closure records only; it adds no production implementation.

## 1. Executive finding

C022 records **insufficient evidence for production admission**. C020 selected native Windows ProcessContainer and passed its observed direct/alias denial and descendant checks, but Python-script/native-helper initialization failed and the inherited-handle probe was unavailable. C021 demonstrated a useful, bounded dedicated-principal filesystem candidate on a disposable Windows Server 2025 runner, including separate SID/Job ordering, allowed stage/tool operations, direct and aliased read/write denials, handle/pipe denial, descendant cleanup and unchanged fixture ACLs. It did not qualify abrupt host loss, interrupted provisioning, orphan/restart recovery, cancellation races, concurrent workspaces, Windows 11/customer host compatibility, or network isolation.

Neither strict-positive candidate exists. This documentary decision closes the comparison work, but it conditionally remains open for an independent Windows security review and further lifecycle evidence before any production proposal. Keep Windows `SandboxRequest::Required` unavailable and do not advertise filesystem or network enforcement. There is no winner and no automatic transfer of C015–C018 or M004/M005 scope.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Common actual-child oracle | `plans/closure/windows-native-sandbox/019-status.md`, run `38330137971` | pass | One matrix test and six restricted-process lifecycle tests; C014 `\\.\NUL` cause remains unconfirmed. |
| MXC native candidate on eligible host | C020 closure, run `38335783458`, exact SHA `cfc5466a78afdb3bd00c090b15d617ebbe40e09d` | negative bounded | ProcessContainer/base-container selected. Deny probes and descendant result passed, but Python script and native helper failed initialization; inherited-handle result unavailable. |
| Dedicated-principal candidate | C021 closure, run `38334807679`, exact SHA `7fb637030f63a3857c1e2afcf84ea566bb6661a1` | positive bounded / conditional | Disposable Windows Server 2025 Datacenter build `26100`, UBR `33438`, 24H2, hotfixes KB5126052/KB5122871/KB5122870. Matrix passed; operational recovery/concurrency/network evidence remains absent. |
| Comparable effective boundary | C019/C020/C021 records | partial | C019 probes share vocabulary; candidates differ in identity/backend and neither completed every required operational dimension. No normalized strict-positive winner can be chosen. |
| Independent Windows security review | No reviewer report in this workstream | outstanding | Required before any future production admission or re-entry to C011/M004. |
| Successor admission and current product truth | Source and registry/roadmap updates in this closure | pass | Retain unavailable status. No production ADR, backend enrollment, `Enforced` outcome, or user-facing guarantee is authorized. |

## 3. Production implementation evidence

No production changes were made. C020 is an isolated research harness and test-only workflow; C021 is a test-only disposable-account workflow. Neither path is called by the agent, tool broker, scheduler or `ManagedProcessService`. No new trust edge, host account, firewall rule, persistent ACL, API or dependency is admitted.

## 4. Verification executed

### Commands run

```text
Review of C019/C020/C021 exact-SHA GitHub Actions reports and closure records
git diff --check
```

### Results

- C019: hosted Windows run `38330137971` passed the shared actual-child matrix (1 selected test) and restricted-process lifecycle suite (6 tests).
- C020: hosted run `38335783458` completed successfully as a report-producing workflow on exact SHA `cfc5466a78afdb3bd00c090b15d617ebbe40e09d`; the report explicitly records an unavailable inherited-handle result and a failed strict lifecycle gate. It is negative evidence, not a positive gate.
- C021: hosted run `38334807679` completed on exact SHA `7fb637030f63a3857c1e2afcf84ea566bb6661a1`; the bounded child matrix passed on Windows Server 2025. No Windows 11 equivalence is inferred.
- The comparison itself is documentation-only; no native rerun is needed to make the disposition. Repository CI or workflow completion does not replace the missing independent review and operational tests.
- `git diff --check` passed after these records were prepared.

## 5. Invariant review

- Windows production `SandboxRequest::Required` remains unavailable; no filesystem or network `SandboxExecutionOutcome::Enforced` is advertised.
- No automatic AppContainer, MXC, or dedicated-user fallback was added.
- C011/M004–M006 remain blocked; C015–C018 remain blocked following C014's negative result.
- No original user/workspace ACL or profile ACL was edited. The C021 workflow used only synthetic fixtures and a disposable account, and removed those resources.
- Network and offline guarantees remain unclaimed.

## 6. Failure and recovery review

C020's observed child could not initialize a Python script or native helper; no inherited-handle verdict is available. C021's disposable-account setup/normal cleanup and Job descendant termination passed, but missing-credential rollback, interrupted provisioning, abrupt runner loss, orphan recovery, stale SID reuse, concurrency and explicit cancellation races were not injected. Those are explicit conditions on future production reconsideration, not implicit passes. No durable product resources were created by this comparison.

## 7. Migration and compatibility review

No database, protocol, public configuration, installer, default dependency, MSRV or runtime compatibility change was made. The isolated C020 experiment used Rust `1.93.1`; CodeGG retains its Rust `1.89` MSRV. Hosted Windows Server 2025 evidence does not establish a customer Windows 11 support matrix.

## 8. Security review

The evidence supports several real child-level denial claims for each candidate's synthetic fixtures, but the gaps above prevent a complete security boundary qualification. C020's inherited handle is unmeasured. C021's broader system objects, network and operational recovery remain unqualified. No high-severity successful prohibited access was reported in C019–C021; independent review is still a required future gate. No finding or untested area is reclassified as positive evidence.

## 9. Documentation and operations

- Updated the C020 and C022 implementation-plan status lines, C020 and C022 closure records, C021 closure wording, ADR-0020 research comparison, Windows sandbox roadmap and `plans/registry.md`.
- The comparison preserves exact run IDs, evidence SHAs, Windows edition/build/servicing, candidate disposition and unsupported probes.
- Historical M007, C013 and C014 closure records remain unchanged.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | No independent Windows security reviewer has assessed the candidate traces and trust boundaries. | No production backend admission or C011/M004 re-entry is justified. | Obtain a documented independent review before creating any production integration plan. |
| medium | C021 did not qualify interrupted provisioning, abrupt host loss, orphan/restart recovery, cancellation races or concurrent workspaces. | Lifecycle and recovery behavior remain insufficient for production. | A separately approved successor feasibility/integration plan must specify and pass these cases. |
| medium | C020 Python-script/native-helper initialization failed and inherited-handle denial was unavailable. | MXC is not a positive successor candidate on the measured host. | Do not select MXC unless a new investigation first resolves child initialization and measures the handle case. |
| low | Neither candidate tested network confinement; C021 is only Windows Server 2025 evidence. | Offline guarantee and customer OS support matrix are unknown. | Keep network unclaimed and require explicit edition/build/socket qualification in any future scope. |

## 11. Roadmap disposition

C022 is conditionally closed with the explicit decision **insufficient evidence; retain Windows constrained execution unavailable**. The ADR-0020 experiment line is complete at C019–C022. A future production direction requires independent security review, a bounded recovery/concurrency plan and a new maintainer-approved decision. Neither C020 nor C021 unblocks C011/M004/M005/M006 or C015–C018. If the dedicated-principal design is reconsidered, identity foundations must be evaluated as a new M004 input while M005's network qualification remains separate.

## 12. Registry updates

- Move C019–C022 out of the dependency-ready list into recently closed/conditionally closed work, with C020 closed negative and C021/C022 conditionally closed.
- Record the comparison and its evidence under the Windows native sandbox subsystem row and current execution gate.
- Dependency audit: no registered blocked implementation plan became eligible. C011/M004–M006 and C015–C018 remain blocked; M007 remains historically deferred. No integration plan is registered because no strict-positive candidate or independent review exists.
