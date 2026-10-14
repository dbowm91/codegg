# Windows Native Sandbox C049 — Closure Status

Status: closed (decision-ready dossier only)
Source implementation plan: `plans/implementation/windows-native-sandbox/049-broad-read-compatibility-decision-dossier.md`
Source subsystem roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md`
Repository baseline reviewed: `39f149a15ef529d66787d1d11551872ea22ec384`
Implementation commits: `cb614bb4` — close C049 decision dossier (rebased SHA)

## 1. Executive finding

C049 produced a bounded threat/guarantee matrix and a ratification packet for
ADR-0030. It requests an explicit maintainer accept/reject decision. It does
not accept ADR-0030, add a profile, change public behavior, or admit a Windows
backend.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Separate confidential host reads, writes, aliases, host mutation, process/IPC, network and FullHost | `research/windows-native-sandbox/c049-broad-read-decision-pack.md`, threat matrix | Complete as a proposal; unknowns stay explicit |
| State profile name, risk, opt-in, migration and parent/subagent ceiling | Same dossier, proposed contract | Complete; no public schema change authorized |
| Compare WRITE_RESTRICTED stage, original ACL, dedicated principal and strict candidate | Same dossier, candidate disposition | Complete; only disposable-root test study remains eligible |
| Reconcile C042 typed negotiation and tests | Same dossier, C042 section | Complete; no code changes |
| Request maintainer decision | Same dossier, decision record | Requested; response remains outstanding |

## 3. Production implementation evidence

No production files changed. ADR-0030 remains proposed; ReadOnly and
WorkspaceWrite semantics, Windows fail-closed behavior, and the explicit
FullHost boundary are unchanged.

## 4. Verification executed

- `python3 scripts/check_adr_integrity.py` — passed; active ADR IDs are unique and local paths resolve.
- `python3 scripts/check_sandbox_contract.py` — passed.
- `git diff --check` — passed.
- Windows native test — not run; this milestone is document-only.

## 5. Invariant review

The dossier states that WRITE_RESTRICTED does not prove confidential reads,
and that stage creation, Job ownership, proxy variables, or vendor labels do
not independently prove containment. No strict profile or approval ceiling
was weakened.

## 6. Failure and recovery review

No runtime behavior changed. The proposal requires unsupported versions and
missing capabilities to refuse before target entry; no retry or fallback is
introduced.

## 7. Migration and compatibility review

No migration is implemented. The proposed future contract requires old strict
profile representations to retain their meaning and unknown versions to fail
closed pending explicit authorization.

## 8. Security review

No new security boundary is claimed. Host reads may include user-readable
credentials under a broad-read profile; network remains unrestricted absent
independent WFP/socket proof. C011/C013/C014/C038/C043/C044 historical
closures are not edited.

## 9. Documentation and operations

Added the decision dossier at
`research/windows-native-sandbox/c049-broad-read-decision-pack.md`. The
maintainer decision is still required before public profile implementation.

## 10. Unresolved findings

- Medium: ADR-0030 ratification is outstanding. No public profile or backend
  work may rely on this packet until maintainers record the decision.
- Low: C050/C052/C053 native evidence remains uncollected; those are separate
  milestones and do not block this document-only closure.

## 11. Roadmap disposition

C049 closes as a decision-ready artifact. C050 and C052 remain independently
eligible for bounded test-only research; C053 remains source-review eligible.
No production candidate is admitted.

## 12. Registry updates

Mark C049 closed and link this record. The dependency audit finds that no
blocked plan is unblocked by C049: C051 requires positive C050; C054 still
requires C050, C052 and C053 closures. The maintainer ADR-0030 decision remains
an explicit open item.
