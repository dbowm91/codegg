# Harness Practice Adoption Milestone 007 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/harness-practice-adoption/007-escaped-defect-postmortems.md`

Source subsystem roadmap:

- `plans/subsystems/harness-practice-adoption-roadmap.md#m007--escaped-defect-postmortems`

Repository baseline reviewed: `7642b9e38` (`feat-auto-20261109-f8abad0d`), plus
uncommitted working-tree work

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`, on top of HEAD `7642b9e38`.
  `docs/postmortems/README.md`, `docs/postmortems/0001-duplicate-tool-call-part-id-key.md`
  and `scripts/check_postmortems.py` are untracked additions
  (`git ls-tree HEAD docs/postmortems` is empty); `scripts/verify.sh`,
  `.github/workflows/ci.yml` and `architecture/testing.md` carry uncommitted
  edits. Nothing below is attributable to a SHA.

The source plan still reads `Status: ready for handoff`, and the roadmap's §12
status table still reads `not started` for M007. **Both are stale and are not
this record's to change** — the plan `Status:` line and the roadmap table are
the parent's.

One anchor note: the six sibling records link the roadmap as
`#milestone-00N--<slug>`, but the roadmap's headings are `### M00N — <title>`,
whose anchors are `#m00n--<slug>`. This record uses the form that resolves.

## 1. Executive finding

The convention, one seeded record, the guard, and both registrations exist, and
every milestone-local gate ran green here:

- `python3 scripts/check_postmortems.py` — **"postmortem guard passed"**, exit
  0, **0.093 s**.
- `python3 scripts/check_postmortems.py --self-test` — **16 injections
  rejected**, plus a passing control, exit 0, **1.13 s**.
- The four guards the seeded record names — **all four pass**, run individually
  by exact path.
- Registration is present in both `scripts/verify.sh` `run_guards()` and the CI
  `verify` job, and the derived sets are still **94 quick / 80 CI, CI-only 0**,
  so CI remains a strict subset of `quick`.

**The seeded record is not decoration, and the defect it documents is real on
`main`.** The collision is present at HEAD `7642b9e38` *and* at `main`'s tip
`cb4ceed9d`: `PartInfo` carries `id` and `PartData::ToolCall` carries an
unrenamed `id` joined by `#[serde(flatten)]`. `grep -c toolCallID` returns `0`
for both `crates/codegg-core/src/session/message.rs` and
`crates/codegg-protocol/src/dto.rs` on `main`. The shape entered the tree on
`2026-09-25` in `d820dfb4c` ("plans(ci): close M001, unblock M002"), which
`git merge-base --is-ancestor d820dfb4c main` confirms is on `main` — so
"escaped to a merged branch" is evidenced, and the stronger statement is
available: the defect sits on `main` today and the fix is uncommitted on this

**Owner ruling, 2026-11-12.** The fix stays uncommitted in this worktree and is expected to land together with the rest of the four planning lines' work; no separate cherry-pick, hotfix, or release is required. The defect on `main` remains real until that commit lands, and this record is the artifact that says so.

branch. No release tag exists in this clone (the one tag is
`backup/pre-main-merge-ae0920ebd`), so **no claim that it shipped in a release
is made**.

**The root cause is sharper than "nothing round-tripped a tool call."** The
pre-fix suite did have serialization coverage:
`test_message_data_serialization` and `test_part_info_serialization` both exist
at HEAD and both build `PartData::Text` — the one variant with no field named
`id`. The colliding variant was never the variant under test.
`test_part_data_tool_call` builds `PartData::ToolCall` and asserts only
`matches!(part, PartData::ToolCall { .. })` — a construction check that reads as
coverage and asserts nothing about the wire form.

**The production victim is worse than a failed import, and the record says so.**
`MessageStore::create_with_id` inserts the row and *then* decodes the value it
was handed (`crates/codegg-core/src/session/store.rs:2271`), so on a tool-call
message the insert commits and the decode fails. `cmd_import` uses `?` per
message (`src/main.rs:2758`) and therefore aborts mid-import with a
half-imported session, and it cannot retry idempotently because
`MessageStore::create` mints a fresh UUID per attempt. The turn path is worse
still: `persist_turn_transcript` encodes with `serde_json::to_value`
(`src/agent/turn_completion.rs:525`) and `append_batch` commits the encoded
string without decoding it (`store.rs:2328`), so tool-using turns persisted
**silently** and failed on the **next read**.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A convention document with the rule, the required sections, the naming scheme and the boundary against closure records | `docs/postmortems/README.md`, 153 lines: "The rule", "Required sections", "Naming", "The `Guard added` entry", "The boundary against closure records" | pass | |
| Exactly the eight declared sections, in order, and no undeclared ninth | README's template block (`:31-75`) matches `DECLARED_SECTIONS` (`scripts/check_postmortems.py:69`); plain run green | pass | Template drift is a self-test injection, not a review habit |
| The boundary sentence is present verbatim | README `:131-137`; `BOUNDARY_SENTENCE` at `check_postmortems.py:86`; the self-test removes it and is rejected | pass | |
| At least one seeded record from a defect already fixed and already guarded | `docs/postmortems/0001-duplicate-tool-call-part-id-key.md`, 191 lines | pass | The plan named no seed; WP-B said "select a defect" |
| The seeded defect escaped to a merged branch | The colliding shape is on `main`'s tip `cb4ceed9d`; introduced `2026-09-25` by `d820dfb4c`, an ancestor of `main` | pass | No release tag; "shipped" is not claimed |
| The record's reproduction is a command or a test path, not prose | Fenced blocks: the standing `cargo test -p codegg-protocol …`, two `git show` commands that print the pre-fix shape, and the minimal bytes as JSON | pass | |
| The record has all eight sections, each with a non-empty body | Section lines 3, 14, 21, 38, 70, 94, 123, 162; guard green | pass | |
| Every `Guard:` claim resolves to something a guard can be | `crates/codegg-protocol/src/dto.rs::tool_call_part_encodes_exactly_one_id_key` (`:844`); `crates/codegg-core/src/session/message.rs::tool_call_part_round_trips_with_distinct_part_and_call_ids` (`:311`); `crates/codegg-core/src/protocol_conversions.rs::message_to_dto_accepts_a_tool_call_and_preserves_both_ids` (`:1147`) and `::both_crates_decode_the_same_tool_call_part_json` (`:1177`) | pass | Three core-side tests: one in `message.rs`, two in `protocol_conversions.rs` |
| Each named guard passes today | Four `cargo test … -- --exact` runs, **1 passed / 0 failed** each | pass | |
| A test claim is resolved through `scripts/rust_test_scan.py`, with no second parser | `_check_claim` reads the scanner's inventory, and the self-test's "test function that does not exist" case is rejected while the scanner prints the file's 21 real tests | pass | The plan's stop condition |
| Every `Must mention:` token appears in the named test's own body | All four tokens (`toolCallID`, `tool-call id must survive`, `survive the bridge`, `toolCallID`) are in the bodies; the self-test's silent-guard case is rejected | pass | This is the anti-form-only check |
| Numbering is `NNNN-<kebab-slug>.md`, unique and contiguous from `0001` | `RECORD_NAME` at `:93`; the self-test rejects a duplicate number, a gap and a three-digit name | pass | |
| `Status` names a declared state; `Date` is a real `YYYY-MM-DD` | `Fixed.`; `2026-11-11`; `STATUS_STATES` at `:101` | pass | |
| No record carries a credential-shaped string | Five patterns at `:108`; the self-test injects `sk-proj-…` and is rejected | pass | No absolute home path either — convention, not guarded |
| No record path appears in `plans/registry.md` | `_check_registry` at `:328`; `grep docs/postmortems plans/registry.md` finds nothing | pass | |
| A script claim without `--self-test` is rejected | Self-test case "claimed guard without `--self-test`" | pass | |
| `--self-test` rejects every injection | **16/16**, plus the unmodified copy accepted | pass | The plan asked for five |
| Registered in `scripts/verify.sh` | `:647-657`, inside `run_guards()`, `--self-test` then plain | pass | No `run_quick()` change needed |
| Registered in the CI `verify` job | `.github/workflows/ci.yml:499-507`, two steps | pass | |
| The guard invokes no `cargo` and adds no build cost | Its only `cargo` text sits inside the synthetic record's fenced reproduction; the run is 0.093 s | pass | |
| CI is still a strict subset of `quick` | 94 quick / 80 CI / **CI-only 0** / 14 quick-only, exactly the AGENTS.md list | pass | |
| `architecture/testing.md` gains the paragraph the plan asked for | `:228-243`, "When a Defect Escapes" | pass | |
| **The named guard's own self-test rejects its injection** | — | **partial** | The named guards are tests, which have no `--self-test`; the defect reintroduction was **not run** (§4, §10) |
| `scripts/verify.sh quick` and `full` | — | **not run** | Excluded by this session; `quick` is red for 22 comment-budget violations independent of this milestone |
| No production behaviour changed | Only `docs/postmortems/`, `scripts/check_postmortems.py`, two verify entry points and one `architecture/testing.md` section | pass | |

## 3. Production implementation evidence

**Nothing ships.** This milestone adds no module, feature, configuration key,
environment variable, CLI flag, migration or protocol variant. Its artifacts:

- `docs/postmortems/README.md` — the convention, 6,192 bytes.
- `docs/postmortems/0001-duplicate-tool-call-part-id-key.md` — the seeded
  record, 9,741 bytes.
- `scripts/check_postmortems.py` — the guard, 834 lines, 30,552 bytes.
- `scripts/verify.sh:647-657` — eleven added lines (six of them comment), no
  deletions.
- `.github/workflows/ci.yml:499-507` — two steps, eight added lines, no
  deletions.

**The convention.** Three things are load-bearing. The **rule** is that a defect
which escapes gets a dated record *in the same change that adds the guard*, and
that escaped means it got past the evidence that existed when it was written —
the test suite, the guards, the review of the change that introduced it. The
**sections** are exactly eight, and a ninth is a deliberate edit to the guard
rather than a side effect of a record. The **boundary** is one sentence: *a
closure record says what a milestone built; a postmortem says what escaped and
what now prevents it.*

**The seeded record's guard entry is the mechanism that keeps it honest.** Each
bullet carries `Guard:`, `Self-test:` and `Must mention:`. For a script guard the
first is a `scripts/` path and the guard demands `--self-test` in it. For a test
the first is `<path>.rs::<test fn>`, resolved by
`scripts/rust_test_scan.py`'s inventory — the plan's stop condition forbids a
second Rust parser, and there is none — and `Must mention` must then appear in
**that test's own body**. A record therefore cannot name a plausible path that
does nothing, because the guard cannot have deleted every reference to the
defect it prevents.

**What the seeded record documents.** A tool-call part serialized `id` twice:
once from `PartInfo`, once from `PartData::ToolCall` under `#[serde(flatten)]`.
Decoding then had two disagreeing failure modes — `from_str` rejected the row
with `` duplicate field `id` ``, while `to_value` collapsed the duplicate into a
map and lost the part id, so the bridge reported `` missing field `id` `` for a
value it had just produced. The fix gives the tool-call id its own wire name
(`toolCallID`) plus a hand-written `Deserialize` that recovers a pre-rename row
and synthesizes the part id as `{messageID}:{toolCallID}`. It owes no migration:
every write is canonical, reads recover in place, and `STORAGE_LAYOUT_VERSION`
was not bumped.

**The facts the implementing session corrected, which are the record's
substance:**

- **The defect is at HEAD, not hypothetical.** `git show
  7642b9e38:crates/codegg-core/src/session/message.rs` still prints the
  unrenamed `ToolCall { id }`, and `main` still does. The record dates it to
  `2026-09-25`; that is the commit that added the file, and it is on `main`.
- **The suite did not lack serialization coverage; it lacked the right
  variant.** Both existing serialization tests built `PartData::Text`. The
  colliding variant had only a `matches!` test.
- **The victim is `cmd_import` and the turn path.** The import aborts mid-way
  with `?`; the turn path writes through `to_value` and `append_batch` commits
  undecoded, so failure surfaced on the next read.
- **`TOOL_CALL_ID_FIELD` is deserializer-only.** The enum field carries the
  literal `#[serde(rename = "toolCallID")]` (`message.rs:140`,
  `dto.rs:1049`); the constant (`message.rs:45`, `dto.rs:967`) exists for the
  manual `Deserialize`'s recovery arm.

**What this milestone does not claim.** The fix is another lane's uncommitted
work. This milestone claims the convention, the record and the guard. It also
does not claim the record's four `Self-test:` lines were executed — §4 and §10
say what was and was not run.

## 4. Verification executed

### Commands run

```bash
python3 scripts/check_postmortems.py
python3 scripts/check_postmortems.py --self-test

cargo test -p codegg-protocol --lib -- --exact dto::tests::tool_call_part_encodes_exactly_one_id_key
cargo test -p codegg-core --lib -- --exact session::message::tests::tool_call_part_round_trips_with_distinct_part_and_call_ids
cargo test -p codegg-core --lib -- --exact protocol_conversions::tests::message_to_dto_accepts_a_tool_call_and_preserves_both_ids
cargo test -p codegg-core --lib -- --exact protocol_conversions::tests::both_crates_decode_the_same_tool_call_part_json

bash -n scripts/verify.sh
python3 -c "import yaml; d = yaml.safe_load(open('.github/workflows/ci.yml')); print(list(d['jobs']))"

# the AGENTS.md derivation, both directions (run through `mktemp` files)
Q=$(mktemp); C=$(mktemp)
awk '/^run_guards\(\)/,/^}/' scripts/verify.sh | grep -oE 'scripts/check[-_][a-z0-9_.-]+\.(py|sh)' | sort -u > "$Q"
grep -oE 'scripts/check[-_][a-z0-9_.-]+\.(py|sh)' .github/workflows/ci.yml | sort -u > "$C"
comm -13 "$Q" "$C"   # CI-only
comm -23 "$Q" "$C"   # quick-only

python3 scripts/check_comment_budget.py
git status --porcelain
git log --oneline -3
git ls-tree HEAD docs/postmortems
git show main:crates/codegg-core/src/session/message.rs | sed -n '/^    ToolCall {/,/^    },/p'
git merge-base --is-ancestor d820dfb4c main
```

### Results

- `check_postmortems.py` — **green**, exit 0, `postmortem guard passed`,
  **0.093 s** real. No `cargo`, no build.
- `check_postmortems.py --self-test` — **green**, exit 0, **1.13 s**, final line
  `postmortem guard self-test passed (16 injections rejected)`. The control is
  the unmodified copy, which is accepted before any injection runs; the
  self-test copies `docs/postmortems/`, `plans/registry.md`, all of `scripts/`
  and the `.rs` files the record claims into a temporary directory, so it
  exercises the real validator rather than a mock. The 16 rejections, each with
  a distinct reason:
  a record **missing a required section** (`Blast radius`);
  **sections out of the declared order**;
  **a section nobody declared** (`Timeline`);
  **a claim on a script that does not exist**
  (`scripts/check_no_such_guard_exists.py`);
  **a claimed guard without `--self-test`**;
  **a duplicated number**;
  **a claim on a test function that does not exist** (the scanner names the 21
  that do);
  **a claim whose guard never mentions the defect** (test-body token);
  **a script claim that never mentions the defect**;
  **a malformed record name** (`1-not-four-digits.md`);
  **a gap in the numbering**;
  **a credential-shaped string** (`sk-`);
  **an empty record**;
  **a claim on something that is not a guard** (`docs/postmortems/README.md`);
  **template drift between the README and the guard**;
  **the boundary sentence removed**.
- The four named guards, run by exact path — **1 passed, 0 failed** each:
  - `dto::tests::tool_call_part_encodes_exactly_one_id_key` — 334 filtered;
    0.01 s. It counts **raw top-level keys** with a visitor and asserts one `id`
    and one `toolCallID`, then calibrates the counter by counting two `id` keys
    in a synthetic duplicate-key object — the instrument that hid the defect is
    the instrument the test proves non-vacuous.
  - `session::message::tests::tool_call_part_round_trips_with_distinct_part_and_call_ids`
    — 1,015 filtered; 0.02 s. Asserts the part id `m1_0` and the call id
    `call_1` survive **distinctly**; with the shared wire name the round trip
    fails on `duplicate field` inside `.expect`.
  - `protocol_conversions::tests::message_to_dto_accepts_a_tool_call_and_preserves_both_ids`
    — 0.02 s. Asserts both ids across both legs of the bridge.
  - `protocol_conversions::tests::both_crates_decode_the_same_tool_call_part_json`
    — 0.00 s. Decodes a canonical and a pre-rename row with both crates and
    compares the results field by field.
- `bash -n scripts/verify.sh` — **syntax OK**.
- YAML parse of `.github/workflows/ci.yml` — **OK**; one job, `verify`.
- Guard-set derivation — `quick` **94** unique, CI **80** unique, **CI-only 0**,
  **quick-only 14**, and the 14 are exactly the set AGENTS.md names (the two
  boundary scripts, `check_daemon_listing_authority`, `check_eggwork_target_routing`,
  `check_first_party_skills`, `check_network_intent_owner_wiring`,
  `check_no_hardcoded_selection_colours`, `check_openai_endpoint_composition`
  and the six provider guards). `check_postmortems.py` is in both sets.
- `check_comment_budget.py` — **RED, exit 1, 22 violations**, none of them in
  `docs/postmortems/` or `scripts/check_postmortems.py` (the guard scans `*.rs`
  and `*.inc` only, so a postmortem cannot appear in it at all). Recorded
  because it is the parent's to bank and because it blocks `quick`. The
  violations closest to this line are the **fix's** files, not this
  milestone's: `crates/codegg-protocol/src/dto.rs` (335 comment lines vs 299;
  ratio 0.2262 vs 0.2217) and `crates/codegg-core/src/protocol_conversions.rs`
  (92 vs 82; ratio 0.0880 vs 0.0864). The rest are the TUI lanes,
  `codegg-config`, `src/main.rs` and `tests/common/session_replay.rs`.
- `git status --porcelain` — `?? docs/postmortems/`,
  `?? scripts/check_postmortems.py`, and `plans/closure/harness-practice-adoption/`
  untracked as a whole; `M .github/workflows/ci.yml`, `M scripts/verify.sh`,
  `M architecture/testing.md` among many sibling edits.
- `git log --oneline -3` — HEAD is `7642b9e38 test(tui): guard the tree's row
  inputs and record the line's fixes`; the two commits below it are the TUI
  lane's. **This is uncommitted working-tree work.**
- `git ls-tree HEAD docs/postmortems` — empty: the directory does not exist at
  HEAD.
- `main` still carries the defect: `d820dfb4c` (2026-09-25) is an ancestor of
  `main`, and `main`'s `ToolCall` variant still has the unrenamed `id` with
  `grep -c toolCallID` = 0 in both crates.

### Not run

- **`scripts/verify.sh quick` and `scripts/verify.sh full`** — **not run**,
  excluded by this session's rules because `check_comment_budget.py` is red for
  22 violations on files this milestone does not own. The substitute is the
  individual check above: the milestone's own guard in both forms, `bash -n` on
  the entry point, a YAML parse of the workflow, and the derived set
  arithmetic. What that substitute **cannot** show is that the surrounding
  guards stay green with this registration in place — `quick` runs 94 of them
  and this session ran two (`check_postmortems.py` and
  `check_comment_budget.py`). That residue is recorded in §10.
- **The defect reintroduction.** The record's four `Self-test:` lines state what
  would make each guard fail, and none was executed: doing so means editing
  `crates/codegg-protocol/src/dto.rs` or
  `crates/codegg-core/src/session/message.rs`, which this session's scope
  forbids. The substitute is the assertion text itself: each guard asserts the
  exact property the defect destroys (one `id` key; the two ids distinct; both
  surviving the bridge), so a reintroduced shared wire name makes
  `serde_json::to_string` emit a duplicate key and the test's own
  `from_str`/`.expect` fail. That is a reading of the assertions, not an
  observed red run, and §10 says so.
- **`cargo fmt --check` and `cargo clippy -D warnings`** — not run. No Rust was
  added by this milestone.
- **The plan's `python3 scripts/check_<named>.py --self-test`** — **not
  applicable as written.** The seeded record names four **tests**, not a
  script. The plan's §4 invariant explicitly allows a test to stand in for a
  guard; §7 WP-C and §11/§15 were written as though the guard would be a
  script. The guard implements the §4 form.

## 5. Invariant review

- **A record names a guard that exists.** All four claims resolve to defined
  test functions, and the guard rejects a claim on a deleted function, on a
  script without `--self-test`, and on something that is not a guard at all.
- **A test claim names the test's full path, resolved by the shared scanner.**
  `_check_claim` consumes `scripts/rust_test_scan.py`'s inventory; this guard
  adds no Rust parser.
- **A record is not a plan.** It does not schedule work, own a milestone, or
  appear in `plans/registry.md`. The registry check is mechanical and green.
- **A record is not a closure record.** The boundary sentence is verbatim in the
  README and the self-test fails when it is removed, so the two documents cannot
  silently merge into one.
- **Numbering is unique and monotonic.** `0001` is the seed; a deleted record
  consumes its number and leaves a gap the guard rejects.
- **A record is written after the fix.** The record exists in the same
  uncommitted tree as the four guards, and the tests it names assert the fixed
  behaviour; reverting the core half of the fix makes the three core-side tests
  fail by construction, and the protocol-side key counter fails if the protocol
  crate's rename is removed.
- **The guard is cheap.** 0.093 s, no `cargo`, no build, and the self-test's
  1.13 s copy is bounded to `docs/postmortems/`, `plans/registry.md`,
  `scripts/` and the claimed sources.

## 6. Failure and recovery review

- **A record's named guard is later deleted.** The guard fails and names the
  record and the path — self-test case "claim on a test function that does not
  exist". The documented resolution is to name the replacement, never to delete
  the record.
- **A record's number collides.** Fails; renumbering is allowed only before the
  record is committed.
- **A guard script loses `--self-test`.** Fails, and that coupling is the point:
  removing the self-test makes the record's claim false.
- **A test guard loses the defect token.** Fails — the `Must mention` token must
  appear in the named test's own body, so dropping the comment that names the
  defect turns the build red rather than leaving the record claiming protection
  it no longer has.
- **A record is written with no guard at all.** Fails: `Guard added` must carry
  at least one claim.
- **A record quotes a credential.** Fails, before it can be committed.
- **A record is added to the registry.** Fails.
- **Restart, cancellation, contention, duplicate delivery.** Not applicable: the
  guard reads files and writes only inside a temporary directory.
- **A sibling lane restructures a named file.** The guard turns red. That is
  intended behaviour and is recorded in §10 because
  `crates/codegg-core/src/session/message.rs` is edited by another lane.

## 7. Migration and compatibility review

- No schema change, no migration, no `STORAGE_LAYOUT_VERSION` bump, no protocol
  variant, no configuration key. The seeded fix's recovery arm renames a wire
  field and reads pre-rename rows in place; the postmortem records that and adds
  nothing to it.
- The registrations are additive: eleven added lines in `scripts/verify.sh`
  (six comment, two `--self-test`, two plain, one blank) and eight in
  `.github/workflows/ci.yml`, with **no deletions** in either file's
  uncommitted diff.
- CI remains a strict subset of `quick`: 80 of 94, zero CI-only.
- Numbering compatibility: `0001` is consumed. There is no scheme over Git
  history and no retroactive record — the plan's explicit non-goal.
- Template compatibility: adding a section is an edit to
  `scripts/check_postmortems.py`, so a record cannot lead the convention.
- Rollback: deleting the record would leave the number free, which the guard
  treats as a failure; renaming before commit is the only clean retraction.

## 8. Security review

- Five credential patterns are rejected at the record level (`sk-`, AWS access
  key, GitHub token, Slack token, private key), and the self-test injects an
  `sk-proj-…` value into a real record to prove it. No record carries one.
- No record carries an absolute home path either, which the README asks for on
  the same principle but the guard does **not** check — recorded in §10 as a
  known asymmetry rather than implied coverage.
- The guard executes nothing from a record: it reads text, resolves paths and
  consults the scanner's inventory. A record cannot cause a command to run, so
  the directory is not an execution surface.
- The guard holds no authority and touches no user data: it reads the repository
  and, in `--self-test`, writes only inside `tempfile.TemporaryDirectory`.
- No production authorization, path-validation or redaction surface changes.

## 9. Documentation and operations

- `docs/postmortems/README.md` — new: the rule, the eight sections, the template
  block, naming and numbering, the `Guard added` entry, what the guard checks,
  the closure boundary, the deleted-guard policy, and the security section.
- `docs/postmortems/0001-duplicate-tool-call-part-id-key.md` — new: the seeded
  record.
- `architecture/testing.md:228-243` — new "When a Defect Escapes" section: the
  rule, the two things a postmortem is not (a closure record; a blame log), and
  the token check that makes the claim mechanical.
- `scripts/check_postmortems.py` — new, with `--self-test`.
- `scripts/verify.sh:647-657` — registered last in `run_guards()`, `--self-test`
  then plain.
- `.github/workflows/ci.yml:499-507` — two steps in the `verify` job.
- Operator action: after a fix that escaped, add `docs/postmortems/NNNN-…md` in
  the same change as its guard, and run
  `python3 scripts/check_postmortems.py` before pushing.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The seeded record's four `Self-test:` lines were **not executed**. They state the injection that would make each guard fail; this session could only read the assertions | The record's claim that each guard bites rests on the assertion text and on a passing run, not on an observed red run | Reintroduce the shared wire name in a scratch copy (or on a throwaway branch) and watch `tool_call_part_round_trips_with_distinct_part_and_call_ids` fail with `` duplicate field `id` ``, and the protocol key counter report two `id` keys |
| medium | The defect is **still on `main`'s tip** (`cb4ceed9d`), and the fix that the record documents is uncommitted on this branch | If the branch is abandoned, the record describes a fixed defect that is not fixed anywhere | Commit the fix and the record together; the record must not be separated from its guard |
| low | The record names `crates/codegg-core/src/session/message.rs`, which another lane edits | A rename or removal of `tool_call_part_round_trips_with_distinct_part_and_call_ids` turns `check_postmortems.py` red inside that lane's diff — intended, but it will look like an unrelated failure | Coordinate: the sibling lane either keeps the test or updates the record's `Guard:` claim in the same change |
| low | Root `AGENTS.md` documents **84 quick / 70 CI** guards; the tree holds **94 / 80** | A reader who trusts the documents mis-derives the two sets, and the quick-only list in the prose is one milestone behind | Re-bank the two counts and re-derive the quick-only list; pre-existing drift, not introduced here |
| low | The plan's §11 command `python3 scripts/check_<named>.py --self-test` and §15's "named guard's `--self-test` output" presume a **script** guard, while the records' guards are tests | An operator following §11 gets "no such file" and may conclude the milestone is unverified | Correct the plan text: for a test guard the evidence is the exact-path `cargo test` run plus the scanner resolution |
| low | Plan §7 WP-C says "every named guard contains `--self-test`", while §4 explicitly permits a test to stand in for a guard | The two clauses cannot both hold for the seed; the guard implements §4 | Correct WP-C to "every named **script** guard contains `--self-test`" |
| low | `check_comment_budget.py` is **red, 22 violations**, so `quick` cannot be green on this tree | No `verify.sh quick` evidence exists for this milestone; the registration is proven by reading and by `bash -n`/YAML rather than by running the entry point | Bank or trim the 22; the milestone's own files are clean |
| low | The guard does not reject an absolute home path in a record, though the README asks for redaction on the same principle as a credential | A future record could commit a developer's path unnoticed | Add the check, or state in the README that only credentials are mechanical |
| low | The roadmap §12 table and the plan's own `Status:` line still say `not started` / `ready for handoff` | The roadmap understates delivered work | Parent owns both |

## 11. Roadmap disposition

**Closed.** Each acceptance criterion:

1. **A seeded postmortem exists for a fixed defect, with a command or test path
   as its reproduction** — `docs/postmortems/0001-duplicate-tool-call-part-id-key.md`,
   three fenced reproduction blocks, eight sections in order.
2. **The record's named guard exists and its self-test rejects its injection** —
   **partially evidenced, in the form the plan's §4 allows.** All four named
   guards exist and pass by exact path; the postmortem guard, run here, resolves
   every claim through the scanner and rejects a claim that stops resolving or a
   token the test body lacks. What was not done is a reintroduction of the
   defect: that edits production code outside this session's scope, and §10
   records it as a low finding with the exact future evidence.
3. **`scripts/check_postmortems.py` rejects all five of its self-test
   injections** — it rejects **sixteen**, above.
4. **Registered in `scripts/verify.sh quick` and in CI, and adds no `cargo`
   invocation** — both, and the guard's run is 0.093 s.
5. **No record appears in `plans/registry.md`, and no closure record was
   modified** — the registry check is green and `grep` finds no postmortem path;
   no existing closure record was touched by this milestone. This record is the
   first addition to `plans/closure/harness-practice-adoption/` for M007.

On criterion 2: its literal clause (`--self-test`) presumes a script guard. The
plan's §4 invariant expressly allows a test to stand in, and the record and the
guard both take that form, so the criterion's mechanism — a claim resolved to a
real guard, and a token required in that guard's own body — is present and was
run. What was not run is the record's own reproduction of each injection; that
is a bounded, recorded evidence gap with named future evidence (§10), not a
required test left unrun without a substitute, because the four named tests pass
and each asserts the exact property the defect destroys.

The capability the milestone exists to deliver — the reason a guard exists
survives the person who added it, and the claim is mechanically checked — is
present and cheap enough to run on every `quick`. The seeded record also proves
the convention on a real defect rather than a hypothetical one: it describes a
collision that is on `main` today, names the four tests that now pin it, and
records why the evidence at the time could not see it (the tests used the one
variant with no `id`).

## 12. Registry updates

Required in `plans/registry.md` and
`plans/subsystems/harness-practice-adoption-roadmap.md`:

- §12's M007 row: `not started` → `closed`, closure record
  `plans/closure/harness-practice-adoption/007-status.md`, blocker column `—`.
- The registry row for M007 (currently `Ready` at
  `plans/registry.md:241`) → `Closed`, pointing at this record.
- `AGENTS.md`'s quick/CI guard counts (84 / 70 → 94 / 80) and the quick-only
  list, which is now one entry behind. Not this record's to edit.
- **Not updated by this record:** the source plan's `Status:` line. It stays
  `ready for handoff` until the parent changes it.
