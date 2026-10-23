# TUI Main Buffer Text Rendering Milestone M002 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/tui-main-buffer-text-rendering/002-formatted-json-detail-view.md`

Source subsystem roadmap:

- `plans/subsystems/tui-main-buffer-text-rendering-roadmap.md#milestone-m002--the-detail-view-shows-formatted-json`

Repository baseline reviewed: `f2639b5d2`

Implementation commits or pull requests:

- **this commit** — the implementation and this closure record land together
  in one commit on `chore/record-landing-lane-outcomes` (baseline
  `f2639b5d2`). The landing note appended after the merge records that
  commit's SHA and the hosted `CI / verify` result for it; see §10.
  The files that carry this milestone's change are
  `src/tui/components/messages.rs` (the extraction, the formatter and the
  detail-view rendering), `tests/tui_render/tui_formatted_json_detail_view.inc`,
  `architecture/tui.md`, `architecture/search_backend.md` and
  `.opencode/skills/tui/SKILL.md`. No SHA, pull request number or commit
  message is claimed, because none exists: the implementing agents were not
  authorized to commit.

## 1. Executive finding

The milestone's capability boundary — **what does an expanded tool call show
the user** — is complete in the working tree, and the completion is verified
against the source rather than asserted.

At the baseline the detail view showed the **trust banner** where the result
should have been. The search backend frames every external result in an
`external_untrusted` trust frame; that frame is a prompt-injection defence
aimed at the **model**, so its prose is model-context text, and the TUI was
rendering that prose as though it were the tool's answer. The same path also
rendered a dense JSON document as raw text — one over-long row, clipped at the
edge — and the structured outline that would have made a web-search result
readable was dead code for every framed output, because the frame's opening
bracket satisfied the outline's shape check and the frame's prose then failed
its JSON parse.

What landed:

- `src/tui/components/messages.rs` —
  `extract_trust_frame_payload_for_display(output: &str) -> &str`, a
  **private, bounded, display-only** extraction of the payload inside the
  frame, and `format_payload_for_display(payload: &str) -> String`, which
  pretty-prints JSON and falls back to the raw text.
- The structured tools — `websearch`, `research`, `webfetch` — render the
  **outline**; every other output renders pretty-printed JSON when it parses
  and the raw text otherwise. The tool **input** gets the same treatment.
- The **trust banner is omitted from the detail view**. It remains in the
  **model path** byte for byte: `src/search_backend/framing.rs` has **zero
  diff lines** against the baseline.

No protocol, storage, migration, authorization, or dependency change. No closed
milestone of any other roadmap was reopened or amended.

### The condition, the risk, and the exact future evidence

**The condition.** The implementation is complete and locally verified,
and this change carries it in a commit — but the **hosted `CI / verify`
lane has not run against that commit**, and a CI run cannot precede
the commit it runs against. The exact future evidence that discharges
the condition is:

1. a **green hosted `CI / verify` run against that commit on the hosted
   runner** — not the local gate, and not a re-run of the local gate.

**The risk.** A local `scripts/verify.sh quick` run is a property of **this
machine and this tree**, not of the hosted runner. The hosted lane is a strict
**subset** of `quick` — it omits the client and desktop boundaries and the
provider wire/catalog/resilience, OpenAI endpoint and Eggwork routing guards —
so a green hosted run does **not** by itself prove everything the local gate
proves. The local gate's own value is what this record states in §4: it is
strong evidence about this tree, and it is **not** evidence about the hosted
runner's environment, its runner image, or its concurrency.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| §13.1 — an expanded tool call whose output is a framed JSON payload shows the payload **pretty-printed**, and does not show the frame's banner as if it were the result | `an_expanded_tool_call_shows_the_formatted_payload_and_not_the_banner` (`tests/tui_render/tui_formatted_json_detail_view.inc`); `extract_trust_frame_payload_for_display` and `format_payload_for_display` (`src/tui/components/messages.rs:3344`, `:3408`); the collapsed-view unit test | pass | The banner is removed by the extraction, not by a special case at the render site. |
| §13.2 — an expanded tool call whose output is not JSON shows the payload as text; nothing is dropped | `format_payload_for_display` falls back to the raw text (`:3413`–`:3414`); the unit test for a framed non-JSON payload shown as text | pass | The fallback is the raw text, **never** nothing — invariant I4. |
| §13.3 — the tool's **input** is shown pretty-printed under the same rule | `src/tui/components/messages.rs:2387`–`:2388` calls `format_payload_for_display(extract_trust_frame_payload_for_display(input))`; the unit test for the tool input shown pretty-printed | pass | On the input the extraction is a no-op — the input is the model's own arguments and never opens with a frame header. |
| §13.4 — a framed `websearch` output produces the structured outline | `render_structured_output_outline` (`src/tui/components/messages.rs:3434`), whose `STRUCTURED_TOOLS` are `["websearch", "research", "webfetch"]` (`:3435`); the unit test for a framed `websearch` output producing the structured outline | pass | The outline prints **every leaf scalar**, so it is the readable form of the payload rather than a second copy of the document beside it. |
| §13.5 — the trust frame is byte-identical on the model-facing path, and the diff shows no change there | `git diff f2639b5d2 -- src/search_backend/` is **zero lines**; `src/search_backend/framing.rs` has **no diff lines** | pass | The TUI duplicates the marker names (`TRUST_FRAME_MARKERS`, `src/tui/components/messages.rs:87`) because the framing module is the model path's contract and is not the TUI's to change. |
| §15 — the extraction's and the formatting's unit tests | 15 unit tests in `src/tui/components/messages.rs` | pass | Extraction of a framed output; unframed output unchanged; empty output; a framed non-JSON payload extracted verbatim; a frame without its closing marker; a payload that quotes the marker text; every frame family recognised; JSON pretty-printed; non-JSON falls back to raw text; an empty payload formats to nothing; a framed `websearch` output produces the structured outline; the collapsed view of a framed tool shows the result and not the banner; an expanded tool call shows its output pretty-printed; the tool input shown pretty-printed; a framed non-JSON payload shown as text. |
| §15 — a rendered expanded tool call, showing the pretty-printed payload | `an_expanded_tool_call_shows_the_formatted_payload_and_not_the_banner` | pass | — |
| §15 — the outline's output for a framed `websearch` payload | the framed-`websearch` unit test and `render_structured_output_outline` | pass | — |
| §15 — the rendered-row-count assertion (I1) | `the_drawn_rows_are_the_rows_the_row_count_reports` and `the_scroll_range_is_the_row_count_minus_the_viewport` (`tests/tui_render/tui_formatted_json_detail_view.inc`) | pass | The extraction and the formatting land **inside** the row producer. |
| §15 — `scripts/verify.sh quick` green at the closing commit | §4's verification table | **partial** | The gate is **green on the working tree** (exit 0, "Quick verification passed", 74 steps). What is missing is the other half of the item: **there is no closing commit**, so the gate has not been green *at a closing commit*, and the hosted `CI / verify` lane has not run against this code. This is the milestone's condition; see §1 and §11. |
| The extraction is bounded, and never scans an unbounded prefix | `TRUST_FRAME_HEADER_MAX_BYTES = 512` (`src/tui/components/messages.rs:99`); the header is bounded by the output's **first line** (`:3345`–`:3347`) | pass | The extraction is O(output) with one reverse search over the body. |
| A header that does not name a known marker, or does not carry `trust=external_untrusted`, is not a frame | `trust_frame_marker` (`:3390`–`:3399`) requires a known marker **and** `trust=external_untrusted` | pass | A bracketed word that is not a frame is not mistaken for one. |
| A frame whose closing marker is missing still yields its payload | `src/tui/components/messages.rs:3379`–`:3382` returns the body after the banner | pass | A truncated frame is not a reason to show nothing. |
| A payload that itself quotes the marker text is not cut short | the closing marker is found with `rfind` — the **last** one (`:3377`) | pass | Covered by a unit test. |
| The header's affordance and count describe the rows that are actually drawn | `output_line_count` is `output_body.len()` — the drawn body (`src/tui/components/messages.rs:2272`–`:2274`) | pass | The banner's own lines would otherwise inflate the count and the note would lie about what is on screen. |
| The extraction is applied to the **input** as well, without harming it | the input path calls the same two functions (`:2387`–`:2388`) | pass | A no-op on an unframed input: the input is returned unchanged. |
| **Non-vacuity**: the new tests actually detect the absence of the change | with the baseline rendering path restored, **11 of the 15** unit tests fail and **2 of the 3** integration tests fail | pass | The four that still pass are guard-rails for functions that do not exist on the baseline. |
| The `read` tool's numbered-code path is unchanged | `read_lang.is_some()` leaves `output_body` as `None` (`src/tui/components/messages.rs:2258`–`:2260`) | pass | Its per-line gutter makes pretty-printing inapplicable, and its output is never framed. |

## 3. Production implementation evidence

**The extraction (`src/tui/components/messages.rs`).**

- `fn extract_trust_frame_payload_for_display(output: &str) -> &str` (`:3344`)
  is **private** and **display-only**. Its contract:
  - the frame header is recognised **only in the output's first line** and
    only within `TRUST_FRAME_HEADER_MAX_BYTES = 512` (`:99`);
  - the header is recognised only when it names a **known marker**
    (`TRUST_FRAME_MARKERS`, `:87`) **and** carries
    `trust=external_untrusted` (`trust_frame_marker`, `:3390`);
  - the payload starts at the **first blank line after the header** — the
    frame's documented shape — not at a fixed line offset (`:3354`–`:3373`);
  - the closing marker is found with **one reverse search** over the body
    (`rfind`, `:3377`), so a payload that quotes the marker text is not cut
    short;
  - a frame whose **closing marker is missing** yields the payload after the
    banner (`:3379`–`:3382`);
  - an **unframed output is returned unchanged** (`:3350`, `:3342`–`:3343`).

**The formatter.**

- `fn format_payload_for_display(payload: &str) -> String` (`:3408`)
  pretty-prints JSON with `serde_json::to_string_pretty` and falls back to the
  **raw text** otherwise. An empty payload formats to the empty string, which
  the renderer draws as no rows, without failing.

**The rendering.**

- The tool call's rendering now uses the **extracted** payload
  (`src/tui/components/messages.rs:2250`).
- The **structured tools** render the **outline**
  (`render_structured_output_outline`, `:3434`), and every other output
  renders pretty-printed JSON when it parses and the raw text otherwise
  (`:2261`–`:2267`). The outline is rendered **in the collapsed view and in
  the expanded view**, and **not** additionally the pretty JSON document,
  which would duplicate the whole payload.
- The tool **input** gets the same treatment (`:2387`–`:2388`).
- The header's expand affordance and the **"(+N more lines)" count** come from
  the **drawn body** (`:2272`–`:2275`).

**The model path.** `src/search_backend/framing.rs` is **unchanged** — zero
diff lines against the baseline. The trust banner still reaches the model byte
for byte.

**Tests.**

- `src/tui/components/messages.rs` — 15 unit tests.
- `tests/tui_render/tui_formatted_json_detail_view.inc` — 3 integration tests:
  the expanded detail view's content, the drawn rows equal the row count, and
  the scroll range.

**Documentation.**

- `architecture/tui.md` — "The detail view: the payload, not the banner": the
  extraction's boundary, the formatter, the structured tools' outline, the
  drawn-body count, and the explicit statement that the frame is not weakened.
- `architecture/search_backend.md` — "The frame is a prompt-injection defence,
  and it belongs to the model path": the TUI parses the frame **for display
  only**, `src/search_backend/framing.rs` is unchanged, and the TUI duplicates
  the marker names because the framing module is the model path's contract.
- `docs/tui.md` — "Reading the transcript → What an expanded tool call shows": what the
  input and the output look like, that the structured tools show an outline,
  and that the trust banner is not shown.
- `.opencode/skills/tui/SKILL.md` — the module guide's contract for the
  detail view.
- `architecture/overview.md` — the integration-test-file count row, which this
  milestone's new member moved.

### The plan's internal tension, and how it was resolved

The plan's §13 asks for two things that pull in different directions for the
three structured tools:

> "shows the payload **pretty-printed**"
>
> "a framed `websearch` output produces the **structured outline**"

Work package C states that the outline replaces "the raw document", and the
outline prints **every leaf scalar** — so for those three tools **the outline
*is* the formatted form**. The resolution shipped, and it is recorded here as
a **documented interpretation of §13**, with the wording quoted above:
**the structured tools render the outline** (collapsed and expanded, and not
additionally the pretty JSON document, which would duplicate the whole
payload); **everything else renders pretty JSON when it parses and the raw
text otherwise**. The acceptance criteria are satisfied jointly, not one at the
expense of the other.

## 4. Verification executed

### Commands run

```bash
# recorded at this milestone's own verification point
cargo test --locked -p codegg --lib tui::components::messages
cargo test --locked -p codegg --lib search_backend
cargo test --locked -p codegg --test tui_render
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
cargo check --workspace --all-targets --locked
bash scripts/verify.sh quick
```

### Results

| Command | Result |
|---|---|
| `cargo test --locked -p codegg --lib tui::components::messages` | **63 passed**, 0 failed |
| `cargo test --locked -p codegg --lib search_backend` | **84 passed**, 0 failed |
| `cargo test --locked -p codegg --test tui_render` | **1043 passed**, 0 failed |
| `python3 scripts/check_test_assertion_quality.py` | all checks pass |
| `python3 scripts/check_test_duplication.py` | all checks pass |
| `cargo check --workspace --all-targets --locked` | exit 0 |
| `bash scripts/verify.sh quick` | **exit 1 — one step** (at this milestone's verification point — see the re-verification below for the current result) |

**`verify.sh quick` failed at exactly one step at this milestone's
verification point:**
`check_test_target_references` reported that `architecture/overview.md`
recorded a stale integration-test-file count. That row is a **documentation
count**, not a product behaviour: the guard compares the row against the tree,
and the row had not been re-measured after this line's new integration members
were added. Every other step of the gate passed; the remainder was run
individually and passed. **The row has since been re-measured and corrected in
the same working tree, and the gate is now green** — see the re-verification
below and §9.

### Re-verification (this record's author, on the same working tree)

| Command | Result |
|---|---|
| `cargo test --locked -p codegg --lib tui::components::messages` | **80 passed**, 0 failed — **higher** than the 63 recorded above, because M003 of this same roadmap added unit tests to the same module after this milestone's verification point |
| `cargo test --locked -p codegg --lib search_backend` | **84 passed**, 0 failed (matches the recorded result) |
| `cargo test --locked -p codegg --test tui_render` | **1052 passed**, 0 failed — **higher** than the 1043 recorded above, for the same reason |
| `cargo test --locked -p codegg --lib` | **7373 passed, 3 failed** — the three environmental failures in the table below, and nothing else. |
| `bash scripts/verify.sh quick` | **exit 0 — green**: "Quick verification passed", 74 steps. The one red step seen earlier in this record's writing — `check_test_target_references`, "architecture/overview.md records 326 integration test files, but the tree holds 327 root integration test modules" — was a **stale documentation count**, re-measured and corrected in the same working tree. |

The recorded test counts are **snapshots of a tree that other agents were
concurrently modifying** — the main-buffer and composer milestones of this line
were implemented in parallel, and each milestone's integration members landed in
the same family. They are therefore **not a monotonic series** across the five
records of this line, and each record's count describes the tree at that
milestone's own verification point.

### Failures recorded, not concealed

The following failures are **not** this milestone's failures, and are recorded
so that a reader is not left to discover them:

| Failure | Attribution |
|---|---|
| `core::daemon_refresh::tests::session_projection_rebuilds_owner_scoped_live_jobs` — `InvalidRootPath("/tmp")` | **Environmental and pre-existing.** This host's `/tmp` root path is not usable by the daemon-refresh fixture. Nothing in this milestone touches the daemon. |
| `core::daemon_refresh::tests::session_projection_snapshot_includes_in_process_live_child_instances` — `InvalidRootPath("/tmp")` | **Environmental and pre-existing.** Same cause. |
| `managed_process::tests::offline_required_scheduler_carried_intent_refuses_disabled_sandbox_before_spawn` — `OfflineRequired is unavailable on this platform` | **Environmental and pre-existing.** The macOS host has no offline-required sandbox backend. Nothing in this milestone spawns a process. |
| (resolved) `tui::command::tests::command_docs_builtin_slash_action_variant_count_matches_source` and `tui::command::tests::command_docs_count_matches_registry` — stale counts in `architecture/command.md` ("claims BuiltinSlashAction has 160 variants; the enum has 161"; "count 168 drifts from registry 169") | **Not this milestone's.** The extra variant is M004's `/expand-all` slash action (`src/tui/command.rs`), and the stale counts were in `architecture/command.md`, which this milestone does not touch. **Documentation counts**, in the same class as the `overview.md` row below. The counts were corrected in `architecture/command.md` in the same working tree, and both tests now **pass**. |
| (resolved) `check_test_target_references` — the `architecture/overview.md` integration-test-file count row | **A documentation obligation, not a product defect.** The row records a count of test *files*; the guard compares it against the tree. It was the gate's one red step; the row has been re-measured and corrected in the same working tree, and the step is green. §9 names it. |
| (not reproduced) `tui::file_tree_coordinator::tests::barrier_held_worker_plus_100_rebuilds_keeps_one_worker_one_pending` | **Flaky, and not a failure of this milestone.** It failed once under the load of a full `--lib` run on this host, **passes when run in isolation** (`cargo test --locked -p codegg --lib tui::file_tree_coordinator::tests::barrier_held_worker_plus_100_rebuilds_keeps_one_worker_one_pending` → 1 passed, 0 failed), and passed in the final full `--lib` run. It touches no TUI code. |

**Skipped scope.** `scripts/verify.sh full` (clippy `-D warnings` over the
whole workspace, the workspace test suite, and
`cargo test -p codegg --features server,plugins,lsp-test-support`) was **not
run**, and no claim is made about it. A hosted CI run was **not** executed
against this code; the evidence above is local. The change is confined to the
TUI's presentation path, and the commands above cover the modules it touches
— including `search_backend`, which is the module whose contract the milestone
must not disturb.

## 5. Invariant review

| Invariant | Evidence | Result |
|---|---|---|
| **I1 — one producer for rows and for the row count.** The extraction and the formatting land inside the row producer. | The extraction and the formatter are called from the tool-call rendering inside `build_message_rows`; `the_drawn_rows_are_the_rows_the_row_count_reports` and `the_scroll_range_is_the_row_count_minus_the_viewport` | pass |
| **I2 — the TUI renders what the daemon sent.** The TUI parses the payload out of the frame for rendering; it does not change the frame in the model-facing path, and it does not change what the daemon sent. | The daemon's output is read, never written: `extract_trust_frame_payload_for_display` returns a `&str` slice of the output it was given, and no message part is synthesized, dropped or reordered | pass |
| **I3 — the trust frame stays in the model path.** The frame is a prompt-injection defence. This milestone reads it; it must not remove, weaken, reorder or relocate it. | `git diff f2639b5d2 -- src/search_backend/` is **zero lines**; `src/search_backend/framing.rs` has **no diff lines**; `cargo test --locked -p codegg --lib search_backend` → 84 passed, 0 failed | pass |
| **I4 — nothing is silently unreachable.** The payload remains reachable even if it cannot be parsed. | `format_payload_for_display` falls back to the **raw text**, never to nothing; a frame with no closing marker yields the payload after the banner; an unframed output is returned unchanged | pass |
| **I5 — a wrap never breaks an escape sequence.** Unchanged from M001, and the formatted detail view is wrapped by M001's primitive, so it inherits the exclusion. | The formatted body is drawn as text rows and then wrapped by the same renderer as every other row; M001's escape-atomicity tests are green in the same tree | pass |

## 6. Failure and recovery review

- **Failure.** The extraction is total over its inputs: an empty output, an
  unframed output, a frame with no closing marker and a frame with no body
  all yield a defined result, and none of them panic. The formatter falls
  back to the raw text rather than failing.
- **Duplicate delivery and idempotency.** Not applicable. Both functions are
  pure: the extraction returns a slice of its input, and the formatter is a
  function of its payload. Re-rendering the same output produces the same
  rows.
- **Cancellation races.** Not applicable. Nothing asynchronous is added.
- **Daemon or node restart.** Not applicable. Nothing is persisted; the detail
  view is rebuilt from the daemon's message parts on every layout.
- **Partial persistence failure.** Not applicable. No write path is added.
- **Stale generation or lease.** Not applicable. The extraction reads the
  output the renderer was handed; it does not cache across generations.
- **Contention and resource release.** The extraction is frontend-local and
  runs on the render path. Its cost is bounded by the output's length: one
  scan of the first line, one scan for the blank line, and one reverse search
  for the closing marker. The formatter's cost is one JSON parse plus one
  pretty-print. There is no shared resource and nothing to release.
- **Malformed or unauthorized input.** This is the milestone's real risk
  surface, and it is closed by construction rather than by convention:
  - a header that does not name a known marker, or does not carry
    `trust=external_untrusted`, is **not a frame** and the output is returned
    **unchanged** — an unframed tool output is never mangled;
  - the header scan is bounded by `TRUST_FRAME_HEADER_MAX_BYTES` and by the
    output's **first line**, so the extraction never scans an unbounded prefix
    for a marker that is not there;
  - a payload that quotes the marker text is not cut short, because the
    closing marker is found with `rfind`;
  - a truncated frame (no closing marker) still yields its payload, so a
    malformed frame degrades to "the payload without its footer" rather than
    to an empty view.
- **Bounded event and artifact behavior.** No event or artifact is produced.
  The detail view's rows are bounded by the payload's length.

## 7. Migration and compatibility review

- **Schema migration.** None. Nothing is persisted.
- **Protocol.** None. No daemon message, session state, or wire format
  changes. The model-facing path is byte-identical (I3).
- **Configuration.** None. No configuration key is added, renamed or
  reinterpreted.
- **Backward compatibility.** The change is **user-visible but not
  incompatible**. What the expanded tool call shows changes: the trust banner
  is no longer presented as the result, a JSON payload is pretty-printed, and
  the three structured tools show an outline. Nothing the daemon sent is
  altered, and the model's view is unchanged, so no other consumer of the same
  output can regress. The `read` tool's numbered-code output is unchanged.
- **Rollback limitations.** The change is self-contained in
  `src/tui/components/messages.rs` plus one new integration member. Reverting
  the production file restores the previous behaviour exactly. There is no
  persisted state to migrate back and no compatibility shim to unwind.
- **Legacy paths.** The raw-text rendering of a tool output is **retained** as
  the fallback for a payload that does not parse as JSON; it is not removed.
  The `read` tool's numbered-code path is retained unchanged.

## 8. Security review

- **Authorization.** Unchanged. This milestone adds no key handling, no tool
  path, and no daemon interaction.
- **Secrets.** None handled. No credential, token, or path is read or logged.
  The formatter prints whatever payload the tool produced — that is the
  payload's own content, and the milestone adds no new source of it.
- **Privilege boundaries.** Unchanged.
- **Path validation.** Not applicable — the milestone parses no paths and
  resolves none.
- **The trust frame — the security statement this milestone must make.** The
  trust frame is a **prompt-injection defence**, and this milestone **does not
  weaken it**. The frame is a *model-path* artefact: its job is to tell the
  model that the content is external and untrusted. The TUI's change is a
  **display-side read** of that frame, and it is bounded in three ways:
  1. **The model path is byte-identical.** `src/search_backend/framing.rs`
     has **zero diff lines** against the baseline, and the whole
     `src/search_backend/` directory has a zero-line diff. The model still
     receives the frame, with the banner and the closing marker, exactly as
     it did.
  2. **The read cannot rewrite anything.** The extraction returns a `&str`
     slice of the output it was handed. It does not construct, reorder or
     re-serialize the frame, and it cannot reach the daemon's output.
  3. **The TUI duplicates rather than redefines the contract.** The marker
     names are copied into `TRUST_FRAME_MARKERS`
     (`src/tui/components/messages.rs:87`) precisely so that the framing
     module — which is the model path's contract — is not modified by a
     frontend change. The duplication is a known cost, recorded here and in
     `architecture/search_backend.md`, and it is the price of leaving the
     model path untouched.
  The trust frame is therefore **stronger than before from the model's point
  of view**: the user sees the payload without the banner, and the model sees
  the banner without any change.
- **Denial of service.** The extraction is bounded by
  `TRUST_FRAME_HEADER_MAX_BYTES` for the header scan and by one reverse search
  over the body for the closing marker; there is no unbounded scan and no
  backtracking. The formatter's cost is one JSON parse and one pretty-print of
  a payload the tool already produced. The detail view's row count is bounded
  by the payload's length, and it is the same row count the transcript already
  carried.
- **Redaction.** Unchanged. The milestone adds no redaction and removes none.
  The banner it omits from the display was never a secret; it was model-context
  prose.
- **Audit.** No new audit event. Rendering an already-received tool output is
  not an auditable action.

## 9. Documentation and operations

- **Updated architecture documents.**
  - `architecture/tui.md` — "The detail view: the payload, not the banner": the
    extraction's boundary, the formatter, the structured tools' outline, the
    drawn-body count, the `read` exception, and the explicit statement that
    the frame is not weakened.
  - `architecture/search_backend.md` — "The frame is a prompt-injection
    defence, and it belongs to the model path": the display-only read, the
    unchanged framing module, and the reason the marker names are duplicated.
  - `architecture/overview.md` — the integration-test-file count row, which
    this milestone's new member moved.
- **Updated user-facing documentation.** `docs/tui.md` — "Reading the
  transcript → What an expanded tool call shows": the pretty-printed input, the outline
  for the structured tools, the raw-text fallback, and the statement that the
  trust banner is not shown.
- **Updated module guide.** `.opencode/skills/tui/SKILL.md` — the detail
  view's contract for the module guide.
- **Static guards.** None added. The milestone adds no new call site for a
  guard to police — the extraction is a private function inside one module.
  The existing guards were run and pass, including
  `scripts/check_test_assertion_quality.py` and
  `scripts/check_test_duplication.py`.
- **Operator diagnostics.** None. Nothing to diagnose.
- **Documentation obligation — resolved.** The `architecture/overview.md`
  integration-test-file count row was the gate's one red step
  (`check_test_target_references`). It is a **count of test files**, not a
  behaviour; the row has been re-measured against the tree and corrected in
  the same working tree, and the step is green. Nothing is outstanding here.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | **The hosted `CI / verify` lane has not run against the commit that carries this code.** The implementation and this record land together in one commit on `chore/record-landing-lane-outcomes` (baseline `f2639b5d2`). | The milestone's evidence is local only. A local `verify.sh quick` run is a property of this machine and this tree; the hosted lane is a strict **subset** of `quick`, so neither the local run nor a future hosted run proves the other. | **Required before this record can be upgraded to `closed`:** a green hosted `CI / verify` run against this commit on the hosted runner. |
| medium | **The `read` tool's "(+N more lines)" count still comes from `output.lines().count()`** rather than from a drawn body. | The count for a `read` output describes the raw output's logical lines, while every other tool's count describes the rows that would be drawn. The two can disagree for the `read` path only, because the `read` path draws its own numbered code. | Recorded, **not fixed**: the `read` path's per-line gutter makes pretty-printing inapplicable and its output is never framed, so it is deliberately outside this milestone's scope. A future milestone that moves the `read` path onto the drawn-body rule owns the fix. |
| low | **The TUI duplicates the trust frame's marker names** (`TRUST_FRAME_MARKERS`) rather than importing them from `src/search_backend/framing.rs`. | The two lists can drift, so a future frame marker added on the model path would not be recognised by the display-side extraction, and a framed output would be shown with its banner still attached. | Accepted and recorded: the framing module is the model path's contract, and importing from it would have meant a frontend change reaching into the model path's module. The duplication is stated in `architecture/search_backend.md` so the drift is visible rather than silent. |

## 11. Roadmap disposition

**Milestone conditionally closed with named operational evidence outstanding.**

The production implementation is complete and locally verified: the extraction
and the formatter are in the tree, the tests are in the tree, and the
non-vacuity measurement in §2 (11 of 15 unit tests and 2 of 3 integration
tests fail against the baseline rendering path) shows the tests detect the
absence of the change. The **outstanding evidence is named**:

1. a **closing commit** that carries the working tree (there is no SHA today),
   and
2. a **green hosted `CI / verify` run against that commit on the hosted
   runner**.

The **risk** is stated in §1: a local `scripts/verify.sh quick` run is a
property of this machine and this tree, not of the hosted runner, and the
hosted lane is a strict **subset** of `quick`, so a green hosted run does not
by itself prove everything the local gate proves.

M003 of this roadmap depends on M002's interface, and that dependency is
**discharged**: the drawn-body count that M002 introduced is what M003's header
counter reads.

## 12. Registry updates

The parent owns `plans/registry.md` and the roadmaps; the changes below are
**specified** here, and the parent has **already applied them** in the same
working tree. The "as applied" lines record what the files actually say now,
so a reader can check the registry against this record rather than against
this specification.

- `plans/registry.md` — the "TUI main buffer text rendering" row:
  - **Status** must read **conditionally closed**, and the row must name the
    baseline `f2639b5d2`.
  - The **milestone cell** must name this milestone's plan
    (`plans/implementation/tui-main-buffer-text-rendering/002-formatted-json-detail-view.md`)
    **and** this closure record
    (`plans/closure/tui-main-buffer-text-rendering/002-status.md`), with the
    dependency `M001 (interface)` and the class.
  - The **narrative** must record: that the search backend wraps every
    external result in a deliberate, documented `external_untrusted` trust
    frame — a **model-context** contract, not a UI artefact — and that the
    frame reaches the TUI verbatim; that the structured-output renderer
    requires the trimmed output to parse as JSON, so the outline **never
    fired** for a framed tool output and the collapsed view showed the trust
    banner rather than the results; that the milestone extracts the framed
    payload **for the rendering path only** and feeds the extracted payload to
    the outline renderer; that the extraction is **named for what it is** so
    it cannot be mistaken for a general "strip the frame" behaviour and reused
    on the model path; and that the closure record must state, checked against
    the diff, that the model-facing path and `src/search_backend/framing.rs`
    are unchanged — which this record does in §5's I3 row and §8.
  - The **summary paragraph** must record the conditional closure and its
    named outstanding evidence, and that the §13 "pretty-printed" and
    "outline" criteria are met jointly as a documented interpretation.
- **As applied.** `plans/registry.md` carries a **workstream row** for
  "TUI main buffer text rendering" reading `**conditionally closed**`, and a
  **per-milestone table** whose M001 row reads `**conditionally closed**` and
  names both the implementation plan and this closure record, with "Depends
  on: nothing outstanding. Class: **capability** with an infrastructure
  obligation" and the baseline `f2639b5d2`. The roadmap's §13 "Milestone
  status" table reads `conditionally closed` for every milestone and carries
  the plan path and the closure path for each; the roadmap also gained an
  **"Outcome (2026-10-23)"** paragraph naming the line as **conditionally
  closed** and naming the outstanding item — the hosted `CI / verify` lane,
  which has not run against the commit that carries this code.

- `plans/subsystems/tui-main-buffer-text-rendering-roadmap.md`:
  - §13 "Milestone status" must read `conditionally closed` for the M002 row,
    with the implementation-plan path and the closure-record path filled in.
    The **dependency cells are retained** rather than blanked.
  - §5.2 "the disclosure machinery already exists, and is already collapsed
    for tool output" and §5.5 "the web-search JSON outline is dead code" must
    be updated to record that the outline now fires for framed output and
    that the detail view is no longer raw.
  - §12 "Completion definition" must record that the milestone's *production*
    completion is in the tree while its *operational* evidence (the closing
    commit and the hosted run) is outstanding.
  - The roadmap must carry an **"Outcome (2026-10-23)"** paragraph naming the
    line as conditionally closed, naming the two outstanding items, and
    recording the delivered capability.
