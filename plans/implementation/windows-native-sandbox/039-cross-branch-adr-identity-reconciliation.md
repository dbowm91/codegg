# Windows Native Sandbox C039 — Cross-branch ADR Identity Reconciliation

Status: **implemented**; no new sandbox mechanism or architecture decision
Primary class: architecture/governance corrective
Baseline: `work/windows-native-sandbox-foundation` HEAD `631ae9488f251ac70e91b9c8299d2c1c3bff83fa`; `main` HEAD `b9ba494c1b21638c2e2f660301ba207ce8859682`; common baseline `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`. Refresh both heads and allocation table before implementation.
Source: `plans/subsystems/windows-native-sandbox-roadmap.md#post-adr-0024-pr-111-mainline-reconciliation`.
Canonical: `plans/000-long-term-specification.md` §§4,17,27,29; `plans/001-terminology-and-domain-model.md`; `plans/003-planning-process.md` §2.2 and §6; `plans/adrs/README.md` from current `main`.
Authority: no Windows security-profile change. Prior Windows ADR decisions and C029/C034/C038 negative closures remain unchanged.

## 1. Defect found in the live merge audit

Mainline TUI work independently allocated `ADR-0021-native-editor-completion-read-and-local-edit.md` and `ADR-0022-daemon-authorized-workspace-directory-listing.md` (now on `main`, at `b9ba494c`). The Windows branch independently has `ADR-0021-windows-privileged-launch-vs-guest-isolation.md` and `ADR-0022-windows-two-stage-native-launch-and-mxc-requalification.md` plus Windows ADR-0023/0024. The filename suffixes are different, so Git does **not** report these as textual conflicts, but two different decisions have the same global ADR ID. The active mainline `plans/adrs/README.md` explicitly requires monotonically increasing ADR IDs that are never reused and documents a cross-branch reservation. This is a **semantic conflict** distinct from the sole observed overlapping text file `plans/registry.md` (C040).

Do not turn a successful `git merge` exit into a declaration of governance correctness while duplicate IDs remain. Conversely, do not silently rewrite accepted ADR content or prior closure outcomes to make an ID appear historically unique.

## 2. Requested output and ownership

Produce a canonical, collision-free ADR listing for combined `main` + Windows branch while maintaining permanent traceability for the already published Windows ADR filenames, Git SHAs, and C029/C034/C038 closure decisions. This is a document-identity migration, **not** an architectural reversal, not authorization for C030/C031, and not a production MXC admission.

Recommended deterministic allocation **if the refreshed mainline still ends at ADR-0022**:

| Original Windows record on feature branch | Proposed canonical merged identity |
|---|---|
| ADR-0021 windows privileged launch vs guest | ADR-0023 windows privileged launch vs guest |
| ADR-0022 windows two-stage launch and MXC | ADR-0024 windows two-stage launch and MXC |
| ADR-0023 windows MXC helper corrective | ADR-0025 windows MXC helper corrective |
| ADR-0024 windows MXC capture/handle review | ADR-0026 windows MXC capture/handle review |

ADR-0019/0020 Windows remain unchanged. Mainline ADR-0021/0022 keep their exact identities, statuses and content. If another branch/mainline commit has allocated IDs 0023–0026, do **not** hardcode this table; take the next free four monotonically increasing IDs and record the actual mapping. If the mainline status of either TUI ADR changed, preserve the new status, rather than assuming acceptance or rejection.

## 3. Bounded work packages

**A — Freeze current evidence and namespace.** Get exact current `origin/main` and `origin/work/windows-native-sandbox-foundation`, their merge-base and all `plans/adrs/ADR-*.md` paths. Read `plans/adrs/README.md` from the current mainline and inspect each collision's decision/status. Inventory path/ADR ID references in `plans/registry.md`, `plans/subsystems/windows-native-sandbox-roadmap.md`, `plans/closure/windows-native-sandbox/`, `plans/implementation/windows-native-sandbox/`, architecture/research/scripts/tests and other feature branches as visible. Explicitly count documents and each old-ID occurrence. Do not rename unrelated TUI decisions or invent a new security ADR to repair metadata.

**B — Identity-only migration, preserving history.** Prefer copying the *unchanged decision text* of each already accepted/rejected Windows ADR into a new uniquely numbered canonical filename with only the necessary title/identity metadata corrections, and preserve the previous decision text/provenance at its original Git SHA and an unambiguous historical alias location outside the active numbered `plans/adrs/` set (e.g. `plans/archive/windows-adr-id-aliases/`, following §2.6 of the planning process). Write `plans/adrs/windows-adr-id-migration.md` recording original ref+SHA, new canonical path, reason, disposition and no-authority-change statement. The migrated canonical text MUST NOT rewrite C029's broker/guest rejection, C032/C033/C034/C035/C036/C037/C038 negative status, or claim Windows production `Required`. If governance requires a separate explicit approval before identity metadata change, stop and record that as a blocker—do not fabricate approval.

**C — Atomic reference migration.** Update each active path ref to the new canonical filename and each *unambiguously Windows-scoped* ID mention. Do not bulk replace bare `ADR-0021` or `ADR-0022` without proving referent: after merge those IDs correctly mean editor completion/directory listing in TUI contexts. For completed closure records that must stay historical, prefer referential **metadata-only** link updates with a provenance note; do not change results, run IDs, evidence, test counts or past decision wording. Preserve original historical records in archive/Git so readers can reconstruct before/after. Ensure replacement mapping is idempotent and old Windows ADR filenames do not remain as duplicate active allocations.

**D — Registration and cross-subsystem regression.** Update `plans/adrs/README.md` only as needed with a concise collision-resolution map; keep its monotonic-numbering rule. Update registry/Windows roadmap and any mainline TUI roadmap paths only when genuinely required, preserving both authorities. Add a lightweight deterministic CI checker (or extend existing planning guard) that indexes filenames under active `plans/adrs/` and fails on duplicate numeric IDs or dangling local ADR links, with fixtures covering collision, valid migration and false positives for archived aliases. Do not conflate duplicate-number checks with acceptance-status validation.

**E — Closure.** Create `plans/closure/windows-native-sandbox/039-status.md`, mapping table, moved/copied file paths, exact commit SHAs, provenance snapshots, before/after duplicate-ID scan, reference-check results and authority invariants; change registry and roadmap to **closed** only when collision-free for a merged-tree preview. C040 may then integrate mainline.

## 4. Verification and hard acceptance

Run at minimum: `git diff --check`, the ADR uniqueness/dangling-link checker on a synthetic **merged** file list as well as the feature branch, `python3 scripts/check_sandbox_contract.py`, `python3 scripts/check_execution_ownership.py`, `bash scripts/verify.sh quick` and an actual `git merge-tree`/merge preview to ensure the mainline ADRs appear once each. Disclose unavailable checks, and do not assert file-path presence merely because historical refs exist in Git.

Acceptance:
- Exactly one active `ADR-NNNN-*.md` for each global ID in the **proposed combined tree**, including both mainline TUI ADRs and Windows decisions. No orphaned links; no incorrect bare-ID interpretation.
- Prior decisions, status, negative security evidence and future-ADR gates are unchanged.
- Historical aliases preserve exact old provenance; a title/identity migration cannot masquerade as a new security approval.
- No production code, Windows process launcher, sandbox backend, CLI, RunStore schema, host ACL, installer or network setting changes.

Stop rather than force an identity choice if mainline allocated overlapping successor IDs after this baseline or if preservation of an accepted ADR's historical content cannot be demonstrated. Re-register the revised mapping explicitly before proceeding.
