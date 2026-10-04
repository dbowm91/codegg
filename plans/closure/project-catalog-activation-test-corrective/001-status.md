# Project Catalog Activation Test Corrective C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/project-catalog-activation-test-corrective/001-concurrent-coalescing-lease-lifetime.md`

Source corrective addendum:

- `plans/subsystems/project-catalog-activation-test-corrective-addendum.md`

Predecessor record this corrective discharges (immutable, not edited):

- `plans/closure/project-catalog/003-status.md` (implementation `27cbd43`,
  Milestone 3 — lazy activation and health).

Repository baseline reviewed: `6a679bd2` (fix head `30e00916`)

Implementation commits:

- `30e00916` — project-catalog activation test C001 (barrier-held lease
  lifetime in one test; no production change).

## 1. Executive finding

The concurrent same-owner coalescing test was asserting a concurrency property
while holding no concurrency, and that is now corrected. The product's
reference-counted lease semantics were correct throughout; the test released
each lease as its own task body completed, so whether a later task coalesced
depended on whether it acquired before any earlier task's release landed. A
`Barrier(8)` now holds all eight leases alive until every task has acquired,
which is the precondition the assertion describes, making the test
deterministic by construction. The change is test-only: no production code,
contract, or behavior moved, and the assertion is unchanged and unweakened.

## 2. Requirement-to-evidence matrix

| Requirement (plan §4/§13) | Evidence | Result | Notes |
|---|---|---|---|
| Root cause demonstrated deterministically, not inferred | Direct measurement against the registry: a lease **held** across a second activation returns the same id; a lease **released** before it leaves `active_count() == 0` and the next activation returns a **different** id. Observed ids differed on the released path and matched on the held path | pass | This is the actual mechanism, exercised directly rather than inferred from a red log |
| All eight lease ids still asserted equal | `tests/project_activation.rs:146-148` unchanged in the diff | pass | The assertion text is byte-identical |
| Eight-way concurrency and `multi_thread`/`worker_threads = 2` retained | Unchanged; the fix only inserts a barrier wait | pass | Concurrency is strengthened, not reduced |
| Both post-conditions still asserted | `active_count() == 0` and `workspace_services.active_count() == 1` unchanged | pass | — |
| Product code unchanged | `git show --stat 30e00916` touches only `tests/project_activation.rs`; `src/core/project_activation.rs` absent from the diff | pass | `acquire`, `release_inner`, handle accounting, capacity policy, TTL, eviction all untouched |
| Other tests in the file unchanged | Diff confined to the one test function | pass | 6/6 pass |
| Stable under repetition and induced load | 25/25 sequential runs with 4 background CPU-load processes; 24/24 across three rounds of 24 concurrent instances; 12/12 on the pre-fix binary under the same conditions for contrast | pass | See §4 on what this does and does not prove |
| `cargo fmt` and clippy clean | `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets --locked -- -D warnings` clean | pass | — |
| Hosted full sweep completes and E2E green | see §4 hosted block | pass | — |

## 3. Production implementation evidence

None. This milestone changes no production code.

What changed, in one test:

- `tests/project_activation.rs`,
  `concurrent_same_owner_activation_coalesces_scope_and_bundle` — each task
  now extracts `lease_id` and `coalesced` from its activation, awaits a
  shared `Barrier(8)`, and only then returns, so no lease is released before
  all eight have been acquired. A comment records why: coalescing is a
  concurrency property, and `ProjectActivationLease::drop` decrements the
  activation's handle count and removes the registry entry at zero
  (`src/core/project_activation.rs:182-191`).

What deliberately did not change:

- `ProjectActivationRegistry::acquire` and its per-key single-flight lock.
- `ProjectActivationLease::release_inner` and handle accounting.
- Capacity policy, lease TTL, and eviction.
- The assertion, the concurrency level, the runtime flavor, and the
  post-conditions.
- The other five tests in the file.

Root cause, established by direct measurement rather than by inference from a
failing log. `acquire` reuses an existing activation only when
`ActiveActivation::retain` succeeds, and `retain` increments the handle count
(`src/core/project_activation.rs:283-294`); `release_inner` decrements it and
removes the registry entry when it reaches zero
(`src/core/project_activation.rs:178-192`). The two behaviors were measured
side by side on identical inputs:

| Precondition at the second activation | Registry after the first lease drops | Lease id at the second activation |
|---|---|---|
| First lease still held | 1 active | **same** as the first — coalesces |
| First lease released | **0 active** | **different** — does not coalesce |

The original test's task bodies released their own leases on return, so which
row applied depended entirely on task scheduling. On a loaded shared runner a
task could finish and release before a later task acquired, and the later
task then behaved correctly by minting a new lease — which the test read as a
failure. The production contract was never violated; the test was measuring
its own scheduling.

## 4. Verification executed

### Commands run

```bash
cargo test --test project_activation
# -> 6 passed; 0 failed

# repetition under induced load (4 background CPU-load processes)
for i in $(seq 1 25); do <test binary> concurrent_same_owner --test-threads=1; done
# -> pass=25 fail=0   (fixed binary)
# -> pass=25 fail=0   (pre-fix binary, same conditions — see the honesty note)

# contention (6 background load processes, 24 concurrent instances, 3 rounds)
# -> 24/24, 24/24, 24/24   (fixed binary)

# deterministic mechanism demonstration (temporary, removed after use)
# -> HELD   -> same id
# -> DROPPED-> active_count()==0, different id

cargo fmt --all -- --check                      # clean
cargo clippy --workspace --all-targets --locked -- -D warnings   # clean
TMPDIR=/tmp cargo nextest run --workspace --locked --profile ci
```

Hosted:

```bash
gh run view 37224840466   # CI / verify on 6a679bd2 (identical code to the red run)
gh run view 37224840476   # Desktop E2E on 6a679bd2
gh run view 37227652568   # CI / verify on 30e00916 (fix head)
gh run view 37227652578   # Desktop E2E on 30e00916
```

### Results

Local:

- `cargo test --test project_activation`: **6 passed, 0 failed**.
- Fixed binary under induced load: **25/25**. 24 concurrent instances across
  three rounds: **24/24** each.
- **Honesty note on that evidence:** the *pre-fix* binary also passed 12/12
  under load and 24/24 across three rounds of 24 concurrent instances. Local
  repetition therefore does **not** demonstrate that the race existed, and is
  not offered as the root-cause proof. The proof is the deterministic
  mechanism demonstration above, which shows the exact condition under which
  the original assertion is false. Repetition only establishes that the fixed
  test is not newly flaky.
- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean.
- Local full workspace sweep: see §4 hosted block for the authoritative
  result. A local `nextest` run was attempted but was **invalid as a signal**:
  it was run concurrently with `cargo check --workspace --all-targets` from
  `scripts/verify.sh quick` on the same machine, so it reproduced known
  load-sensitive failures rather than anything about this change. Those two
  failures are themselves already-documented pre-existing flakes in this
  repository's own records, and neither is touched by this corrective:
  `tool::lsp_preview_apply::tests::fresh_tool_registry_expires_prior_preview_id`
  (recorded at `plans/closure/eggwork-fixed-target-remote-execution/001-status.md`
  and `plans/closure/tool-selection-advisor/005-status.md`, both noting it
  passes in isolation and did not recur on the gating re-run) and
  `agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication`
  (recorded at `plans/closure/toolchain-clippy-drift-corrective/001-status.md`
  as "one transient `asset_refresh` coalescing timeout on the first attempt,
  green on `--failed` rerun with no code change — timing flake"). The
  `$TMPDIR`/`SUN_LEN` constraint on `gui_client` noted above is likewise
  environment-specific and pre-existing.

Hosted:

- On `6a679bd2` — the same compiled workspace as the red run `37223675386` —
  run **`37224840466`** is **green** with the full sweep completing:
  **`12180` tests run, `12180` passed, `7` skipped** in 380.916s. Run
  **`37224840476`** (`Desktop E2E`) is also **green**: 73 desktop host checks
  passed and every WebDriver spec file passed. This is the direct
  confirmation that both failures in `37223675386` were flakes on unchanged
  code, and it is the second consecutive fully-green full sweep.
- On the fix head `30e00916` — run **`37227652568`** (`CI / verify`) is
  **green** with the full sweep completing: **`12180` tests run, `12180`
  passed, `7` skipped** in 398.283s, all twenty guard and lint steps green.
  Run **`37227652578`** (`Desktop E2E`) is **green**.
  `concurrent_same_owner_activation_coalesces_scope_and_bundle` passed at
  0.300s (test 6,884/12,180 — past the point where fail-fast previously cut
  the sweep), alongside the causal timing gates
  `m005_holdout_structural_gates` (13.837s),
  `observe_mode_leaves_live_definitions_byte_identical`, and
  `replay_p95_within_budget`.

## 5. Invariant review

- The assertion still requires all eight lease ids to be equal, byte-identical
  in the diff.
- Eight-way concurrency and `flavor = "multi_thread", worker_threads = 2` are
  retained; the test is now guaranteed real overlap, which is a stronger
  property than the accidental overlap it previously relied on.
- Both post-conditions (`active_count() == 0`,
  `workspace_services.active_count() == 1`) are still asserted.
- `ProjectActivationRegistry::acquire`, `release_inner`, handle accounting,
  capacity policy, lease TTL, and eviction are unchanged; `src/core/` does not
  appear in the diff at all.
- The other five tests in `tests/project_activation.rs` are unchanged and pass.
- `architecture/project-catalog.md` is untouched: no ownership, contract, or
  documented behavior changed. The lease semantics the test now asserts were
  already implemented and already documented.
- No historical closure record was edited. `plans/closure/project-catalog/`
  `001`–`004` are unchanged, as is the M003 record this corrective discharges.

## 6. Failure and recovery review

Not applicable in the production sense: no production state, process,
authority, or persistence surface is involved.

The one test-introduced hazard is a hang rather than a false pass: if a task
panicked before reaching the barrier, the remaining tasks would block
indefinitely. This is accepted deliberately. Adding a timeout would
reintroduce exactly the timing sensitivity this milestone removes, and the
test's inputs are entirely local with `.unwrap()` on every fallible step, so
the realistic failure is a panic, which surfaces loudly. MSRV 1.89 is
preserved; `tokio::sync::Barrier` is long stable.

## 7. Migration and compatibility review

None. No schema, storage, protocol, configuration, or public API change. The
change is confined to one test function in one integration test file.

## 8. Security review

None applicable. No authorization, credential, network, sandbox, or privilege
surface is touched, and no production code changes. The change does not relax
any check: the coalescing assertion is stricter in practice, because it now
runs under guaranteed concurrency instead of accidental concurrency.

## 9. Documentation and operations

- `plans/subsystems/project-catalog-activation-test-corrective-addendum.md`.
- `plans/implementation/project-catalog-activation-test-corrective/001-concurrent-coalescing-lease-lifetime.md`.
- This closure record.
- `plans/registry.md` rows.
- No `architecture/` change. The in-test comment carries the lease-lifetime
  reasoning for the next reader at the point of confusion.
- Operator note: this defect and the causal timing flake share a *shape* — a
  test whose pass/fail depends on runner scheduling rather than on the
  property it names. That shape, not the specific bug, is what the two
  correctives have in common.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | `Desktop E2E` `m004-session.e2e.ts` fails intermittently with "completed assistant transcript never rendered" at `apps/desktop/e2e/specs/m004-session.e2e.ts:183`. That `waitUntil` allows 60 s, while the comparable waits in the same file allow 60 s for controller state, 120 s for fail-closed submit, and 180 s for turn completion. | Can redden the branch independently of any code change; it did so in run `37223675386` and was green in `37224840476` on identical code. | Not diagnosed here. Deliberately out of scope: it is a different mechanism in a different subsystem. Worth its own look at whether the 60 s transcript wait is simply too tight relative to the render path it depends on. |
| low | The mechanism demonstration used for §3 root cause was a temporary test, removed after use. | The evidence exists only in this record, not as a permanent regression test. | Accepted. Re-adding it would duplicate the coalescing property already asserted both synchronously (`activation_refreshes_assets_and_is_idempotent_per_owner`) and concurrently (the fixed test). |

Neither is a regression introduced by this corrective, and neither blocks
closure of the change itself.

## 11. Roadmap disposition

The corrective addendum is terminally satisfied; no successor milestone exists
in this track.

This reopens no project-catalog scope: Milestones 001–004 remain closed with
their original dispositions, and the activation, lease, capacity, and eviction
semantics are exactly as M003 implemented them. The practical effect is that
the workspace sweep is no longer truncated by a defect in a test of an already
closed milestone.

It also completes the chain opened by the causal frontier timing corrective
(`cdfd6257`): that fix removed the truncation which had been hiding this
defect, and this fix removes the defect the truncation had been hiding.

## 12. Registry updates

- `plans/registry.md` roadmap/status table: add the project-catalog activation
  test corrective row, closed, with implementation `30e00916`.
- `plans/registry.md` implementation-plan table: add C001, closed.
- `plans/registry.md` closure table: add C001, closed, with the hosted run ids.
- No other registered plan lists this corrective as a hard dependency, so
  nothing else is unblocked by it.
