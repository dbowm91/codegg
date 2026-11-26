# Harness Practice Adoption Milestone 001 — Recorded-Session Replay

Status: conditionally closed

Repository baseline: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`
(branch `feat/auto-20261109-f8abad0d`)

Source roadmap:

- `plans/subsystems/harness-practice-adoption-roadmap.md` §7 M001, §6, §9

Long-term requirements:

- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`
  — durable replay, snapshot/event equivalence, replay after reconnect and
  daemon restart
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
  — bounded payloads and restart-safe behaviour
- `plans/001-terminology-and-domain-model.md#6-session-and-interaction-terms`
- `plans/003-planning-process.md`

Applicable ADRs:

- **None.** This milestone adds no production path, no storage, no protocol
  variant and no dependency. It is confined to the test tree, `scripts/`, and
  documentation.

Primary class: infrastructure

## 1. Objective

A committed fixture drives the **shipped agent loop** through one complete turn
with no API key and no network, and the durable session state that results is
compared against the fixture's expectation — so a change to prompt assembly,
tool admission, projection or compaction fails a keyless test instead of
shipping.

## 2. Why this milestone is ready

- There is no hard dependency: the roadmap's §6 records the line as free of
  hard dependencies, and this milestone is first by consequence.
- The interface it depends on is already stable and already exercised: a shared
  scripted provider exists at `tests/common/scripted_provider.rs` and is used
  by the agent-loop harness and the M011 composed race. Its constructor takes
  the exact value a fixture needs to produce — `Vec<Vec<ChatEvent>>` — and it
  records every `ChatRequest` it receives.
- The shared test tree already has a binding rule
  (`scripts/check_test_common_binding.py`) and a scanner
  (`scripts/rust_test_scan.py`), so a new family has a defined shape.
- No decision in this milestone requires an ADR: it adds no production
  authority.

## 3. Current implementation evidence

**The seam exists.** `tests/common/scripted_provider.rs` defines
`ScriptedProvider`, a `Provider` implementation that answers with a fixed list
of scripted turns (`responses: Vec<Vec<ChatEvent>>`) and records every request
it receives. It is shared deliberately, so two suites cannot drift. Its
optional barrier exists for the two-agent race and is irrelevant here.

**The event type is not serializable.** `ChatEvent` in
`crates/codegg-providers/src/provider_core.rs` derives `Debug, Clone` only. It
has no `Serialize`/`Deserialize`, and it should not gain one for this purpose:
a serialization contract on a production event type is a protocol commitment
this milestone does not need.

**Scripts are hand-written per test.** `tests/agent_loop/agent_loop_harness.inc`
and its siblings construct scripts inline in Rust. That is appropriate for a
test that asserts one loop behaviour, and it is not a corpus: there is no
recorded format, no fixture that a real model produced, no expectation
comparison, and nothing that re-runs the same turn after a prompt change.

**Nothing replays a real exchange.** `capture-test-support` in
`crates/codegg-providers` redirects one provider's origin to a loopback capture
server and is compiled out of production. It can record HTTP frames; it cannot
drive the loop from them, and no code does.

**There are no recorded fixtures.** The tree contains no `*.jsonl` or `*.snap`
fixture and no `snapshots/` directory.

**The known trap this milestone must not repeat.** Nine root integration
targets are gated behind `required-features` and no CI workflow enables those
features, so **149 test functions are never compiled in CI**. A replay family
declared with `required-features` would join them. This milestone therefore
adds no feature gate and no `required-features`; the family compiles and runs
in the routine sweep.

## 4. Invariants that must not regress

- **Nothing ships.** The milestone adds no code to any shipped target: no
  production module, no feature, no configuration key, no environment variable,
  no CLI flag, and no protocol variant. A guard asserts that the fixture reader
  and recorder are reachable only from the test tree.
- **One mocked boundary.** The model adapter is scripted; the tool registry,
  `ToolBroker`, the scheduler, the session store and the projection are the
  real implementations. A scenario that also stubs a tool proves the bridge
  moves bytes, not that the tool works.
- **Every new test body asserts a checkable property.** The new family is
  subject to `scripts/check_test_assertion_quality.py` and
  `scripts/check_test_duplication.py`; a body that asserts nothing must be
  recorded in `docs/test-assertion-quality.toml` with its reason, and no two
  bodies may assert the same thing.
- **The shared tree is bound correctly.** Any binder of `tests/common/` uses
  `pub mod common;`; a private binding makes the tree's `pub` items dead code
  under `-D warnings`.
- **No wall-clock and no randomness in a scenario.** Identities come from the
  fixture; ordering comes from the fixture. A scenario that needs the clock is
  not a scenario.
- **The comparison can fail.** A perturbation — a changed prompt section, a
  changed tool declaration — must make at least one scenario fail. A tier that
  passes whatever the loop does is decoration.

## 5. Scope

### In scope

- A fixture format for one recorded turn: a schema version, a scenario
  identity, the user input, the ordered scripted responses, and the expected
  durable outcome.
- A fixture-local deserialization DTO in the test tree that lowers into the
  existing `Vec<Vec<ChatEvent>>` shape. The production `ChatEvent` does not
  gain a serde contract.
- A runner in the shared test tree that loads a fixture, constructs
  `ScriptedProvider`, drives the real loop, reads the session back, and
  compares.
- A recording decorator in the shared test tree that wraps a real provider,
  tees the stream it forwards, and writes a fixture. Local-only, requires a
  key, never invoked by CI or by the routine sweep.
- A new root test family with one member per scenario, plus the fixtures.
- A guard over the fixture corpus, with a self-test.

### Explicitly out of scope

- Replaying a *whole session* with many turns and restarts. One turn per
  scenario; a multi-turn scenario is expressed as consecutive scripted
  responses inside one turn's loop, not as a second session.
- Browser, desktop or WebSocket rendering evidence. The comparison is over
  durable rows, not over a rendered frame.
- Replacing or extending `capture-test-support`. The two seams answer different
  questions and neither is removed.
- Any change to `crates/codegg-providers`, including a serde derive on
  `ChatEvent`.
- A record mode in CI, or a fixture that requires a key to pass.

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None. The scenario reads the session the loop already writes; it does not add a
table, a column, or a row of its own.

### Protocol and DTOs

None in the production protocol. The fixture DTO is test-local by design; its
schema version is a fixture-format version, not a protocol version, and the
plan says so in the fixture's own header comment.

### Runtime and concurrency

None. The runner drives the loop in-process under the nextest `ci` profile
(one process per test), so a scenario cannot observe another test's registry or
store.

### Frontend or operator surface

None.

### Security and authorization

None in production. The fixture guard is the security surface: it rejects a
credential-shaped string, an absolute home path, and an oversized fixture
before the corpus can carry one into a repository.

### Documentation and static guards

- `architecture/testing.md` gains the replay tier, its recording rule, and the
  reason it is not feature-gated.
- A new guard, `scripts/check_session_replay_fixtures.py`, registered in
  `scripts/verify.sh quick` and the CI `verify` job.
- `tests/session_replay/mod.rs` binds `pub mod common;`.

## 7. Ordered work packages

### Work package A — The fixture format and its DTO

Intent: define what a recorded turn *is* before writing a runner for it, so the
format is a contract rather than an accident of the first scenario.

Required changes:

- A versioned fixture document with, at minimum: `format` (integer), `id`,
  `user_input`, `responses` (an ordered list of event lists), `expect`
  (assistant text, tool calls with name and normalized arguments, tool results
  with an outcome class, the turn's terminal state, and the number of model
  requests), and an optional `notes`.
- A test-local DTO whose deserialization lowers into `Vec<Vec<ChatEvent>>`. The
  DTO covers every `ChatEvent` variant the scenarios need — `TextDelta`,
  `ReasoningDelta`, `ToolCall`, `ToolResult`, `Finish` — and **fails closed**
  on an unknown variant rather than silently skipping it.
- A rejection rule for an unknown `format` version: the fixture fails to load
  with a message naming the version, rather than being interpreted as the
  current one.

Acceptance evidence:

- A round-trip test proves a fixture loads into the exact `Vec<Vec<ChatEvent>>`
  the provider expects, for a fixture containing every supported variant.
- A test proves an unknown variant and an unknown version each fail to load,
  and names what was unknown.

### Work package B — The scenario runner

Intent: one function that turns a fixture into a real turn and a comparable
result.

Required changes:

- A runner in `tests/common/` that: loads the fixture, builds
  `ScriptedProvider` with the lowered responses, builds the **real** tool
  registry for the fixture's declared profile, drives the loop with the real
  store, and reads the session back.
- A comparison over semantic fields only — role, text, tool name, normalized
  arguments, tool outcome class, terminal state, request count. Timestamps,
  row ids that are not fixture-supplied, and durations are excluded from the
  comparison, and the plan's §16 records that exclusion as a deliberate limit
  rather than an oversight.
- A failure message that prints the expected and actual rows side by side and
  names the first divergence.

Acceptance evidence:

- A scenario passes with the network unavailable (the suite is run with no
  provider credential present, and the fixture is the only model input).
- A deliberately perturbed expectation fails with a divergence that names the
  field.

### Work package C — Three scenarios

Intent: prove the tier on the three shapes that matter, rather than on a shape
chosen to be easy.

Required changes:

- **S1 plain turn** — one assistant text response, no tool call.
- **S2 tool call** — one tool call through the real broker and the real
  scheduler, with the tool's real result recorded.
- **S3 multi-step turn** — a tool call followed by a second model request whose
  result depends on the first tool's real output, so the loop's second
  iteration is exercised.

Each fixture is committed, bounded, and contains no credential and no absolute
path from a developer's machine.

Acceptance evidence:

- Each scenario passes keyless and asserts its own terminal state and request
  count.
- S3's second request is asserted to contain the first tool's real result, so a
  scenario cannot pass by ignoring the tool.

### Work package D — The recorder

Intent: a fixture can be produced from a real run, so the corpus is recorded
rather than invented.

Required changes:

- A provider decorator in the shared test tree that wraps a real provider, tees
  the events it forwards, and writes a fixture in the format from work package
  A.
- The recorder is selected by an environment variable read **only** in the test
  tree, refuses to run when no credential is present, and writes to a path the
  caller names. It is not reachable from any shipped binary and is not invoked
  by the routine sweep or by CI.
- Redaction at write: the recorder refuses to emit a fixture whose content
  matches a credential shape, and refuses an absolute path under a home
  directory.

Acceptance evidence:

- A recorded fixture, produced locally, loads and passes without the key.
- A recorder run whose content contains a credential-shaped string fails to
  write and names the reason.

### Work package E — The guard

Intent: the corpus cannot rot silently.

Required changes:

- `scripts/check_session_replay_fixtures.py` verifies: every fixture declares a
  supported `format`; every fixture is referenced by exactly one family member;
  every member binds a fixture that exists; no fixture exceeds the size bound;
  no fixture contains a credential-shaped string or an absolute home path; and
  the replay reader and recorder are referenced only from the test tree.
- `--self-test` injects, and proves rejected: an orphan fixture, a dangling
  reference, an oversized fixture, a credential-shaped value, and a reference
  from a non-test path.

Acceptance evidence:

- `--self-test` rejects all five injections.
- The guard is green on the committed corpus.

## 8. Failure, cancellation, restart, and contention semantics

- **Fixture exhaustion.** A scenario whose script runs out while the loop is
  still asking fails with an error naming the request index; it must not hang,
  and it must not synthesize an empty response.
- **Malformed fixture.** Fails at load with the offending path and reason. A
  partially loaded fixture is never driven.
- **Turn failure.** A scenario may declare a failed turn. The expectation then
  names the failure class, so a scenario cannot pass by accepting any error.
- **Cancellation.** Out of scope for the first corpus: the runner drives a turn
  to completion. A cancellation scenario is a follow-on milestone, not a
  widened scope here.
- **Restart.** The scenario reads the session back through a freshly opened
  store handle, so a row that only exists in memory fails the comparison.
- **Contention.** One process per test under the `ci` profile; the runner holds
  its own store and registry, so two scenarios cannot share state.

## 9. Compatibility and migration

- The fixture format is versioned from its first commit; a later version adds a
  successor and never rewrites an existing fixture in place.
- No production compatibility surface changes: no protocol, no configuration,
  no storage layout, no dependency.
- The recorder is additive and local-only; removing it later would not affect
  any committed fixture.

## 10. Required tests

### Focused unit tests

- Fixture DTO round-trip covering every supported `ChatEvent` variant.
- Unknown event variant fails to load and names the variant.
- Unknown `format` version fails to load and names the version.
- The comparison reports the first divergent field, not just "mismatch".

### Integration tests

- S1, S2 and S3 pass keyless.
- S3 asserts the second request carries the first tool's real result.
- A perturbed expectation fails and names the field.

### Restart and recovery tests

- A scenario reads its session through a reopened store handle.
- Fixture exhaustion fails with a request index rather than hanging.

### Contention and cancellation tests

- Two scenarios in one process do not observe each other's requests (the
  `ScriptedProvider` instance is per-scenario).
- Cancellation is explicitly not covered, and the closure record says so.

### Security and negative tests

- A credential-shaped fixture value is rejected by the guard.
- An absolute home path in a fixture is rejected by the guard.
- The recorder refuses to write a credential-shaped fixture.
- The replay reader is not referenced from any non-test path.

### Migration and compatibility tests

- Not applicable: no production migration. The fixture-format version rule is
  covered by the focused version test.

## 11. Required verification commands

```bash
# the new tier, focused
cargo test --test session_replay

# one scenario
cargo test --test session_replay -- s2_tool_call

# the guard, and its self-test
python3 scripts/check_session_replay_fixtures.py
python3 scripts/check_session_replay_fixtures.py --self-test

# test-tree guards this milestone is subject to
python3 scripts/check_test_common_binding.py
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
python3 scripts/rust_test_scan.py --validate

# formatting and linting
cargo fmt --check
cargo clippy -p codegg --tests -- -D warnings

# the routine set
scripts/verify.sh quick
```

The closure record states which of these actually ran. It must not claim a
command it did not run.

## 12. Documentation updates

- `architecture/testing.md` — a replay-tier subsection: what a fixture is, the
  recording rule, why the tier is not `required-features`-gated, and the
  comparison's excluded fields.
- `architecture/overview.md` — the integration-test count row follows its listed
  source; it is not incremented by hand.
- `plans/subsystems/harness-practice-adoption-roadmap.md` §12 — status only.

## 13. Acceptance criteria

1. Three scenarios pass with no provider credential present and no network
   access, each asserting its own terminal state and model-request count.
2. S3's second model request is asserted to contain the first tool's real
   result.
3. A perturbation to prompt assembly or to the tool declaration makes at least
   one scenario fail, and the failure names the divergent field.
4. The replay reader and the recorder are referenced only from the test tree,
   proven by a guard.
5. The fixture guard rejects all five of its self-test injections.
6. No production target gains a feature, a configuration key, a protocol
   variant, a migration, or a dependency.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- driving the loop from a fixture requires a daemon singleton or a second turn
  driver; the tier must use the shipped loop or not exist;
- a `ChatEvent` variant cannot be expressed in the test-local DTO without a
  production serde contract — the mirror is preferred, and a production derive
  is a stop, not a convenience;
- a scenario cannot be expressed under the fixture size bound; drop the
  scenario, do not raise the bound;
- the comparison needs wall-clock or random identity to pass, which would mean
  the loop is not deterministic at the boundary this tier claims to cover;
- a `required-features` gate appears to be the only way to keep the family out
  of a sweep — that is the 149-unrun-functions trap, and the answer is a
  smaller scenario, not a gate.

## 15. Closure evidence required

- The three fixtures, with their ids and sizes.
- The command and result for each scenario, keyless, with the credential
  absence stated explicitly.
- The perturbation: what was changed, and the observed failure text.
- The guard's `--self-test` output showing all five injections rejected.
- The result of `scripts/check_test_common_binding.py`,
  `check_test_assertion_quality.py`, `check_test_duplication.py` and
  `rust_test_scan.py --validate`.
- A statement that no production file changed, verified by the diff.
- The named non-claim: cancellation and whole-session restart are not covered.

## 16. Handoff notes

- **Do not add a feature gate.** The nine `required-features` targets already
  leave 149 test functions unrun in CI. This family must compile and run in the
  routine sweep.
- **Do not add serde to `ChatEvent`.** The test-local DTO exists for that
  reason. If a variant is hard to mirror, mirror it more carefully.
- **The comparison excludes timestamps and non-fixture ids on purpose.** Say so
  in the closure record. A reader who assumes byte-identical rows will
  otherwise read the exclusion as a hole.
- **`tests/common/` is shared.** Any binder uses `pub mod common;`. The tree
  carries no blanket `#[allow(dead_code)]`; a private binding turns every `pub`
  item into a dead-code error under `-D warnings`.
- **The recorder needs a key and a human.** It is a local convenience. If the
  environment has no credential, record nothing and say so; do not invent a
  transcript.
- **Keep fixtures small.** One turn, text only. A fixture that carries an image
  or a long file body is a different artifact with a different bound.
