# Linux and Command ADR Identity Migration

Date: 2026-10-14; reconciled against current main on 2026-10-19

The C004 identity-only migration archived the original Linux and command decisions
under `plans/archive/linux-command-adr-id-aliases/`, preserving source blobs,
digests, and mapping history in `migration.json`. Current main now allocates ADR-0031
through ADR-0041 to other accepted or proposed decisions. To retain unique canonical
identities without changing decision substance or status, command authorization is
ADR-0042 and Linux execution isolation is ADR-0043. The proposed contained-Git
live-object and metadata-promotion decision is ADR-0044. The command and Linux
accepted decisions remain accepted; ADR-0044 remains proposed.

Identity history:

- Linux execution isolation: ADR-0027 → ADR-0031 → ADR-0033 → ADR-0035 → ADR-0043.
- Command syntax and execution authority: ADR-0028 → ADR-0032 → ADR-0034 → ADR-0042.
- Contained Git live-object and metadata authority: proposed ADR-0036 → ADR-0044.

IDs 0029/0030 remain assigned to the Windows decisions. Current main's ADR-0031
through ADR-0041 retain their meanings and contents. The rename preserves the
accepted decisions' substance and acceptance dates and does not admit a Linux
production backend or authorize the proposed Git route.

Exact pre-migration records and their source Git blob IDs and SHA-256 digests remain
in `plans/archive/linux-command-adr-id-aliases/`. Implementation plan:
`plans/implementation/linux-execution-isolation/012-adr-identity-collision-and-admission-gate-corrective.md`.
Closure evidence: `plans/closure/linux-execution-isolation/012-status.md`.
