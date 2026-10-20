# Developer Harness Lab Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/harness-lab/001-concurrent-daemon-subject-lanes.md`

Source subsystem roadmap:

- `plans/subsystems/harness-lab-roadmap.md#milestones`

Repository baseline reviewed: `7a1b9d9a1d5a28e7919e3d69851a3ae710b0afbf`

Implementation commits or pull requests:

- `d64da3d8951d743b6aae8a29f6fa3a390e6d4cb5` — isolated ACP subject lanes and harness supervisor

## 1. Executive finding

M001 is closed. The external controller creates concurrent, private subject lanes and proves their daemon identities and state roots without attaching to or mutating the default user daemon.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Private daemon, data, config, workspace and scratch state per subject | `dev/harness_lab/supervisor.py`; two-subject live smoke | pass | Lane paths are private and provider credentials are removed from child environments. |
| Identity and binary provenance | smoke receipt includes binary SHA, daemon generation and process identity | pass | Both live subjects reported distinct identities. |
| No default user-daemon attachment or mutation | before/after default identity check in live smoke | pass | Provider calls: zero. |
| Owner-scoped cleanup and bounded teardown | supervisor cleanup and smoke process inventory | pass | No orphan subject processes remained. |

## 3. Production implementation evidence

This is development-only Python infrastructure. The supervisor creates one isolated root per subject, starts the requested CodeGG binary under a private `CODEGG_DAEMON_HOME`, controls its environment and lifetime, records provenance, and removes only owned state. No production database or runtime schema changed.

## 4. Verification executed

### Commands run

```bash
python3 -m dev.harness_lab smoke --binary target/debug/codegg --subjects 2 --timeout 30 --output-dir /tmp/codegg-harness-m001
python3 -m unittest discover -s dev/harness_lab/tests -v
scripts/verify.sh quick
```

### Results

The live Linux two-subject smoke passed with distinct daemon process/generation identities, binary SHA-256 `8f2ed0bb529099a996da3a89edd856e76a656622f89b68e2e84f59a031805c62`, zero provider calls, unchanged default daemon identity and no orphan processes. The Python suite passed 12 tests. `verify.sh quick` passed locally. No hosted CI run is claimed.

## 5. Invariant review

Subject state is separate by construction; the controller owns process startup, timeout and cleanup; no provider key is inherited; and the default user daemon is not used as a subject. Identity receipts include the exact binary digest and generation.

## 6. Failure and recovery review

Startup and trial timeouts are bounded. Cleanup checks ownership before terminating processes or deleting roots. A failed subject is reported and its lane is cleaned; the controller does not infer a daemon identity from a shared path. No controller-restart continuation protocol is claimed by M001.

## 7. Migration and compatibility review

No database, protocol, configuration, or shipped-binary migration. The controller uses existing daemon root overrides and ACP interfaces.

## 8. Security review

Subject roots are created under the controller-owned output directory, provider credentials are stripped, cleanup is ownership-scoped, and subprocess lifetime is bounded. The trusted development workstation remains the execution boundary; this is not a VM or hostile-code sandbox.

## 9. Documentation and operations

The `dev/harness_lab` README documents the smoke and lane operation. The roadmap and registry record M001 as closed and the M003 dependency audit.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | No VM/container isolation | Candidate code shares the trusted workstation boundary | Retain this explicit limitation; do not describe the harness as a security sandbox. |

## 11. Roadmap disposition

Milestone closed. M003 still depended on M002 at this point in the closure sequence; M004 remained gated on M003.

## 12. Registry updates

M001 is removed from active work and listed as closed. M003 is advanced only after M002 closes in the same closure commit; M004 remains dependent on M003.
