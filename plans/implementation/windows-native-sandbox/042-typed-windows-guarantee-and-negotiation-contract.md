# Windows Native Sandbox C042 — Typed guarantee contract and fail-closed negotiation

Status: implemented; closed — internal contract only (closure `plans/closure/windows-native-sandbox/042-status.md`, implementation `edde2bebe`); public broad-read profile still blocked on ADR-0028 ratification
Class: invariant
Baseline: main a7b116af3243623099b80cc200b843a36b0ce3d2 (refresh exact SHA before implementation).
Source roadmap: plans/subsystems/windows-native-sandbox-roadmap.md (ADR-0027 successor addendum).
Authority: ADR-0004, ADR-0016, ADR-0027 (bounded test-only), proposed ADR-0028 is NOT product authorization.
Dependencies: ADR-0016 accepted; ADR-0027 bounded scope; C041 interface/soft
Closure target: plans/closure/windows-native-sandbox/042-status.md.
General verification: git diff --check; python3 scripts/check_adr_integrity.py; python3 scripts/check_sandbox_contract.py; python3 scripts/check_execution_ownership.py; bash scripts/verify.sh quick. Use exact-SHA native Windows runs for all platform/security claims and record unrun limitations.

## Objective and existing evidence

The sandbox registry currently exposes only Landlock and Seatbelt; src/security/sandbox.rs differentiates Unconstrained/Contained/Unavailable and records an Enforced outcome, but a future Windows backend needs independent read/write/network/process/metadata-mutation claims. src/managed_process.rs owns dispatch; approval and parent/subagent ceilings live in codegg-core. We need the contract before a candidate can be integrated without weakening profiles.

## Work packages

A. Inventory SandboxProfile, SandboxEnforcement, SandboxLaunchSpec, ExecutionPolicySnapshot, ApprovalRouter, tool routing, status and audit. Create an internal typed requirement and observed-capability model: read scope and explicit denials, write scope, network egress, Job/descendants, host metadata mutation, backend/tier/provenance, diagnostics. Specify exact semantics for unknown, not-run, unavailable, failed, observed-denied and observed-enforced. Avoid serializing secrets, SIDs, raw credentials or unnecessarily exposing host paths.
B. Implement a pure negotiation function that accepts immutable requested policy + host inventory/attested candidate capabilities and either produces an exact satisfied contract or refuses. No partial host read claim may satisfy strict ReadOnly/WorkspaceWrite; network Unrestricted cannot satisfy Offline; invalid/unknown tier or fallback cannot satisfy host-mutation prohibition. Never convert unavailable to FullHost or broaden child authority.
C. Preserve Unix status frames and old serialized configs. Introduce versioned internal Windows helper status only if needed, with integrity/provenance checks and rejection of stale/target-controlled claims. No product-visible profile addition unless ADR-0028 separately accepted and migration/approval tests land.
D. Write exhaustive table/property tests: strict->broad profile selection denied, parent ceiling maintained, tier downgrade denied, WFP absent => no offline claim, Job-only => no filesystem claim, test-only helper cannot report Enforced, malformed and forged status refused. Update architecture/security.md and process-tool ownership references after code lands.

## Verification and acceptance

Focused approval, permission, sandbox, tool-execution and managed_process suites on Linux; Windows native capability-probe contract (even absent backend) on eligible runner; unchanged Landlock/Seatbelt regressions. Exact requested-vs-obtained audit results tested through actual daemon dispatch using a mocked backend that never starts an untrusted target. No backend registration, product admission, persistent host changes, public schema change or authority change in this milestone. A later public broad-read mode remains blocked on ADR-0028.
