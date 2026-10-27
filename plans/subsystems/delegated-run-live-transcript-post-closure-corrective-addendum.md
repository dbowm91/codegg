# Delegated Run Live Transcript — Post-Closure Corrective Addendum

Status: ready for staged handoff — C001 ready; C002 blocked on C001
Review baseline: plan/delegated-run-live-transcript at 21dca074b09eb00c34527ff13067352d0641897d, 2026-10-26
Target PR: #203 (draft; not merged; not merge-ready at review).
Source roadmap (historical implementation line): plans/subsystems/delegated-run-live-transcript-roadmap.md (M001–M003 closed *on the feature branch*).
Historical closures (retain, do not rewrite): plans/closure/delegated-run-live-transcript/001-status.md, 002-status.md, 003-status.md.
Canonical planning: plans/000-long-term-specification.md; plans/001-terminology-and-domain-model.md; plans/002-long-term-roadmap.md; plans/003-planning-process.md.
Accepted ADRs (unchanged): ADR-0051 durable delegated transcript, ADR-0043 session-owned child lifecycle, ADR-0041 parent-mediated steering, ADR-0053 daemon-published projections.

## 1. Purpose and ownership

A feature branch with three closed implementation milestones is not equivalent to an integrated feature. Correct two concrete live-follow progress hazards, restore branch-local lint/CI gates and accurate planning status, and produce credible end-to-end proof before PR #203 is considered merge-ready. Do not reopen M001–M003's accepted functional scope or broaden the daemon's execution authority.

Owners: C001 is transcript publication/TUI follow forward progress; C002 is exact-head qualification, narrow build/lint remediation, and planning reconciliation. No new scheduler, session identity, content surface, or human-to-child control.

## 2. Findings on the reviewed branch

**F001 — P1: bounded TUI catch-up can become permanently stuck.** src/tui/commands/transcript_follow.rs::start_follow returns when cache.pages_in_pass >= MAX_TRANSCRIPT_PAGES_PER_PASS (4). apply_page increments pages_in_pass, continues automatically only while it is <4, and clears it when !dirty. For a source with has_more after page 4, pages_in_pass remains 4 and dirty remains true. on_tab_focused calls start_follow without rearming the pass; on_snapshot_watermarks does likewise; retry() resets the budget but is only available for retryable *error* statuses, not an ordinary CatchingUp. Thus an open long transcript can remain stalled indefinitely even with further hints or focus changes. The 512-entry memory cap and page-size bound are not themselves the defect. The progress-scheduling rule is.

**F002 — P1/P2: a sub-cap advisory can wait until termination instead of the claimed 500 ms.** src/agent/transcript_publisher.rs::CoalescingTranscriptPublisher::should_publish compares elapsed time, but the decision is called only by publish_advance() on an append or flush() on completion. Once the first advisory publishes, a subsequent burst of 1–7 rows within the interval can become pending. If the child is then blocked in a tool/provider wait, there is no timer or other guaranteed wake-up to flush that pending watermark after 500 ms. The TUI may stay stale until another append, focus catch-up or terminal flush. The 'interval' currently expresses a *test on the next append*, not a delivery latency guarantee. Prove and correct without a per-token feed or unbounded per-run background tasks. A daemon-owned bounded timer or prompt semantic-boundary flush are candidates; the choice must be measured. This need not imply a TUI polling timer.

**F003 — P1 integration gate: hosted CI red on the exact head.** GitHub Actions CI run 40399676826 on 21dca074 fails Workspace Clippy, 16 errors total under Rust 1.99. Three errors come from newly introduced async-trait expansions: src/agent/transcript_capture.rs:62 and src/agent/transcript_publisher.rs:73, :89 (clippy::double_must_use). The other 13 are reported in existing src/eggsact/adapter.rs, src/search_backend/{eggsearch,framing}.rs, src/test_runner/{index,runner}.rs and src/tool/{task,edit,skill,deterministic}.rs. They must be distinguished from feature regressions with a main-baseline comparison, then dispositioned without a general lint campaign. Existing codegg-core::agent_run_transcript has a scoped #[allow(clippy::double_must_use)] precedent at the async-trait declaration; prefer a matching narrow fix over global lint suppression.

**F004 — P2 registry/roadmap contradiction.** The registry's Active subsystem row and §7 milestone paragraphs say M001–M003 closed, but the source roadmap header, dependency graph and §12 status table still say ready/blocked/pending. Registry 'Blocked work' still lists M002 and M003 as blocked. Those records are contemporaneous control-plane facts, unlike the immutable historical closure evidence. Reconcile, and explicitly distinguish closed-on-branch from merged-to-main.

**F005 — P2 insufficient composed integration evidence and red baseline.** Closure documents record green focused test members (M001: 10, M002: 4 daemon/10 projection, M003: 1109 render), and a baseline-compared workspace sweep (16,569 pass, 30 fail, 1 timeout, 9 skipped). Those runs do not provide an exact-head fully green hosted CI or a single assembled real-daemon + blocked child + live TUI follow + reconnect qualification, and the M003 closure explicitly does not claim live-provider fidelity. The desktop E2E workflow 40399676939 was in progress at review; its final result must be fetched, not predicted. Platform workflows on the reviewed head reported success for Windows native probe, Windows process launcher, macOS shared checkout and Linux M004.

## 3. Invariants and non-goals

- Retain M001–M003 as historically closed on their branch; this is a *new* corrective line rather than a retroactive rewrite.
- Durable AgentRunTranscriptStore is the only transcript body source; redaction at write, 8 KiB/row and 256 KiB/run bounds, and no raw reasoning, text, secrets or tool args on the advisory stream.
- Read authorization remains the daemon's SessionRead + exact durable run ownership check; observer-only access cannot read bodies.
- No child session rows, no new child provider execution, direct steering or widened sandbox/worktree ceiling.
- No unbounded timers, queues, subscriptions, catch-up reads or cache growth. Do not trade the stuck 4-page loop for a hot request loop.
- Do not change unrelated domain functionality merely to suppress warnings; a narrow, evidenced, mechanically safe CI lint correction is acceptable if the build gate otherwise stays permanently red.
- No claim of 100% green full suite or merged status while the referenced evidence does not support it.

## 4. Target corrections

**C001: Live-follow forward progress and notification liveness (invariant, ready).**
Plan: plans/implementation/delegated-run-live-transcript-post-closure-corrective/001-progress-and-advisory-liveness.md.
Mandatory: a stale source with >4 pages progresses across bounded passes without manual retry, while other open tabs retain fair scheduling; a 1–7 row coalesced update is signalled to an open active tab after a bounded quiet period even if the child stays blocked; both are tested against failing baseline. Producer and consumer must remain bounded with no TUI poll. Close C001 only on code and regression evidence, not the old M003 closure.

**C002: Hosted verification, narrow lint gate, and status reconciliation (integration/polish, blocked until C001).**
Plan: plans/implementation/delegated-run-live-transcript-post-closure-corrective/002-exact-head-ci-and-handoff-qualification.md.
Mandatory: the three new async-trait lint errors are absent, remaining lint baseline is dispositioned without blanket suppression, relevant CI workflow is green on an exact source head or a named external blocker remains explicitly open; a composed daemon/TUI fixture and interruption/reconnect test demonstrates actual forward progress; status tables/registry/PR evidence agree. C002 may be prepared in parallel, but cannot claim closure before C001.

## 5. Dependencies

M001–M003 (closed on branch) -> C001 (ready) -> C002 (hard; blocked on C001).

Interface considerations: C001 touches src/tui/commands/transcript_follow.rs, transcript_cache.rs and src/agent/transcript_publisher.rs plus narrowly scoped tests. C002 may touch the two async-trait files, CI tooling/docs, and the exact list of baseline lint sites only if safely needed; coordinate shared-file changes with C001.

## 6. Cross-cutting qualification

Require non-vacuous tests (demonstrably fail against the inspected defect): >4-page source with has_more at page four, no extra hint/retry/focus trick, and a quiet-child interval flush; an actually blocked child and a live open tab with a real SQLite daemon and authenticated CoreClient; reconnect/terminal and wrong-session negative; 16-child fan-out bounded cost; no body on EventLog. In addition, record exact Clippy errors and baseline comparison, full guard and test results and GitHub hosted workflow run/commit linkage.

## 7. Stop conditions and unresolved decisions

If correct finite catch-up requires bypassing the TUI task/reconnect fence, stop. If publishing promptly requires unbounded per-run timers or violating EventLog ownership, stop and propose a narrower daemon-owned alternative. If persistent red CI comes from unrelated repository failures that cannot be safely repaired inside the named lint inventory, stop and register a separate repository-wide gate corrective; do not mark C002 or PR #203 merge-ready. If no interactive real-provider credentials are available, use the deterministic integrated fixture and explicitly state that provider-backed fidelity remains unverified; do not fake provider-backed evidence.

## 8. Completion

C001 and C002 each require closure records under plans/closure/delegated-run-live-transcript-post-closure-corrective/, an exact-head verification receipt, updated registry and status table, and a PR #203 summary that reflects what was actually run. A passing focused test does not overrule red hosted CI. No auto-merge requested.

## 9. Milestone status

| Milestone | Status | Plan | Closure |
|---|---|---|---|
| C001 | closed | plans/implementation/delegated-run-live-transcript-post-closure-corrective/001-progress-and-advisory-liveness.md | closed (`plans/closure/delegated-run-live-transcript-post-closure-corrective/001-status.md`) |
| C002 | closed | plans/implementation/delegated-run-live-transcript-post-closure-corrective/002-exact-head-ci-and-handoff-qualification.md | closed (`plans/closure/delegated-run-live-transcript-post-closure-corrective/002-status.md`) |
