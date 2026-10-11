# Windows Native Sandbox Milestone 005 — Dedicated Identity and Offline Network

Status: blocked (M004 closure required)

Repository baseline: `8e9d8b01e5c229715c8e4dea929e050b391e252c`

Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#7-milestones`

Long-term requirements:
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Applicable ADRs: `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`; `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`

Primary class: capability

## 1. Objective

Add explicit optional Windows sandbox-user provisioning and machine-enforced offline egress policy. This strengthens isolation but must not be a prerequisite for non-administrative file-only execution.

## 2. Why this milestone is ready / dependency gate

Hard: M004 closed, its filesystem principal/ACL lifecycle and status reporting stable. Installation/provisioning design reviewed; admin or isolated VM test fixture available. Do not present an empty firewall ruleset as offline containment.

## 3. Current implementation evidence

`SandboxEnforcement` distinguishes network independently from filesystem; existing Unix backends always report unrestricted network. Windows token/Job alone cannot promise network deny, and firewall identity scoping depends on a real Windows principal, not simply a synthetic restricting SID.

## 4. Invariants that must not regress

- Explicit operator/admin approval for account/firewall configuration; never use elevated agent shell.
- Distinct online/offline identity or unambiguous identity-bound policy.
- Offline mode denies direct IPv4/IPv6/TCP/UDP/DNS egress as specified, including process descendants, without blocking daemon itself.
- Loss of firewall rule or identity results in unavailable/denied, not uncontained execution.

## 5. Scope

### In scope

- Provision, validate, repair and deprovision sandbox local accounts and SID-bound firewall rules where OS allows them.
- Launch restricted process as selected user with minimal token/ACL and Job guarantees from M004.
- Probe and record actual offline enforcement, exposing online and offline modes explicitly.

### Explicitly out of scope

- Automatic elevation per tool call, global network disables, undocumented firewall ownership changes, proxy-only network enforcement, VPN/WSL tunneling, unrestricted clearnet identity leakage.

## 6. Required production changes

### Security/provisioning
A separate administrator-facing installer command/UX owns durable identities and firewall rules. Record owner/version and exact SID, avoid passwords persisted in clear text; select a documented Windows credential method (DPAPI/OS account management) and review operational risks. No UI agent command can bypass. Refuse preexisting incompatible account/rule ownership.

### Runtime
When offline selected, launch under provisioned principal, verify actual primary token identity and firewall policy before resume. Revalidate after restart and before subsequent runs; never silently select an online identity. No daemon privilege change.

### Policy/reporting
Introduce explicit offline capability separate from `SandboxProfile` filesystem semantics; do not repurpose `ReadOnly` as 'offline'. Specify operator mode/authorization/version and durable preference only if canonical owners support it. Return `network=Enforced` solely for verified offline mode.

### Docs/operations
Provide one-command inspect/repair/revoke path plus explicit risk discussion for ACL/account/firewall state.

## 7. Ordered work packages

### WP-A — Provisioning contract
Separate privileged account/rule setup with idempotent ownership records; assess privilege needs on supported Windows versions.

### WP-B — Offline identity admission
Bind requested network policy to actual user SID/token and Job, fail closed when offline identity or rules stale.

### WP-C — Firewall backend
Implement narrowly scoped outbound deny (and explicitly defined loopback) rules for sandbox identity. Ensure rule priority/profile coverage and no wider accidental host firewall change.

### WP-D — Functional and recovery qualification
Native sockets from real child/grandchild (IPv4, IPv6, DNS, UDP/TCP, localhost, proxy, alternate interface) and online-mode comparison; simulate deleted rule, disabled firewall, stale account SID, interrupted provisioning and cleanup.

## 8. Failure, cancellation, restart, and contention semantics

Failure or loss of account/rule must stop new offline child before resume. Cancellation kills Job and retains/proves rule ownership. Restart must verify live identity SID and firewall configuration before new launches. Deprovision waits for or terminates owned jobs, removes only Codegg-owned rules/accounts and never deletes unrelated similarly named entities. Concurrent online and offline jobs may not share the same unqualified identity.

## 9. Compatibility and migration

No default admin requirement for file-only M004. Windows installations without provisioning report `network=Unrestricted` (or unavailable for requested offline), never silent downgrade. Migration records ownership only for newly provisioned resources; no existing account is adopted without explicit verification/consent.

## 10. Required tests

- Non-admin unit: requested/obtained network mode, stale rule/token errors, FullHost independence.
- Isolated admin VM live: create/repair/remove identity and rules, tcp/udp DNS + IPv4/IPv6 socket blocks, loopback decision, descendants, online comparison.
- Negative: attempt alternate network interface, environment proxy, pipe-assisted egress, parent daemon egress unaffected, preexisting named resources untouched.
- Restart/fault tests: disable firewall/delete rule, crash during provisioning, stale SID reused.

## 11. Required verification commands

`cargo test -p codegg --lib security::sandbox`
`python3 scripts/check_sandbox_contract.py`
On isolated Windows VM: `cargo test -p codegg --test windows_network_offline -- --nocapture` (new target)
On isolated Windows VM: `cargo test -p codegg --test windows_identity_provisioning -- --nocapture` (new target)
`cargo fmt --all -- --check`
`bash scripts/verify.sh quick` on Unix compatible host.

## 12. Documentation updates

`architecture/security.md`, `architecture/permission.md`, operator security/installation/removal guide and plan registry.

## 13. Acceptance criteria

- Offline child/grandchild cannot create prohibited IPv4/IPv6/DNS/socket egress; the normal daemon can still network.
- Provisioning requires explicit administrator participation and is reversible and idempotent.
- Existing online/file-only mode survives; missing offline policy stops execution with precise reason.

## 14. Stop conditions

STOP when the firewall API cannot isolate per-principal robustly on targeted Windows builds, if account credentials require insecure storage, if rules could affect arbitrary host users, or if CLI elevation is invoked by agent command. Record compatibility and alternate reviewed method.

## 15. Closure evidence required

OS/version/firewall mode, independent packet/sockets evidence, rule identity ownership, account/ACL before-after state, negative failure/recovery cases, offline capability status, native CI vs privileged VM labels.

## 16. Handoff notes

This workstream is optional for initial file-only M006 security support; **not** optional if claiming network deny/offline support.
