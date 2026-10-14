# Windows Native Sandbox C048 — Independent Windows security admission and release gate

Status: blocked; final admission only (closure `plans/closure/windows-native-sandbox/048-status.md`)
Class: capability
Baseline: main a7b116af3243623099b80cc200b843a36b0ce3d2 (refresh exact SHA before implementation).
Source roadmap: plans/subsystems/windows-native-sandbox-roadmap.md (ADR-0029 successor addendum).
Authority: ADR-0004, ADR-0016, ADR-0029 (bounded test-only), proposed ADR-0030 is NOT product authorization.
Dependencies: C042 + C047 hard; independently strict-positive C043 or C044 and new accepted production-admission ADR hard; C045 only if network modes claimed; C046 only if stage mode claimed; Windows VM/host operational
Closure target: plans/closure/windows-native-sandbox/048-status.md.
General verification: git diff --check; python3 scripts/check_adr_integrity.py; python3 scripts/check_sandbox_contract.py; python3 scripts/check_execution_ownership.py; bash scripts/verify.sh quick. Use exact-SHA native Windows runs for all platform/security claims and record unrun limitations.

## Objective

Make one separately authorized production-support decision for a concrete backend, host support matrix and profile set. A successful experimental candidate, green GitHub workflow or accepted ADR-0029 does not close this plan.

## Work packages

A. Freeze exact code/SDK/runtime/helper SHA, feature flags and OS edition/build/UBR/arch matrix, supported profile/network combinations, operator setup conditions, minimum Git/shell/native/managed toolchains, performance/startup overhead and residual limitations. Independently review boundary selection, actual child entry/token and no original host metadata mutation.
B. Execute adversarial fixture matrix against real children and descendants: same broad-read positive control, denied host reads (junction/hardlink/ADS/UNC/device where applicable), allowed scoped writes, outside modifications/rename/delete, inherited/DuplicateHandle, IPC/proxy escape, hostile parent/child, executable substitution, package cache credentials, cancellation and restart/reboot, concurrent worktrees and stale stage promotion.
C. Verify fail-closed packaging and CI on a Windows 11 eligible host and at least one separately specified alternate architecture/version where support is advertised; separate hosted Windows Server 2025 evidence. Require negative unsupported-host probe, offline actual sockets only if claimed, no hidden DACL fallback and verified cleanup after panic/kill/interruption. Re-run existing Unix Landlock/macOS Seatbelt compatibility gates.
D. Obtain independent security review and explicit maintainer production-admission ADR. Only after acceptance register the backend, update installer/diagnostics/help/docs, preserve upgrade compatibility and record production release policy. Run a real canonical daemon-to-agent-to-tool integration test plus coverage across MCP/LSP/test runners.
E. Close with exact-SHA artifact evidence, raw classification of every test (pass/fail/blocked/not_run), unresolved risks classified by severity and specific declared guarantees. Update M004/C011/M005/M006 only with newly independently accepted evidence; never retroactively change historical negative closures.

## Admission / stop

A secret read leak, uncontrolled original host-object mutation, missing required process teardown, unqualified inherited/IPC exposure or unsupported tier is a hard stop. Missing optional offline policy narrows advertised network support, not filesystem proof. No release unless all guarantees for the selected profile and supported hosts have evidence. If no candidate qualifies, terminate this lane with honest no-admission and retain Windows Required unavailable; do not invent another unbounded corrective cycle.
