# Windows Native Sandbox C029 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/windows-native-sandbox/029-broker-vs-guest-architecture-decision.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0021-launch-authority-decision-gate`

Repository baseline reviewed: `c94180d8` (2026-10-13)

Implementation commits or pull requests:

- Documentation-only architecture disposition and planning-state update in this closure commit.

## 1. Executive finding

C029 is closed with the maintainer's explicit **no-admission** decision. Neither the privileged broker nor native Windows guest feasibility route is selected. Windows constrained execution remains unavailable; `SandboxRequest::Required` remains fail-closed. This closes the architecture decision milestone, not the underlying Windows isolation requirement.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Compare broker, guest, and unavailable default | `research/windows-native-sandbox/adr-0021-authority-options.md`; ADR-0021 alternatives and consequences | pass | Broker IPC/privilege and guest bridge/edition/concurrency risks remain unqualified. |
| Use C028 native phase evidence | Run `38396698509`; `plans/closure/windows-native-sandbox/028-status.md`; `research/windows-native-sandbox/c028-launch-trace.md` | pass | Four token-child cases timed out before user entry after token, session, Job, and suspend-count checks. No child read denial was proved. |
| Record a maintainer disposition | User decision received 2026-10-13; `plans/adrs/ADR-0021-windows-privileged-launch-vs-guest-isolation.md` | pass | No admission; neither test-only route selected. |
| Keep unselected candidates out of execution | C030/C031 implementation plans; subsystem roadmap; registry | pass | Both are terminally blocked and require a new explicit architecture decision to reconsider. |
| Preserve production fail-closed state | ADR-0021 and subsystem roadmap | pass | No backend, privileged install, ACL change, production `Required`, or filesystem/network `Enforced` claim. |

## 3. Production implementation evidence

No production code, service, guest, installer, protocol, schema, account, ACL, or daemon IPC changes were made or authorized. The only outcome is an architecture decision and corrected planning state. No Windows constrained execution capability is claimed.

## 4. Verification executed

### Commands run

```bash
git diff --check
python3 scripts/check_sandbox_contract.py
python3 scripts/check_execution_ownership.py
bash scripts/verify.sh quick
```

### Results

All listed repository checks passed, including `verify.sh quick`. No native broker or guest experiment was run. C028's hosted evidence is used only for the scope it establishes: child creation and token/Job/session/suspend checks succeeded, followed by a timeout before user entry. It does not establish filesystem isolation, a usable child, or a preferred successor route.

## 5. Invariant review

- Windows `Required` remains fail-closed and unavailable.
- No default FullHost fallback, host ACL mutation, privileged service, or agent-controlled elevated command path is introduced.
- No filesystem or network `Enforced` guarantee is claimed.
- C011/M004–M006 and C015–C018 remain blocked; historical closures remain unchanged.
- C030 and C031 are terminally blocked as unselected alternatives.

## 6. Failure and recovery review

No executable lifecycle was added. Missing privileged VM and guest evidence are treated as reasons for no admission, not as success. There are no new resources to recover, persist, or clean up.

## 7. Migration and compatibility review

No schema, protocol, configuration, installer, account, or ACL changes. Existing Windows behavior is unchanged and constrained execution remains unavailable.

## 8. Security review

The unavailable default avoids introducing unqualified privileged IPC, service identity, guest bridge, host data transfer, and lifecycle boundaries. No security boundary is asserted beyond the existing fail-closed behavior.

## 9. Documentation and operations

- Rejected ADR-0021 with the explicit no-admission rationale.
- Recorded C029 closure and the terminal unselected state of C030/C031 in the registry and Windows native sandbox roadmap.
- Updated the ADR comparison research disposition.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | Windows native constrained execution remains unavailable; no restricted child read-denial evidence exists. | Native Windows tools cannot be admitted under the requested constrained boundary. | Reconsider only through a new explicit architecture decision supported by fresh qualified evidence and required operational hosts. |

## 11. Roadmap disposition

Milestone closed. No next feasibility plan is eligible. C030 and C031 remain terminally blocked and unselected; this decision does not reopen C025/C026, qualify M004, or grant production admission.

## 12. Registry updates

The registry and subsystem roadmap record ADR-0021 as rejected with no admission, C029 as closed, and C030/C031 as terminally blocked. Windows constrained execution and downstream security milestones remain unavailable/blocked.
