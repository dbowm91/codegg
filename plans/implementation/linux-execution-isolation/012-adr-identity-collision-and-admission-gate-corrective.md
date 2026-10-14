# Linux Execution Isolation C004 — ADR Identity Collision and Admission Gate Corrective

Status: **implemented** — identity-only migration complete; closure evidence is being recorded
Class: invariant / cleanup
Owner: planning integrity / security architecture
Closure target: `plans/closure/linux-execution-isolation/012-status.md`.
Baseline: implementation branch @ `eb2ca664d8e8aebe51fddeed8ac06d63f15f555b` (2026-10-14 UTC). Long-term: `plans/000-long-term-specification.md` §§4/17/27/29, `plans/001-terminology-and-domain-model.md` §8, and `plans/003-planning-process.md`. Execution authority: ADR-0001/0004/0016; command classification: currently accepted `plans/adrs/ADR-0032-command-syntax-and-execution-authority.md` (canonical ID to be reconciled under C004). Existing Landlock ABI >=3 filesystem-only support must remain intact. This is a handoff plan, not a claim of implementation or test execution.

## 1. Defect confirmed

The pre-migration checked-out tree had two active decisions named ADR-0027:
- `plans/adrs/ADR-0027-windows-sandbox-capability-successors.md` (accepted for **bounded test-only Windows work**, not production);
- `plans/adrs/ADR-0031-composable-linux-execution-isolation.md` (**proposed**; no production backend authorization).

It also had two active ADR-0028 decisions:
- `plans/adrs/ADR-0028-windows-broad-read-compatibility-profile.md` (**proposed**; not approved);
- `plans/adrs/ADR-0032-command-syntax-and-execution-authority.md` (**accepted**; existing low-friction command-policy authority).

The existing `scripts/check_adr_integrity.py` checks unique active IDs and local ADR links; therefore the exact current tree is statically inconsistent with its own required guard, irrespective of otherwise passing historical CI. Cross-branch authorship: Windows decisions were committed at `2fe3cab5` (2026-10-13 18:56 UTC), Linux/command ADR path allocations at `f7ac5228` (2026-10-14 02:00 UTC). The open Windows identity migration branch reserves IDs 0029/0030, so this branch assigns the Linux mechanism ADR to 0031 and accepted command ADR to 0032. This is a path/identity reconciliation only; it does not change either decision or disposition.

## 2. Identity-only migration contract

- Inventory **all** active `plans/adrs/ADR-NNNN-*.md` from the current target head, including open feature branches and any newly allocated IDs; if 0029/0030 are taken, select the next free unique monotonically increasing IDs and update this mapping before editing.
- Migrate proposed Linux composable ADR-0027 -> **ADR-0031**, accepted command-syntax ADR-0028 -> **ADR-0032** on a single branch. Statuses, decisions, underlying authority, non-goals, security limits and architecture must be preserved byte-for-byte except canonical ID/path and unambiguous cross-references. An accepted command ADR is **not accepted anew** and cannot have its content silently revised.
- Copy exact *pre-migration* originals to `plans/archive/linux-command-adr-id-aliases/` and record source ref, blob hashes, mapping and provenance in `plans/adrs/linux-command-adr-id-migration.md`. Archived copies are historical; do not leave old conflicting numbered files active under `plans/adrs/`.
- Update `plans/adrs/README.md`, Linux/command roadmaps, implementation plans (including M001-M008/C001-C003), active registry rows, architecture docs, policy skills, research and any other live reference with **referent-aware** replacement. Bare "ADR-0027" in Windows docs means Windows; bare "ADR-0028" in Windows docs means Windows broad-read; these MUST NOT be globally replaced. IDs 0029/0030 are reserved by the concurrent Windows identity migration.
- Preserve older `plans/closure/**` as historical evidence. Do not rewrite status, test counts, host provenance, security findings or closure verdicts to fabricate continuity. Record an explicit machine-readable migration/reference map; if a historical closure has a now-dangling literal local file path, fix only the path reference with a conspicuous provenance note, otherwise leave history intact and explain alias lookup.
- Extend `scripts/check_adr_integrity.py --self-test` with merged Windows/Linux/command two-pair duplicate fixture, corrected 0027–0030 fixture, dangling link fixture and archived alias fixture; keep the guard in `scripts/verify.sh quick`. Index all active numerical IDs and active local paths, not just filenames changed in this branch.

## 3. Architecture and dependency consequences

- **Do not mark Linux ADR-0031 accepted during a numbering rename.** Its proposed Bubblewrap/mount/process/network design needs an explicit separate owner review and recorded acceptance/rejection. Command ADR-0032 stays accepted as before.
- Linux M003/M004 production namespace backend promotion is blocked until (a) this identity cleanup closes, (b) the proposed Linux ADR receives an explicit architecture decision, and (c) relevant native setup/denial evidence exists. Pre-ADR test-only feasibility does not imply acceptance.
- Windows ADR-0027 remains test-only; Windows ADR-0028 remains proposed; no Windows `SandboxRequest::Required` or broad-read production enablement.
- The earlier Windows C039 accepted identity migration and its historical archive must stay intact. Do not "repair" duplicate IDs by deleting an unrelated Windows ADR or changing its disposition.

## 4. Work packages

1. Freeze/refetch main SHA, timeline and all active ADR IDs; check cross-branch allocations and prepare deterministic mapping.
2. Materialize byte-identical original aliases + provenance manifest; create identity-adjusted canonical copies and remove old active duplicate paths.
3. Rewrite references by subsystem identity and reject ambiguous bare-number substitutions. Migrate active registry/ADR README and add a short resolved collision note to Linux/command/Windows roadmaps without rewriting historical tests.
4. Extend existing ADR guard/self-tests and perform exact-revision checks; prepare closure `012-status.md` with before/after duplicate scan, old/new blob hashes, git diff statistics and unchanged authority table.
5. Re-run repository quick verification and code-independent CI guards. Stop rather than force a decision if new ID conflicts appear mid-change.

## 5. Acceptance tests / stop

Run `python3 scripts/check_adr_integrity.py --self-test`, `python3 scripts/check_adr_integrity.py`, `git diff --check`, `python3 scripts/check_sandbox_contract.py`, `python3 scripts/check_execution_ownership.py`, `scripts/verify.sh quick` on the actual combined tree, not the earlier feature branch. Search active references and ensure no dangling `plans/adrs/ADR-....md` links, exactly one active ADR per numeric ID, no changed decision status/authority and exact historical provenance retained.

**Stop** if a later branch now owns 0029/0030, an accepted ADR would need substantive rewriting, or a Windows reference would be misidentified as Linux/command. Record blocked provenance and revisit allocation; do not suppress the guard.

Storage/API/runtime/daemon change: **none**. Closure of this corrective makes the ADR *identifiable*, not accepted, and cannot be used as evidence that Linux isolation mechanisms have landed.
