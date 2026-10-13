# Windows ADR identity migration

Status: completed identity-only reconciliation before PR #111 integration
Date: 2026-10-13
Source branch snapshot: `work/windows-native-sandbox-foundation` at
`631ae9488f251ac70e91b9c8299d2c1c3bff83fa`
Refreshed feature head before migration: `33e73915bb1789511d3fb7829141de05870d2325`
Mainline snapshot reviewed: `origin/main` at
`b9ba494c1b21638c2e2f660301ba207ce8859682`
Common base: `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`

## Decision identity mapping

| Original Windows identity | Canonical identity | Status and authority |
|---|---|---|
| `ADR-0021-windows-privileged-launch-vs-guest-isolation.md` | `ADR-0023-windows-privileged-launch-vs-guest-isolation.md` | Rejected; no admission, neither test-only route selected |
| `ADR-0022-windows-two-stage-native-launch-and-mxc-requalification.md` | `ADR-0024-windows-two-stage-native-launch-and-mxc-requalification.md` | Accepted for bounded test-only C032/C033 feasibility and C034 comparison; no production mechanism |
| `ADR-0023-windows-mxc-helper-diagnostic-corrective.md` | `ADR-0025-windows-mxc-helper-diagnostic-corrective.md` | Accepted solely for bounded test-only C035 evidence recovery; no production candidate |
| `ADR-0024-windows-mxc-capture-and-handle-semantics-review.md` | `ADR-0026-windows-mxc-capture-and-handle-semantics-review.md` | Accepted solely for bounded test-only C036/C037 and C038 review; no production selection |

Mainline TUI ADR-0021 (`native-editor-completion-read-and-local-edit`) and
ADR-0022 (`daemon-authorized-workspace-directory-listing`) retain their
identities and content. They remain separate decisions from Windows ADR-0023
through ADR-0026.

## Provenance and change boundary

Each file under `plans/archive/windows-adr-id-aliases/` is an exact copy of the
corresponding original file from source snapshot
`631ae9488f251ac70e91b9c8299d2c1c3bff83fa`. SHA-256 fingerprints are:

| Archived alias | SHA-256 |
|---|---|
| `ADR-0021-windows-privileged-launch-vs-guest-isolation.md` | `24701bccb5f0c6284c8a95b9c2891ac8a9518c6ef3b143fa4ee6a69f33cce8b9` |
| `ADR-0022-windows-two-stage-native-launch-and-mxc-requalification.md` | `27025bbca21c755c6a2518fb3d679b2d05dac5b855c71b78c89cfe6aee068859` |
| `ADR-0023-windows-mxc-helper-diagnostic-corrective.md` | `49cfa33fe949fa570e5fdb52d1b7aed3b07011726b55c8c32325478e5638784a` |
| `ADR-0024-windows-mxc-capture-and-handle-semantics-review.md` | `61c250868ad59c4024b662aecc4174d9624b0d8bc1dbc19c1a4e620a86592a55` |

Canonical copies preserve each decision and status; edits are limited to the
identity/title, Windows ADR cross-references, and canonical path references.
This migration changes no architecture decision, security finding, closure
outcome, capability, or authority. In particular, C029/C034/C038 remain
no-admission, C030/C031 remain terminally blocked, and Windows production
`SandboxRequest::Required` remains unavailable. The historical filenames are
not active numbered ADRs and are excluded from the active namespace check.
