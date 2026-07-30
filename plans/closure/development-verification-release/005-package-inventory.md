# M005 Package Inventory — crates.io Publication Evidence (HISTORICAL / STALE)

Status: **historical / stale**

This file is retained for traceability. It contains outdated dependency
relationships and a contradictory verification claim. The corrected,
authoritative M006 inventory is at:

- `plans/closure/development-verification-release/006-package-inventory.md`

Specific defects:

- `codegg-providers` was incorrectly listed as depending on `codegg-protocol`.
  The actual manifest at `crates/codegg-providers/Cargo.toml` declares
  `codegg-config` as its only internal dependency.
- `egglsp` was incorrectly listed as depending on `codegg-protocol`.
  `crates/egglsp/Cargo.toml` declares no internal dependencies.
- The verification section claims `scripts/verify.sh full` passed while
  recording a failing integration test. That is contradictory.
- The CI section references an earlier implementation commit plus uncommitted
  fixes rather than one exact accepted revision.
- The topological layer column for `codegg-providers` is wrong (it should be
  layer 1, depending only on a layer-0 leaf).

Do not copy this file's contents into new evidence documents.
