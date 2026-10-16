# Linux and Command ADR Identity Migration

Date: 2026-10-14

The C004 identity-only migration mapped the proposed Linux execution-isolation ADR
from ADR-0027 to ADR-0031 and the already-accepted command-syntax authority from
ADR-0028 to ADR-0032. IDs 0029/0030 are reserved by the concurrent Windows ADR
identity migration. No decision status, authorization, design choice, or
security boundary changed.

Current-main reconciliation on 2026-10-15 revealed that main had independently
accepted ADR-0031 for authenticated idle continuation. The Linux decision retains
its accepted status and substance under ADR-0033; idle continuation retains
ADR-0031 and command syntax retained ADR-0032 at that point.

Reconciliation with current main on 2026-10-16 found ADR-0032 allocated to the
accepted agent-workstream coordination decision. The command-syntax decision
retains its accepted status and substance under ADR-0034. Current canonical
identities are idle continuation ADR-0031, agent-workstream coordination
ADR-0032, Linux execution isolation ADR-0033, and command syntax ADR-0034. No
decision content, status, authority, or security boundary changed.

Exact pre-migration records and their source Git blob IDs and SHA-256 digests
are preserved in `plans/archive/linux-command-adr-id-aliases/`. The machine-
readable mapping is
`plans/archive/linux-command-adr-id-aliases/migration.json`.

Implementation plan: `plans/implementation/linux-execution-isolation/012-adr-identity-collision-and-admission-gate-corrective.md`.
Closure evidence: `plans/closure/linux-execution-isolation/012-status.md`.
