# Runtime Consolidation, Deletion, and Footprint M010 — TUI Durable Schedule Identity and Label Closure

Status: active

Source roadmap: `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`

Historical corrective closure: `plans/closure/runtime-consolidation-deletion-footprint/009-status.md`

Controlling corrective addendum: `plans/subsystems/runtime-consolidation-deletion-footprint-tui-closure-addendum.md`

Planning baseline: `376f0041a0f759805152fb0eaaa293bc57b24fdd`

Primary class: narrow correctness / TUI usability corrective pass

## 1. Objective

Correct the final user-visible defect left by M009's durable TUI schedule migration without reopening scheduler architecture, provider ownership, dependency/footprint work, or broad verification.

M009 correctly moved the active TUI off the removed legacy `Task*` scheduler API and onto the durable `ScheduleCreate`, `ScheduleList`, and `ScheduleDelete` protocol. It also preserved opaque durable `ScheduleId` values and the single daemon-owned scheduler. However, the current TUI presentation truncates those opaque IDs to eight characters while deletion submits the user's text as the exact durable ID. The same list projection also loses the scheduled prompt/message because `ScheduleSummaryDto` does not carry the job template.

The result is a coherent backend contract with an incoherent user-facing command workflow:

```text
/loop 5m "check the build"
  -> ScheduleCreate
  -> durable UUID schedule id
  -> toast shows only the first 8 characters

/tasks
  -> ScheduleList
  -> display shows only the first 8 characters
  -> prompt/message is unavailable, so label falls back to "interval"

/task-del <displayed-8-char-id>
  -> ScheduleDelete { schedule_id: displayed-8-char-id }
  -> durable store performs exact id lookup/delete
  -> displayed token is not the actual stored UUID
```

M010 restores one internally consistent TUI contract: the identifier presented to the user must be accepted by `/task-del`, and `/tasks` must show enough schedule metadata to distinguish user-created recurring tasks.

## 2. Evidence and defect classification

Baseline evidence at `376f0041`:

- `src/tui/commands/tasks.rs::durable_schedule_task_value()` projects `schedule_id`, `kind`, `state`, and `interval_secs`, but not the scheduled prompt/message.
- `apply_tasks_listed()` renders `id.chars().take(8)` and falls back from missing `message` to the schedule kind.
- the successful schedule-created toast also renders only the first eight characters of the returned durable ID.
- `start_delete_task()` forwards the user-supplied string directly to `CoreRequest::ScheduleDelete`.
- durable schedule IDs are opaque string newtypes generated from UUID v4 values.
- the SQLite schedule store deletes with exact `WHERE id = ?` semantics.
- the existing daemon create/list/delete test succeeds because it passes the complete ID returned by create directly into delete; it does not use the TUI-presented token.
- the TUI tests validate schedule DTO construction and summary projection but do not prove that the exact token shown by `/tasks` can be used by `/task-del`.

Classification:

- **medium correctness/usability defect**: the supported deletion command is not operational using the identifier exposed by the supported list/create UI;
- **low usability defect**: multiple scheduled tasks are not meaningfully distinguishable because their prompt/message is lost from list presentation;
- **verification gap**: M009 acceptance criterion 7 required a user-visible create/list/delete path, but the accepted evidence stopped at protocol-level full-ID deletion and helper-level TUI projection.

This plan does not invalidate the architectural parts of M009. Scheduler convergence, provider-turn ownership, M006 measurements, and the M009 hosted verification remain historical accepted evidence. M010 owns only the missed TUI contract and the corresponding closure correction.

## 3. Governing implementation rules

1. Preserve the durable scheduler and `ScheduleStore`/job infrastructure as the sole scheduling authority.
2. Do not restore `BackgroundScheduler`, `BackgroundTask`, legacy task persistence, numeric task identifiers, or UUID-to-`u64` bridges.
3. Keep durable `ScheduleId` opaque. The TUI may present a shortened token, but it must never manufacture a different authoritative identity.
4. Resolve any shortened user token only against schedules visible in the active workspace. Never resolve globally across unrelated workspaces.
5. A shortened token may resolve only when it matches exactly one visible durable schedule. Ambiguity must fail closed with an actionable error; never select the first match.
6. Exact full durable IDs must continue to work.
7. Do not introduce a TUI-owned authoritative schedule cache.
8. Use existing `ScheduleList` / `ScheduleGet` / `ScheduleDelete` protocol where possible. Do not add a storage schema, scheduler service, or new identifier type.
9. Do not expand this pass into scheduler recurrence syntax, pause/resume UX, provider/runtime work, dependency upgrades, binary-size work, release automation, or broad TUI cleanup.
10. Verification must remain narrow and behavior-oriented. Do not add a source-text guard or another CI lane.

## 4. Preferred corrective design

### A. Make the displayed short schedule token deletable

Preserve the current compact eight-character presentation rather than forcing full UUIDs into ordinary task toasts/dialog rows.

Introduce one small pure/helper boundary for user-entered schedule identifier resolution. The exact name is implementation-defined, but its behavior must be equivalent to:

```text
resolve_schedule_id(input, workspace_schedules) -> Result<full_schedule_id, ResolveError>
```

Required semantics:

1. trim surrounding whitespace;
2. reject an empty identifier;
3. if `input` exactly equals a listed schedule's full `schedule_id`, return that ID;
4. otherwise treat `input` as a prefix only when it is at least the TUI's documented/displayed short-ID length (currently 8 characters);
5. compare the prefix against full IDs from `ScheduleList` scoped to the active workspace;
6. exactly one match -> return that full durable ID;
7. zero matches -> return a task/schedule-not-found error;
8. more than one match -> return an explicit ambiguous-ID error and require a longer/full ID;
9. matching is exact byte/string prefix matching; do not normalize UUID text, parse it numerically, hash it, or perform fuzzy matching.

`start_delete_task()` should therefore require the active workspace identity just as task listing already does, call the existing workspace-scoped durable list path, resolve the user's token, then send `ScheduleDelete` with the full resolved ID.

The helper must not consult process CWD, project-name heuristics, or any local legacy task store.

If the implementation can share an existing workspace-scoped schedule-list helper with `/tasks`, prefer that small reuse. Do not create a generalized repository/service abstraction for two TUI handlers.

### B. Make creation and list presentation use one stable token convention

The successful schedule-created toast and `/tasks` list must present the same short-ID convention that the delete resolver accepts.

Requirements:

- keep the compact token at eight characters unless current TUI conventions clearly specify another constant;
- define the display length once in `tasks.rs` rather than scattering literal `8` values across create/list/delete logic;
- if the full durable ID is shorter than the display length, show it unchanged;
- do not imply that the short token is globally unique; ambiguity is handled by the resolver;
- an ambiguous prefix must not delete anything.

The command help/usage string for `/task-del` should say that it accepts the ID shown by `/tasks` (and optionally a full schedule ID). Do not add a new command.

### C. Restore meaningful task labels without changing scheduler storage

`ScheduleSummaryDto` intentionally does not contain the full `job_template`; the current TUI projection therefore cannot recover the original scheduled prompt from `ScheduleList` alone.

Use the existing durable `ScheduleGet` record path to enrich user-visible list rows instead of adding another persistence field or duplicating the prompt into schedule labels.

Preferred flow:

```text
ScheduleList(workspace)
  -> summaries
  -> for each displayed summary, ScheduleGet(full schedule_id)
  -> extract display text from durable job_template
  -> render compact row
```

Implementation constraints:

- run enrichment inside the already-spawned asynchronous TUI command so no render/event-loop blocking is introduced;
- simple sequential enrichment is acceptable for this corrective pass because task listing is user-initiated background work and avoiding a new concurrency abstraction is preferred;
- if the repository already has a bounded request helper that makes small concurrent fan-out trivial, it may be reused, but M010 must not introduce a new generic fan-out framework;
- one failed `ScheduleGet` must not make every other listed schedule disappear. Preserve the summary and fall back to kind/state for that row while surfacing a bounded warning only if useful;
- do not expose private provider reasoning, credentials, or unrelated job payload metadata;
- for the existing recurring subagent schedule shape, extract the user-visible prompt from the durable subagent job template payload;
- for schedule/job kinds without an obvious prompt, use a stable fallback such as kind/state rather than attempting ad-hoc JSON dumps;
- retain the existing bounded display truncation for long labels.

Do not add `message` or prompt duplication to SQL schedule rows merely for TUI display. Do not change `ScheduleSummaryDto` unless source inspection proves the existing `ScheduleGet` path cannot provide the needed record. Any protocol shape change is a stop-and-reassess condition for this narrow plan.

### D. Keep the durable protocol behavior unchanged

The backend delete operation should continue to require the exact full durable ID. Prefix resolution belongs to the human-facing TUI compatibility layer, not `ScheduleStore` and not the public durable protocol.

This preserves:

- unambiguous daemon semantics;
- opaque typed durable IDs;
- compatibility for non-TUI clients that already use full IDs;
- a single scheduler identity model;
- no accidental prefix-delete API exposed to remote/programmatic callers.

Do not change SQLite delete semantics to `LIKE`, prefix matching, or partial-key deletion.

## 5. Required regression coverage

The missing behavior must be covered directly. Prefer pure helper tests plus one narrow client/daemon-path test rather than a broad new harness.

### Resolver unit coverage

Add focused tests for the short-ID resolver:

1. full exact ID resolves to itself;
2. the exact eight-character token displayed by `/tasks` resolves to the corresponding full ID when unique in the active workspace;
3. an ID prefix shorter than the supported display token length is rejected rather than matched broadly;
4. an unknown token returns not found;
5. a deliberately constructed prefix collision returns ambiguous and deletes neither schedule;
6. schedules outside the active workspace are not candidates because resolution consumes only the workspace-scoped list result.

### Label projection coverage

Add focused tests proving:

1. an existing durable subagent `ScheduleRecordDto` yields the original prompt as the user-visible label;
2. a long prompt remains bounded/truncated only at the presentation boundary;
3. an unsupported/non-prompt schedule shape falls back to a stable kind/state label;
4. a failed detail enrichment leaves the summary list usable rather than failing the whole command.

### User-visible create/list/delete regression

Add one narrow regression proving the path M009 missed.

The preferred test should exercise the same logical contract as the TUI:

1. create a durable schedule through the existing core/daemon client path;
2. obtain the workspace-scoped schedule list;
3. derive exactly the short token the TUI presents;
4. resolve that token using the production resolver;
5. delete using the resolved full ID through `ScheduleDelete`;
6. list again and prove the schedule is absent;
7. prove the display label recovered from the durable record corresponds to the scheduled prompt.

It is acceptable to build this around an in-process `CoreClient`/test daemon rather than constructing a full Ratatui terminal. The point is to cover the real identifier presentation/resolution and durable request path, not rendering pixels.

Retain the existing daemon full-ID create/list/delete test; it still proves the durable protocol itself.

## 6. Ordered work packages

### Work package 1 — Rebase and reconfirm

Before editing:

- record current `main` SHA;
- confirm `tasks.rs` still truncates schedule IDs for display and forwards delete input verbatim;
- confirm `ScheduleGet` still returns `ScheduleRecordDto` including `job_template`;
- confirm durable schedule IDs remain opaque strings and backend deletion remains exact-ID;
- confirm no independent legacy scheduler has returned;
- preserve unrelated changes landed after baseline `376f0041`.

If another commit has already fixed the visible-ID/delete contract and label projection with equivalent tests, narrow M010 to closure verification rather than duplicating it.

### Work package 2 — Centralize display token and resolution

- introduce one small display-token constant/helper;
- update creation/list presentation to use it;
- implement exact-or-unique-prefix resolution over workspace-scoped durable schedules;
- update `start_delete_task()` to resolve before deletion;
- preserve full-ID support;
- provide explicit ambiguous/not-found errors;
- do not mutate backend schedule APIs.

### Work package 3 — Enrich task list from durable records

- after `ScheduleList`, use existing `ScheduleGet` for visible schedule details;
- extract only the minimal human-facing label needed by `/tasks`;
- preserve fallback behavior for missing/unknown templates;
- avoid storing duplicate prompt metadata or creating a new DTO unless unavoidable;
- keep list handling asynchronous and bounded in presentation size.

### Work package 4 — Add focused regression tests

- resolver exact/unique/ambiguous/not-found tests;
- label extraction/fallback tests;
- one create -> list/display-token -> resolve -> delete -> list regression;
- retain legacy `Task*` rejection and durable full-ID protocol tests.

### Work package 5 — Minimal verification and closure

Run only the change-relevant verification plus the repository's ordinary quick contract:

```bash
cargo fmt --all -- --check
cargo check -p codegg --locked
cargo test -p codegg --lib tui::commands::tasks::tests -- --nocapture
cargo test -p codegg --lib core::daemon::tests::durable_schedule_protocol_supports_create_list_delete -- --nocapture
scripts/verify.sh quick
git diff --check
```

If the new user-path regression is placed in a dedicated integration target, run that target explicitly as well.

Do not rerun M006 binary-size measurements: this correction has no dependency/profile/topology objective and the previous measurements remain valid historical evidence.

Do not create a new CI lane or matrix. If the normal existing `CI / verify` workflow runs on the implementation PR/commit, record its result in closure evidence; a special hosted run is not required solely for this small TUI correction unless an in-scope local/hosted discrepancy appears.

## 7. Explicit acceptance criteria

M010 may close only when all applicable criteria are satisfied:

1. `BackgroundScheduler`, `BackgroundTask`, and the removed independent scheduler path remain absent.
2. The durable `Schedule*` API remains the only production TUI scheduling path.
3. Creation and `/tasks` use one centralized short-ID presentation convention.
4. `/task-del` accepts the exact token shown by `/tasks` when it uniquely identifies a schedule in the active workspace.
5. `/task-del` continues to accept a complete durable schedule ID.
6. Short-ID resolution is workspace-scoped and never searches schedules from unrelated workspaces.
7. An ambiguous prefix is rejected and no schedule is deleted.
8. An unknown prefix is rejected and no schedule is deleted.
9. Too-short broad prefixes are rejected unless they are an exact full durable ID by contract.
10. Backend `ScheduleDelete` and `ScheduleStore::delete` continue to operate on exact full durable IDs; no prefix semantics leak into the protocol/store.
11. `/tasks` shows a meaningful prompt/label for the current recurring subagent schedule shape rather than displaying only `interval` for every task.
12. Label enrichment uses existing durable schedule records and does not introduce a second persistence source or duplicate authoritative prompt field.
13. A detail-enrichment failure degrades one row to a safe fallback rather than making the entire task list unusable.
14. The scheduled prompt remains bounded at the presentation boundary and no credentials/private reasoning/unrelated payload data are surfaced.
15. Focused tests cover exact, unique-prefix, ambiguous-prefix, unknown-prefix, and label extraction/fallback cases.
16. A regression test proves create -> list -> use the displayed token -> delete -> list absent through the durable client/daemon path.
17. The existing legacy `Task*` rejection test remains green.
18. The existing durable full-ID create/list/delete protocol test remains green.
19. No schema migration, new ID type, new scheduler, new scheduler cache, new generic service framework, or new CI workflow is introduced.
20. `cargo fmt`, `cargo check -p codegg`, focused tests, `scripts/verify.sh quick`, and `git diff --check` pass on the accepted implementation candidate.
21. No critical/high/medium finding remains in this M010 scope.
22. A closure record `plans/closure/runtime-consolidation-deletion-footprint/010-status.md` records the baseline/final SHA, implementation commit, test evidence, ID-resolution behavior, label behavior, compatibility/security review, and unresolved findings.
23. `plans/registry.md` removes M010 from dependency-ready work and returns the runtime-consolidation corrective addendum to closed only after the closure record is accepted.
24. M006 measurements are not rerun or rewritten unless the implementation unexpectedly changes dependencies/features/profile/topology; if it does, stop and reassess rather than silently broadening M010.

## 8. Non-goals

M010 must not:

- restore or redesign background scheduling;
- change recurrence syntax or add cron/calendar scheduling;
- add pause/resume/edit task UX;
- change durable schedule storage schema;
- change durable schedule identifiers to numeric or user-chosen IDs;
- make prefix deletion a daemon/public protocol feature;
- redesign `ScheduleSummaryDto` merely to avoid using existing `ScheduleGet`;
- introduce a TUI schedule repository/cache abstraction;
- change provider retry/streaming ownership;
- reopen M002 recovery, M003 decomposition, M004 prompt/history, M005 verification cleanup, or M006 footprint work;
- upgrade dependencies or tune release profiles;
- add static source scanners, CI matrices, benchmark/coverage/size gates, dependency bots, release automation, or fixed release cadence;
- turn this into broad TUI polish.

## 9. Failure, authority, and security semantics

Deletion must fail closed:

- zero prefix matches -> no delete request;
- more than one prefix match -> no delete request;
- core/list failure -> no delete request;
- missing active workspace -> no delete request;
- only one unique/full resolved durable ID may be sent to `ScheduleDelete`.

Authority remains daemon/workspace based. The TUI uses the active session's canonical `workspace_id`; it does not infer ownership from path strings. Prefix resolution is a presentation convenience over already-authorized workspace-visible schedules, not a new authorization mechanism.

Schedule detail enrichment must not widen information exposure. It requests records for schedules already returned by the workspace-scoped list and extracts only the user-facing prompt/kind needed for display.

No secret material should be added to logs, labels, toast metadata, or schedule IDs.

## 10. Planning and closure disposition

M009 remains historical evidence that the architectural corrective pass landed: durable scheduling authority, provider-turn extraction, M006 measurement, and exact-candidate CI were completed. M010 records that the post-M009 audit found a narrower supported-TUI contract defect that its verification did not exercise.

Do not rewrite `009-status.md` to pretend the defect was known at M009 closure. Preserve that record and Git history. The new TUI corrective addendum temporarily supersedes only the current overall disposition of this workstream until M010 closes.

While M010 is open:

- source roadmap history: closed by M009, preserved;
- current corrective disposition: active through the TUI closure addendum;
- M010: active implementation and closure review;
- M001–M009: historical predecessor records, not reopened for implementation;
- M006 footprint evidence: remains accepted and is not a dependency of this small pass;
- unrelated Provider M007, Tool Programs M019, DVR M006, and runtime-safety work: unchanged.

After M010 closes, the registry may again show runtime consolidation as closed with M010 as the final corrective control point.

## 11. Required closure record

Create `plans/closure/runtime-consolidation-deletion-footprint/010-status.md` containing:

- exact implementation baseline and final candidate SHA;
- implementation commit(s);
- disposition of the short-ID deletion defect, missing-label defect, and missed-test defect;
- exact user-visible short-ID resolution semantics;
- proof that backend exact-ID deletion remains unchanged;
- proof that task labels come from the existing durable record path;
- focused test commands and results;
- user-path create/list/display-token/delete/list regression evidence;
- legacy scheduler absence confirmation;
- storage/protocol/schema/migration assessment;
- authority/security assessment;
- unresolved findings classified critical/high/medium/low/deferred;
- final recommendation: closed, corrective pass required, or blocked.

A documentation-only closure is not acceptable. The token shown by `/tasks` must actually delete the corresponding durable schedule through the production resolver path.
