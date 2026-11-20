# Harness Practice Adoption Milestone 001 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/harness-practice-adoption/001-recorded-session-replay.md`

Source subsystem roadmap:

- `plans/subsystems/harness-practice-adoption-roadmap.md#m001--recorded-session-replay`

Repository baseline reviewed: `7642b9e38` (`feat-auto-20261109-f8abad0d`), plus
uncommitted working-tree work

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`, on top of HEAD `7642b9e38`. `tests/session_replay/`
  (five modules and four fixtures), `tests/common/session_replay.rs`,
  `tests/common/replay_recorder.rs` and
  `scripts/check_session_replay_fixtures.py` are untracked additions; the
  `[[test]] name = "session_replay"` entry and the
  `docs/test-ownership.toml` block are uncommitted edits. Nothing below is
  attributable to a SHA.

The source plan and the roadmap's §12 status table have since been updated by
the parent: both now read `conditionally closed` for M001
(`plans/implementation/harness-practice-adoption/001-recorded-session-replay.md:3`,
`plans/subsystems/harness-practice-adoption-roadmap.md:500`). This record states
the evidence; it does not edit either.

## 1. Executive finding

The tier exists, it drives the shipped loop, and it fails when the loop changes.
`cargo test --test session_replay` runs **20 tests, all passing**, in 2.15 s,
with no provider credential and no network: the only model input is each
fixture's scripted responses. The three scenarios the plan required are
present and each asserts its own terminal state and request count; S3's second
model request is asserted to carry the first tool's real output; two
perturbation tests prove the comparison can fail and name the field.

Two things kept this from an unqualified `closed` at authoring; the first is
now met:

1. **The milestone's own corpus guard was red on this baseline — met as of
   2026-11-13.** The guard failed because the uncommitted M002 CLI follow-up
   added a `struct RecordingProvider` inside `src/main.rs`'s `#[cfg(test)] mod
   tests`, colliding by name with the shared tree's `RecordingProvider`. The fix
   taken is the second of the two §6 named: the guard now scans the **production
   view** (`scripts/check_session_replay_fixtures.py:255-262`), so a
   `#[cfg(test)]` test double is not a production dependency. The double still
   exists at `src/main.rs:5526`; `python3 scripts/check_session_replay_fixtures.py`
   passes on 2026-11-13, and the guard's `--self-test` still rejects a reference
   injected into `src/lib.rs`. `scripts/verify.sh full` passed on 2026-11-13 with
   the guard in place.
2. **Work package D's live recording was not executed.** No provider credential
   exists in this environment. The substitute is named in §4. This remains open.

A third, documentation-only residue remains open: `architecture/testing.md` has
no replay-tier subsection (§10).

The tier also did its intended job on a real defect: it found that a tool-call
part could not be decoded at all. That defect was fixed **outside this
milestone**; §3 records it as a downstream consequence and claims none of it.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A versioned fixture format with a scenario identity, user input, ordered responses and an expectation | `tests/session_replay/fixtures/*.json`, all `"format": 1`; `tests/session_replay/fixture_format.inc` | pass | Four fixtures: three scenarios plus one decode fixture |
| The fixture DTO fails closed on an unknown variant and names it | `fixture_format::unknown_event_variant_fails_to_load_and_names_it` | pass | |
| An unknown `format` version fails to load and names the version | `fixture_format::unknown_format_version_fails_to_load_and_names_it` | pass | |
| A round trip covering every supported variant | `fixture_format::fixture_round_trip_covers_every_supported_variant`, fed by `fixtures/all_variants.json` | pass | |
| A fixture's declared id matches its file stem | `fixture_format::fixture_identity_must_match_its_file_stem` | pass | Not in the plan; added defense |
| The runner drives the **real** loop and reads the session back through a reopened handle | `runner_contract::a_scenario_reads_its_session_through_a_reopened_handle` | pass | |
| S1 plain turn passes keyless, asserting terminal state and request count | `scenarios::s1_plain_turn_matches_its_expectation` | pass | `request_count == 1`, no tool calls |
| S2 tool call passes keyless through the real broker | `scenarios::s2_tool_call_matches_its_expectation` | pass | |
| S3 multi-step turn passes keyless | `scenarios::s3_multi_step_turn_matches_its_expectation` | pass | |
| S3's second request carries the first tool's real result | `scenarios::s3_multi_step_turn_matches_its_expectation` asserts `run.requests[1]` holds exactly one `Message::Tool` whose content contains the committed marker `The recording marker is alpha-7731.` | pass | Criterion 2 — a stubbed or dropped result leaves that message empty and fails |
| A perturbed expectation fails and names the divergent field | `runner_contract::perturbed_expectation_fails_and_names_the_field` | pass | Criterion 3 |
| Fixture exhaustion fails with the request index, not a hang | `runner_contract::fixture_exhaustion_fails_with_the_request_index` | pass | |
| Two scenarios in one process do not observe each other's requests | `runner_contract::two_scenarios_do_not_share_requests` | pass | |
| A present credential changes nothing | `runner_contract::a_present_provider_credential_does_not_change_the_outcome` | pass | The substitute for the unrun live recording |
| The recorder tees every event and request | `recorder::recording_provider_tees_every_event_and_request` | pass | |
| The recorder refuses a credential-shaped fixture | `recorder::recorder_refuses_a_credential_shaped_fixture` | pass | |
| The recorder refuses an absolute home path | `recorder::recorder_refuses_an_absolute_home_path` | pass | |
| The recorder refuses a run with no credential | `recorder::recorder_refuses_a_run_without_a_credential` | pass | |
| A recorded fixture loads and passes without the key | `recorder::recorded_fixture_loads_and_passes_without_a_key` | pass | The full path: tee → `fixture_from_observation` → `write_fixture` → `load_path` → replay → compare |
| **A live recording against a real provider with a real credential** | — | **not run** | WP-D. No credential here; see §4 for the substitute |
| The reader and recorder are referenced only from the test tree, proven by a guard | `scripts/check_session_replay_fixtures.py` | pass | Green 2026-11-13: the guard scans the production view, so `src/main.rs`'s `#[cfg(test)]` test double is excluded; `--self-test` still rejects a production-path reference |
| The fixture guard rejects its self-test injections | `--self-test` | pass | Six now, not the plan's five — a sixth (`required-features`) case was added |
| No production target gains a feature, config key, protocol variant, migration or dependency | `Cargo.toml` `[[test]] name = "session_replay"` carries no `required-features`; the guard checks that explicitly | pass | The family compiles and runs in the routine sweep |
| The test-tree guards this milestone is subject to | `check_test_common_binding.py`, `check_test_assertion_quality.py`, `check_test_duplication.py`, `rust_test_scan.py --validate` | pass | All four green |

## 3. Production implementation evidence

**Nothing ships.** This milestone adds no production module, feature,
configuration key, environment variable, CLI flag or protocol variant. Its
artifacts live in `tests/` and `scripts/`:

- `tests/session_replay/mod.rs` — the single `[[test]]` target. It binds
  `#[path = "../common/mod.rs"] pub mod common;` (public, as the shared tree
  requires) and four `.inc` members so Cargo's directory discovery does not make
  each member its own executable.
- `tests/session_replay/fixture_format.inc`, `recorder.inc`,
  `runner_contract.inc`, `scenarios.inc` — 20 test functions.
- `tests/session_replay/fixtures/` — four fixtures plus two plain-text read
  targets the tool scenarios read.
- `tests/common/session_replay.rs`, `tests/common/replay_recorder.rs` — the
  shared runner, the fixture DTO, the recorder decorator and the write-time
  redaction.
- `scripts/check_session_replay_fixtures.py` — the corpus guard.
- `Cargo.toml` `[[test]] name = "session_replay"` with **no**
  `required-features`, and a `docs/test-ownership.toml` entry mapping the family
  to its tests.

**The plan's §15 asks for "a statement that no production file changed, verified
by the diff". That statement cannot be made honestly about this tree.** The
working tree also carries the tool-call serde fix and two sibling lanes' edits,
so the diff is not attributable to this milestone — and the serde fix *did*
change production files (`crates/codegg-protocol/src/dto.rs`,
`crates/codegg-core/src/protocol_conversions.rs`,
`crates/codegg-core/src/session/message.rs`). What this record claims instead is
the milestone-local fact: every file this milestone authored is under `tests/`
or `scripts/`, and its only manifest change is the `[[test]]` target with no
feature gate. The production change it found is recorded below as a downstream
consequence and is not claimed here.

**The comparison excludes timestamps and non-fixture ids on purpose.**
`tests/session_replay/mod.rs:12-15` and the runner state it: the comparison is
over role, text, tool name, normalized arguments, tool outcome class, terminal
state, request count and the message-role shape of each request. A reader who
expects byte-identical rows must read that as a decision, not a hole.

**The fixtures.**

| Fixture | Id | Size |
|---|---|---|
| `s1_plain_turn.json` | `s1_plain_turn` | 709 bytes |
| `s2_tool_call.json` | `s2_tool_call` | 1,570 bytes |
| `s3_multi_step_turn.json` | `s3_multi_step_turn` | 2,289 bytes |
| `all_variants.json` | `all_variants` (decode fixture, never driven) | 1,135 bytes |

All four are under the 8,192-byte bound the guard enforces, and the guard
rejects an oversized one in its self-test.

**The recorder.** `tests/common/replay_recorder.rs` defines `RecordingProvider`
(a decorator that forwards and tees), `require_credential`, `credential_shape`,
`home_path`, `fixture_from_observation` and `write_fixture`. It is selected by
`CODEGG_SESSION_REPLAY_RECORD=1`, refuses when no credential is present
(`CODEGG_REPLAY_RECORD_CREDENTIAL`, `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`,
`DEEPSEEK_API_KEY`), and refuses at write a credential-shaped value, an absolute
home path, an oversized document, or an event with no fixture spelling. Nothing
in the routine sweep or CI calls `record_fixture`.

**Downstream consequence — the tool-call serde defect this milestone found, and
did not fix.** The replay runner reads the durable transcript back through
`MessageStore::list`, the production decode path. That path could not decode a
tool-call part at all: `PartInfo` and `PartData::ToolCall` both serialized `id`
under `#[serde(flatten)]`, producing a duplicate key. The tier surfaced it
because a tool-call scenario could not read its own session back.

The defect was fixed **outside this milestone's line**, in work that is also
uncommitted here:

- the tool-call id has its own wire name — `toolCallID` at
  `crates/codegg-protocol/src/dto.rs:1049` and
  `crates/codegg-core/src/session/message.rs:140`;
- both crates recover a pre-rename row that carries the old single `id` —
  `crates/codegg-core/src/protocol_conversions.rs:1175`,
  `crates/codegg-core/src/session/message.rs:56-64`, and tests at
  `dto.rs:884` (`legacy_part_info_reserializes_in_the_canonical_shape`) and
  `dto.rs:874` (the duplicate-key refusal);
- the replay module's own duplicated decoder was **removed in favour of** the
  production reader — `tests/common/session_replay.rs:780-791` records that it
  existed, why, and that its removal is what makes a decode regression a
  scenario failure.

This milestone claims none of that fix. It claims only that the tier found it,
and that the tier now uses the production reader rather than a private mirror.

## 4. Verification executed

### Commands run

```bash
cargo test --test session_replay
python3 scripts/check_session_replay_fixtures.py
python3 scripts/check_session_replay_fixtures.py --self-test
python3 scripts/check_test_common_binding.py
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
python3 scripts/rust_test_scan.py --validate
python3 scripts/check_comment_budget.py
```

### Results

- `cargo test --test session_replay` — **20 passed, 0 failed, 0 ignored**, 2.15 s
  (build 2 m 49 s). The three scenario members, the seven runner-contract
  members, the six recorder members and the four format members all reported
  `ok`. **No provider credential was set and no network call is possible**: the
  family constructs a `ScriptedProvider` from the fixture, so the only model
  input is the fixture.
- `check_session_replay_fixtures.py` — **green, exit 0** on 2026-11-13:
  `session replay fixture guard passed`. The violation this bullet used to record
  (`src/main.rs` naming `RecordingProvider`) is discharged by the guard now
  scanning the production view, which excludes `#[cfg(test)]` modules; the
  `--self-test` still rejects a reference injected into `src/lib.rs`. See §6.
- `check_session_replay_fixtures.py --self-test` — **green**, exit 0, all six
  injections rejected:
  - an orphan fixture (a fixture no member binds);
  - a dangling reference (a member running a fixture that is not there);
  - an oversized fixture (8,443 bytes against the 8,192-byte bound);
  - a credential-shaped value (`sk-` in `all_variants.json`);
  - a reference from a production path (`src/lib.rs` naming `ReplayFixture`);
  - a `required-features` gate on the family.

  The plan asked for five; the guard has six, and the sixth is the one that
  keeps the 149-unrun-functions trap shut.
- `check_test_common_binding.py` — green: "shared helper-tree binding guard
  passed".
- `check_test_assertion_quality.py` — green: "all checks pass".
- `check_test_duplication.py` — green: "all checks pass".
- `rust_test_scan.py --validate` — green: `src` 8,160 test attributes = 8,159
  parsed functions + 1 macro template (residual 0); `crates` 3,953 = 3,953 + 0;
  mask lost and gained no newline.
- `check_comment_budget.py` — **green in the 2026-11-13 `scripts/verify.sh full`
  run**, and a standalone re-run shows no violation in this milestone's file.
  That violation is discharged by a deliberate bank:
  `tests/common/session_replay.rs` is recorded at its measured ceiling of exactly
  0.2708 (`docs/comment-budget.toml:815`); the corpus total is banked at 139,245
  lines. The 44-violation figure this bullet recorded is superseded. (At
  correction time the guard's remaining red traces to WP D's in-flight
  `diff.rs`/`baseline.rs`, including the corpus totals — not to this
  milestone's files.)

### Not run

- **Work package D's live recording.** No provider credential exists in this
  environment. **The substitute, named exactly:**
  `recorder::recorded_fixture_loads_and_passes_without_a_key` exercises the
  whole recorded path with a scripted provider — tee every event and request →
  `fixture_from_observation` → `write_fixture` → `load_path` → replay →
  compare — and `runner_contract::a_present_provider_credential_does_not_change_the_outcome`
  proves that the presence of a credential variable changes nothing about the
  outcome. What the substitute **cannot** prove is that a fixture produced by a
  real provider's stream is loadable and replayable: the scripted provider
  yields the same event shapes, but a real provider may emit a shape the DTO
  cannot spell, and only a live run would find it.
- `cargo fmt --check`, `cargo clippy -p codegg --tests -D warnings`,
  `scripts/verify.sh quick`: **not run in the authoring session**, because the
  root package's lib-test target did not compile then (a sibling lane's in-flight
  edit). **Superseded 2026-11-13:** `scripts/verify.sh full` ran on this tree and
  passed — `cargo fmt --all --check` clean, clippy `-D warnings` clean, the
  workspace sweep green, and `full` includes `quick`, which includes the guards.

## 5. Invariant review

- **Nothing ships.** No production module, feature, configuration key,
  environment variable, CLI flag or protocol variant was added by this
  milestone. `Cargo.toml`'s `session_replay` target has no `required-features`,
  and the guard fails if one appears.
- **One mocked boundary.** The provider is scripted; the tool registry, the
  broker, the scheduler, the session store and the projection are real. S2 and
  S3 run the real `read` tool against committed text files.
- **Every new test body asserts a checkable property.** Guard-enforced and
  green.
- **The shared tree is bound correctly.** `mod.rs` binds
  `#[path = "../common/mod.rs"] pub mod common;`; `check_test_common_binding.py`
  is green.
- **No wall-clock and no randomness in a scenario.** Identities come from the
  fixture (`session_id` is the fixture id); the runner bounds the whole scenario
  with `SCENARIO_TIMEOUT` so a hang fails rather than stalls, but no scenario
  asserts on a duration.
- **The comparison can fail.** Two tests demonstrate it:
  `perturbed_expectation_fails_and_names_the_field` and
  `removed_tool_declaration_fails_the_scenario`.
- **The reader and recorder are reachable only from the test tree.** Met: the
  guard scans the **production view**, so `#[cfg(test)]` modules are excluded
  and `src/main.rs`'s local `RecordingProvider` test double is correctly not a
  dependency on `tests/common/`. The self-test proves the check is not vacuous:
  a reference injected into `src/lib.rs` is still rejected.

## 6. Failure and recovery review

- **Fixture exhaustion** — `fixture_exhaustion_fails_with_the_request_index`:
  the run fails naming the request index; it does not hang and does not
  synthesize an empty response.
- **Malformed fixture** — fails at load with the offending path and reason; a
  partially loaded fixture is never driven. The DTO fails closed on an unknown
  variant.
- **Turn failure** — a scenario may declare a failed turn and the expectation
  names the failure class, so a scenario cannot pass by accepting any error.
- **Cancellation** — **not covered**, and this record says so. The runner drives
  a turn to completion. A cancellation scenario is a follow-on milestone.
- **Restart** — `a_scenario_reads_its_session_through_a_reopened_handle`: a row
  that only exists in memory fails the comparison.
- **Contention** — one process per test under the `ci` profile;
  `two_scenarios_do_not_share_requests` proves two scenarios in one process do
  not observe each other's requests.
- **The guard's red state — resolved 2026-11-13.** Root cause: `src/main.rs`
  declared `struct RecordingProvider` in an uncommitted `#[cfg(test)] mod tests`
  added by M002's CLI follow-up work. The fix taken is the second of the two this
  record named: `scripts/check_session_replay_fixtures.py` now scans the
  production view and skips `#[cfg(test)]` modules the way it already skips
  comments and literals. The test double was not renamed; the guard's
  `--self-test` still rejects a production-path reference.

## 7. Migration and compatibility review

- No production migration. The fixture-format version rule is covered by the
  unknown-version test; a later version adds a successor and never rewrites a
  fixture in place.
- No protocol, configuration, storage-layout or dependency change.
- The recorder is additive and local-only; removing it later would not affect
  any committed fixture.
- **The tier is not `required-features`-gated**, which is the plan's explicit
  compatibility requirement: nine root targets are gated today and no CI
  workflow enables those features, so a gated family would never be compiled
  there. The guard now rejects a gate being added.

## 8. Security review

- The fixture guard rejects a credential-shaped string, an absolute home path,
  an oversized document, and a reference from a production path — before a
  corpus can carry any of them into a repository.
- The recorder refuses the same two shapes at write, and refuses a run with no
  credential rather than writing an empty transcript.
- The fixtures contain no credential and no absolute developer path; the guard
  is green on the whole corpus as of 2026-11-13.
- No new authority: the family reads a temporary store it creates itself and
  writes nothing outside it.

## 9. Documentation and operations

- `architecture/overview.md:603` — a count-reconciliation section recording that
  M001 added four integration-test **modules** and one test target, and that the
  *Integration test files* row was re-measured to 371 rather than incremented.
- `Cargo.toml` — the `session_replay` `[[test]]` target.
- `docs/test-ownership.toml` — the family's ownership block.
- `scripts/check_session_replay_fixtures.py` — wired into `scripts/verify.sh`
  (`:638-642`) and `.github/workflows/ci.yml` (`:376-379`), in both the
  `--self-test` and plain forms.
- **Documentation gap.** The plan's §12 required `architecture/testing.md` to
  gain "a replay-tier subsection: what a fixture is, the recording rule, why the
  tier is not `required-features`-gated, and the comparison's excluded fields."
  **`architecture/testing.md` contains no such subsection.** A search for
  `session_replay`, `RecordingProvider`, `replay fixture` and `record mode`
  across `architecture/` finds only `architecture/overview.md`'s count
  reconciliation and `docs/test-ownership.toml`. The live recording workflow is
  documented only in the two test-tree modules — and
  `tests/session_replay/recorder.inc:8-10` says it is "documented in
  `architecture/testing.md`", which is not true of this tree.

## 10. Unresolved findings

The high finding and the two low findings below were resolved on 2026-11-13 and
are retained here so the conditions are not silently dropped. The two medium
findings remain open.

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| resolved (was high) | `scripts/check_session_replay_fixtures.py` **was red** on this baseline: `src/main.rs` declared `RecordingProvider` inside an uncommitted `#[cfg(test)] mod tests` (M002's CLI follow-up), colliding with the shared tree's type. **Resolved 2026-11-13:** the guard now scans the production view and excludes `#[cfg(test)]` modules | `scripts/verify.sh quick` and the CI `verify` job are no longer blocked by this guard; it passes and its self-test still rejects a production-path reference | None; the second of the two recorded fixes was taken |
| medium | `architecture/testing.md` has no replay-tier subsection, though the plan's §12 required one and `recorder.inc` claims it exists | A reader cannot learn the recording rule or the excluded fields from the architecture docs | Add the subsection: fixture shape, the `CODEGG_SESSION_REPLAY_RECORD` opt-in, why the tier is not feature-gated, the excluded fields |
| medium | Work package D's live recording was never executed | No committed fixture has been produced by a real provider, so the DTO's coverage of real event shapes is unproven | Record one turn against a real provider with a credential and commit the resulting fixture; or record the non-claim permanently |
| resolved (was low) | `tests/common/session_replay.rs` was 0.0028 over its comment-to-code ceiling | `check_comment_budget.py` was red partly because of this file | Banked at its measured ratio of exactly 0.2708 in `docs/comment-budget.toml:815`; guard green 2026-11-13 |
| resolved (was low) | The roadmap §12 table and the plan's own `Status:` line said `not started` / `ready for handoff` | The roadmap understated delivered work | Parent updated both: the roadmap row now reads `conditionally closed` (`plans/subsystems/harness-practice-adoption-roadmap.md:500`) and the plan's line 3 matches |

## 11. Roadmap disposition

**Conditionally closed.** Five acceptance criteria are met outright; criterion
4 is met on this tree but its "merged baseline" form depends on the commit this
tree has not made:

1. Three scenarios pass keyless, each asserting terminal state and request
   count — met.
2. S3's second request carries the first tool's real result — met.
3. A perturbation makes a scenario fail and names the field — met, twice.
4. The reader and recorder are referenced only from the test tree, proven by a
   guard — met on this tree as of 2026-11-13: the guard passes because it scans
   the production view, and its self-test still rejects a production-path
   reference. The "merged baseline" wording still depends on WP 0's commit, but
   no guard defect remains.
5. The fixture guard rejects all five of its self-test injections — met, and
   exceeded: six are rejected.
6. No production target gains a feature, key, variant, migration or dependency —
   met; the family is deliberately ungated.

The named evidence still outstanding: the live recording (WP-D) and the
`architecture/testing.md` subsection. Neither blocks M002 or M003.

## 12. Registry updates

Required in `plans/registry.md` and
`plans/subsystems/harness-practice-adoption-roadmap.md`:

- §12's M001 row: `not started` → `conditionally closed`, closure record
  `plans/closure/harness-practice-adoption/001-status.md`, blocker column naming
  the unrun live recording and the missing `architecture/testing.md` subsection.
  **Done by the parent:** the roadmap row now reads `conditionally closed`
  (`plans/subsystems/harness-practice-adoption-roadmap.md:500`).
- The registry's milestone table entry for M001, if `plans/registry.md` carries
  one.
- **Resolved 2026-11-13:** the `src/main.rs` name collision no longer blocks
  `quick`; the guard scans the production view, so no rename was required.
- **Not updated by this record:** the source plan's `Status:` line; the parent
  has since changed it to `conditionally closed`
  (`plans/implementation/harness-practice-adoption/001-recorded-session-replay.md:3`).

## Correction log (2026-11-13)

The gate-class discharge cited below is the 2026-11-13 `scripts/verify.sh full`
run on this tree: 17,377 workspace tests and 8,441 feature-pass tests, 0
failures; `cargo fmt --all --check` clean; clippy `-D warnings` clean. `full`
includes `quick`, which includes the guards.

- **Preamble** — "the source plan still reads `ready for handoff`, and the
  roadmap … still reads `not started`": superseded by the parent's updates; both
  now read `conditionally closed` (plan:3, roadmap:500).
- **§1 item 1** — "the milestone's own corpus guard is red on this baseline":
  met. Standalone `python3 scripts/check_session_replay_fixtures.py` passes; the
  guard now scans the production view (self-test still rejects a production-path
  injection); the 2026-11-13 `full` run passed with it.
- **§2 matrix row "referenced only from the test tree"** — `fail` → `pass`, same
  discharge.
- **§4 results, replay guard** — "RED, exit 1" → green, exit 0, same discharge.
- **§4 results, comment budget** — "red, exit 1, 44 violations": the milestone's
  file is banked at exactly 0.2708 (`docs/comment-budget.toml:815`), corpus
  banked at 139,245 lines, and the guard was green in the 2026-11-13 `full` run.
- **§4 "Not run" (fmt, clippy, `quick`)** — superseded by the 2026-11-13 `full`
  run, which ran all three and passed.
- **§5 invariant bullet** — "this is the invariant the guard is failing on": met
  by the production-view scan; the double is not a production dependency.
- **§6 "The guard's red state"** — resolved by the second of the two fixes the
  record named; no rename was needed.
- **§8 security bullet** — "its one failure is the production reference": the
  whole corpus check is green.
- **§9 wiring line numbers** — `verify.sh` `:620-624` → `:638-642`, `ci.yml`
  `:358-361` → `:376-379`.
- **§10 high row** — resolved, retained above.
- **§10 low comment-budget row** — resolved by the recorded bank, retained above.
- **§10 low roadmap/plan row** — parent updated both, retained above.
- **§11 criterion 4** — met on this tree; only the commit-dependent "merged
  baseline" wording remains, and it is WP 0's.
- **§12 registry bullets** — corrected to name the remaining residues and to
  record the parent's update.

**Residue kept open, deliberately:** the `architecture/testing.md` replay
subsection (§10, medium) and WP-D's live recording (§10, medium). Neither is a
gate condition and neither is discharged by the 2026-11-13 run.
