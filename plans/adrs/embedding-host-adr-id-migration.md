# Embedding-host ADR identity migration

Date: 2026-11-17

Source branch snapshot: `feat/auto-20261109-f8abad0d` at
`87a1fab7611ea21f390d5fa59eeec834b133416a`

Mainline snapshot reviewed: `origin/main` at
`dd77c8709e75aaaa684d1d5765fc8d0c5ee60d1b`

Common base: `1ca8f7b01180e9d57e8c41faff41fb9c06d8a91a`

## Identity mapping

| Decision | Original identity | Canonical identity | Disposition |
|---|---|---|---|
| Default Linux containment policy | ADR-0061 | ADR-0061 | Mainline allocation retained; accepted 2026-11-10 |
| Embedding host wire public contract | ADR-0061 | ADR-0063 | Renumbered to avoid the mainline collision; accepted 2026-11-12 |
| Bundled palette contrast authority | ADR-0062 | ADR-0062 | Existing feature-branch allocation retained; accepted 2026-11-12 |

Mainline keeps ADR-0061. ADR-0062 was already allocated to the bundled-palette
decision, and ADR numbers are monotonically increasing and never reused; the
embedding-host decision therefore takes the next available identity, ADR-0063.
This follows the repository's prior cross-branch rule: mainline allocations win,
while the displaced decision's substance, status, and acceptance date remain
unchanged.

## Provenance and change boundary

The exact pre-migration embedding-host record is archived at
`plans/archive/embedding-host-adr-id-aliases/ADR-0061-embedding-host-wire-as-a-public-contract.md`.
Its source branch commit is `87a1fab7611ea21f390d5fa59eeec834b133416a`; the
source Git blob is `77ee45d9940c6e2596702fdf8256135779c10fcd`; and its SHA-256 is
`fb3f462d9d5d6d6bd0b2432b9ab69ade17278e782a21ae3b7090856d7b719fb5`.

The active record is
`plans/adrs/ADR-0063-embedding-host-wire-as-a-public-contract.md`. Its decision,
accepted status, and acceptance date are unchanged; only the identifier in its
title and its inbound references were updated. The three embedding-host closure
records retain their evidence and now link to the canonical identity; their stale
`ADR-0060` link labels were corrected to match the referenced decision. This
migration changes no architecture decision, capability, or authority. The
archived filename is historical provenance, not an active ADR allocation.
