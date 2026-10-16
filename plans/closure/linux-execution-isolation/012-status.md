# Linux Execution Isolation C004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/linux-execution-isolation/012-adr-identity-collision-and-admission-gate-corrective.md`

Source subsystem roadmap:

- `plans/subsystems/linux-execution-isolation-roadmap.md#next-handoffs--explicit-test-only-vs-production-gates-2026-10-13`

Repository baseline reviewed: `eb2ca664d8e8aebe51fddeed8ac06d63f15f555b`

Implementation commits:

- `2e8d93189f3dae73d7a8a5cc5446aebea181101a` — identity-only Linux/command ADR migration, provenance archive, reference updates, and expanded ADR guard fixtures.

## 1. Executive finding

C004 is complete. At the reviewed C004 baseline, the Linux mechanism ADR had unique identity ADR-0031 and remained **proposed**. The accepted command-authority ADR had unique identity ADR-0032 and remained **accepted** with unchanged authority. Exact pre-migration files and Git/SHA-256 fingerprints are preserved. No Linux sandbox feature was selected or enabled by this work.

IDs 0029/0030 were reserved by the concurrent Windows identity migration; the next unused IDs, 0031/0032, were assigned to Linux/command. The Windows ADR references were kept referent-specific.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Unique active ADR IDs and resolvable local ADR paths | `python3 scripts/check_adr_integrity.py`; duplicate, corrected merged-ID, dangling-link, and archived-alias fixtures in `--self-test` | pass | Active IDs include Windows 0027/0028 and Linux/command 0031/0032 on this branch; concurrent Windows renumbering reserves 0029/0030. |
| Preserve exact pre-migration ADR records | `plans/archive/linux-command-adr-id-aliases/` plus `migration.json` | pass | Git blob IDs and SHA-256 hashes match the pre-migration source blobs. Canonical records differ only in their ID and referent-aware cross-references. |
| Preserve status and authority | Before/after ADR comparison and current ADR headers | pass | Linux stays proposed; command stays accepted. No new permission or backend selection. |
| Update active Linux and command references without changing Windows meaning | ADR README, Linux/command roadmaps, implementation plans, registry, and migration note | pass | Windows references remain subsystem-specific; no blanket number replacement was applied. |
| Full quick verification | `scripts/verify.sh quick` | pass | Local run on this implementation tree; workspace check completed. No hosted CI claim. |

## 3. Production implementation evidence

No runtime, storage, protocol, or backend code changed. ADR-0033 still selects no production backend until separate maintainer acceptance and native proof. ADR-0032 preserves the already-accepted command classification and execution-authority contract.

## 4. Verification executed

### Commands run

```bash
python3 scripts/check_adr_integrity.py --self-test
python3 scripts/check_adr_integrity.py
git diff --check
scripts/verify.sh quick
```

### Results

All commands passed locally. `scripts/verify.sh quick` reported `Quick verification passed`, including formatting, repository guards, ADR integrity, and `cargo check --workspace --all-targets --locked`.

## 5. Invariant review

- No authority ceiling, explicit deny, sandbox default, requested capability, or obtained-capability claim changed.
- Linux ADR-0033 remains proposed; no namespace or mount backend was registered.
- Command ADR-0032 remains accepted; its content was not substantively revised.
- Windows ADR-0027/0028 references continue to mean Windows decisions on this branch.

## 6. Failure and recovery review

This is a documentation and static-guard migration. It changes no process lifecycle, persistent state, or runtime recovery behavior. Archived originals provide exact rollback and provenance. The ADR guard catches duplicate active numbers and dangling active local paths.

## 7. Migration and compatibility review

No schema, protocol, config, or user-facing runtime migration occurred. ADR paths and cross-references now use canonical IDs; the migration manifest preserves old aliases for traceability.

## 8. Security review

No execution authority was added. The Linux mechanism remains unapproved and unavailable for production selection. Historical Windows decisions were not deleted or assigned Linux meaning.

## 9. Documentation and operations

Updated ADR index, Linux and command roadmaps, registered implementation references, migration note, machine-readable provenance, and the ADR integrity self-test. The self-test covers duplicate Windows/Linux/command identities, corrected unique IDs through 0032, dangling links, and archived aliases.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The concurrent Windows identity migration must be integrated with this branch using its reserved ADR-0029/0030 paths and references. | Cross-branch integration still needs the ordinary exact-head guard run. | Re-run `python3 scripts/check_adr_integrity.py` after integration; the IDs are reserved and no active collision exists in either branch's intended mapping. |
| low | At the C004 review date, Linux ADR-0031 acceptance and positive native Linux isolation evidence remained outstanding. | C006 and production M003–M008 remained gated. | Obtain a separate maintainer decision and required native proof; C004 did not satisfy either gate. |

No medium or high finding remains for the identity migration itself.

## 11. Roadmap disposition

C004 is closed. C005 remains eligible and may proceed. At the C004 review date, C006 remained blocked on explicit ADR-0031 disposition, qualifying positive C005 native evidence, and M002. C007/C008 and command M004 remained independently ready for their bounded audit/test-only scopes.

## 12. Registry updates

- Move C004 from dependency-ready work to recently closed with implementation commit `2e8d93189f3dae73d7a8a5cc5446aebea181101a` and this closure link.
- Mark C004 closed in the Linux roadmap.
- Unblock audit: no new plan became ready because of C004. C005/C007/C008 and command M004 were already ready; C006 remains blocked by its other hard gates.

## Current identity corrections (2026-10-15 and 2026-10-16)

Current main had accepted authenticated idle continuation as ADR-0031. The
Linux decision remains accepted with unchanged substance as ADR-0033; command
authority was ADR-0032 at that correction point. This later canonical-ID
correction does not alter the historical C004 baseline or its evidence.

On 2026-10-16, current main introduced the accepted agent-workstream ADR-0032.
Command authority therefore keeps its accepted status and unchanged substance
under ADR-0034; Linux remains ADR-0033 and idle continuation remains ADR-0031.
See `plans/adrs/linux-command-adr-id-migration.md` and the current canonical
command ADR at `plans/adrs/ADR-0034-command-syntax-and-execution-authority.md`.
