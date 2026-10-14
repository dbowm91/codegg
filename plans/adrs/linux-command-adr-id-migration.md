# Linux and Command ADR Identity Migration

Date: 2026-10-14

The C004 identity-only migration maps the proposed Linux execution-isolation ADR
from ADR-0027 to ADR-0031 and the already-accepted command-syntax authority from
ADR-0028 to ADR-0032. IDs 0029/0030 are reserved by the concurrent Windows ADR
identity migration. No decision status, authorization, design choice, or
security boundary changed.

Exact pre-migration records and their source Git blob IDs and SHA-256 digests
are preserved in `plans/archive/linux-command-adr-id-aliases/`. The machine-
readable mapping is
`plans/archive/linux-command-adr-id-aliases/migration.json`.

Implementation plan: `plans/implementation/linux-execution-isolation/012-adr-identity-collision-and-admission-gate-corrective.md`.
Closure evidence: `plans/closure/linux-execution-isolation/012-status.md`.
