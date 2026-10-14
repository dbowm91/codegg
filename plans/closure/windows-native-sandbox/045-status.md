# Windows Native Sandbox C045 — Closure Status

Status: blocked
Source implementation plan: `plans/implementation/windows-native-sandbox/045-optional-identity-wfp-and-provisioning-lifecycle.md`
Source subsystem roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0027-capability-stratified-successor-handoff-post-pr-111-merge-2026-10-13`
Repository baseline reviewed: `76e9e9c9`
Implementation commits: none (provisioning explicitly forbidden until candidate + separate ADR)

## 1. Executive finding

C045 is blocked by design. The plan forbids implementing provisioning
until a qualified principal backend exists plus a separately accepted
operator/provisioning ADR with explicit consent. Neither exists — and
the backend gate has now failed closed rather than merely unrun: C044
is closed negative this revision (run `38468468249`: static native
child alive 30 s without entry, no strict-positive), C043 is closed
negative (pinned MXC init/handle gaps reproduce), no provisioning ADR
was proposed or accepted, and no consented disposable Windows VM ran
beyond the CI fixtures above. No CLI onboarding, WFP rules, identity
lifecycle, or network proof was built, and none is claimed.
Filesystem-only containment needs no WFP, and no network claim is made.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Qualified C044/approved principal backend | C044 closed negative + C043 closed negative (this revision) | blocked | Hard gate failed closed, not merely unrun |
| C042 contract | C042 closed at `edde2bebe` | pass (predecessor) | Network-offline dimension ready when needed |
| Explicit provisioning ADR + operator consent | no ADR, no consent | blocked | Plan forbids implementation without both |
| Disposable Windows VM lifecycle proof | not run | blocked | No host |
| WFP offline/mediated socket/descendant negatives | not run | blocked | Never infer from account name or installer success |

## 3. Production implementation evidence

None. No CLI, dry-run, DPAPI/secret, provisioning, firewall, or
network-contract code. The C042 `with_network_offline` + proven-WFP
rule is the future integration point, unexercised.

## 4. Verification executed

```bash
git diff --check
python3 scripts/check_adr_integrity.py
```

Local: no C045 files; guards pass. No Windows/network execution
attempted (honest not-run). No offline claim is made anywhere.

## 5. Invariant review

Preserved by inaction: agent is not a privileged broker; no
machine-wide firewall rules; no permanent user/firewall alteration;
network stays `Unrestricted` truthfully; no filesystem proof borrowed
from network work or vice versa.

## 6. Failure and recovery review

Not applicable (nothing provisioned, no rollback surface created).

## 7. Migration and compatibility review

None.

## 8. Security review

Blocking is the secure disposition: no installer, no credential
handling, no log/secret exposure, no WFP bypass via proxy env
(nothing to bypass — no mediated mode exists).

## 9. Documentation and operations

This closure record only. Future work needs a provisioning ADR first,
then the C045 work packages A–D on a disposable host.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | No qualified principal backend | Nothing to provision WFP for | Strict-positive C044 (or equivalent) + independent review first |
| medium | No provisioning ADR/consent | Implementation forbidden | Draft and accept operator/provisioning decision separately |

## 11. Roadmap disposition

Milestone blocked (conditional). The backend gate is now settled
negative on both lines (C043/C044 closures, this revision), so no
provisioning ADR has a qualifying backend to premise on. MXC
filesystem-only qualification (C043) explicitly did not need it.

## 12. Registry updates

Covered by the single line-closure commit (see C041 §12): C045 row
stays `blocked (conditional)` with unchanged blockers (qualified
C044/principal + new provisioning ADR + consent). Implementation plan
`045-*.md` status stays `blocked`. Unblock audit: nothing newly ready.
