# Linux and Command ADR Identity Migration

Date: 2026-10-14; reconciled against current main on 2026-10-19 and again on 2026-10-21

The C004 identity-only migration archived the original Linux and command decisions
under `plans/archive/linux-command-adr-id-aliases/`, preserving source blobs,
digests, and mapping history in `migration.json`. Current main allocates ADR-0031
through ADR-0043 to other accepted or proposed decisions. To retain unique canonical
identities without changing decision substance or status, command authorization is
ADR-0045 and Linux execution isolation is ADR-0046. The proposed contained-Git
live-object and metadata-promotion decision is ADR-0044. The command and Linux
accepted decisions remain accepted; ADR-0044 remains proposed.

Identity history:

- Linux execution isolation: ADR-0027 → ADR-0031 → ADR-0033 → ADR-0035 → ADR-0043 → ADR-0046.
- Command syntax and execution authority: ADR-0028 → ADR-0032 → ADR-0034 → ADR-0042 → ADR-0045.
- Contained Git live-object and metadata authority: proposed ADR-0036 → ADR-0044.

The 2026-10-21 reconciliation is a second collision, found by the C001 branch-and-CI
stabilization line. `main` had, since the 2026-10-19 pass, allocated **ADR-0043** to the
accepted session-owned subagent lifecycle projection decision and **ADR-0042** to the
accepted composer normal-mode key ownership decision. The Linux execution isolation
record therefore could not keep ADR-0043, and the command authority record could not
keep ADR-0042. Main wins both numbers — the same rule the first reconciliation
applied — so the Linux execution isolation record moved to ADR-0046 and the command
authority record to ADR-0045, and the ADR-0043 file the branch had published for the
Linux decision is archived beside the earlier ADR-0027 and ADR-0028 aliases. The
decision's substance, its accepted status, and its acceptance date are unchanged;
only the canonical identifier moved. `scripts/check_adr_integrity.py` enforces the
uniqueness and the resolution of every `plans/adrs/ADR-NNNN-….md` path reference,
and its self-test proves the duplicate check is armed.

IDs 0029/0030 remain assigned to the Windows decisions. Current main's ADR-0031
through ADR-0043 retain their meanings and contents. The renames preserve the
accepted decisions' substance and acceptance dates and do not admit a Linux
production backend or authorize the proposed Git route.

Exact pre-migration records and their source Git blob IDs and SHA-256 digests remain
in `plans/archive/linux-command-adr-id-aliases/`. Implementation plan:
`plans/implementation/linux-execution-isolation/012-adr-identity-collision-and-admission-gate-corrective.md`.
Closure evidence: `plans/closure/linux-execution-isolation/012-status.md`.
