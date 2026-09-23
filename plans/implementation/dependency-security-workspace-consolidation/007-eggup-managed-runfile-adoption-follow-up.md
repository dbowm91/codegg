# Dependency Security and Workspace Consolidation M007 — Eggup Managed-Runfile Adoption Follow-up

Status: implemented; local qualification in progress

Repository baseline: `220d3638fe043b24e65a7817241543612f9e82af`

Historical predecessor evidence:

- M005 plan: `plans/implementation/dependency-security-workspace-consolidation/005-generic-updater-interface-and-codegg-adoption.md`
- M005 blocked closure: `plans/closure/dependency-security-workspace-consolidation/005-status.md` (preserved unchanged)
- Eggup consumer plan: `plans/implementation/consumer-adoption/005-codegg-managed-runfile-bundle-adoption.md`

## Objective

Close the external updater-interface blocker identified by historical M005 by
adopting Eggup's immutable-pinned `eggup-acquisition` and `eggup-core` APIs for
CodeGG's managed three-runfile bundle. Preserve the existing Eggfetch/Rustls/
WebPKI trust profile and all CodeGG-owned release policy.

## Ownership and constraints

- CodeGG owns release repository/tag policy, supported target mapping, archive
  naming, eggsearch pin, helper identity probe, CLI behavior, and strict
  CodeGG-specific extraction.
- Eggup owns generic acquisition contracts and verified local multi-artifact
  transaction/rollback/recovery mechanics.
- Eggpack remains the producer authority; this change does not create release
  manifests or alter packaging.
- Normal `codegg upgrade` never fetches or executes `install.sh` and never
  invokes external `curl`.
- The initial in-place target set is the four supported Linux/macOS targets.
  Windows and unsupported installations keep explicit manual guidance.

## Qualification

The source dependency is pinned to Eggup revision
`66813b3b94de3a9b2f270e0000dc339ef6f0b478`; no floating branch or published
package assumption is used. Required local checks and hosted CI results are
recorded in `plans/closure/dependency-security-workspace-consolidation/007-status.md`.
