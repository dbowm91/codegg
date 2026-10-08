# Provider Connect Qualification / Multi-Surface Post-Closure Corrective C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/provider-connect-qualification-wire-dispatch-post-closure-corrective/001-terminal-reconciliation-and-hosted-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/provider-connect-qualification-wire-dispatch-post-closure-corrective-addendum.md`

Predecessor roadmap reconciled by this corrective:

- `plans/subsystems/provider-connect-qualification-wire-dispatch-corrective-addendum.md` (M010 + M011 closed)

Implementation revision: `ce23b4e432884ba19b21609e2eb997e18f5d6f9c`

Closure revision (branch tip, carries the runs recorded below): `4a7567d4`

Implementation commit:

- `ce23b4e4` — test(provider): prove durable OpenCode Go selection reaches the wire

Accepted upstream revision (unchanged, not repinned):

- EggPool shared provider profile `9ac6a1318e8db3c034b5ab54987317752d5ffea6`

Hosted evidence:

- [CI / verify run 37736136266](https://github.com/dbowm91/codegg/actions/runs/37736136266) — SHA `4a7567d4` (**the branch tip and closure revision**), **green on the first attempt**: `12386 tests run: 12386 passed, 5 skipped`, with every guard, formatting, and Clippy step green. Authoritative.
- [Desktop E2E run 37736136595](https://github.com/dbowm91/codegg/actions/runs/37736136595) — SHA `4a7567d4`, **green on the first attempt**. Authoritative.
- [CI / verify run 37734112906](https://github.com/dbowm91/codegg/actions/runs/37734112906) — SHA `4b3bad5a`, green at the same `12386/12386`.
- [CI / verify run 37730586250](https://github.com/dbowm91/codegg/actions/runs/37730586250) — SHA `ce23b4e4` (the implementation commit), attempt 2 green. All three C001 trajectory tests pass on the hosted runner here.
- [Desktop E2E run 37730586236](https://github.com/dbowm91/codegg/actions/runs/37730586236) — SHA `ce23b4e4`, green.
- [Desktop E2E run 37737818626](https://github.com/dbowm91/codegg/actions/runs/37737818626) — SHA `03c4c7e3` (the final tip), attempt 1 red on a pre-existing projection defect, **attempt 2 green** (`7 passing` in the m004 phase). This is the run that reflects the shipped tip; its first-attempt failure is diagnosed below and is not a C001 regression.
- PR [#105](https://github.com/dbowm91/codegg/pull/105) — opened solely to obtain the required hosted evidence (this repository's CI triggers on `pull_request` and pushes to `main`, not on branch pushes). **Not merged.** The branch is a clean fast-forward over `origin/main` (`d85ed67b`, 0 behind). Current check state: `verify` pass, `e2e` pass, GitGuardian pass — `MERGEABLE / CLEAN`.

Every green SHA above differs from the next only in `plans/` documents — for
example `git diff --name-only ce23b4e4..4b3bad5a` — so all of them cover the
identical production/test tree. The two runs on `4a7567d4` are the simplest
statement of the result: both hosted workflows, green on the closure revision
itself.

The branch tip has since advanced to `03c4c7e3` (a `plans/`-only commit, this
record's own correction). Its hosted runs are `verify` 37737818615 green and
Desktop E2E 37737818626 green on attempt 2. The tip therefore carries both
workflows green as well, and the production/test tree is byte-identical to
`4a7567d4` throughout.

### Hosted first attempt — recorded, not hidden

Attempt 1 of run 37730586250 failed with 2 of 12 386 tests:

- `codegg_core::goal::checkpoint::tests::test_read_checkpoint_tail_returns_latest_updates`
  (`crates/codegg-core/src/goal/checkpoint.rs:323`, `tail.contains("Phase 3 newest")`)
- `codegg_client` `session_projection_driver::artifact_excerpt_round_trip_returns_validated_outcome`
  (`crates/codegg-client/tests/session_projection_driver.rs:1389`, plus a worker panic at `:196`)

Both are outside this corrective's blast radius, established rather than
assumed:

- `git diff origin/main...HEAD -- crates/codegg-core/src/goal/ crates/codegg-client/`
  is **empty**; all 12 commits on this branch, C001 included, never touched
  either path.
- C001's entire diff is `codegg-providers` (test-only cfg-gated seam), one root
  `Cargo.toml` dev-dependency line, one new integration test, and documentation.
- Both tests **pass locally** on the closure revision
  (`goal::checkpoint::tests` 8/8; `session_projection_driver` 16/16).
- Attempt 2 of the same run, same SHA, is fully green.

A third flake appeared on the later closure-commit push, in Desktop E2E run
37734113006: `apps/desktop` `src/App.test.tsx` failed with
`Unable to find an element by: [data-testid="workspace-list"]`. The decisive
evidence is that the green Desktop E2E SHA and the failing SHA differ **only**
in `plans/` documents:

```text
git diff --name-only ce23b4e4..4b3bad5a
  plans/closure/.../001-status.md
  plans/registry.md
  plans/subsystems/...-post-closure-corrective-addendum.md
```

Closure-document edits cannot change an `apps/desktop` React renderer test, and
the branch never touched `apps/desktop`. (Desktop E2E triggers on this branch at
all only because an earlier pre-C001 commit, `d2c82e08` from M010, touched
`crates/codegg-protocol/src/provider.rs`; C001's own commit `ce23b4e4` does not.)

A re-run of attempt 2 makes the flake unambiguous: it failed on a **different**
test in the same file with the same symptom —

| Attempt | Failing test |
|---|---|
| 1 | `session route flow > submits a prompt once and clears the draft on accept` |
| 2 | `session route flow > drives projectdetail to workspaceselect to session open and projection` |

A deterministic defect fails the *same* assertion every time; two different
tests in the same suite failing on the same missing `data-testid` is render
ordering or timing, not a logic error. The conclusion is now confirmed by a
third run: Desktop E2E on `4a7567d4` (run 37736136595) is **green on the first
attempt**, from a tree that differs from the failing `4b3bad5a` only in
`plans/` documents. Two consecutive failures followed by a clean pass on
unchanged desktop code is the signature of a flake.

### The fourth failure is a different, diagnosed defect — not one of the above

A fourth Desktop E2E failure appeared on the final tip, in run
[37737818626](https://github.com/dbowm91/codegg/actions/runs/37737818626)
attempt 1 at SHA `03c4c7e3`. It is **not** the same defect as the three above
and must not be filed with them:

| | |
|---|---|
| Failing test | `M004 ... > runs a deterministic live turn: assistant text, denied write, completion` (`apps/desktop/e2e/specs/m004-session.e2e.ts:183`) |
| Symptom | `completed assistant transcript never rendered` |
| Not | a React renderer-ordering / `data-testid` timing issue |

Attempt 2 of the identical SHA is **green**, and the specific test passes
(`7 passing`), so the failure is nondeterministic. The WebDriver logs in the
run artifact localize it precisely:

- `[data-testid="message-list"]` was polled **600 times over 60 seconds** and
  returned `assistantE2E deterministic turn complete.assistantE2E deterministic
  turn complete.` on **every** poll.
- The substring `Examining your request.` appeared **zero times** in the whole
  log. In the green run both texts are present on the *first* poll.
- The turn still reached `completed` ~200 ms after the permission denial, and
  `truncated-messages` was never rendered, so the projection's bounded window is
  not implicated.

So the first assistant message — the one carrying the tool call — was absent
from the projection the daemon produced, while the final one arrived intact.

**Root cause (traced, high confidence).** Assistant text reaches the projection
only as `TurnTextDelta` → `MessageAppended`
(`src/agent/provider_turn.rs:468-476` → `src/core/mod.rs:428-436` →
`crates/codegg-core/src/projection_replay/publication.rs:63-81`), and
`should_persist` (`src/core/event_log.rs:44-47`) deliberately **excludes**
`TurnTextDelta`. Streamed assistant text is therefore *never durable*. The loss
happens when a mid-turn resync installs a snapshot that cannot contain it:

1. The permission round-trip stalls the driver long enough for a live history
   gap (`crates/codegg-protocol/src/projection/consumer.rs:440-444`); a full
   subscription channel flips to `ResyncRequired`
   (`crates/codegg-core/src/projection_replay/subscription.rs:206-212`).
2. `driver.rs:187-228` (`converge`) then issues a fresh `ProjectionSubscribe`.
3. That path installs a **durable-only** snapshot built at
   `src/core/daemon_refresh.rs:45-166`, which populates runs/worktrees/run-groups
   but never `active_turn`/`messages`.
4. `consumer.rs:494-513` (`accept_replay`) replaces the snapshot wholesale and
   advances the cursor to `high_water_seq`
   (`src/core/daemon_projection.rs:333-337`).

Because the cursor is already past those sequences and the events were never
durable, the `MessageAppended` envelopes already folded into `active_turn` are
discarded and **can never be replayed**. The turn still completes, because
`TurnCompleted` and the final delta arrive on the new subscription — which is
exactly the observed shape. `apps/desktop/src-tauri/src/present.rs:230-243`
renders messages solely from `snapshot.active_turn`.

Two secondary paths reach the same symptom and are not excluded:
`reducer.rs:357-366` (`TurnStarted` installs a fresh empty turn, discarding
accumulated messages) and the `orphan_message` drop at `reducer.rs:369-376`
(a `MessageAppended` arriving with no `active_turn`).

**This is pre-existing and outside this corrective.** The failing and green
revisions differ only in `plans/` documents:

```text
git diff --name-only 4a7567d4..03c4c7e3
  plans/closure/provider-connect-qualification-wire-dispatch-post-closure-corrective/001-status.md
  plans/registry.md
```

No provider decision path is involved: the M010 credential observer acts on the
already-terminal `Result` of `stream_with_retry`
(`src/agent/provider_turn.rs:72-105`) and is explicitly non-blocking
(`src/core/provider_qualification.rs:98-110` spawns the write). It cannot
reorder or drop envelopes already published upstream. It is, however, *bound* in
this fixture — `selectMockModel` (`apps/desktop/e2e/fixture-client.ts:214`)
creates a real `openai` provider connection — so the observer does run here; it
simply cannot cause this.

The spec's own comment is also inaccurate and is worth correcting in the owning
subsystem: it says the assistant tool-call message "may not be committed to the
session projection until the tool result is recorded". Messages are appended as
each delta streams, not at tool-result time; the real hazard is that they are
projection-only and unreplayable.

The three earlier flakes and this defect are all pre-existing and in unrelated
subsystems. None is on a provider decision path, so fixing them would widen this
corrective past its stated scope. Each — and this one with the mechanism above —
warrants separate triage by the owning subsystem.

## 1. Executive finding

M010 and M011 were both closed with local evidence only, and M011 recorded that
hosted CI had never been observed. That low finding was not merely missing
paperwork. Behind it sat a real evidence gap:

> Nothing proved that a **persisted** OpenCode Go connection plus a **persisted**
> model selection actually reaches the correct multi-surface runtime.

M011's tests begin at the provider (`OpenCodeGoProvider::stream`). M010's tests
begin at durable health (`ProviderConnectionCredentialReporter`). The join
between them — durable connection → durable selection → runtime resolution →
wire — was untested. A regression that broke only that join would have passed
every existing test in both milestones.

C001 closes that gap with one cross-layer trajectory, reconciles the stale
status text that made a closed roadmap look active, and disposes of both M011
low findings against evidence rather than assumption.

## 2. The cross-layer trajectory

`tests/opencode_go_connection_trajectory.rs` crosses the join with no layer
bypassed:

1. an `other:opencode_go` durable connection whose secret resolves through the
   real encrypted `CredentialStore` (a genuine `SecretRef` bound to the
   `("opencode_go", account, BearerToken)` namespace);
2. a persisted bounded model catalog containing wire-resolved models, with a
   health row that starts `unverified` — catalog reachability is not credential
   verification (M010);
3. `session_selection::update_selection` at a pinned connection revision and
   catalog revision;
4. `durable_selected_runtime_model`, the projection a turn actually consumes;
5. `ConnectionManager::resolve_with_runtime_reference` over the real
   `ProviderConnectionFactory`, which re-reads the durable row, enforces
   lifecycle state and secret binding, and constructs through
   `setup_catalog::build_durable_provider`;
6. one inference request, captured on a loopback socket;
7. M010's `ProviderConnectionCredentialReporter` reacting to the outcome; and
8. secret-absence across the database, the protocol DTO, the request body, and
   the encrypted credential file on disk.

Three tests, each proving a distinct claim:

| Test | Claim proven |
|---|---|
| `durable_opencode_go_selection_reaches_the_profile_owned_responses_surface` | A durable selection reaches `POST /responses` with `Authorization: Bearer`, no `x-api-key`, `x-opencode-session`, and the selected model in the body; a persisted catalog still reads `unverified`, and a completed authenticated request promotes it to `verified`. |
| `the_durable_selection_not_the_connection_picks_the_surface` | On **one** connection, credential, and revision, re-selecting to a Messages model moves the request to `POST /messages` with `x-api-key` and no `Authorization`. The surface follows the *selection*, not the connection. |
| `a_typed_auth_rejection_writes_a_revision_scoped_verdict` | A typed 401 yields `authentication_failed` via `ProviderError::error_class()` alone, and a verdict for a superseded revision matches no row and cannot overwrite the current one. |

### Negative mutation testing

The plan requires the trajectory to fail against synthetic regressions. Both
required regressions were injected, observed, and reverted:

| Injected regression | Result |
|---|---|
| OpenCode Go constructed as the old single-surface compatible provider (`create_opencode_go` **and** the capture construction path) | **Caught** — 2 of 3 trajectory tests fail; the compatible provider ignores the surface contract and never reaches the profile-owned path |
| The durable selection not carried to runtime (`durable_selected_runtime_model` returns a fixed model) | **Caught** — all 3 trajectory tests fail on the projection assertion (`opencode_go/glm-5.3-flash` vs `opencode_go/gpt-5.6-luna`) |

Both mutations were reverted and the tree returned to a clean `git diff`
state before this record was written. The trajectory is therefore evidence,
not decoration.

## 3. The capture seam

The shared profile pins the production origin, so observing the request needs a
loopback capture server. That required the test-only seam the plan permitted.
It is narrow by construction, and each property was verified rather than
asserted:

| Property | How it holds | Evidence |
|---|---|---|
| Compiled out of production | `#[cfg(any(test, feature = "capture-test-support"))]`; the root crate enables the feature through a `[dev-dependencies]` entry only, so under resolver 2 it is absent from `cargo build` | `nm -C target/debug/codegg` over the built binary: **0** seam symbols |
| Present when it should be | The `cfg(test)` rlib does carry them, proving the scan above is meaningful and not a failed lookup | `nm` over `libcodegg_providers-*.rlib`: **4** symbols in the `cfg(test)` rlib, **0** in the other eight |
| Not reachable from config/protocol/env/CLI | There is no production setter; only the two cfg-gated builders exist | Source review; no manifest, protocol, or env reference |
| Origin only | Path, per-surface auth shape, and the surface decision still come from the shared profile, so a captured path is the production path | Captured assertions (`/responses`, `/messages`, `Bearer` vs `x-api-key`) |
| Instance-scoped | The base lives on one `ProviderConnectionFactory`, so concurrent tests cannot observe each other's origin | See the rejected alternative below |
| Credential policy unchanged | The capture construction path applies the same `credential_capability.accepts` check as the production arm | `setup_catalog::build_opencode_go_with_capture_base` |

**Rejected alternative, recorded deliberately.** The first implementation used a
process-global capture origin with a drop guard. It compiled and its tests
passed individually, but under `cargo test` the three trajectory tests run
concurrently in one process and **raced**: one test's provider was constructed
against another test's capture server, and the Responses test failed with
another test's 401. A test that only passes in isolation is not evidence. The
seam was moved onto the factory instance and the race disappeared. This is why
the seam must not be converted back into process or environment global state.

## 4. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| One cross-layer test proves persisted OpenCode Go connection/model selection reaches the correct multi-surface runtime without live credentials | `cargo test -p codegg --test opencode_go_connection_trajectory --locked` | pass — 3/3 |
| Trajectory uses the durable connection/provider resolution path, not a direct unrelated provider constructor | Resolves through `ConnectionManager` + `ProviderConnectionFactory`; `ProviderConnectionFactory::new` is never used by the test | pass |
| Trajectory fails against a single-surface-compatible OpenCode Go | Injected mutation 1 | pass — caught |
| Trajectory fails when the durable selection is not carried to runtime | Injected mutation 2 | pass — caught |
| Seam is impossible to activate from config, protocol, environment, or production CLI | No production setter; cfg-gated; `nm` shows 0 symbols in the shipped binary | pass |
| Seam is absent from the public production provider contract | `nm` over the built `codegg` binary: 0 symbols | pass |
| M010 regressions | `cargo test -p codegg --lib core::eggpool::tests` / `provider_qualification` | pass — 28 + 3 |
| OpenCode Go provider suite | `cargo test -p codegg-providers --lib -- --list` | pass — 242 total; 20 `opencode_go`, 12 `provider_profile`, 21 `wire::` |
| Provider qualification guard | `scripts/check_provider_qualification.py` | pass — 4/4 |
| Multi-surface dispatch guard | `scripts/check_provider_multi_surface_dispatch.py` | pass — 6/6 |
| Wire boundary / catalog consistency / resilience / OpenAI endpoint guards | `check_provider_wire_boundary.py`, `check_provider_catalog_consistency.py`, `check_provider_resilience_ownership.py`, `check_openai_endpoint_composition.py` | pass |
| Formatting | `cargo fmt --all -- --check` | pass — clean |
| Workspace Clippy | `cargo clippy --workspace --all-targets -- -D warnings` | pass — clean |
| Canonical quick verification | `bash scripts/verify.sh quick` | pass — exit 0 |
| Hosted canonical CI | run 37736136266 (closure revision `4a7567d4`, first attempt); run 37734112906 (`4b3bad5a`); run 37730586250 attempt 2 (`ce23b4e4`) | pass — 12386/12386, 5 skipped, 0 failed |
| Hosted path-gated workflow | run 37736136595 (Desktop E2E, `4a7567d4`, first attempt); run 37730586236 (`ce23b4e4`); run 37737818626 attempt 2 (Desktop E2E, tip `03c4c7e3`, after attempt 1 hit the projection defect diagnosed in "The fourth failure is a different, diagnosed defect") | pass |

## 5. WP-B — Pin/dependency audit

Verified on the C001 head:

- `eggpool-provider-profile` resolves to `9ac6a1318e8db3c034b5ab54987317752d5ffea6`.
- `eggpool-wire` resolves to that **same** revision (`Cargo.lock`, one source line).
- `Cargo.lock` contains exactly **one** `eggpool-wire` package identity, so one
  `WireSurface` vocabulary links.
- No EggPool runtime, account, or catalog crate was added. The lock holds three
  `eggpool*` packages: `eggpool-model-routing` (pre-existing, introduced by an
  earlier M001 commit at `d70b5963`, depends only on `sha2`),
  `eggpool-provider-profile`, and `eggpool-wire`. M011 touched exactly one
  manifest, `crates/codegg-providers/Cargo.toml`.
- The durable CodeGG provider id remains `opencode_go`; the adapter maps only at
  the shared-profile boundary via the single explicit entry
  `SHARED_IDS = [("opencode_go", "opencode-go")]`.

Upstream EggPool `main` state was **not** re-checked for repin purposes and
**nothing was repinned**. This corrective qualifies the accepted M011 revision,
not latest upstream.

## 6. WP-D — Low-finding disposition

**Finding 1 — stale OpenCode hint rows in `eggpool-wire`.** Disposition:
**non-blocking upstream documentation/data debt; no action.**

Evidence that CodeGG dispatch reads only `eggpool-provider-profile`:

- Every `eggpool_wire` API path used anywhere in CodeGG is one of
  `ir::*`, `codec::StreamAdapterKind`, `profile::WireSurface` (the enum type
  only), `stream::StreamEventDecoder`, `stream::StreamTerminalOutcome`,
  `sse_split_points`, and `stream_conformance_vectors`. None of these resolves
  a model-to-surface hint.
- The stale rows do exist upstream and are compiled in:
  `eggpool-wire/assets/_wire_profiles.toml` carries seven `provider_id =
  "opencode-go"` hint rows, embedded by `include_str!` in
  `eggpool-wire/src/profile.rs`.
- They are nevertheless **unreachable from CodeGG's decision path**. The only
  consumer of model-to-wire mapping is
  `provider_profile::resolve_route`, which reads `eggpool-provider-profile`
  (`ProviderProfileRegistry`), and its tests pin that resolution is exact and
  fails closed.

No EggPool file was modified by this corrective.

**Finding 2 — opaque `ProviderProfile::runtime_capabilities` passthrough.**
Disposition: **no action.**

Evidence: `runtime_capabilities` appears **zero times** across all CodeGG
`.rs` sources and manifests. CodeGG cannot consult it for wire, auth, or
qualification decisions because it does not reference it at all.

Neither finding is on a production decision path, so no separate bounded
corrective was registered.

## 7. Safety and compatibility review

- Production behavior is unchanged. The only new branch in a runtime path is
  `#[cfg]`-gated and defaults to the pre-existing construction.
- No schema, migration, protocol, catalog, pin, or dependency change.
- No new provider, endpoint constant, or configuration key.
- Credential handling is unchanged: capability policy is identical, the
  trajectory asserts the secret is absent from the database, the protocol DTO,
  the request body, and the on-disk credential file, and no secret appears in
  logs or in this record.
- The trajectory creates no billable request and performs no network I/O off
  loopback in a passing run.
- Tests are deterministic and use `isolated_pool()` with `current_thread`
  flavor.

## 8. Registry reconciliation

`plans/subsystems/provider-connect-qualification-wire-dispatch-corrective-addendum.md`
now reads `closed (both milestones accepted)`, M011 reads `closed`, the
dependency graph marks all prerequisites historical and satisfied, the
completion definition is marked satisfied, and the terminal status table is
aligned.

In `plans/registry.md`:

- the parent corrective row is `closed/current`;
- the C001 roadmap row is `closed/current`;
- the C001 implementation row in **Dependency-ready implementation plans** is
  `closed`, with its closure record, production-delta-zero statement, and
  hosted run ids inline;
- a matching row was added to **Recently closed or conditionally closed work**;
- the gate paragraph at `registry.md:200` was updated from "C001 … is the
  strict merge/closure authority until it closes" to record that C001 has
  closed and what it discharged, without altering any historical M010/M011
  claim.

Note on table placement: the M011 row remains in **Dependency-ready
implementation plans** carrying the terminal status `implemented; closed`. That
is the registry's existing convention for finished plans in that table (see the
decision-model `C001` and Desktop `M006-A` rows), not a leftover ready state.
No M010 or M011 row is in a ready, active, or blocked state anywhere.

The M010 and M011 closure records were **not** edited; `git diff` against the
predecessor closure directory is empty. Their historical claims stand unaltered.

## 9. Registry unblock audit

**No registered downstream plan was unblocked.**

A full sweep of `plans/` (950 files, including `plans/archive/`) for
`provider-connect-qualification-wire-dispatch`, `provider[ _-]connect[ _-]qualification`,
`wire-dispatch`, `multi[- ]surface`, `opencode[- ]go`, `eggpool-provider-profile`,
`9ac6a131`, and the milestone ids `M010`/`M011`/`C001` found only:

- the roadmap's own file set, and
- `plans/registry.md` rows that **report** on this workstream.

Critically, the registry's **Blocked work** table (10 rows) contains no entry
naming this roadmap, and no table anywhere lists C001, this roadmap, M010, or
M011 as a dependency **of another plan**. `registry.md:200` already scopes C001
as *closure authority* rather than a gateway others wait on, and
`registry.md:329-330` carry the equivalent negative audits for M010 and M011.

Closing C001 therefore flips this workstream's own rows to closed and produces
no other status change. This is consistent with the plan's own handoff note:
this is a terminal corrective, not the start of a new provider feature line.

## 10. Unresolved findings

None in this corrective.

Two items are explicitly **closed rather than left open**:

- M011's "hosted CI not observed" low finding is now discharged by runs
  37730586250 (attempt 2) and 37730586236 on the C001 head.
- M011's and M010's "no cross-layer trajectory" evidence gap is discharged by
  `tests/opencode_go_connection_trajectory.rs` plus its two mutation checks.

The two §6 low findings are disposed as non-blocking with evidence, not
silently dropped.

### Carried forward, not closed by this corrective

Four pre-existing hosted failures in unrelated subsystems surfaced. Three are
runner-sensitive flakes: `goal::checkpoint` tail ordering, `codegg-client`'s
`session_projection_driver`, and one `apps/desktop` renderer test (detailed
above). All three are green on a re-run of the same SHA.

The fourth is a **diagnosed projection defect**, not a flake of the same kind:
in Desktop E2E run 37737818626 attempt 1, a mid-turn resync replaced the
client snapshot with a durable-only one and advanced the cursor past
never-durable `TurnTextDelta` envelopes, so an already-rendered assistant
message was permanently lost. `should_persist` excludes `TurnTextDelta`
(`src/core/event_log.rs:44-47`), so the dropped events can never be replayed.
Full mechanism, file:line chain, and the two secondary paths are in "The fourth failure is a different, diagnosed defect".

All four are outside this corrective's blast radius and none is on a provider
decision path. They are **not** claimed as fixed and **not** silently dropped.
Each warrants separate triage by the owning subsystem — the projection one most
of all, since unlike the flakes it is a deterministic *design* gap that will
recur whenever a resync lands mid-turn. No registry plan was registered here
because doing so would require judging subsystems this corrective never read;
that judgment belongs to whoever owns those plans.

## 11. Closure disposition

C001 is formally closed. The M010 and M011 closures remain authoritative and
unedited. This record closes the post-closure corrective that reconciled the
workstream's status text, proved the durable-connection-to-wire trajectory,
disposed both low findings, and obtained the hosted CI evidence its predecessors
lacked.

The provider connection qualification / multi-surface workstream now has no
active successor. PR #105 remains open for review and is **not** merged by this
corrective.