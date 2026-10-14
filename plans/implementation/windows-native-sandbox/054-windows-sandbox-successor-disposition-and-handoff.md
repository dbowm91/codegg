# Windows Native Sandbox C054 — Next-candidate disposition and bounded handoff

Status: closed — no candidate qualifies; C049–C053 dispositions recorded in `plans/closure/windows-native-sandbox/054-status.md`
Class: invariant (evidence/authorization handoff)
Baseline: main `39f149a15ef529d66787d1d11551872ea22ec384` (reviewed 2026-10-14).
Source roadmap: plans/subsystems/windows-native-sandbox-roadmap.md (post-C043/C044 successor).
Authority: ADR-0004, ADR-0016, accepted bounded ADR-0029, proposed ADR-0030; no production admission from this plan.
Dependencies: C049 + C050 + C052 closure hard (positive or negative); C053 source-level delta disposition hard; C051 only if C050 write-positive and stage chosen. Independent security/provisioning/staging ADRs remain separate hard gates for production.
Closure target: plans/closure/windows-native-sandbox/054-status.md.

## Objective

Make one falsifiable choice among (1) a broad-read/write-confined disposable-root native candidate, (2) dedicated-principal/two-hop launch as an independently investigated substrate, (3) new-revision BaseContainer strict candidate, or (4) no candidate. Prevent indefinite repeats of negative C011–C044 probes. A positive test-only research result never registers a Windows backend or changes requested strict profile semantics.

## Work packages

A. Collect exact-SHA closure reports from C049–C053. Build an operation-by-operation matrix: actual child first instruction; host-read confidentiality; allowed stage writes; outside modifications; alias/hardlink/ADS/reparse, inherited/duplicated handles and privileged IPC; descendants/Job; process-memory access; network sockets/WFP; original-object mutation; teardown/recovery; native Git/Python/Cargo compatibility; Windows editions/builds and toolchain pins. Every result must be pass, fail, unsupported, blocked or not-run and traceable to an original log. Treat same-file positive controls as mandatory for any denial claim.

B. For a broad-read candidate, explicitly require accepted ADR-0030 **before** public profile implementation or launch. If a real staging/promote path is needed, request a separate accepted staging architecture and hand off to existing conditional C046; C051 alone cannot promote real data. For dedicated-principal offline/mediated egress require separate provisioning ADR and existing C045. For strict MXC, require a separate production-backend ADR and independent full read/IPC matrix. Reject any candidate needing original-existing-file ACL/MIC edits under current policy.

C. Reconcile process-tool coverage audit C047 against test-runner, local MCP and LSP direct spawn paths. Specify the concrete integration plan that must route or deny each untrusted process while retaining trusted in-process file tools, user-controlled PTY, human promotion and remote-node authority as separately defined surfaces. Do not implement production process routing before backend authorization or allow tool-name/regex restrictions to pretend to enforce OS policy.

D. Issue an explicit yes/no recommendation for an independent security review and a proposed production-admission ADR. Existing C048 assumes strict-positive C043/C044; if the selected candidate is *broad-read only*, do not silently reuse that dependency or mark C048 ready. Instead propose an amendment/new profile-specific gate once ADR-0030 is accepted. If no candidate qualifies, close negative/no-admission and retain Windows Required unavailable. Do not rewrite C011/M004–M006 or C038/C043/C044 histories.

## Acceptance / closure

Produce a signed-off, source-pinned evidence matrix, dependency disposition for C045/C046/C047/C048, explicit ADR decision owner request, supported-host exclusions, untested gaps, and operator-visible guarantee wording. Confirm the release/test-support guard and capability negotiation cannot report a test-only candidate as production Enforced. Documentation/registry updates and source-level integrity guards required; no new permanent CI lane, perf gate, product profile, installer, schema or backend registration. Any claimed Windows native security property must have actual child-level evidence from its exact revision. If all candidates are negative, the correct outcome is a completed **no-candidate** disposition, not a new open-ended corrective plan.
