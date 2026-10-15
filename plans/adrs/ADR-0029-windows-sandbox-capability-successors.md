# ADR-0029 — Windows sandbox capability-stratified successor research

Status: accepted — **bounded test-only successor architecture and contract preparation only; no production backend/profile admission**
Date: 2026-10-13
Decision owners: Codegg maintainers
Baseline: main at a7b116af3243623099b80cc200b843a36b0ce3d2; refresh before implementing any handoff.

Authority: ADR-0004 (approval and sandbox ceilings), ADR-0016 (platform-owned sandbox launch), ADR-0019/0020 (negative bounded predecessors), ADR-0023 (privileged broker/guest not selected), ADR-0024–0026 (test-only successor evidence), sections 4/17/27/29 of plans/000-long-term-specification.md and section 8 of plans/001-terminology-and-domain-model.md.
Roadmap: plans/subsystems/windows-native-sandbox-roadmap.md
Research: research/windows-native-sandbox/2026-10-13-harness-successor-comparison.md

## Context and evidence

Windows M001–M003 are closed: launch ownership, suspended process/Job supervision, shell dispatch and policy inputs. Production SandboxRequest::Required still fails closed: src/security/sandbox/backend.rs registers only Landlock/Seatbelt. C011 direct/junction/hardlink credential reads, C013 original-object ACL/descriptor mutation and C014 Git /dev/null startup incompatibility rule out silently restarting the original-tree AppContainer grant path. C032 two-hop restricted child never reached user entry. C033/C035 showed narrowly successful MXC BaseContainer read denials and developer tooling, but C036 lacked correlated denial capture and confirmed timeout cleanup; C037 lacked descendant/explicit-transfer proof; C038 closed no candidate. Those records remain negative/unqualified, not superseded success.

Major harness comparison: Codex and Anthropic Windows sandbox-runtime use dedicated principals, two-stage restricted execution, process ownership and identity-based network control; trade-offs include original-host ACL grants and broad read scope. Gemini's native low-integrity path can leave integrity labels on disk. Grok Build documents Unix kernel backends and a more permissive workspace profile, not an independently verified Windows backend. Microsoft MXC/ProcessContainer is a viable *candidate* for supported Windows 11 hosts, but tier, fallback policy, SDK version and actual child behavior must be measured rather than inferred from other harnesses.

## Decision

1. Adopt a **capability-stratified research and implementation sequence**. Treat filesystem reads, writes, host-secret access, network egress, process-tree containment and host-object mutation as distinct requested/observed dimensions. Backend selection is a meet of all requirements, never a preference-based downgrade. Preserve existing SandboxProfile::ReadOnly / WorkspaceWrite semantics and all policy snapshots, parent ceilings, independent ApprovalMode and explicit FullHost escalation.
2. Authorize C041 mainline verification/build guard, C042 internal non-public guarantee contract, C043 test-only MXC BaseContainer adapter and C044 test-only compiled two-hop native-runner diagnosis. Test-only outcomes cannot register any backend or enable production Windows Required. An independent future production-admission decision is mandatory.
3. Prefer MXC BaseContainer as the **first eligible research candidate** on supported Windows 11, not as an automatically selected production default. Pin SDK/helper/runtime ABI, exact host tier and OS edition/build/architecture. Require a proven *no-original-host-DACL-mutation* path. Verify that the pinned API exposes, honors and reports disabling fallback DACL mutation; if missing or ambiguous, stop rather than assuming compatibility. Do not introduce SDK MSRV uplift into the Rust 1.89 workspace solely for a spike.
4. Retain dedicated-principal/two-stage launch as a parallel research candidate. Replace PowerShell-dependent child-entry inference with a small compiled native first-instruction probe. Restricted token creation, identity, loader startup, target operation and process cleanup are independent test results. No durable account, original-user ACL grant or automatic elevation follows from this ADR.
5. WFP offline/mediated network capability, persistent account provisioning, staged-workspace promotion, public broad-read profiles and release admission require **separate explicit decisions and affirmative evidence**. ADR-0030 is the proposed public broad-read profile decision; it is not approved by this ADR.
6. Canonical ManagedProcessService / JobSubmissionService / daemon approval and audit remain the only local execution owners. Shell syntax/regex filtering is not a substitute for OS containment; avoid obstructing ordinary Git/Python/Cargo tools with superficial deny rules.
7. C011/M004–M006 and C015–C018 remain blocked, C030/C031 terminally unselected, and C029/C034/C038 no-admission unchanged. No upstream negative closure may be rewritten.

## Alternatives considered

A. Retain absolute strict read confinement for all routine workflows, pursuing C011 repeatedly: rejected as a default development path because it conflates broader read allowlists with strong confidentiality and repeats negative evidence.

B. Accept a write-only sandbox under the existing WorkspaceWrite name: rejected; changing established guarantees without an explicit new profile/approval decision is unsound.

C. Make MXC mandatory on every Windows host: rejected because tier and host coverage are not uniform, and C038's lifecycle/handle failures remain unqualified.

D. Select original-tree ACL grants or low-integrity relabeling immediately: rejected because host mutation violates the existing safety invariant and creates persistent side effects requiring explicit separate consent/decision.

E. Make WSL/VM mandatory: rejected for normal Win32 native tooling; retained as separately scoped, optional stronger isolation or fallback future architecture.

## Contract and failure semantics

Requested requirements are immutable for an attempt; backend capability negotiation is a proof check, not a permission grant. Each result must distinguish observed enforced, observed denied, unsupported, unavailable, setup failed, not run and unknown; never infer read restriction from WRITE_RESTRICTED, a Job, successful launch, a capture log, or path normalization alone. A backend's inability to satisfy a requirement returns Unavailable before untrusted target entry. No fallback to FullHost occurs without a separate explicit authorized escalation.

Tests must verify effective backend/tier, actual child entry, same-file broad-read baseline, alias/hardlink/ADS/reparse/device/UNC where applicable, write/move/delete outside scope, inherited and duplicated handles/IPC, Job descendants, bounded timeout/cancellation, process exit proof, original descriptor/content integrity and run provenance. Distinguish missing telemetry from a demonstrated read leak, but do not qualify unsupported monitoring if it is promised. Network=Unrestricted unless WFP or equivalent independently proves a narrower claim.

## Compatibility, migration, operational and security effects

No product config, public schema, service, installer, production registry, existing SandboxProfile or historical status changes. Proposed future compatibility profile is explicitly separate in ADR-0030. Windows-native test-support is not a release feature. Test-only operations use disposable synthetic fixtures, bounded profiles/handles, narrow inherited stdio, and exact-SHA native reports. Real user worktrees, credentials and privileged host endpoints are out of scope. Windows edition/architecture gaps stay unavailable, not automatic host access.

## Verification and stop criteria

Plans must run repository guards and focused Unix regression tests as appropriate, plus exact-SHA native tests for Windows-specific security claims; hosted workflow success is evidence collection, not proof of qualification. C043/C044 close positive, negative or blocked after bounded reproducible attempts. C047/C048 cannot admit production until a later accepted admission decision; do not silently reinterpret this bounded ADR to authorize release.

## Supersession

None. Complements ADR-0016 and preserves historical negative/disposition records; it supersedes only the informal idea that a single experimental candidate must automatically become the next production backend.
