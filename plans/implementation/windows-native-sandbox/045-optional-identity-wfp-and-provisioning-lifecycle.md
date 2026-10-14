# Windows Native Sandbox C045 — Optional principal-scoped WFP and provisioning lifecycle

Status: blocked; do not implement provisioning until candidate + separately accepted operational decision (closure `plans/closure/windows-native-sandbox/045-status.md`)
Class: capability (conditional)
Baseline: main a7b116af3243623099b80cc200b843a36b0ce3d2 (refresh exact SHA before implementation).
Source roadmap: plans/subsystems/windows-native-sandbox-roadmap.md (ADR-0029 successor addendum).
Authority: ADR-0004, ADR-0016, ADR-0029 (bounded test-only), proposed ADR-0030 is NOT product authorization.
Dependencies: C044 strict-positive or separately approved principal backend hard; C042 hard; explicit operator/provisioning ADR and disposable Windows VM operational
Closure target: plans/closure/windows-native-sandbox/045-status.md.
General verification: git diff --check; python3 scripts/check_adr_integrity.py; python3 scripts/check_sandbox_contract.py; python3 scripts/check_execution_ownership.py; bash scripts/verify.sh quick. Use exact-SHA native Windows runs for all platform/security claims and record unrun limitations.

## Objective

If a dedicated-principal backend is selected after C044 and independent review, implement operator-owned, reversible setup for network policy and identity lifecycle without converting Codegg's agent into a privileged broker. This is NOT a prerequisite for qualifying MXC filesystem-only containment, nor authority to install machine-wide firewall rules.

## Work packages

A. Specify explicit CLI onboarding, dry-run inventory, consent, admin/UAC boundary, secret encryption/DPAPI, identity ownership/versioning, no default credential, repair/rekey/uninstall and daemon restart recovery. Privileged setup may install fixed known policy but cannot execute arbitrary agent command bytes.
B. Scope WFP rules to the dedicated principal and documented processes; offline blocks IPv4/IPv6, TCP/UDP, DNS, direct sockets and descendants. Evaluate local loopback/daemon named-pipe access separately. Mediated mode must use a narrowly authorized proxy socket with fixed firewall allowances and no generic bypass via environment proxy settings; daemon provider credentials remain inaccessible.
C. Prove concurrent sessions, crashed setup, reboot, stale ACL/identity cleanup, rollback failure handling and uninstallation on a disposable Windows host. Capture signed/hashed helper provenance and port/account inventory, protect logs and all credentials.
D. Integrate observed network proof into C042 typed guarantee contract, never treat an account name or successful installer as WFP enforced.

## Acceptance / stop

No permanent user/firewall alteration without explicit separate operator consent and accepted production-provisioning ADR. Socket-level and descendant negative tests pass for each advertised offline/mediated mode; rollback after forced failures is reproducible. If cleanup is incomplete, deny new sessions and surface actionable repair. Host without permission or WFP support remains filesystem-only/unavailable as requested, never unrestricted networking mislabeled Offline.
