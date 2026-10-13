# Windows Native Sandbox C047 — Authoritative process-tool coverage and adapter seam

Status: blocked on C042 typed contract; implementation split into audit-safe and backend-gated slices
Class: invariant
Baseline: main a7b116af3243623099b80cc200b843a36b0ce3d2 (refresh exact SHA before implementation).
Source roadmap: plans/subsystems/windows-native-sandbox-roadmap.md (ADR-0027 successor addendum).
Authority: ADR-0004, ADR-0016, ADR-0027 (bounded test-only), proposed ADR-0028 is NOT product authorization.
Dependencies: C042 hard; C043 or C044 evidence needed only for restricted execution integration; separate backend admission decision hard for production activation
Closure target: plans/closure/windows-native-sandbox/047-status.md.
General verification: git diff --check; python3 scripts/check_adr_integrity.py; python3 scripts/check_sandbox_contract.py; python3 scripts/check_execution_ownership.py; bash scripts/verify.sh quick. Use exact-SHA native Windows runs for all platform/security claims and record unrun limitations.

## Objective

Ensure the same OS capability requirements and process ownership apply to all untrusted native executables launched via Codegg, not only the bash tool. Keep daemon trusted built-in file mutations under their existing authorization rather than falsely advertising an OS boundary for in-process actions.

## Work packages

A. Inventory all spawn surfaces in src/managed_process.rs, src/tool/bash/process.rs, src/scheduler/executors.rs, test runner, interactive processes, LSP, MCP, plugins, Git, Python and remote execution adapters. Classify: daemon-trusted native file operation, controlled untrusted process, privileged setup, independent remote node. Record inherited rights, environment, cwd and host execution identity for each.
B. Expose one platform launcher contract from ManagedProcessService for a future admitted backend; ensure JobSubmissionService job provenance, immutable policy snapshots, audit/status receipts, cancellation and process-tree lifecycle are enforced for each native untrusted surface. Route all process launches to canonical authority or explicitly exclude a surface from the advertised sandbox coverage with testable denial. No shell regex or tool-name allowlist substitute.
C. When (and only when) an independently qualified backend is separately admitted, wire exact policy negotiation and private launch-status verification to the real caller. Ensure no child starts before policy+Job success, no FullHost fallback, no backend switch midway through an attempt, and no agent-controlled privileged helper path.
D. Add negative tests for subprocess indirection (shell -> Python -> Git, MCP -> process, LSP -> subprocess, plugin -> process), symlinked cwd, cancelled jobs, malicious command quoting and environment-provided tooling. Test trusted file tools remain approval-controlled but are not mislabeled sandboxed.

## Acceptance / stop

The audit slice may close with explicit uncovered items, but production integration cannot close until every advertised untrusted-process entry point is either controlled or refused. Preserve Linux/macOS containment, remote node ownership, human shell explicit promotion and all existing approval gates. No schema migrations unless justified, and no release claim until C048 review.
