# Delegated Live Transcript Post-Closure C002 — Exact-Head CI and Handoff Qualification

Status: blocked — hard dependency C001 accepted closure; preflight allowed
Primary class: invariant
Repository baseline: plan/delegated-run-live-transcript at 21dca074b09eb00c34527ff13067352d0641897d, 2026-10-26; refresh HEAD and CI before implementing.
Source roadmap: plans/subsystems/delegated-run-live-transcript-post-closure-corrective-addendum.md §4 C002.
Long-term requirements: plans/000-long-term-specification.md (reliability and daemon-owned frontend projections), plans/001-terminology-and-domain-model.md, plans/003-planning-process.md.
Accepted ADRs: ADR-0051, ADR-0043, ADR-0041, ADR-0053. Hard predecessor: C001. Historical feature milestones M001–M003 remain closed on branch.

## 1. Objective

Turn PR #203 into a qualified integration candidate on its exact code head. Repair feature-introduced Rust 1.99 lint failures; explicitly disposition the enumerated baseline lint debt without a global suppression; obtain assembled real-daemon/SQLite/TUI live-follow and reconnect evidence after C001; make planning statuses consistent; and finish with exact-head hosted workflow receipts. Do not declare merge readiness with a red CI gate.

## 2. Why this milestone is ready after C001

M001–M003 landed a working in-loop capture, authorized page API and bounded TUI follow. C001 owns two specific progress hazards: a permanent four-page catch-up limit and a quiet-run pending advisory. Only after C001 closes can verification prove live-follow rather than a partially functional preview. Inventory and fix designs may begin before C001 closure; integration/closure may not.

## 3. Current implementation evidence

- Hosted CI on feature head 21dca074: run 40399676826, job 121841859221, failed Workspace Clippy under Rust 1.99 with **16 errors**. New-source clippy::double_must_use errors occur at src/agent/transcript_capture.rs:62 and src/agent/transcript_publisher.rs:73 and :89, from async_trait macro expansion. crates/codegg-core/src/agent_run_transcript.rs has a precedent: a narrow scoped allow at the async_trait macro site with explanatory comment.
- Thirteen further lint errors are in existing paths: src/eggsact/adapter.rs (collapsible_if), src/search_backend/eggsearch.rs (manual_clamp), src/search_backend/framing.rs (bool_comparison cases), src/test_runner/index.rs (manual_contains), src/test_runner/runner.rs (field_reassign_with_default), src/tool/deterministic.rs (manual_contains), src/tool/edit.rs (type_complexity), src/tool/task.rs (question_mark), and src/tool/skill.rs (items_after_test_module). Recount and compare against actual main baseline/toolchain; do not assume all errors have the same provenance.
- Existing closure receipts: M001 focused 10 pass; M002 daemon 4 pass + projection 10 pass; M003 1109 TUI render pass. Full nextest 16,569 pass, 30 fail, one timed out, nine skipped. The closures ascribe 29/30 failures to baseline and one to contention, but a red full-suite receipt is not green CI.
- At review: Desktop E2E workflow 40399676939 in progress; four platform workflows on feature commit successful: Windows native probe, Windows process launcher, macOS shared checkout, Linux M004 process/IPC. Re-fetch, do not predict.
- The original roadmap header, dependency graph and §12 table still say M001 ready/M002–M003 blocked even though milestone §7 and registry say closed. The registry still lists M002/M003 as blocked.
- M001 proves real SQLite capture while a child is blocked. M003 tests exercise TUI follow with fake page data. A composed blocking child → real daemon/EventLog → authorized page → TUI (including reconnection) is not independently proven by those two separate receipts; C001/C002 must qualify the composed path.

## 4. Invariants

Keep accepted ADRs and M001–M003 historical closure evidence intact. No global Clippy suppressions or CI gate disable. No unrestricted cleanup of unrelated code; only specifically enumerated and tested mechanical lint fixes may be admitted here, with an upstream owner for anything more substantive. Read authorization remains SessionRead plus exact durable run owner; body only through redacted store, no body in advisory, no child session and no widened tool authority. No fake test result, exact-head claim or pending workflow outcome.

## 5. Scope

In: three new async_trait lint errors, detailed baseline classification of remaining 13, narrow mechanical fixes if needed to restore mandatory CI gate, C001-corrected end-to-end qualification, roadmap and registry reconciliation, exact-head workflow/PR status.

Out: new agent features, new daemon auth/protocol, generalized lint campaigns, testing with user-paid provider credentials without permission, desktop live-follow, automatic merging or weakening guards to obtain a green build.

## 6. Required production changes

### Core/domain
Use a narrowly scoped clippy::double_must_use allow on the three async_trait trait macro sites (or equivalent idiomatic remedy), consistent with existing repository precedent. Preserve async trait semantics and doc comments. Do not hide other lint categories.

### Storage and migrations
None.

### Protocol/DTO
No wire change. Retain existing additive read/advisory compatibility and error classes.

### Runtime/concurrency
After C001, demonstrate an actual blocked child capturing into durable SQLite, daemon publishing committed high water, client fetching authorized pages and real TUI updating while child still runs; continue across >4 pages; force lag/reconnect; verify no stale routes or foreign session data. Optionally run a real-model smoke test on a trusted workstation if already provisioned, but identify its evidence separately from deterministic fixture.

### Frontend
No new controls beyond C001; preserve view ownership and scroll pin/anchor and truthful transcript completeness/cap indications.

### Security
Verify SessionRead and exact-owner negative cases and redaction on both stored rows and wire output. An observer with only SessionObserve cannot read content. No raw body in CoreEvent advance.

### Documentation and static guards
Fix source roadmap header/dependency diagram/§12 status table: M001–M003 closed **on branch** and reference corrective C001/C002. Label roadmap's original code-state observations as historical. Remove stale M002/M003 Blocked work entries from registry. The corrected registry retains the historical closed features plus new corrective stages. Run repository ADR integrity and relevant planning guards; verify paths.

## 7. Ordered work packages

### A — Freeze current lint inventory and ownership
Retrieve latest branch CI log and compiler version, reproduce Workspace Clippy exactly. Compare with main (same pinned compiler) and record which diagnostics are introduced by this branch vs already upstream, with file/line/rule. Do not infer everything from the historical closure narrative.

Acceptance: a signed source SHA, CI run id and exact diagnostic inventory, including the three introduced errors.

### B — Narrow lint fixes without weakening the gate
Repair all new-source double_must_use errors. For the other 13: either apply **only** finite mechanical changes to the listed sites, with separate commits and unchanged-semantic tests if verified necessary and safe; or document a separate upstream CI baseline owner and keep C002 blocked if they require wider behavioral rewrites. Do not suppress workspace warnings or change lint thresholds. Rerun under pinned hosted Rust toolchain.

Acceptance: no new-source Clippy error; exact-head CI green for closure, or C002 explicitly blocked with no merge-ready claim.

### C — Composed execution/transport/TUI smoke
With C001 corrected, start real daemon with SQLite and a scripted provider/blocked tool (controlled FIFO/barrier, not sleep alone). While child is blocked, assert: committed call row, body-free EventLog hint, authorized CoreRequest response, open TUI tab shows call. Cross at least 5 cursor pages without new hint; append/result visible without reopen. Disconnect/reconnect and verify tail, run identity, scroll, and denied foreign/observer requests. Record test command, stdout/assertions, data/byte bounds and cleanup.

Acceptance: evidence spanning real daemon transport and TUI rendering, not merely two separate fake tests.

### D — Reconcile planning, review merge safety and re-run workflows
Correct original roadmap's live status, dependency graph and §12. Remove stale registry blocked rows and update C001/C002. Check PR #203's latest diff and branch versus main for conflicts, run focused and full guards, and trigger/fetch exact-source CI and Desktop E2E receipts. If workflow fails, classify/reproduce rather than declare green. Update PR summary only with observed status.

Acceptance: no contradictory roadmap/registry facts, exact-head workflow links, and a C002 closure record with pass/fail/nonclaims.

## 8. Failure, cancellation, restart and contention

A CI failure is a blocker. A baseline-failing test still appears as failing in any summary (record independent regression comparison). If a concurrent branch modifies a lint site, reconcile instead of overwriting. If an integration fixture blocks indefinitely, cancel it and clean session scratch. A client disconnect/reconnect must never skip already committed rows; pending publication must be recoverable from cursor reads. A session/project switch invalidates cache and in-flight answers. Never infer complete source transcript solely from the last page length.

## 9. Compatibility and migration

No storage or protocol migration. Older daemon keeps initial inspector/snapshot fallback, new daemon and old TUI remain compatible with advisory omission. Only source and documentation changes anticipated. Rerun exact-head checks after rebases and closure documentation commits.

## 10. Required tests

- Focused M001, M002 and M003 tests; C001 quiet-child and >4-page progress tests; per-store bounds, truncation, redaction and observer/cross-session negative tests.
- Real daemon/SQLite + TUI integration with paused tool and live updates before completion; 6-page cursor walk, cancellation, reconnect, stale route, wrong principal, fair 16-run bound.
- Hosted Rust 1.99 Clippy -D warnings, fmt, static guard self-tests and quick suite. Full nextest and Desktop E2E outcomes preserved even when non-green; detailed baseline differential.
- If actual live-provider smoke runs, disclose model/route, permission, cost, duration, provenance and observed behavior; otherwise explicitly state unrun.

## 11. Required verification commands (not run by this plan author)

cargo test --test agent_loop -- delegated_transcript
cargo test --test core_daemon -- delegated_transcript
cargo test --test projection_replay -- delegated_transcript
cargo test --test tui_render -- transcript_follow
cargo test --test tui_render
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 scripts/check_delegated_transcript_store.py --self-test
bash scripts/check_projection_disclosure.sh --self-test
bash scripts/check_projection_publication_seam.sh --self-test
python3 scripts/check_tui_transport_authority.py --self-test
python3 scripts/check_execution_ownership.py
python3 scripts/check_adr_integrity.py
bash scripts/verify.sh quick
cargo nextest run --workspace --locked --profile ci --no-fail-fast

Commands and test filters must be revalidated against current Cargo.toml and suite. Fetch CI workflow conclusions for the final source commit rather than relying on a previous SHA or a documentation-only commit.

## 12. Documentation

Source roadmap header, graph, milestone table; plans/registry.md active/ready/blocked sections; this corrective addendum; new C002 closure; PR #203 description. Historical closure reports are immutable evidence, not current status controls.

## 13. Acceptance criteria

1. Three branch-introduced Clippy failures are gone, without blanket suppression. Exact-head CI green to close C002, otherwise report named external blocker and leave it open.
2. Full live-follow path is qualified with an actually blocked tool, daemon, authorized CoreClient read and real TUI showing pre-completion entries, surviving >4 pages and reconnect.
3. Security, route fencing, store redaction and pagination bounds remain verified; no listener/observer can read unauthorized transcript.
4. Original roadmap/registry agree: M001–M003 closed on branch; C001 closed on accepted evidence; C002 status accurately reflects its verified gate. Stale blocked history is removed from current registry tables, not from historical closure files.
5. CI, Desktop E2E and platform results name exact commit, run id and observed outcome; full-suite baseline failures remain disclosed. No merge claim for red or pending required checks.

## 14. Stop conditions

Stop if C001 has not closed; if a lint fix involves broad functionality beyond the finite inventory; if hosted CI remains red and no approved separation of baseline debt exists; if integrated test cannot exercise the actual daemon-to-TUI path; if any authorization or body-free event invariant fails; if only a previous SHA has green CI. No automatic merge.

## 15. Closure evidence

Record exact source SHA and PR, new-source and baseline lint inventory, list of file edits, full focused + static + hosted CI receipt links/results, a composed blocked-tool trace with cursor/page advancement and reconnect, session negative tests, status reconciliation check and remaining limitations. If C002 stays blocked, record the precise reason and owning corrective line.

## 16. Handoff

C001 is the hard predecessor. One qualification owner should coordinate finite unrelated lint edits to avoid collisions. Preserve developer changes and worktrees, revalidate repository head, and do not mark PR #203 ready for review or merge without the verified workflow gate.
