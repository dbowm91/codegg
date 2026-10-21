# Branch and CI Stabilization Milestone C001 — Branch and CI Stabilization

Status: implemented

Repository baseline: branch `planning/linux-sandbox-next-admission-and-adr-cleanup` at `d251b11c` (pre-rebase tip), rebased onto `main` at `fe2c1306`

Source roadmap: `plans/subsystems/branch-and-ci-stabilization-roadmap.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md` §4 (correctness before transparent magic);
- `plans/002-long-term-roadmap.md` — cross-phase execution rules;
- `plans/003-planning-process.md` §7 (corrective passes) and §11 (planning anti-patterns);
- `architecture/testing.md` — the verification contract and CI policy;
- `architecture/overview.md` — the "Verified counts" table.

Applicable ADRs: none required. The command-authority ADR's **identifier** moves
(ADR-0042 → ADR-0045) with its accepted status, date, and substance unchanged;
that rename is recorded in `plans/adrs/linux-command-adr-id-migration.md` and in
`plans/implementation/linux-execution-isolation/012-adr-identity-collision-and-admission-gate-corrective.md`.

Primary class: invariant

## 1. Objective

Make this branch integrate with `main` without losing either side's content, and
make the default test sweep's green result mean what it claims: a property of
the product, not of the host it ran on.

## 2. Why this milestone is ready

- `main` is reachable and has been read, so the collision set is known rather
  than guessed.
- The branch's own feature work (C007's production private-index publication,
  C023's obtained-network-fact contract) is landed and committed, so the
  rebase replays a stable history rather than an in-flight one.
- No other milestone is a hard dependency of this one.

## 3. Current implementation evidence

At the baseline the branch was 295 commits ahead of and 90 behind `main`, and
the tree carried three defects that only `main`'s recent work exposed:

1. **Storage-layout collision.** `main` allocated layout 83 to the
   managed-agent-scratch migration; the branch allocated 83 to Linux M005's
   network-intent preference and 84 to C007's publication journal. Merged, the
   file contained two functions named `migrate_v83`.
2. **ADR-identifier collision.** The branch had taken ADR-0042 for the
   accepted command-authority decision, reading `main`'s allocation as ending
   at ADR-0041; `main` had since allocated ADR-0042 to the accepted composer
   decision. `scripts/check_adr_integrity.py` failed on the branch and passed
   on `main`.
3. **Lost content.** The branch's own commit `c446af35` had, as collateral
   damage, deleted eleven registry rows and their prose for three unrelated
   workstreams.

Two independent defects made the sweep's signal untrustworthy:

4. **A transient read reported as a refusal.** `crates/egggit`'s private
   object-view path mapped a failed object-store read to
   `BackendUnavailable([RepositoryAlias])` — "Git backend unavailable for
   [RepositoryAlias]" — even when the cause was Git's own background
   maintenance removing a pack file mid-walk. Fourteen tests failed that way in
   one sweep and passed in the next.
5. **Host-dependent tests in the default sweep.** Three LSP tests query a live
   language server; their runtime is a workspace index. One took 90 s on this
   workstation, against a nextest kill threshold of 120 s, and was killed in
   both recorded sweeps.

## 4. Invariants that must not regress

- One storage-layout number per migration; `STORAGE_LAYOUT_VERSION` tracks the
  highest wired migration.
- One ADR identifier per decision; every live local ADR path resolves.
- A persistent object-store failure stays a typed refusal. The fix must never
  turn an unreadable store into a fabricated success or a fabricated path.
- The default sweep must not require an installed toolchain.
- No accepted decision's status, date, or substance changes.

## 5. Scope

**In scope**

- Rebase the branch onto `main`; resolve every conflict by an explicit rule.
- Resolve the storage-layout and ADR identifier collisions.
- Restore content the branch had lost.
- Close the integration gaps the merge exposed.
- Make the object-store read failure an instability rather than a refusal.
- Move the live-language-server tests behind the existing feature gate.
- Re-measure the published counts.
- Record the plan, this closure, and the registry rows.

**Out of scope**

- Any admission decision, any ADR-0044 status change, any host qualification.
- The C005 plan and its blocked owner decisions.
- Product behaviour of any kind.
- Historical closure verdicts, statuses, and findings.

**Scope correction — 2026-10-20 (recorded, not silent).** The "product behaviour
of any kind" exclusion above was written before the authoritative sweep had run.
The sweep showed that this milestone's own objective — "make the default test
sweep's green result mean what it claims: a property of the product, not of the
host it ran on" (§1) — cannot be met without **one** product change: the scratch
allocator's per-user cache root. The exclusion and the objective conflict, and
the objective governs, because the roadmap's completion definition
(`plans/subsystems/branch-and-ci-stabilization-roadmap.md` §11) requires the
sweep's result not to depend on the host.

The change is bounded and is the only product behaviour this milestone touches:
a cache root the product creates is created private, so the operator's umask
cannot produce a root the product's own ownership check would then refuse. No
fail-closed path is weakened — a cache root that already exists and another
user could write to is still refused, and is still refused with the same typed
error. The work package, the tests, and the evidence are §7 WP-G, §10, and the
closure's §3.7.

## 6. Required production changes

- **Storage** — `crates/codegg-core/src/session/schema.rs`: the branch's M005
  migration moves 83 → 84, the C007 journal 84 → 85, and the wiring and
  dispatch arms follow. `crates/codegg-core/src/storage/mod.rs`:
  `STORAGE_LAYOUT_VERSION` becomes 85.
- **Git read path** — `crates/egggit/src/process.rs`: an object-store read
  failure is instability, not refusal; the existing bounded attempts absorb
  it.
- **Test surface** — `src/tool/lsp.rs`: three live-language-server tests move
  behind `lsp-real-server-tests`.
- **Scratch allocator** — `src/scratch.rs`: the per-user cache root the
  allocator depends on is created private (0700) when the product creates it.
  The allocator must not depend on the operator's umask: it creates the root,
  so it has to be able to use what it created. A cache root that already exists
  is validated, never re-permissioned, and one another user could write to is
  still refused with the same typed `WrongOwner` error.
- **Identifiers and records** — the ADR file, its references, the migration
  record, the guard's self-test fixture, and the affected roadmaps and closure
  records.

## 7. Ordered work packages

**WP-A — Integrate with `main`.** Rebase 295 commits onto `main`. Resolve every
conflict by an explicit rule, never by a bulk strategy: take `main`'s side
where `main` had already landed the same refactor, keep both sides where each
added a field the other needed, and compose by hand where a merge would have
produced a call that satisfies neither side.

**WP-B — Clear the identifier collisions.** Renumber the branch's migrations
past `main`'s, and renumber the branch's command-authority ADR past `main`'s
composer ADR. Update the path-form and referent-aware references, the
migration record, and the ADR guard's own self-test fixture.

**WP-C — Restore lost content.** Re-admit the eleven registry rows and their
prose that the branch's own commit had deleted, and correct the stale ADR
identifier in the editor-Helix roadmap that named a different decision.

**WP-D — Close the integration gaps.** Fix every compilation error the merge
exposed, so the tree builds with every target.

**WP-E — Make the sweep's signal trustworthy.** Treat a transient object-store
read failure as instability with a bounded retry, and keep a persistent
failure a typed refusal. Gate the live-language-server tests.

**WP-F — Re-measure and record.** Re-measure the published counts against this
tree and against their listed `Source`; write the plan, the closure, and the
registry rows.

**WP-G — Make the sweep independent of the operator's home directory.** The
authoritative sweep failed with 44 tests across 15 binaries, every one of them
the same typed refusal: `WrongOwner("/home/sugarwookie/.cache")` or
`WrongOwner("/tmp/.tmp…")`. The cause is the scratch allocator's ownership
check on the per-user cache root: it refuses a root that is group- or
world-writable, and on a host whose cache root is 0777 — or whose umask lets a
created root become 0775 — the allocator refuses the machine it runs on.

Three changes close it:

- **The product** creates the cache root it depends on privately, so the
  operator's umask can no longer produce a root the allocator then refuses.
- **The fixtures** stop lying about their mode: `tempfile` creates directories
  with the *default* permissions, so a fixture that claims "a private 0700
  directory" got 0775 on a host with umask 002. The scratch-home fixture now
  sets the mode it documents.
- **The runners** give the sweep a private cache root of its own, so the
  sweep's result is a property of the product rather than of the machine it
  ran on. `scripts/verify.sh full` creates one with `mktemp -d`; the CI job
  sets one under the runner's temporary directory; and the root stays outside
  the workspace, because several qualifications grant the repository itself as
  the workspace root.

No fail-closed path is weakened: the refusal of an unsafe cache root is
unchanged, and it is now a **tested contract** rather than an accident of the
host.

**WP-H — Make the branch pass the CI clippy step.** `ci.yml` runs
`cargo clippy --workspace --all-targets --locked -- -D warnings`, and this
branch had never been observed in a hosted run — `ci.yml` runs on pull
requests and on pushes to `main`, and no pull request had been opened for this
branch. Four lints in branch-owned code would have failed that step: a
`let...else` that returns `Option` in C007's startup reconciler, a same-type
`.into()` and the redundant closure around it in the publisher, the
`#[must_use]` the async-trait expansion injects in the staging trait, and a
needless `to_vec`/`&` round trip in the scheduler executor. All four are
mechanical and behaviour-preserving. The two `let...else` patterns on `main`
are **not** affected — they sit in functions that return `()`, where the `?`
operator does not apply — so the branch does not diverge from `main` for a
lint `main` does not trip.

**WP-I — Preserve the work the remote branch carried past the rebase base.**
The remote branch was two commits ahead of the rebase base `d251b11c`:
`32611c8d` ("register Linux sandbox admission follow-up correctives") and
`5019fb26` ("correct section references in Linux handoff packet"). They add a
parallel planning packet — the *Linux Sandbox Admission — Post-Review
Corrective Addendum* with its own C001/C002/C003 — and a force-push without
them would have destroyed it. Both are replayed onto the rebased branch; the
only conflict was a registry insertion, resolved by keeping every row from
both sides. See §3.10 of the closure.

**WP-J — Dispose of the stray `plans/.mutate-probe`.** The branch carried a
tracked file of 295 lines of the single character `x`. The parallel packet
above asks for its disposition and sanctions removal "only with
provenance/test confirmation, never by guessing". The provenance is a commit
whose subject describes recording a sweep result, the content carries no
information, nothing in the repository reads it, and it is absent from `main`.
It is removed, with the evidence recorded in the closure's §3.9.

**WP-K — Make the live Eggwork qualification independent of the operator's
umask.** The feature-enabled tier of `verify.sh full` failed five live
qualifications on a tree, a fixture, and a code base that were identical to a
green run. The pinned node refuses a sandbox helper it cannot trust, and
neither cargo nor `tempfile` narrows the modes they create: the helper, the
target directory above it, and the "private" temporary directory all inherited
the process umask, so on a host with a permissive umask the node dropped
`isolation.landlock.workspace-rw.v1` and the executor refused every
required-isolation run. The qualification now **stages** the node and the
helper in a private directory whose modes it sets itself — the directory at
`0700`, the helper an independent copy at `0755` — and the node is started
from that directory. The upstream refusal is untouched; the prebuild script
only has to produce the artifacts, not trust them. Non-vacuity is proved in
the qualification itself: with the build tree restored to the failing state
the same binary goes from 8 passed / 5 failed to 14/14, and a pinned
regression test carries a mirror of the node's trust check. See §3.11 of the
closure.

**WP-L — The fresh rebase (2026-10-21).** `main` advanced by **103 commits**
while this milestone was in flight, so the branch was rebased again, onto
`main` at `cb0e79c0`, replaying **304** commits. The same explicit-rule
discipline as WP-A applies, and it produced four obligations this milestone
did not originally plan for:

- **A second ADR collision.** `main` allocated **ADR-0043** to the accepted
  session-owned subagent lifecycle projection decision, which this branch had
  published for the accepted composable Linux execution isolation decision.
  Under WP-B's rule — main wins a contested number — the branch's decision
  moves to **ADR-0046**, with its substance, accepted status, and acceptance
  date unchanged, and the guard's self-test gains a non-vacuous case for that
  collision class.
- **A file-level no-loss proof.** For every file the branch changed against
  the merge base, the canonical per-file three-way merge of (branch, merge
  base, `main`) is compared with the rebased tree: **93** files match
  byte-for-byte, **0** deviate, and the tree holds **zero** conflict markers.
  This proves *content*; it cannot prove *compilation*, because a pairing
  between a file `main` added and a type this branch extended is invisible to
  any two-way merge.
- **Three cross-file pairings.** `main` added 26 new `.rs` files that use
  three types this branch extended: `JobSubmissionError` (the
  `LabelsTooLarge` variant), and `JobPayload::ManagedArgv` / `JobPayload::Test`
  (the `sandbox_profile` and `network_intent` fields). Each is closed with the
  branch's established values, and the `submission_error_code` slug — which had
  no test at all — is now pinned.
- **One inherited race.** The feature-enabled tier failed
  `real_tui_rollback_invariants_on_writer_closed`, a test whose file and
  `src/server/ws.rs` are byte-identical to `main`'s. Its wait condition omitted
  the counter its own assertion checks, so it failed **7 of 12** isolated
  runs. The fix waits for that counter: strictly more synchronization, no
  assertion weakened, no timeout widened, no product code touched.

WP-A's rule was applied per conflict, not as a bulk strategy, and the same
holds here: `main`'s side was taken where `main` had already landed the same
refactor, both sides were kept where each added something the other needed,
and the registry was resolved to `main`'s side during the replay and then
reconciled by a row-level three-way merge against the merge base, which gained
**64** of `main`'s rows, superseded **9** stale inherited rows, and de-
duplicated **2** placements the branch had listed twice.

## 8. Failure, cancellation, restart, contention semantics

- A history surgery must not change the tree. The correction is verified by
  diffing the tree before and after; an empty diff is the proof.
- A transient object-store failure is retried inside the existing bounded
  attempt count. The bound is the same one the code already used for "the
  source changed"; only the *class* of event that reaches it widened. A
  persistent failure still refuses, and it refuses with the typed outcome.
- A conflict resolution must never be guessed from a marker: every resolution
  is recorded as a rule and every resulting file is checked for surviving
  markers.

## 9. Compatibility and migration

- The layout version moves 84 → 85. A database already at 84 gains only the
  C007 publication journal; a database at 83 gains the M005 preference and the
  journal. Both are additive.
- No user-visible behaviour changes. No protocol changes. No decision changes
  substance.
- The command-authority ADR keeps its accepted status and date; only its
  canonical identifier and its unambiguous cross-references move.

## 10. Required tests

- `cargo check --workspace --all-targets --locked` — every target compiles.
- `crates/egggit` — a regression test that a transient object-store read
  failure is retried rather than refused, and that a persistent one still
  fails closed. Both must be **provably non-vacuous**: the first must fail
  against the pre-fix behaviour.
- The fourteen tests that failed in the recorded sweep — they must pass.
- `scripts/check_adr_integrity.py` and `--self-test` — identifiers unique,
  paths resolve, and the guard's own fixtures hold.
- `scripts/check_project_catalog_invariants.py` — the layout version tracks the
  highest wired migration.
- The default sweep must no longer contain a live-language-server test: the
  three gated names are absent from the test listing without the feature, and
  the four rejection tests remain.
- The scratch allocator — a cache root the product creates is private (0700)
  and umask-independent; a group- or world-writable cache root is refused with
  the typed `WrongOwner` error and is never re-permissioned. The first test
  must be **provably non-vacuous**: it must fail against the pre-fix
  behaviour.
- The scratch allocator's resolution — with no `XDG_CACHE_HOME` the scratch
  root lives under the home directory's cache, and with one it lives under
  that root.
- The forty-four tests that failed in the authoritative sweep — they must
  pass.
- The live Eggwork qualification's trust chain — a staged sandbox helper must
  satisfy the pinned node's trust check whatever the umask, a group-writable
  helper in a group-writable directory must not, and staging must not rewrite
  the build artifact's mode. The test must be **provably non-vacuous**, and the
  non-vacuity is proved in the qualification itself: with the build tree
  restored to the failing state the same binary goes from 8 passed / 5 failed
  to 14/14.

## 11. Required verification commands

```bash
cargo check --workspace --all-targets --locked
cargo fmt --all -- --check
cargo test -p egggit --locked --lib process::tests
cargo test -p codegg --locked --lib tool::bash
cargo test -p codegg --locked --test git_index_publication -- --test-threads=1
python3 scripts/check_adr_integrity.py
python3 scripts/check_adr_integrity.py --self-test
python3 scripts/check_project_catalog_invariants.py
scripts/verify.sh quick
XDG_CACHE_HOME="$(mktemp -d)" cargo nextest run --workspace --locked --profile ci
XDG_CACHE_HOME="$(mktemp -d)" cargo nextest run -p codegg --locked \
  --features server,plugins,lsp-test-support --profile ci
```

The sweep's private cache root is part of the command, not an environment the
runner happens to have: `scripts/verify.sh full` and `.github/workflows/ci.yml`
both supply it, and §7 WP-G says why. The second sweep command is the
feature-enabled tier of `verify.sh full`; it carries the live Eggwork
qualification, whose outcome must not depend on the operator's umask (§7
WP-K).

## 12. Documentation updates

- `architecture/overview.md` — the "Verified counts" table and its
  re-measurement note.
- `architecture/testing.md` — the CI/test determinism contract, if the
  live-server gating needs to be stated there; and the live Eggwork fixture's
  trust contract.
- `architecture/security.md` — the scratch allocator's cache-root contract.
- `plans/adrs/linux-command-adr-id-migration.md` — the identity history.
- The affected roadmaps and closure records — corrected identifiers, with the
  historical narrative left intact.

## 13. Acceptance criteria

- The branch contains `main`'s tip and no conflict markers in any commit.
- `scripts/verify.sh quick` passes.
- Both identifier guards pass, and the ADR guard's self-test passes.
- The default sweep contains no test that requires an installed language
  server.
- A transient object-store read failure is retried; a persistent one refuses.
- The scratch allocator's cache root does not depend on the operator's umask,
  an unsafe cache root is still refused, and the default sweep's result does
  not depend on the operator's home directory.
- Every published count is measured against its listed `Source` on this tree.

## 14. Stop conditions

- If a conflict cannot be resolved from the two sides' intent, report it
  rather than picking a side.
- If the migration renumbering would require altering an accepted decision's
  substance, report it.
- If a fix would weaken a fail-closed path, report it.
- If a sweep failure cannot be reproduced, record it as unreproduced with the
  evidence, and do not report it as fixed.

## 15. Closure evidence required

A closure record at `plans/closure/branch-and-ci-stabilization/001-status.md`
with the requirement-to-evidence matrix, the exact verification commands and
their results, an explicit statement of what was **not** verified (hosted CI on
the rebased head, native Windows/macOS), and the registry updates.

## 16. Handoff notes

The rebase rewrote the branch's history. Every commit subject is preserved, but
every SHA changed; the pre-rebase tip `d251b11c` remains reachable through the
`backup/pre-rebase-032b-c001` branch for provenance. The hosted CI runs recorded
in the C007 and C023 closures qualify the *code* those closures describe; they
were run against the pre-rebase source and are **not** re-interpreted as
qualifying the rebased head.
