# TUI Main Buffer Text Rendering Milestone M001 — Word-Aware Wrapping in the Main Buffer

Status: implemented — closure record at `plans/closure/tui-main-buffer-text-rendering/001-status.md`

Repository baseline: `f2639b5d2` (`origin/main`)

Source roadmap: `plans/subsystems/tui-main-buffer-text-rendering-roadmap.md#milestone-m001--word-aware-wrapping-in-the-main-buffer`

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- None. This milestone changes no ownership, identity, protocol, authorization
  or durability decision, so it does not meet the ADR threshold in
  `plans/adrs/README.md`. It does not touch ADR-0042, which governs the
  composer.

Primary class: capability (roadmap C1) with an infrastructure obligation
(roadmap IN1)

## 1. Objective

Make the main buffer's model-authored prose break at **word boundaries** rather
than at arbitrary characters, through one styled primitive that the composer
roadmap will also consume.

The primitive is the deliverable that matters; the prose switching to it is
the observable result.

## 2. Why this milestone is ready

- **No hard dependency.** It is the first milestone of the line.
- **The interface it fixes is consumed elsewhere.** The composer roadmap's M001
  needs a styled, word-aware primitive with byte-offset preservation. Fixing
  that signature here — rather than leaving two wrappers to be written — is
  why this milestone exists in its own right.
- **The disclosure model it must not break is already settled.** The main
  buffer's rendered rows and the scrollbar's row count come from one producer
  today. Preserving that is a constraint, not a prerequisite.

## 3. Current implementation evidence

Verified at baseline `f2639b5d2`.

### 3.1 Three wrap engines

| Engine | Location | Rule | Applies to |
|---|---|---|---|
| word-aware | `src/tui/wrap.rs` (`wrap_to_strings`) | whitespace; over-long tokens hard-broken | user body, tool header, plain tool output, shell stdout/stderr |
| character-aware | `src/tui/components/messages.rs` (`wrap_styled_spans`, `wrap_code_spans`) | every character | assistant prose, reasoning body, streaming tail, markdown headings, blockquotes, list items, fenced code blocks, `read` tool output |
| unwrapped | raw `Line::from(...)` | none | tool-input preview, shell command header, projection metadata, image alt text, run-cell title, "(+N more lines)" hints |

The character-aware engine is the one that breaks mid-word, and it is the one
the model's own output goes through.

### 3.2 The character-aware engine emits one span per character

`wrap_styled_spans` builds its output by pushing a `Span` per character. On a
long answer that is one span per displayed character, rebuilt on every layout.
Replacing that with word-sized spans is a reduction in per-layout allocation
proportional to the answer's length, and it is a free side effect of doing
the wrapping properly.

### 3.3 The word-aware primitive loses styles and byte offsets

`wrap_to_strings` operates on `&str` and returns `Vec<String>`. It cannot be
used directly on styled spans, and it cannot report where a wrapped row came
from in the original text. The composer needs both (a selection highlight
that survives the wrap); the transcript needs the styles.

### 3.4 The escape-sequence hazard is real and measured

`wrap_osc8` builds `ESC ]8;; URL BEL text ESC ]8;; BEL`. For an auto-linked
URL the target **is** the text, so the URL appears **twice** in the span's
content. The whole blob contains no whitespace, so a word-wrap treats it as
one word and hard-breaks it at an arbitrary character — which can cut the
escape sequence in half. `src/tui/wrap.rs` contains no escape-sequence
awareness today (verified: no occurrence of the escape byte, `osc`, or
`\u{1b}`).

The same blob also inflates the measured width, because the printable `]8;;`
and the duplicated URL both count, so an *n*-character URL measures roughly
`2n + 8` columns.

### 3.5 The row count and the scrollbar share one producer

`build_message_rows` produces the rendered rows; `MessageLayoutCache` holds
them and answers `total_lines()`; `scrollbar_state()` is built from that
count; the renderer draws the same `Vec<Line>` objects. The scrollbar cannot
drift from the content as long as a wrap change lands inside that producer.

### 3.6 Two search functions have no coverage and both are broken today

- `highlight_match_in_rendered` returns the rendered lines unchanged unless
  the match is on the first logical line of its part, and it compares a
  **byte** offset against **display columns**. Searching inside a long
  paragraph therefore highlights nothing.
- `scroll_to_current_search_match` adds a **visual** row offset (the
  message's first row in the cache) to a **logical** newline count (the
  match's line within the part). On a wrapped paragraph the two coordinate
  systems do not agree, so `n`/`N` lands on the wrong row.

Neither has a test. Changing the wrap algorithm under an unmeasured function
that depends on it is how a silent regression becomes permanent.

## 4. Invariants that must not regress

- **I1 — one producer for rows and for the row count.** The wrap change lands
  inside the row producer; the scrollbar's count and the drawn rows continue
  to come from the same `Vec<Line>`.
- **I2 — the TUI renders what the daemon sent.** No message part is
  synthesized, dropped or reordered.
- **I3 — the trust frame stays in the model path.** This milestone does not
  touch the model-facing path at all. (M002 parses the frame for rendering;
  this milestone does not.)
- **I4 — nothing is silently unreachable.** No content becomes less reachable
  than it is today.
- **I5 — a wrap never breaks an escape sequence.** OSC-8 spans are excluded
  from word segmentation and from width measurement.

## 5. Scope

### In scope

- One styled, word-aware wrapping primitive in `src/tui/wrap.rs`:
  word boundaries, per-word style preservation, **byte-offset preservation**,
  escape-sequence exclusion, and hard-breaking of a token wider than the row.
- The assistant prose, the reasoning body and the streaming tail use it.
- The removal of the one-span-per-character construction.
- Test coverage for the two unmeasured search functions, written so that they
  **fail against the baseline**.

### Explicitly out of scope

- Fixing the search highlight or the search scroll. They are made
  **observable**, not repaired (roadmap §10, R4).
- The unwrapped paths. D9: the wrap plus the collapsed default are the
  remedy, and those paths are M003's business.
- The disclosure model, the detail view and the click targets (M002–M004).
- The frame extraction (M002).
- Any change to the daemon's projection or to the model's context.

## 6. Required production changes

### Core/domain

None. This is a rendering concern.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None.

### Frontend or operator surface

**`src/tui/wrap.rs` — the primitive.** Add a styled, word-aware wrapping
function. Its requirements:

- it accepts styled spans and a width, and returns wrapped lines;
- it breaks at whitespace, and hard-breaks a token that alone exceeds the
  width at character boundaries — the same rule the existing word-aware
  wrapper uses, so the two agree;
- it preserves each word's style, splitting a span at word boundaries rather
  than flattening it;
- it reports the **byte range** of the original text behind each wrapped row,
  so a consumer can map a byte offset to a row and a column;
- it **excludes escape sequences** from word segmentation and from width
  measurement, so an OSC-8 span is never split and never inflates the width
  of the text around it.

The primitive's **signature is fixed by this milestone**: the composer
roadmap's M001 consumes it, and the signature is what that consumption is
written against. Any change to it after this milestone closes is a change to
an interface another roadmap depends on.

**`src/tui/components/messages.rs` — the consumers.** The character-aware
wrapper gains word awareness, or is replaced by the new primitive. The
assistant prose, the reasoning body and the streaming tail route through it.
The one-span-per-character construction goes away.

**Escape-sequence handling.** The exclusion must be a property of the
primitive, not a special case at each call site, so a new consumer inherits
it.

### Security and authorization

None. No authority changes.

### Documentation and static guards

- `architecture/tui.md` — the main buffer's wrap algorithm and the primitive
  it names.
- `docs/tui.md` — only if a user-visible behaviour changes. The visible
  change is that prose stops breaking mid-word; if the document describes
  wrapping today, it is corrected.
- `.opencode/skills/tui/SKILL.md` — if it states the wrap behaviour.
- **No new static guard.** `scripts/check_test_assertion_quality.py` and
  `scripts/check_test_duplication.py` already run in `scripts/verify.sh quick`
  and CI; the new tests are subject to them.

## 7. Ordered work packages

### Work package A — The search coverage lands first

Intent: the two unmeasured search functions become observable **before** the
algorithm under them moves.

Required changes: add tests for `highlight_match_in_rendered` and
`scroll_to_current_search_match` that assert their **actual current
behaviour**, and mark the known defects explicitly in the test's own comment
and in the plan's closure evidence. The tests must pass against the baseline
and must continue to pass after the wrap change **unless** the wrap change
moves them — in which case the change is recorded as an observation, not
silently absorbed.

Acceptance evidence: the two tests exist, pass against the baseline, and are
runnable by name.

### Work package B — The primitive

Intent: one styled, word-aware, byte-offset-preserving, escape-aware wrapper.

Required changes: add the primitive to `src/tui/wrap.rs` with focused unit
tests for word boundaries, an over-long token, style preservation across a
word boundary, byte-offset preservation, and the escape-sequence exclusion.

Acceptance evidence: the unit tests pass; the escape-sequence test asserts
that a wrapped OSC-8 span still contains the complete sequence, and that the
sequence's printable parts are not counted toward the row width.

### Work package C — The model-authored paths switch

Intent: the assistant prose, the reasoning body and the streaming tail break
at word boundaries.

Required changes: route the three paths through the primitive; remove the
one-span-per-character construction; keep the change inside the row producer
so I1 holds.

Acceptance evidence: a rendered assistant answer breaks at word boundaries;
the reasoning body and the streaming tail do the same; the scrollbar's
`total_lines` still equals the number of rows drawn (an integration
assertion, not a code-reading claim).

### Work package D — The documents

Intent: the architecture doc names the algorithm that is actually shipped.

Required changes: update `architecture/tui.md`'s main-buffer text-rendering
section; correct `docs/tui.md` and the TUI skill if either states the old
behaviour; verify every count they state against its listed source.

Acceptance evidence: the documents describe the delivered behaviour, and any
count they state is verified rather than copied.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable. The main buffer is frontend-local; there is no concurrent
writer, no delivery to reconcile and nothing to recover. A wrap that fails to
compute must fall back to the current character-aware behaviour rather than
dropping content (I4) — the failure mode is "no worse than today", never
"missing".

## 9. Compatibility and migration

- **Rendering only.** No stored state, no protocol, no DTO, no migration.
- **The model's context is untouched.** This milestone changes only what the
  user sees.
- **The transcript's existing tests** that assert character-breaking must be
  reviewed one by one: each is rewritten to the word-boundary contract or kept
  as the contract it actually tests, with the disposition recorded.
- **The editor is untouched.** It has no soft wrap today and this milestone
  does not give it one.

## 10. Required tests

### Focused unit tests

- The primitive: word boundaries; an over-long token hard-broken at character
  boundaries; a style that spans a word boundary; the byte range of each
  returned row; an empty input; a width of zero.
- The escape exclusion: an OSC-8 span survives a wrap whole; the sequence's
  printable parts do not consume the row width; a span that is **only** an
  escape sequence is not split.
- The model-authored paths: a rendered assistant answer contains no row that
  ends mid-word (except at a hard-broken over-long token).

### Integration tests

- The rendered row count of a message equals the number of rows drawn, and
  the scrollbar's `total_lines` equals it (I1).
- A message containing a tool call, a reasoning block and assistant prose
  renders with a row count equal to the sum of its parts' rows.

### Restart and recovery tests

None. No durable state.

### Contention and cancellation tests

None. Frontend-local.

### Security and negative tests

- The escape-sequence integrity test above is the security-relevant case: a
  split escape sequence is a terminal injection.
- A test that the model-facing path is unchanged: the milestone touches no
  model-context code (I3).

### Migration and compatibility tests

None. No migration.

### Non-vacuity

Every new test must **fail against the baseline tree**. The repository's
`scripts/check_test_assertion_quality.py` requires every test body to assert
a checkable property, and `scripts/check_test_duplication.py` requires that
no two bodies assert the same thing twice. Both run in
`scripts/verify.sh quick` and in CI. A test written to pass on the baseline
because it asserts the current broken behaviour must say so in its own name
and comment.

## 11. Required verification commands

```bash
# the primitive and the consumers, narrowly
cargo test --locked -p codegg --lib tui::wrap
cargo test --locked -p codegg --lib tui::components::messages

# the search coverage, by name
cargo test --locked -p codegg --lib tui::components::messages::tests::highlight_match
cargo test --locked -p codegg --lib tui::components::messages::tests::scroll_to_current_search_match

# the rendered-row contract
cargo test --locked -p codegg --test tui_render

# static guards that cover the new tests
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py

# formatting and the workspace check
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked

# the routine gate
bash scripts/verify.sh quick
```

Do not claim any command that was not actually run. The closure record labels
local evidence as local and hosted evidence as hosted.

## 12. Documentation updates

- `architecture/tui.md` — the main buffer's text rendering: the wrap
  algorithm, the primitive, the escape-sequence rule.
- `docs/tui.md` — the user-visible prose wrapping, if it is described.
- `.opencode/skills/tui/SKILL.md` — the module guide's contract, if it
  states the wrap behaviour.
- `plans/registry.md` — this milestone's row, when it closes.

## 13. Acceptance criteria

- The main buffer's assistant prose, reasoning bodies and streaming tails
  break at word boundaries. A row ends mid-word only when the word alone is
  wider than the row.
- A styled span's style is preserved across a word boundary, and a
  selection-range-like byte interval maps to the wrapped row that contains it.
- An OSC-8 hyperlink span survives a wrap whole, and its printable parts do
  not consume the row width.
- The scrollbar's row count equals the number of rows drawn, for a message
  containing at least one tool call, one reasoning block and assistant prose.
- The two search functions have tests, and the known defects are recorded
  rather than absorbed.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- the primitive's signature must change after the composer roadmap has
  written against it;
- making the row count agree with the drawn rows would require changing the
  producer, rather than the wrap;
- the escape-sequence exclusion cannot be made a property of the primitive
  without a per-call-site special case;
- the search functions' behaviour changes under the new wrap in a way that
  cannot be attributed;
- scope would expand into the disclosure model (M002–M004) or into the model
  context.

## 15. Closure evidence required

- The primitive's unit tests, with the escape-sequence integrity case.
- The three model-authored paths' rendered output, showing word boundaries.
- An integration assertion that the scrollbar's `total_lines` equals the
  drawn row count (I1).
- The two search functions' tests, with their baseline behaviour and their
  post-change behaviour **both** recorded, and any difference attributed.
- A statement that the model-facing path was not touched (I3).
- The measured effect on per-layout span allocation, reported as a
  measurement and not as an estimate.
- `scripts/verify.sh quick` green at the closing commit.
- A statement of which existing transcript tests were rewritten and which were
  kept, with the disposition of each.

## 16. Handoff notes

- The repository's own workspace is large; prefer the narrow targets above.
  `scripts/verify.sh scoped` selects by `docs/test-ownership.toml` and is the
  right tool when the change's blast radius is uncertain.
- The workspace test suite is slow under contention. Use
  `XDG_CACHE_HOME="$(mktemp -d)" cargo nextest run --workspace --locked --profile ci`
  when a full sweep is required, so the result is a property of the product
  and not of the host's cache.
- The wrap runs on the render path. Measure the cost; do not assert it.
- The escape-sequence hazard is the one failure that would be hard to notice
  and easy to ship. Its test must exist before the primitive is merged.
