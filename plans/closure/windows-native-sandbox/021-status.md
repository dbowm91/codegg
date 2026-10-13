# Windows Native Sandbox C021 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/windows-native-sandbox/021-dedicated-user-native-feasibility.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0020-independent-successor-feasibility-2026-10-12`

Repository baseline reviewed: `71b8508fd9b54cb1d006b2ad79426532cb19400f` (`work/windows-native-sandbox-foundation`)

Implementation commits or pull requests:

- `5e052fe3` — add the isolated Windows dedicated-principal feasibility candidate and workflow.
- `7a22e651` — use PowerShell ACL setup supported by the hosted image.
- `013003b3` — place host and stage fixtures on the same volume for hardlink coverage.
- `bda33967` — launch the child with the standard account's profile environment rather than the administrator's environment.
- `a0ffec78` — launch a bounded script file and persist the Job kill-on-close limit.
- `c07cf166` — test whether an inheritable synthetic secret handle reaches the child.
- `0654950b` — probe a host-created named pipe from the child.
- `71b8508f` — add a separate sibling-secret read probe.
- Hosted Windows run `38332871333`, job `115086345853`, passed on exact SHA `71b8508fd9b54cb1d006b2ad79426532cb19400f`.

## 1. Executive finding

The bounded dedicated-principal feasibility candidate passed its running-child matrix on a disposable GitHub-hosted Windows Server 2025 VM. A newly created standard local account ran Git, Python and Cargo; stage writes succeeded; direct, sibling, junction, case-alias, hardlink and ADS secret reads were denied; outside create, move and delete were denied; the child could not use the parent's inheritable secret handle or connect to the operator-created named pipe; and its descendant was terminated when the Job handle closed. Host and sibling fixture DACLs and secret hashes were unchanged, and the account and fixtures were removed.

This is positive feasibility evidence for the tested dedicated-principal design. It does not qualify concurrent workspaces, abrupt runner termination, interrupted provisioning recovery, restart behavior, the installed runtime matrix, or network isolation. No production identity, firewall, sandbox API or admission behavior was added. Production Windows `SandboxRequest::Required` remains unavailable.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Distinct standard principal and Job ownership before execution | Run `38332871333`, `C021_REPORT` | pass | Child SID matched the new local account SID; `child_is_administrator=false`; suspended creation was assigned and verified inside the kill-on-close Job before resume. |
| Allowed stage operation and developer tools | Run `38332871333`, child result | pass | Stage write, Git init/status, Python 3.13.16 and Cargo version commands succeeded. PowerShell was the child interpreter. |
| Original host, sibling and path-alias reads denied | Run `38332871333`, child result | pass | Direct, separate sibling, junction, case alias, hardlink and ADS reads all returned `denied`. |
| Outside mutations denied | Run `38332871333`, child result | pass | Create/write, move and delete against the protected host fixture returned `denied`. |
| Ambient handle and IPC probes | Run `38332871333`, child result | pass | An inheritable handle opened by the administrator for the synthetic host secret was not usable in the child. The child could not connect to the operator-created named pipe. |
| Descendant termination and owned-resource cleanup | Run `38332871333`, report and cleanup lines | pass | The descendant was gone after Job close. The disposable account, host fixture, sibling fixture and stage were removed. |
| Original fixture integrity | Run `38332871333`, DACL/hash fields | pass | Host and sibling DACL SHA-256 values were unchanged at `7FF5BB47024F511A65FB48E2FAE1824EAABF505A1604CB63EE39BBF8098B10FF`; host secret hash remained `AD6919E8983237D691E45F05C7DF2E8D9CB600C0782391DCCB954286C7BC290C`; sibling secret hash remained `B4C77A28E860E13A3D31755F59BE98D8852CF75B522128B6D89F89EA96DF9BB0`. |
| Network isolation | Run `38332871333`, `network_isolation` | not run | Network/offline behavior is expressly unclaimed. |

## 3. Production implementation evidence

The work adds only `scripts/windows_dedicated_user_feasibility.ps1` and its test-only GitHub Actions workflow. The script creates one randomly named account with a generated password, provisions only synthetic fixtures, starts a PowerShell child suspended through `CreateProcessWithLogonW`, assigns and verifies a kill-on-close Job, resumes the child, and removes the account and fixtures. No product code, configuration, protocol, storage, installer, workspace ACL or persistent account was changed.

## 4. Verification executed

### Commands run

```bash
./scripts/windows_dedicated_user_feasibility.ps1
git diff --check
```

### Results

- Hosted workflow `Windows dedicated principal feasibility`, run `38332871333`, passed on Windows Server 2025 Datacenter, version `10.0.26100`, build `26100`.
- Runner identity was `runnervmfi6oq\runneradmin`, an administrator. The child was a separate standard SID and not an administrator.
- The exact hosted report records all allowed/denied outcomes, DACL and file-hash comparisons, descendant cleanup, and resource removal.
- Earlier hosted attempts exposed and corrected harness issues: ACL API availability, cross-volume hardlink setup, and Job limit assignment through a nested value-type struct. Those failed attempts are not counted as candidate results; the final exact-SHA run passed.

## 5. Invariant review

- Production Windows constrained requests remain unavailable; no backend was enrolled and no `Enforced` outcome was added.
- The principal was test-only and removed by the workflow. The generated password was not emitted or persisted.
- The test changed ACLs only on synthetic fixtures it created. Host and sibling fixture descriptors and secret contents matched before and after.
- The operator's inherited secret handle was not usable in the child; the operator-created named pipe was not reachable.
- No network isolation or firewall guarantee is claimed.

## 6. Failure and recovery review

Suspended launch, Job assignment verification, normal completion, descendant termination on Job close, and `finally` cleanup of the test-owned account and fixtures passed. Missing credentials, interrupted account provisioning, abrupt runner termination, orphan recovery, stale SID reuse, concurrent workspaces and explicit cancellation races were not injected. These remain required before any production integration.

## 7. Migration and compatibility review

No database, protocol, public configuration, installer, dependency, MSRV or runtime compatibility changes were made. The probe uses the hosted Windows PowerShell and tool installations and does not assert availability on customer machines.

## 8. Security review

Only synthetic secrets and a disposable standard account were used. Direct and aliased host reads, sibling reads, outside mutations, a marked inheritable secret handle and a host-created named pipe were exercised by the running child. All reported denials came from the child result, not pre-spawn rejection. No original user ACL was edited. Network, registry, device, profile-cache and broader system-object access were not fully qualified.

## 9. Documentation and operations

- The workflow records host edition/version/build, account identity, child token identity, Job ordering, per-operation result, descriptor/hash comparisons and cleanup.
- `C021_REPORT` is machine-readable in the Actions log and step summary.
- `plans/registry.md`, the Windows native sandbox roadmap and the ADR-0020 research comparison record this disposition and its limits.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Concurrency, abrupt runner loss, interrupted provisioning, orphan recovery and cancellation races were not exercised. | The bounded feasibility result is not a production lifecycle qualification. | Require isolated multi-workspace and failure-injection evidence in any separately admitted integration plan. |
| medium | Network, registry, device, profile-cache and broad system-object access are outside this run. | No offline/network or full host confidentiality claim is supported. | Keep network unclaimed and define a bounded follow-up matrix before production consideration. |
| low | Only GitHub-hosted Windows Server 2025 and its installed toolchain were observed. | Compatibility with Windows 11/customer images and alternate tool installations is unknown. | Qualify an explicit supported host matrix in any future integration plan. |

## 11. Roadmap disposition

C021 is conditionally closed with positive bounded feasibility evidence. C022 may compare this candidate as the stronger observed path, but this record does not unblock M004/M005/M006, C011, or production Windows constrained execution. Network isolation remains separate and unclaimed.

## 12. Registry updates

- Mark C021 conditionally closed with exact run `38332871333` and its Windows Server 2025 host in `plans/registry.md` and the Windows native sandbox roadmap.
- Keep C022 eligible for comparative disposition. Keep production sandbox gates and network guarantees unchanged.
