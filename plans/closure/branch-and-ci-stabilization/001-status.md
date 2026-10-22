# Branch and CI Stabilization Milestone C001 — Closure Status

Status: closed — the branch is integrated with `main`, both identifier namespaces are unique, the content the branch had lost is restored, the tree compiles with every target, the canonical gate passes, and the default sweep's green result is a property of the product rather than of the host it ran on.

Source implementation plan:

- `plans/implementation/branch-and-ci-stabilization/001-branch-and-ci-stabilization.md`

Source subsystem roadmap:

- `plans/subsystems/branch-and-ci-stabilization-roadmap.md#7-milestones`

Repository baseline reviewed: `d251b11c` (branch tip before the first rebase), rebased onto `main` at `fe2c1306`, then — the **fresh rebase** — onto `main` at `cb0e79c0`, and then reconciled with `main` by two merges, at `2a0bae08f` (§3.16) and at `84aa47528` (§3.17)

## 1. Executive finding

The branch is integrated with `main`, and the integration did not cost either
side its content.

> **The fresh rebase (2026-10-21).** `main` advanced by **103 commits** while this
> branch was in flight — the TUI editor milestone series, the live-work
> visibility corrective, the Eggwork worker targeting and telemetry line, the
> harness lab, and the managed-scratch reconciliation. The branch was rebased
> again, onto `main` at `cb0e79c0`, replaying **304 commits**. That rebase
> exposed a **second** ADR collision, **three** cross-file pairings a rebase
> cannot see, and **one** inherited race in a test. All are closed, and all are
> recorded below (§3.12–§3.15). The first rebase's findings stand unchanged;
> this section records what the second one added.
>
> **The reconciliation with `main` at `2a0bae08f`, and the hosted evidence.**
> `main` advanced a further **9 commits** after the fresh rebase's base, and the
> pull request was reported `CONFLICTING`. The branch was reconciled with it by
> a **merge** — the pattern this repository's own history already uses for a
> branch carrying work `main` has not seen — and the tip-level conflict was
> **exactly one file**, resolved by the rule that a later qualification
> supersedes an earlier deferral (§3.16). The branch now carries `main`'s tip
> as a direct ancestor, and **no published count moved**: `main` added and
> deleted no file under any directory a count is measured from.
>
> The hosted evidence this milestone waited for **has landed**. The default
> sweep ran green on the head — **15733 tests run, 15733 passed, 8 skipped** —
> together with the five native workflows (§4.3). The rebased head is therefore
> no longer an expectation; §13 no longer carries that gap, and what remains
> there is narrower and stated plainly.
>
> **`main` moved twice more, and the branch moved with it.** After the
> reconciliation at `2a0bae08f`, `main` advanced again — first by **9 commits**
> to `84aa47528`, the M006 scratch usage census — and the branch was
> reconciled with it by a second merge. That advance is **not** count-neutral:
> it added a `CoreRequest` variant, a `CoreResponse` variant, an authorized
> operation, and an integration test file, so **four** rows of the published
> counts moved and were re-derived (§3.17). The branch now carries `main`'s tip
> as a direct ancestor, and the hosted evidence has been re-obtained on the
> head after the first reconciliation (§4.3).

The rebase replayed **295 commits** onto `main` at `fe2c1306` and stopped at
**21 conflict steps across 20 distinct files**. Every conflict was resolved by
an explicit rule rather than by a bulk strategy: take `main`'s side where
`main` had already landed the same refactor (the workspace-binding fixture
refactors in seven test files), keep both sides where each added a field the
other needed (`scratch_scope` from `main`, the M005 network-intent fields from
this branch), and compose by hand where a mechanical merge produced a call
that satisfied neither side (`dispatch_to_shell`, the durable-route provenance,
and the two `JobPayload` construction sites).

Two conflicts were resolved wrongly during the rebase — a scripted resolution
pattern produced a file with conflict markers in it, and the markers were
committed with the resolution. Those two commits were repaired by a targeted
history rewrite, and the repair was proved by an **empty tree diff** between
the pre-repair and post-repair tips. The final tree contains **zero** conflict
markers of any form.

The integration exposed two identity collisions that had been latent on the
branch for weeks, and one content loss the branch had already inflicted on
itself. All three are closed:

- The branch and `main` had both claimed storage-layout version **83**. `main`'s
  83 (the managed-agent-scratch migration) is already reachable by an existing
  database, so `main`'s number stands and the branch's migrations move: Linux
  M005's network-intent preference 83 → **84**, C007's private-index
  publication journal 84 → **85**, `STORAGE_LAYOUT_VERSION` → **85**.
- The branch and `main` had both claimed ADR **0042**. `main`'s 0042 is the
  accepted composer normal-mode key ownership decision, so the branch's
  accepted command-authority decision moves to **ADR-0045** with its accepted
  status, accepted date, decision text, authority, non-goals, and security
  limits unchanged.
- The branch's own commit `c446af35` ("Close Linux execution isolation M004")
  had deleted **eleven registry rows and their prose** for three unrelated
  workstreams. Those rows are restored.

The branch's test signal had two independent defects, and both are closed:

- A **transient** Git object-store read failure — the ordinary consequence of
  Git's own background maintenance rewriting the pack directory while a read is
  walking it — was reported as a typed "Git backend unavailable". Fourteen
  tests failed that way in one recorded sweep and passed in the next. The
  failure is now instability, absorbed by the bounded attempt count the code
  already had.
- Three **LSP tests that query a live language server** were in the default
  sweep. Their runtime is a workspace index, not a unit of work: one took 90 s
  on this workstation against a nextest kill threshold of 120 s, and all three
  were killed in both recorded sweeps. They now sit behind the repository's
  existing `lsp-real-server-tests` feature, which is what that feature is for.

The authoritative sweep then exposed a **third** defect, with a different
character: 44 tests across 15 binaries failed with one typed refusal,
`WrongOwner`. The scratch allocator refuses a per-user cache root that another
local user could write to — a deliberate hardening that closes the "another
user swaps the scratch root out from under the product" hole — and it created
that root with `create_dir_all`, whose mode follows the process umask. On a
host whose umask grants group-write (this workstation's is `0002`), the
allocator created a `0775` root and then refused it; on a host whose cache
root is `0777`, it refused the machine it was running on. The allocator also
depended on a *fixture* that documented itself as "a private, canonical, 0700
directory" while `tempfile` had in fact created it with the default
permissions. Both are closed: the product creates the root it depends on
privately, the fixture sets the mode it documents, and the sweep gets a cache
root of its own so its result is a property of the product rather than of the
machine it ran on. The refusal itself is unchanged, and is now a **tested
contract** rather than an accident of the host.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| The branch is rebased onto `main` and carries `main`'s tip | **the second reconciliation with `main` at `84aa47528`**: `git merge-base HEAD origin/main` == `84aa47528` and `git merge-base --is-ancestor origin/main HEAD` succeeds, so `main`'s tip is a direct ancestor of the branch (§3.17). The first reconciliation's facts stand: `main` at `2a0bae08f`, one conflicted file (§3.16). The fresh rebase's own facts stand: `cb0e79c0` was the base it replayed **304** commits onto, every subject preserved (the first rebase's 295 onto `fe2c1306` is superseded) | pass |
| No conflict survives in the final tree | `git grep -lE "^(<<<<<<< \|>>>>>>> )" HEAD --` outside `plans/` → **0** | pass |
| A conflict is never committed with markers | two such commits were found during the rebase and repaired by a targeted rewrite; the repair is proved by an **empty** `git diff --stat backup/post-rebase-pre-fix HEAD` | pass |
| One storage-layout number per migration | `crates/codegg-core/src/session/schema.rs` has one `migrate_v83` (main's managed scratch), `migrate_v84` (M005 network intent), `migrate_v85` (C007 journal); `python3 scripts/check_project_catalog_invariants.py` → **7/7** | pass |
| The layout version tracks the highest wired migration | `STORAGE_LAYOUT_VERSION = 85`; `scripts/check_project_catalog_invariants.py` → "storage layout marker tracks highest wired schema migration" PASS | pass |
| One ADR identifier per decision | `python3 scripts/check_adr_integrity.py` → pass; `main` passes the same guard, so the guard is not merely permissive. The fresh rebase's second collision (ADR-0043) is closed by moving this branch's decision to **ADR-0046** (§3.13) | pass |
| The ADR guard is itself trustworthy | `python3 scripts/check_adr_integrity.py --self-test` → "ADR integrity fixtures passed"; the self-test now carries the real merged ADR set and a **non-vacuous** case for the ADR-0043 collision class (§3.13) | pass |
| Every live local ADR path resolves | the guard's own check; two dangling references were found and corrected with a provenance note — the `ADR-0028-command-syntax-and-execution-authority.md` reference in the agent-tool-use addendum, and this record's own reference to the command ADR's **former** filename (the guard treats any `plans/adrs/ADR-….md` path form as a live link, so a historical name is cited by number or filename only) | pass |
| Content the branch had lost is restored | the eleven registry rows and prose for three unrelated workstreams are back in `plans/registry.md` | pass |
| A stale ADR identifier no longer names a different decision | `plans/subsystems/tui-editor-helix-parity-roadmap.md` is now **byte-identical to `main`**; four editor closure records likewise | pass |
| Every target compiles | `cargo check --workspace --all-targets --locked` → clean | pass |
| The canonical gate passes | `scripts/verify.sh quick` → **"Quick verification passed."** | pass |
| A transient object-store read failure is retried, not refused | `crates/egggit` regression test `a_transient_object_store_read_failure_is_retried_and_not_refused`; **non-vacuity proved** — against the pre-fix behaviour the same test fails with `Err(BackendUnavailable { capabilities: [RepositoryAlias] })`, the exact error the recorded sweep reported | pass |
| A persistent object-store read failure still fails closed | `crates/egggit` regression test `a_persistent_object_store_read_failure_still_fails_closed`; also non-vacuous — against the pre-fix behaviour the refusal came after 1 attempt instead of the bounded 3 | pass |
| The default sweep needs no installed toolchain | the three live-language-server tests are **absent** from `cargo test -p codegg --lib -- --list` without the feature; the four rejection tests that use the same tool remain | pass |
| The fourteen tests that failed in the recorded sweep pass | all 18 matching tests (14 failures plus 4 same-named in another binary) → **18 passed** | pass |
| A cache root the product creates is private and umask-independent | `src/scratch.rs` regression test `a_cache_root_the_product_creates_is_private`; **non-vacuity proved** — against the pre-fix behaviour the same test fails with `WrongOwner` on a root the allocator had just created | pass |
| An unsafe cache root is refused, not used and not tightened | `src/scratch.rs` tests `a_group_writable_cache_root_is_refused` (0775) and `a_world_writable_cache_root_is_refused` (0777) — both assert the typed `WrongOwner` refusal and the path it names | pass |
| The scratch root follows the resolved cache root | `tests/scratch_cache_root_contract.rs`: `the_default_cache_root_is_the_home_directory_cache` and `an_explicit_cache_home_is_the_root_the_allocator_uses` | pass |
| The forty-four tests that failed in the authoritative sweep pass | the sweep's result is recorded in §4.1; the previously-failing families (`scratch`, `core::daemon`, `core::daemon_sessions`, `tool_program_m015/016/017`, `session_family control_m004`, `async_background_c006`, `agent_loop_harness`, `identity_live_execution_audit`, `identity_m002_live_audit_hooks`, `workspace_root_execution_authority`, `linux_native_scratch_containment`, `python_script::executor`, `managed_process`, `tool::bash`) pass | pass |
| The sweep's result does not depend on the operator's home directory | `scripts/verify.sh full` and `.github/workflows/ci.yml` both give the sweep a private cache root; the authoritative sweep in §4.1 was run with the host's own `~/.cache` still world-writable (0777) and still passed | pass |
| The branch passes the CI clippy step | `cargo clippy --workspace --all-targets --locked -- -D warnings` → **clean**; four lints in branch-owned code would have failed it (§3.8) | pass |
| The feature-enabled tier passes | `cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci` → **12496 run, 12496 passed, 0 failed** on the merged tree (§4.2); 11804 run, 11804 passed, 0 failed on the pre-rebase tree | pass |
| The live Eggwork qualification's trust chain does not follow the operator's umask | `tests/eggwork_remote_execution_live.rs` regression test `a_staged_sandbox_helper_is_trusted_whatever_the_umask`; **non-vacuity proved** — with the helper group-writable the *same* five qualifications fail (8 passed / 5 failed) and the staged pair passes (14/14) (§3.11) | pass |
| No work is lost by the force push | the remote branch's two commits past the rebase base (`32611c8d`, `5019fb26`) are replayed onto the rebased branch; their files are **byte-identical** to the remote's originals (§3.10) | pass |
| Every published count is measured against its listed `Source` | `architecture/overview.md`'s "Verified counts" table re-measured on 2026-10-20 (**eight** rows corrected) and again on 2026-10-21 against the rebased tree (**nine** more rows moved, two of them because the *measure* was wrong — the tool-registration count and the authorized-operations count); no row is carried forward (§3.6) | pass |
| The canonical gate after the sweep | `scripts/verify.sh quick` → "Quick verification passed." (§4) | pass |
| No branch content is lost by the fresh rebase | for every file the branch changed relative to the merge base, the canonical per-file three-way merge of (branch, merge base, `main`) was compared with the rebased tree: **93 files byte-identical, 0 deviations**; **zero** conflict markers in the tree (§3.12.4) | pass |
| The merged tree compiles with every target | `cargo check --workspace --all-targets --locked` → **exit 0**. The first attempt did not compile: the rebase cannot see a pairing between a file `main` added and a type this branch extended. Three such pairings were found and closed (§3.14) | pass |
| The feature-enabled tier is deterministic | `real_tui_rollback_invariants_on_writer_closed` failed **7 of 12** isolated runs; its wait condition omitted the counter its own assertion checks. Fixed by waiting for that counter; **0 failures in 15 runs** after, and the tier is green. No assertion weakened, no timeout widened, no product code touched (§3.15) | pass |
| The branch is reconciled with `main`'s current tip | the tip-level conflict was **exactly one file**, `.opencode/skills/tool-execution/SKILL.md`; it is resolved by the rule that a later qualification supersedes an earlier deferral, and the branch's M005/M004 text — which `main` does not carry at all — is kept in full (§3.16). The merged tree holds **zero** conflict markers, `cargo check --workspace --all-targets --locked` → **exit 0**, and `scripts/verify.sh quick` → **"Quick verification passed."** | pass |
| The reconciliation does not move a published count | **the first reconciliation** (`main` at `2a0bae08f`) moved none: its nine commits added and deleted no file under any directory a count is measured from (§3.16). **The second reconciliation** (`main` at `84aa47528`) moved **four** rows, and each was re-measured against its own listed `Source` on the merged tree: `CoreRequest` variants 260 → **261** (237 struct + 24 unit), `CoreResponse` variants 169 → **170**, authorized operations 244 → **245**, integration test files 284 → **285**. The enum-variant measure was **calibrated, not assumed** — it reproduces the published values exactly on the pre-merge tree (`CoreRequest` 260 = 236 struct + 24 unit, `CoreResponse` 169, `CoreEvent` 92, `ProjectionEvent` 47, `AppEvent` 60) before it is applied to the merged tree. Every other row was re-measured and did not move (§3.17) | pass |
| The rebased head is observed in a hosted CI run | run **39551117669** on head `352ed719c`: **conclusion success**, all 44 steps green, `Workspace tests` reporting **15733 tests run: 15733 passed, 8 skipped** (`Summary [ 523.092s]`) — the same count the local sweep reports (§4.3) | pass |
| The M004/M005 native tier is qualified on the head | run **39550735458** on head `352ed719c`: **success**, with jobs `restricted-userns-fails-closed`, `process-ipc` and `git-cost-pilot` (skipped) — the ten qualification steps green on a host with Landlock ABI ≥ 6, which this workstation does not have (§4.3). Re-qualified on the two later heads as well: `39567537372` on `979ba14c6` and `39580103924` / `39580168070` / `39580097658` on `2ab5a3335`, all **success** | pass |
| A defect the hosted evidence finds is fixed, not waived | the hosted Windows launcher found a **branch-owned** failure — `scratch::tests::the_session_root_lives_under_the_resolved_cache_root`, a test `main` does not have — and it is fixed by canonicalizing both sides of the comparison, with the assertion's claim unchanged and its discriminating power proved and kept in-tree (§3.18) | pass |
| A hosted failure that is not the branch's is recorded, not fixed | the Desktop E2E failure reproduces on `main` itself, at `84aa47528` and `48a5ca7d2`, and on the M006 and M007 branches; it is recorded as a pre-existing `main` failure rather than fixed or waived (§3.18) | pass |

## 3. Production implementation evidence

### 3.1 Storage-layout identity

`crates/codegg-core/src/session/schema.rs` now wires 83 → main's managed
scratch migration, 84 → the branch's M005 network-intent preference, 85 →
C007's publication journal. `crates/codegg-core/src/storage/mod.rs` declares
`STORAGE_LAYOUT_VERSION = 85`.

The branch's pre-rebase `migrate_v83` (M005) was renamed to `migrate_v84` and
its wiring, dispatch arm, and the three affected test assertions were updated
in the same change; the C007 journal was renamed to `migrate_v85` when its own
commit replayed. The renumbering is recorded in the C007 closure's own
migration evidence and in `plans/closure/linux-execution-isolation/012-status.md`.

### 3.2 ADR identity

The command-authority ADR's file was renamed: the decision's identity moved
from `ADR-0042-command-syntax-and-execution-authority.md` to
`plans/adrs/ADR-0045-command-syntax-and-execution-authority.md`, with the
accepted decision's text, status, date, owners, authority, non-goals, and
security limits unchanged. Fourteen files referenced the old path and thirteen
more carried a bare `ADR-0042` whose referent was the command decision; all
were updated by referent, not by pattern, so the **composer** ADR-0042's
references — in the composer roadmap, the composer implementation plans, the
composer closure records, `architecture/tui.md`, and the composer key-authority
guard — were left untouched.

The identity history is now
`ADR-0028 → ADR-0032 → ADR-0034 → ADR-0042 → ADR-0045`, recorded in
`plans/adrs/linux-command-adr-id-migration.md` and in a dated reconciliation
section of `plans/implementation/linux-execution-isolation/012-adr-identity-collision-and-admission-gate-corrective.md`.

The guard's own self-test fixture was updated to match: the expected active-ID
set now includes 0045, and the duplicate-ID fixture is `ADR-0045-second.md`.

### 3.3 Restored content

`plans/registry.md` carries back the rows for the TUI subagent and job
visibility convergence corrective (C001–C003), the user prompt queue and turn
contention workstream (M001–M007, C001, C002), and the agent loop operational
reliability workstream (M001–M007), together with their prose.

`plans/subsystems/tui-editor-helix-parity-roadmap.md` named the editor
syntax-tree decision "ADR-0037" while pointing at
`plans/adrs/ADR-0034-editor-syntax-tree-dependency.md` — ADR-0037 is a
*different* accepted decision (idle-session queue mutation authority). The
roadmap and four related closure records now name ADR-0034 and are
byte-identical to `main`.

### 3.4 Git object-store instability

`crates/egggit/src/process.rs`'s private-object-view path previously mapped an
object-store read failure to `BackendUnavailable([RepositoryAlias])`. The
object store is read by walking `objects/pack` and the loose-object fanout
directories, stat-ing each entry; a pack file removed between the directory
read and the stat — which is exactly what a concurrent `git gc` does — made the
walk fail, and the failure was reported as an unavailable backend.

The change is threefold:

- the snapshot loop's source fingerprint, snapshot directory, snapshot copy,
  watcher check, and post-copy fingerprint now treat a **failed read** as
  instability and continue to the next attempt inside the existing bound
  (`OBJECT_VIEW_SNAPSHOT_ATTEMPTS`, three) instead of refusing;
- the cache-hit path treats a failed fingerprint or an unanswerable watcher as
  "this generation is invalid" and rebuilds, instead of refusing;
- a **persistent** failure still refuses, with the same typed outcome, after
  the bounded attempts — the fix does not turn an unreadable store into a
  fabricated success or a fabricated path.

The fingerprint is now injected through a parameter so the transient failure is
reproducible in a test without racing a real repository. The two regression
tests are in `crates/egggit/src/process.rs`.

### 3.5 Host-independent default sweep

`src/tool/lsp.rs`: `security_context_preset_scenarios_share_one_service`,
`security_context_call_expansion_scenarios_share_one_service`, and
`security_context_call_expansion_truncated_limit_field_present` now carry
`#[cfg(feature = "lsp-real-server-tests")]`. The feature already existed and
already carries the repository's stated rule that a test needing an installed
server is opt-in and not part of routine PR validation; these three had
escaped it. The four sibling tests that use the same tool but reject their
input before contacting a server stay in the default sweep.

### 3.6 Re-measured counts

`architecture/overview.md`'s "Verified counts" table was re-measured against
this tree on 2026-10-20. Six rows moved and were corrected: `CoreRequest`
variants 243 → 259, `CoreResponse` 158 → 168, `CoreEvent` 88 → 92, identity
newtypes 18 → 19, database tables 91 → 93, and the storage-layout version
84 → 85. The counting method was calibrated against `main`'s own tree first:
the method reproduces `main`'s published values for every row where `main`'s
table is current (`AppEvent` 60, `ProjectionEvent` 47, `codegg-core` modules
47, integration test files 268, guard scripts 58), and the enum-variant counts
were confirmed by a second, independent signal.

The three rows that this milestone originally **carried forward** —
`Authorized operations`, `Tool registrations`, and `Distinct production tool
names` — have since been **re-derived** from sources a reader can re-run, so
no row of the table is now carried forward:

- `Authorized operations` is **243**, the length of `representative_requests()`
  in `crates/codegg-core/src/authorization/policy.rs` — the list the executable
  `operation_capability_matrix()` is derived from, so the count and the matrix
  cannot drift apart. The previously published source, the
  `operation_descriptor` function, yields none of the candidate counts (58
  distinct operation names, 253 call sites, 259 variant references), which is
  why the row could not be reproduced before.
- `Tool registrations` keeps **63**, and its source is widened from
  `src/tool/mod.rs` (26 call sites) to `with_options` call sites across `src/`
  (26 + 37).
- `Distinct production tool names` is **60**: the distinct literal `name()`
  strings of the production `impl Tool for …` in `src/tool/` and `src/context/`,
  after the six test-only mocks are excluded (three in `program_manifest.rs`,
  one in `disabled.rs`, and the two whose `name()` is not a literal, in
  `catalog.rs` and `tool_search.rs`).

Two of those values were **stale in `main` as well** — `main`'s tree measures
243 and 60 by the same methods — so the branch inherits `main`'s drift rather
than adding any of its own; the correction is recorded as pre-existing drift,
consistent with the other rows corrected on the 2026-10-19 pass. Eight rows
moved in total.

### 3.7 The scratch allocator's per-user cache root

The authoritative sweep failed **44 tests across 15 binaries**, and every one of
them carried the same typed refusal. The two shapes of the error named the two
directories the allocator had resolved:

```
WrongOwner("/home/sugarwookie/.cache")   ← the operator's real cache root
WrongOwner("/tmp/.tmpFwxAu8/home/.cache") ← a test's isolated home
WrongOwner("/tmp/.tmprQVE9k")            ← a test's isolated cache root
```

`ensure_private_parent` refuses a cache root that is group- or world-writable,
or that is not owned by the user. That check is a deliberate hardening
(`c05ace49`, "validate per-user cache parent"): it closes the hole where
another local user renames `~/.cache/codegg` out from under the product and
replaces it with their own directory. The check was **not** weakened.

The defect was on the other side of it. The allocator resolved the root and
then created it:

```rust
let cache_root = dirs::cache_dir().ok_or(ScratchError::CacheUnavailable)?;
std::fs::create_dir_all(&cache_root)?;   // mode follows the process umask
ensure_private_parent(&cache_root, expected_uid)?;  // refuses a writable root
```

`create_dir_all` creates with mode `0777 & ~umask`. This workstation's umask
is `0002`, so the allocator created a `0775` root and then refused it. The
same is true of every fixture that builds a cache root: `tempfile` creates its
directories with the *default* permissions too, so a fixture that documented
itself as "a private, canonical, 0700 directory" actually got `0775`. And the
operator's real `~/.cache` was `0777` — genuinely unsafe, and correctly
refused.

Three changes close it, and none of them weakens the refusal:

- **The product** (`src/scratch.rs`). `ensure_private_cache_root` creates the
  root with mode `0700` when the product creates it, so the umask can no
  longer produce a root the allocator then refuses. A root that **already
  exists** is only validated — the product never re-permissions a directory it
  does not own, so an unsafe root is still refused rather than silently
  tightened.
- **The fixture** (`tests/linux_native_scratch_containment.rs`). The scratch
  home sets the mode it documents.
- **The runners.** `scripts/verify.sh full` creates a private cache root with
  `mktemp -d` and exports it for the sweep; the CI job sets one under the
  runner's temporary directory. The root stays outside the workspace, because
  several qualifications grant the repository itself as their workspace root.

A workspace-wide Cargo `[env]` override was considered and **rejected**: it
would also have redirected the product's cache root for a local `cargo run`
(the LSP server download cache lives under the same root), and it would have
placed the scratch root inside the repository. The decision is recorded in the
roadmap's §10.

The refusal of an unsafe cache root is now a tested contract rather than an
accident of the host: `a_group_writable_cache_root_is_refused` (0775) and
`a_world_writable_cache_root_is_refused` (0777) assert the typed error and the
path it names.

### 3.8 The CI clippy step

`ci.yml` runs `cargo clippy --workspace --all-targets --locked -- -D warnings`.
This branch had never been observed in a hosted CI run — `ci.yml` runs on pull
requests and on pushes to `main`, and no pull request had been opened for this
branch — so nothing had ever exercised that step against the branch's own
code. Running it locally exposed **four lints in branch-owned code**:

| Site | Lint | Fix |
|---|---|---|
| `src/core/daemon_bootstrap.rs` — C007's startup reconciler | `clippy::question_mark`: a `let...else` in a function returning `Option` | `let pool = self.pool.as_ref()?;` |
| `src/git_publication/publisher.rs` | `clippy::useless_conversion` and then `clippy::redundant_closure` | `map_err(PublishFailure::Fatal)?` |
| `src/git_publication/staging.rs` | `clippy::double_must_use`: the `#[must_use]` the async-trait expansion injects | `#[allow(clippy::double_must_use)]` at the macro site — the same convention five other traits in this tree already use |
| `src/scheduler/executors.rs` | `clippy::unnecessary_to_owned` and then `clippy::needless_borrow` | pass `args` directly |

All four are mechanical and behaviour-preserving: the same values reach the
same calls. The two `let...else` patterns on `main` are **not** affected —
they sit in functions returning `()`, where the `?` operator does not apply —
so the branch does not diverge from `main` for a lint `main` does not trip.

### 3.9 The stray `plans/.mutate-probe`

The branch carried a tracked file `plans/.mutate-probe`: **295 lines of the
single character `x`** (590 bytes). The *Linux Sandbox Admission — Post-Review
Corrective Addendum* packet flags it and asks for its disposition — "remove an
extraneous probe **only with provenance/test confirmation**, never by
guessing". The confirmation is here:

- **Provenance.** It was added by `27a0207e` ("docs(plans): record the
  authoritative sweep, and the 14 tests that do not reproduce", 2026-10-20),
  a commit whose subject and body describe recording a sweep result and
  nothing else. It was not present on `main`.
- **Content.** Its only unique line is `x`. It carries no information, and it
  is not a copy of any tracked file — the three tracked files that also have
  295 lines are a subagent-name list and two implementation plans, none of
  which it resembles.
- **References.** Nothing in the repository reads it: no test, no script, no
  guard, no plan, no ADR. The only two mentions are the planning packet's own
  two requests for its disposition.

It is removed. Nothing that depended on it was lost.

### 3.10 The work the remote branch carried past the rebase base

The remote branch was **two commits ahead** of the rebase base `d251b11c`:
`32611c8d` ("register Linux sandbox admission follow-up correctives") and
`5019fb26` ("correct section references in Linux handoff packet"). A force
push without them would have destroyed them.

They add a parallel planning packet: the *Linux Sandbox Admission — Post-Review
Corrective Addendum*
(`plans/subsystems/linux-sandbox-admission-follow-up-corrective-addendum.md`)
with its own C001/C002/C003 implementation plans under
`plans/implementation/linux-sandbox-admission-follow-up-corrective/`, plus
registry rows and two parent-roadmap links.

Both commits are replayed onto the rebased branch and their files are
**byte-identical** to the remote's originals. The only conflict was a registry
insertion, resolved by keeping every row from both sides — the one row both
sides touched (`Linux trusted Git cost corrective`) is kept in its rebased
form, with the rebase note, and the incoming side's unmodified copy was
dropped as the duplicate it was.

**This milestone and that packet overlap, and the overlap is recorded rather
than resolved.** Both address the same objective — make the branch
integrate with `main` and make the default sweep's result trustworthy — and
both call their first package "C001", under different roadmaps. Two specific
points where this milestone's implementation differs from that packet's
guidance, both left for the owner to decide:

- The packet asks that the three load-sensitive LSP tests be fixed with
  per-test fixtures or a bounded scheduling fix, and marked *conditional*
  rather than excluded. This milestone instead moved them behind the
  repository's existing `lsp-real-server-tests` feature, which is the policy
  `AGENTS.md` and `architecture/testing.md` already state for a test that
  needs an installed server. The tests are not deleted and not silently
  skipped: they remain runnable through that feature, and this record states
  exactly what ran and what did not.
- The packet's C001 expects its closure at
  `plans/closure/linux-sandbox-admission-follow-up-corrective/001-status.md`.
  This milestone's closure is at
  `plans/closure/branch-and-ci-stabilization/001-status.md`. The evidence in
  this record — the green sweep, the non-vacuity proofs, the clippy findings
  — is the evidence that packet's C001 asks for; merging the two records, or
  folding this one into that path, is an editorial decision this milestone
  does not make on the owner's behalf.



### 3.11 The live Eggwork fixture's trust chain

The five live-Eggwork failures of §4.2 were **not** a product defect. The
pinned Eggwork node (`e6a5d82`) resolves its sandbox helper as a sibling of
its own executable and **refuses a helper it cannot trust**: the helper must
be a regular file, owned by root or the invoking user, executable, and
neither it nor any directory above it may be group- or world-writable (a
root-owned sticky directory is exempt). An untrusted helper makes the node
stop advertising `isolation.landlock.workspace-rw.v1`, and the executor then
refuses every run that requires isolation —
`src/scheduler/eggwork.rs:1512`.

Two facts about how the artifacts were produced made that refusal depend on
the host rather than on the product:

- **Cargo installs build artifacts with the process umask and does not narrow
  them.** On this host (`umask 0002`) the helper was installed group-writable
  (`0775`), and the target directory above it was group-writable too.
- **`tempfile` creates its directories with the process umask as well** — it
  does not narrow them to owner-only. A "private" temporary directory was
  `0775` here, which the ancestor check refuses just as it refuses the
  helper.

So whether the qualification passed depended on the operator's umask. The
evidence for that, in the order it was obtained:

- An **A/B** on the same tree and the same host: the default-feature build of
  the same binary passed **13/13**, while the feature-enabled build failed
  five. The build was not the cause; the artifact state was.
- A **direct reproduction**: with the helper's mode changed to `0775` and the
  target directory left group-writable, the same binary produced **8 passed /
  5 failed** — the *same five* tests, with the *same* refusal. With the
  helper at `0755` it produced 13/13.
- The **fix** (commit `81697139`): the qualification now **stages** the node
  and the helper in a private directory before starting the node. The staging
  directory is set to `0700`, the helper is placed there as an independent
  **copy** with mode `0755` (a copy, not a link, so the guarantee is the
  staging's and not a side effect on the build tree), and the node is started
  from that directory. The node's own TLS directory is narrowed to `0700` as
  well, since the client key lives in it.
- **Non-vacuity, in the qualification itself**: with the build tree restored
  to the failing state — helper `0775`, target directory `0775` — the
  qualification is **14/14 passed**, and the build tree is unchanged
  afterwards.
- **A pinned regression test**, `a_staged_sandbox_helper_is_trusted_whatever_the_umask`,
  carries a mirror of the node's trust check: it asserts that a
  group-writable helper in a group-writable directory is **not** trusted (so
  the mirror discriminates and a staging that did nothing could not pass) and
  that the staged pair **is** trusted, that the staged helper is not
  group- or world-writable and stays executable, that the staging directory is
  not group- or world-writable, and that staging does not rewrite the build
  artifact's mode.

The upstream refusal is **not** weakened: the node still refuses an untrusted
helper, and the product's own executor still refuses a node that cannot
isolate the workspace. What changed is that the fixture now hands the node an
artifact it can trust on any host, instead of one whose trust the operator's
umask decided.

### 3.12 The fresh rebase onto `main` at `cb0e79c0`

`main` advanced by **103 commits** while this branch was in flight. The branch
was rebased again, replaying **304 commits** onto `main` at `cb0e79c0`. The
merge base moved from `fe2c1306` to `cb0e79c0`; 327 files changed on `main`
and 295 on the branch, with **55 files touched by both**.

Every conflict was resolved by an explicit rule, and the conflicts fell into
four classes:

| Conflict | Rule applied | Outcome |
|---|---|---|
| `plans/registry.md` (repeatedly) | `main`'s side during the replay, then the branch's own rows re-applied afterwards by a row-level three-way merge against the merge base (§3.12.1) | 64 of `main`'s rows gained, 9 stale inherited rows superseded, 2 duplicate placements de-duplicated |
| `plans/subsystems/tui-editor-helix-parity-roadmap.md` | `main`'s side | the branch's only delta there was an ADR label `main` had already corrected (§3.12.2) |
| `architecture/overview.md`, `architecture/command.md` | `main`'s side during the replay, then rewritten wholesale by the re-derivation (§3.6) | nine count rows moved, two of them by a corrected measure |
| `src/tui/command.rs`, `tests/eggwork_remote_execution_live.rs` | composed by hand | both sides' content kept (§3.12.3) |

#### 3.12.1 The registry reconciliation

`plans/registry.md` is a hand-maintained table whose sections mix row schemas,
so a conflict region does not determine what a row means. The rebase therefore
resolved each registry conflict to `main`'s side and re-applied the branch's own
rows afterwards, using the merge base as the third side of a **row-level**
three-way merge. The result, compared against the pre-rebase branch tree:

- **64 rows gained** — every one of them a row `main` added.
- **9 rows superseded** — stale inherited rows for workstreams `main` has since
  advanced (the managed-scratch, TUI composer and TUI editor roadmap rows, the
  composer M003 and editor M006–M008 rows `main` implemented, the agent-recovery
  row `main` renamed, and the editor M009 row `main` closed and re-homed as
  M014).
- **2 placements de-duplicated** — the branch listed "Linux execution isolation
  M003" and "M005" in *both* the "Blocked work" table and the
  "Dependency-ready implementation plans" table; the merged tree keeps them
  once, in the latter, carrying the branch's own text.

No branch row was lost. The branch's eleven Linux-execution-isolation rows,
its seven Linux-trusted-Git rows, and its four branch-and-CI rows are all
present in the merged registry with their original content.

#### 3.12.2 The editor-Helix roadmap

The branch's entire delta to `plans/subsystems/tui-editor-helix-parity-roadmap.md`
was an ADR label: it had written `ADR-0037` where the decision is `ADR-0034`,
and later corrected it. `main` carries `ADR-0034` in **29** places and
`ADR-0037` in **none**, and has advanced the roadmap itself (M001–M013 and
M015–M020 closed). `main`'s side was taken; the branch's later correction
commit became a no-op. Nothing was lost — the four unique lines the branch
held were older wordings of lines `main` has since reworded.

#### 3.12.3 The two hand-composed conflicts

`src/tui/command.rs`: `main` registered `/nodes` (Eggwork M002) and this
branch registered `/network` (M005), each moving the same registry-length pin
by one. The merged tree holds **168** built-in commands and **160**
`BuiltinSlashAction` variants — one more than either side alone — and the
conflict was resolved so both commands and both enum variants survive.

`tests/eggwork_remote_execution_live.rs`: `main` added two telemetry
qualifications and this branch added the fixture-trust contract test (§3.11).
The resolution keeps all three, and repairs one structural defect the merge
introduced: the composed file was missing a closing brace, which the compiler
caught (§3.14).

#### 3.12.4 Proof that no content was lost

The file-level three-way merge was computed for **every** file the branch
changed relative to the merge base, and compared with the rebased tree:

    expected = git merge-file -p  (branch, merge-base, main)
    actual   = the rebased tree

**93 files are byte-identical to that merge, and 0 files deviate.** The
rebased tree contains **zero** conflict markers. The three paths the check
reports as moved are the two ADR files this branch renamed (archived as
aliases, §3.13) and one implementation plan this branch moved into
`plans/archive/`.

This proof is about *content*, not about *compilation*: a cross-file pairing is
invisible to it, which is why the compiler is the other half of the evidence
(§3.14).

### 3.13 The second ADR collision, and ADR-0046

`main` had, since the first reconciliation, allocated **ADR-0043** to the
accepted session-owned subagent lifecycle projection decision (Option A
accepted 2026-10-19). This branch had published ADR-0043 for the accepted
composable Linux execution isolation decision. The rebased tree carried two
live records claiming ADR-0043, and `scripts/check_adr_integrity.py` failed
with `duplicate active ADR-0043` plus four dangling path references.

The rule the first reconciliation applied is applied again — **main wins a
contested number; this branch's decision moves**:

| Decision | Before | After |
|---|---|---|
| Composable Linux execution isolation | ADR-0043 | **ADR-0046** |
| Command syntax and execution authority | ADR-0045 | ADR-0045 (unchanged) |
| Session-owned subagent lifecycle projection | ADR-0043 (main) | ADR-0043 (main, unchanged) |
| Contained Git live objects and metadata | ADR-0044 | ADR-0044 (unchanged) |

The decision's substance, its accepted status, and its 2026-10-14 acceptance
date are **unchanged**; only the canonical identifier moved. The published
ADR-0043 file is archived beside the earlier ADR-0027 and ADR-0028 aliases
under `plans/archive/linux-command-adr-id-aliases/`, and `migration.json`
records the mapping with recomputed digests, the full identity history
(ADR-0027 → ADR-0031 → ADR-0033 → ADR-0035 → ADR-0043 → ADR-0046), and the
current `main` allocation set.

The manifest also carried a **stale** claim from the first reconciliation: it
still named ADR-0042 as the command authority's canonical id, which `main` owns
(composer normal-mode key ownership). It is corrected to ADR-0045.

The guard's self-test now carries the real merged ADR set and adds a
**non-vacuity** case for exactly this collision class: two decisions claiming
ADR-0043 must be rejected, so the renumbering is enforced rather than merely
asserted. The storage-layout namespace is unaffected — `main`'s ceiling is 83,
this branch's migrations are 84 and 85, `STORAGE_LAYOUT_VERSION` = 85.

### 3.14 The cross-file pairings a rebase cannot see

A rebase resolves *file-level* conflicts. It cannot see a pairing between a
file one side added and a type the other side extended, because no two-way
merge ever compares them. `main` added 26 new `.rs` files; this branch had
extended three types they use. Three such pairings existed, and the tree did
not compile until each was closed.

- **`JobSubmissionError::LabelsTooLarge` (production).** `main` added
  `src/scheduler/schedule_materializer.rs`, whose `submission_error_code`
  matches exhaustively over `JobSubmissionError`; this branch had added the
  `LabelsTooLarge` variant to that enum. The match had no arm for it. The arm
  maps it to `labels_too_large`, following the neighbouring `payload_too_large`.
  That function had **no test at all**; the compiler enforces the *coverage*,
  while the new `submission_error_codes_are_stable_slugs` test pins the *slug*,
  which a wrong value would have changed silently.
- **`JobPayload::ManagedArgv` and `JobPayload::Test` (tests).** This branch's
  M005 work added `sandbox_profile` and `network_intent` to the command-bearing
  job payloads. Six construction sites in files `main` added after the previous
  rebase base did not know about them:
  `tests/eggwork_target_boundary.rs`, `tests/schedule_materialization.rs`,
  `tests/eggwork_remote_limits.rs`, and four sites in
  `tests/eggwork_remote_execution.rs`. Each is filled with the branch's
  established values — `sandbox_profile: None` and
  `network_intent: NetworkIntent::LegacyUnrestricted`, the enum's `#[default]`
  and the intent a payload had before either field existed. No test's behaviour
  changes; the fields are the ones the type requires.

The structural defect in `tests/eggwork_remote_execution_live.rs` (§3.12.3) is
of the same kind: the composed file was one closing brace short, and the
compiler caught it.

### 3.15 The projection transport's inherited race

The feature-enabled tier failed one test:
`codegg::projection_transport_real::real_tui_rollback_invariants_on_writer_closed`.
The test file and `src/server/ws.rs` are **byte-identical to `main`'s**, so the
defect is inherited rather than introduced by the rebase — but it is a real
defect and it made the tier's green result unreliable.

The race is in the test's own synchronization.
`ConnectionTaskProbe::assert_all_at_baseline` asserts that the TUI request
handler completed exactly once as soon as `mark_tui_task_set()` has been
observed; that flag is set when the task set is **constructed**, not when the
handler finishes, so it is true long before the handler has completed. The
test's wait loop covered the send, receive, and raw-event completions but
**not** the request-handler completion, so the assertion raced the handler it
was waiting for.

The fix waits for the counter the assertion checks. It is strictly *more*
synchronization: no assertion is weakened, no timeout is widened, and no
product code path is touched (`src/server/ws.rs` is unchanged). The rollback
invariants the test exists to prove are asserted exactly as before — the test
simply waits for the teardown to finish before checking it.

**Non-vacuity proved** on the same tree, same host, same command: **7 failures
in 12 isolated runs before the fix, 0 failures in 15 runs after it**.

### 3.16 The reconciliation with `main` at `2a0bae08f`

`main` advanced a further **9 commits** after the fresh rebase's base
`cb0e79c0` — PRs #146 (the TUI editor Helix-parity UX line), #147 (the scratch
record corrections), and #148 (the managed-scratch M007 retention plan). The
pull request was reported `CONFLICTING` / `DIRTY`, and the tip-level conflict
was **exactly one file**.

The reconciliation is a **merge**, not a third rebase, and the reason is the
repository's own precedent rather than convenience: `main`'s history already
carries four `Merge origin/main into <branch>` commits for this exact
situation (`4bd81ef48`, `5bd37fa24`, `1b72c3b8b`, `17f590c3c`), and two
workstreams in this repository recorded their reconciliations the same way
(`plans/closure/tui-editor-helix-parity/021-merge-reconciliation-status.md`,
`plans/closure/managed-agent-scratch-native-qualification-corrective/003-merge-reconciliation-status.md`).
A third rebase would have replayed **309** commits to resolve one file, and
would have invalidated the hosted evidence already obtained on the pre-merge
head.

**The one conflict: `.opencode/skills/tool-execution/SKILL.md`.** The two
sides touch adjacent sentences about session scratch cleanup.

- This branch said: "Current cleanup intentionally retains these resources
  until lease-aware reclamation is qualified."
- `main` said: "Cleanup is lease-aware: a terminal delegated run reclaims its
  own private root, a revoked shared resource is reclaimed after the last
  attempt lease releases, and a soft-deleted session's whole tree is reclaimed
  once its durable runs and jobs are terminal. The daemon resumes that cleanup
  after a restart. Scratch contents remain disposable cache data that host
  cache maintenance may purge."

**The rule applied: the later qualification supersedes the earlier deferral.**
`main`'s M007 *qualified* lease-aware reclamation — which is precisely the
condition the branch's sentence was waiting on — so the branch's sentence is
stale and `main`'s is the accurate one. This is the same shape as WP-B's rule
in a different domain: where one side's later work discharges the other side's
stated precondition, the discharged side yields.

The branch's M005/M004 paragraphs in that file — the authenticated network
intent, the protected Bubblewrap mount view, the `with_linux_process_isolation`
opt-in (Landlock ABI 6 signal/abstract-socket scopes, PID/IPC namespaces,
private procfs, an architecture-checked seccomp denylist), the constrained PTY
owner, and the constrained per-turn LSP service — are **unique to this
branch**: `main` carries no equivalent text anywhere in the file. A grep of
`main`'s copy for `M004|Landlock|OfflineRequired|Bubblewrap|network intent`
returns nothing. They are kept in full, and the resolution is `main`'s
cleanup sentence followed by the branch's M005/M004 paragraphs, so neither side
loses a claim.

**The four auto-merged files were reviewed, not trusted.** `plans/registry.md`
is **996** lines, the branch's own count unchanged; `main`'s four rows changed
in place (the managed-scratch row gains M007, the M006 row gains its named
successor, the harness-lab row is dropped by `main`); and both of this
milestone's own C001 rows — lines 50 and 648 — are intact. `src/tui/app/mod.rs`
carries `main`'s editor-keymap and composer-badge changes, textually disjoint
from the branch's. `architecture/tui.md` and `docs/tui.md` carry `main`'s
editor documentation.

**No published count moves.** `main`'s nine commits added and deleted **no**
file under any directory a published count is measured from — `src/tool/`,
`src/context/`, `src/tui/app/state/`, `crates/codegg-core/`, `crates/egglsp/src/`,
`tests/`, `scripts/`, `architecture/` — because `git diff --diff-filter=A` and
`--diff-filter=D` both return empty for those paths over `cb0e79c0..2a0bae08f`.
The counts were then re-verified against their own listed `Source` on the
merged tree: architecture docs **87**, tool registrations **67** (54 + 13),
tool source files **80**, `codegg-core` modules **48**, authorized operations
**244**, TUI state modules **33** (34 `.rs` files incl. `mod.rs`), storage
layout **85**, integration test files **284**, CI guard scripts **62**,
database tables **89**, built-in agents **10**, workspace member crates **11**
(+ root). The built-in slash-command count is asserted by
`built_in_command_count_matches_release_docs` at **168**, and
`src/tui/command.rs` is byte-identical to the pre-merge tree, so it cannot
have moved.

**Compile-level proof.** `cargo check --workspace --all-targets --locked` →
**exit 0**; `scripts/verify.sh quick` → **"Quick verification passed."**;
`scripts/check_adr_integrity.py` and its `--self-test` pass;
`scripts/check_project_catalog_invariants.py` → **7/7**. The merged tree holds
**zero** conflict markers.

The branch now carries `main`'s tip as a direct ancestor:
`git merge-base --is-ancestor origin/main HEAD` succeeds, and
`git merge-base HEAD origin/main` == `2a0bae08f`.

### 3.17 The second reconciliation, with `main` at `84aa47528`

`main` advanced again while the first reconciliation was being verified and
recorded — **PR #151**, the M006 scratch usage census, moving `main` from
`2a0bae08f` to `84aa47528`. The pull request was reported `CONFLICTING` again,
and the branch was reconciled with it by a second **merge**, for the same
reason as the first: the repository's own history uses that pattern for a
branch carrying work `main` has not seen, and a third rebase would have
replayed 309 commits.

**This advance is not count-neutral, and that is the difference from §3.16.**
It added a `CoreRequest` variant, a `CoreResponse` variant, an authorized
operation, and an integration test file. Two files conflicted, both
documentation:

- `architecture/authorization.md` — the operations table. This branch's
  `network_intent_set` and `main`'s `scratch_usage` are **both** kept. The
  table is not strictly ordered: this branch's own `user_prompt_queue_*` rows
  are an appended block at the end rather than an alphabetical insertion, so
  the two rows sit together at the slot both sides used, and neither side's
  placement is disturbed.
- `architecture/overview.md` — the "Verified counts" table. Four rows move,
  each re-measured against its own listed `Source` on the merged tree: `CoreRequest`
  variants 260 → **261** (237 struct + 24 unit), `CoreResponse` variants 169 →
  **170**, authorized operations 244 → **245**, integration test files 284 →
  **285**. The directory tree's "Integration tests (284 files)" becomes 285.
  Every other row was re-measured and did not move.

**The enum-variant measure was calibrated, not assumed.** The counting method
used here counts struct variants and unit variants separately at the enum's
own brace depth, with attributes and comments stripped first — because
`#[derive(Clone, Debug)]` contains a comma and identifiers, and a naive
counter reads them as variants. That method **reproduces this table's
published values exactly on the pre-merge tree** — `CoreRequest` 260 = 236
struct + 24 unit, `CoreResponse` 169, `CoreEvent` 92, `ProjectionEvent` 47,
`AppEvent` 60 — and only then was it applied to the merged tree. A method
that is not calibrated against known values produces confident wrong numbers,
and the first counter written for this merge produced exactly that: 723
`CoreRequest` "variants".

**The authorized-operations row keeps the `representative_requests()` measure**
pinned on 2026-10-21. `main`'s own published **254** used the superseded
`operation_descriptor` measure, which is a per-request *mapper* rather than a
count of operations, so it is not this row's measure; the merged tree's value
under the pinned measure is **245**.

**Compile-level proof on the twice-merged tree.** `cargo check --workspace
--all-targets --locked` → **exit 0**; `scripts/verify.sh quick` → **"Quick
verification passed."**; the workspace sweep → **15771 tests run: 15771 passed,
8 skipped, 0 failed** (`Summary [ 477.297s]`); the feature-enabled tier →
**12512 run, 12512 passed, 7 skipped, 0 failed** (`Summary [ 434.172s]`). The
merged tree holds **zero** conflict markers, and the branch carries `main`'s
tip as a direct ancestor at `84aa47528`.

### 3.18 A branch-owned defect the hosted Windows run found, and its fix

The `pull_request` event firing for the first time — because the branch became
mergeable — started two workflows this branch had never run: the **Windows
process launcher** and the **Desktop E2E**. One failed for a reason that is
this branch's, and one for a reason that is not. Both are recorded, and the
distinction is the point.

**This branch's own: `scratch::tests::the_session_root_lives_under_the_resolved_cache_root`.**

Run `39580103958`, step "Windows managed scratch lifecycle tests"
(`cargo test -p codegg --lib scratch::tests`): **20 passed, 1 failed**, with

```
the scratch root must live under the resolved cache root:
"\\?\C:\Users\runneradmin\AppData\Local\codegg\scratch\sessions\fd41a13d…"
is not under "C:\Users\runneradmin\AppData\Local"
```

The containment **holds** — the failing path is literally under the cache root.
What differs is the **spelling**: `session_scratch_dir` returns
`std::fs::canonicalize(session_root)`, and on Windows `canonicalize` yields
the extended-length (`\\?\`) device form, while `dirs::cache_dir()` yields the
plain drive form. The test compared a canonicalized path against a
non-canonicalized one, so it failed on its own spelling rather than on its own
claim.

**The test is this branch's own.** It was introduced by `3e722a878` ("fix
(scratch): create the cache root privately, and give the sweep one of its
own"), and it does not exist on `main` at all — `git show origin/main:src/scratch.rs`
carries no such test. The Windows launcher is **green on `main`** at
`84aa47528` and on the M006 and M007 branches, so the failure is a regression
of this branch and not of `main`.

**The fix is in the test, not in the product.** The product's behaviour is
right: `session_scratch_dir` hands out a fully resolved path, which is exactly
what a containment claim needs. The test now canonicalizes **both** sides
before comparing — the same form the product resolves and hands out — and the
assertion's claim is unchanged: the session root lives under the resolved
cache root.

**Non-vacuity proved** two ways. With the containment check pointed at a root
the session root does not live under, the test **fails with the containment
message** — not with an unrelated error — so the comparison still
discriminates. And the test now carries that discriminating power **in-tree**,
as a second assertion that a path outside the resolved cache root does not
read as contained, so the property is checked on every platform rather than
only where the fix was made.

This is the class the hosted evidence exists to find. A Linux-only gate cannot
see a Windows path-spelling difference, and the branch's own hosted Windows
tier is what found it.

**Not this branch's: the Desktop E2E.**

Run `39580103920`, step "Built-app WebDriver trajectory":
`apps/desktop/e2e/specs/m004-session.e2e.ts:169` —
`element ("[data-testid=\"permission-list\"]") still not existing after
120000ms`, with the other six assertions of the same spec passing.

The same workflow **fails on `main` itself**: on `main` at `84aa47528` (push,
2026-10-21T07:07:02Z), on `main` at `48a5ca7d2`, and on the M006 and M007
feature branches — the same spec, the same step, the same timeout. It is a
**pre-existing failure on `main`**, not a regression of this branch, and it is
recorded rather than fixed here: `main`'s Desktop E2E is not this milestone's
work, and this branch does not own the desktop app's session trajectory.

### 3.19 The third reconciliation, with `main` at `28724bd83`

`main` advanced again while the second reconciliation was being verified and
recorded — **PR #152**, the M007 scratch retention sweep, moving `main` from
`84aa47528` to `28724bd83`. Reconciled by a third **merge**, for the same
reason as the first two.

**This advance lands directly on the branch's scratch surface**, and it is the
first reconciliation whose conflict is in product code rather than in
documentation. `main`'s M007 added `src/core/scratch_sweep.rs` (1576 lines),
`tests/scratch_retention_sweep.rs` (946 lines), and 389 changed lines in
`src/scratch.rs` — and the branch's own §3.7 work (the private cache root)
lives in that same file.

**Three conflicted files.**

`src/scratch.rs` — **one hunk**, at the session scratch root's resolution.
The two sides each hold something the other needs, and the resolution keeps
all three parts:

- the branch's `current_owner_uid()` — the owner identity the private cache
  root is validated against;
- `main`'s `scratch_cache_root()` — M007's single source for the cache root,
  which carries the **thread-local test override** the retention sweep needs,
  so a test never reclaims the developer's own scratch;
- the branch's `ensure_private_cache_root(...)` — the private creation that
  keeps the process umask out of the picture.

Dropping the branch's part here would put `main`'s umask-following creation
back: the allocator would create a group-writable root and then refuse it, and
whether the product could run at all would depend on the operator's umask
instead of on the product. That is exactly the defect `3e722a878` fixed, and
it is **not** reintroduced here. `main`'s other three `scratch_cache_root()`
call sites are read-only resolvers that create nothing, so the private
creation is needed at exactly one site — the one the conflict is at.

`architecture/authorization.md` — the operations table. This branch's
`network_intent_set` and `main`'s `scratch_reclaim` are **both** kept.

`architecture/overview.md` — the "Verified counts" table. Four rows move, each
re-measured against its own listed `Source` on the merged tree: `CoreRequest`
variants 261 → **262** (238 struct + 24 unit), `CoreResponse` variants 170 →
**171**, authorized operations 245 → **246**, integration test files 285 →
**286**. The directory tree's "Integration tests (285 files)" becomes 286. The
remaining rows were re-measured and did not move. The enum-variant measure
was calibrated against the tree those values were published against before it
was applied to the merged tree.

**And one cross-file pairing the merge cannot see** — the same class as §3.14.
`main`'s new `src/core/scratch_sweep.rs` builds a `JobPayload::Test` without
the `sandbox_profile` and `network_intent` fields this branch added, so the
merged tree did not compile:

```
error[E0063]: missing fields `network_intent` and `sandbox_profile`
in initializer of `codegg_core::jobs::JobPayload`
   --> src/core/scratch_sweep.rs:896:22
```

The fields are filled with the branch's established values — `None` and
`NetworkIntent::LegacyUnrestricted`, the enum's default — so the fixture
behaves exactly as the other `JobPayload::Test` sites on this branch behave,
and no test's behaviour changes.

**This is the class a rebase cannot see, and the class a *file-level* proof
cannot see either**: a pairing between a file one side added and a type the
other side extended. It is found by compiling the merged tree, which is why
the compile-level proof is a separate step and not a formality.

### 3.20 The fourth reconciliation, with `main` at `c93211542`

`main` advanced again — **PR #153**, the TUI subagent presentation and
name-allocation workstream (P001–P004) — and the pull request conflicted once
more. Reconciled by a fourth **merge**, for the same reason as the first three.

**This was the cheapest of the four reconciliations, and the reason is worth
recording: it is the first one whose conflict set contains no product code at
all.**

`plans/registry.md` was the **only** conflicted file. Both sides' content is
disjoint rows — this branch's three Linux-sandbox-admission corrective rows
against `main`'s four TUI-subagent-presentation rows — so the resolution is a
union, and no row is lost.

**No source file was changed by both sides.** The intersection of the two
sides' changed-file sets is three files, all documentation:

- `architecture/tui.md`
- `docs/tui.md`
- `plans/registry.md`

So this merge introduced **no new cross-file pairing** — the class §3.14 and
§3.19 are about. That is a *measured* result, not an assumption. The pairing
class is invisible to file comparison, which is why the compile-level proof is
still required; but here the compile-level proof had no new pairing to find
beyond the ordinary workspace check.

**No published count moved.** The same calibrated measure that produced
262 / 171 / 246 / 286 on the thrice-merged tree reproduces those values
**exactly** on the four-times-merged tree, so §4's table is unchanged by this
reconciliation. `assets/subagent-names/names.toml` did change, and the file it
produces is generated rather than hand-written:
`python3 scripts/generate_subagent_names.py --check` passes on the merged
tree (46 names).

#### 3.20.1 The verification plan was pruned, deliberately and on the record

This pass is where the milestone's largest time cost was found, and addressed.

**The worktree's build cache was cold.** The earlier reconciliations had left
their compiled artifacts in the *primary* checkout; this linked worktree
carried no `target/` of its own at the start of this pass. A local
full-workspace sweep therefore means compiling and linking roughly **290
integration test binaries** from scratch, which on this host is by a wide
margin the most expensive step in the milestone. The earlier attempt at it on
the thrice-merged tree was killed with **signal 9** while `rustc` was doing
codegen for the `codegg` library test and `presence_m001_leases`, with Cargo
exiting 101 — a host-resource kill during compilation, **not** a test failure.
It is recorded as such.

**Hosted CI is the authoritative full-suite evidence, and it is better evidence
than a local sweep.** `.github/workflows/ci.yml` runs
`cargo nextest run --workspace --locked --profile ci` on the pull request's own
head, on dedicated runners, as the pull request's own required check. That
artifact covers the whole workspace — every one of the integration binaries —
on hardware this milestone does not have to share with anything else. The
local sweep was not a second copy of that evidence; it was a slower, weaker
copy of it.

**The local suite was therefore narrowed to the changed surface**, and the
narrowing is specified rather than implied:

| What | Where it runs | What it proves |
|---|---|---|
| `codegg` library test binary | local | `scratch`, `core::scratch_sweep`, `scheduler::schedule_materializer`, `tui::command`, `agent::progress_recovery`, `agent::subagent_names`, and the `tui::app` modules — the surfaces both sides of this merge touched |
| 13 integration binaries | local | each maps to a file this branch changed |
| `codegg-core`, `codegg-protocol`, `egggit`, `egglsp` | local, in full | the four crates this branch changed |
| Full workspace sweep | **hosted CI** | the complete workspace, on dedicated runners |
| Windows launcher workflow | **hosted** | the branch's own Windows scratch contract, on a Windows runner |

**What this gives up is stated, not hidden.** The full workspace sweep was
**not** run locally on the four-times-merged tree. §13 records that plainly,
and the claim for this head rests on the hosted run.

### 3.21 The fifth reconciliation, with `main` at `239f06e7f`

`main` advanced once more — PR #154, the M006 scratch census's **tri-state**
reclaimable verdict — and it advanced onto **this branch's own scratch
surface**. That is the reason this reconciliation is recorded in its own right
rather than folded into the previous one.

**The merge was textually clean — and that was not sufficient.** `main`'s
change makes the per-tree reclaim verdict tri-state: `ScratchTreeUsage.reclaimable`
and the protocol DTO's `reclaimable` both move from `bool` to `Option<bool>`,
where `None` means the verdict could not be established because the walk was
truncated or the tree vanished. The type change spans **three** files — the
domain type in `src/scratch.rs`, the DTO in
`crates/codegg-protocol/src/scratch.rs`, and the mapping site in
`src/core/daemon_scratch.rs` — and the mapping site is **not** in `main`'s
changed-file set, because `main` changed the two ends of the assignment and
left the middle alone.

**This is exactly the class a file-level proof cannot see, which is why the
compile-level proof is not a formality.** The merge resolved with **zero
conflicts** and the tree **compiles**:
`cargo check --workspace --all-targets --locked` → **exit 0**
(`Finished dev profile in 1m 39s`). The branch's own scratch work survived it
— `current_owner_uid()`, `ensure_private_cache_root(...)`, and the call site at
`src/scratch.rs:198` — and `main`'s tri-state verdict is carried through
unchanged.

**The advance is count-neutral.** The same calibrated measure that produced
262 / 171 / 246 / 286 on the four-times-merged tree reproduces those values
**exactly** on the five-times-merged tree. The tri-state change alters a
field's *type*, not the number of protocol variants, and the DTO is not one of
the counted surfaces.

**Why this reconciliation happened at all.** The branch was green and
`MERGEABLE` on `3a023b04e` when `main` advanced. GitHub's `MERGEABLE` proves
only that the two sides merge without a text conflict — it says nothing about
whether the *result* compiles. Merging the pull request without carrying
`main`'s tip would have combined the branch's scratch work with `main`'s
tri-state verdict and handed the owner a merge whose compile status nobody had
checked. Carrying `main`'s tip as a direct ancestor, and proving the merged
tree compiles, is what turns the pull request's merge into a reviewed one.

## 4. Verification executed

Local, on the final tree, in this order.

The results below are the **five-times-merged** tree — `c78a2a640` (the merge
with `main` at `239f06e7f`) and `4e4023ed8` (this record's own commit), the
fresh rebase's tree reconciled five times with `main`: at `2a0bae08f` (§3.16),
at `84aa47528` (§3.17), at `28724bd83` (§3.19), at `c93211542` (§3.20), and at
`239f06e7f` (§3.21). The rows that predate it are kept below with the tree they
were measured on, so that no number in this record is silently attributed to a
tree it was not measured on.

**The local suite for this tree was narrowed, deliberately.** This linked
worktree's build cache was cold (§3.20.1), and `.github/workflows/ci.yml` runs
the same full-workspace sweep on the pull request's own head, on dedicated
runners, as the pull request's own required check — so the hosted run is the
authoritative full-suite evidence, and the local run is narrowed to the changed
surface. What that gives up is stated in §13.

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo check --workspace --all-targets --locked` | clean (every target) — **exit 0** on `e0036d340`, `17350a5df` and `d24808d4e` |
| `scripts/verify.sh quick` | **"Quick verification passed."** on `e0036d340`, `17350a5df` and `d24808d4e` |
| `python3 scripts/check_adr_integrity.py` | pass |
| `python3 scripts/check_adr_integrity.py --self-test` | "ADR integrity fixtures passed" |
| `python3 scripts/check_project_catalog_invariants.py` | 7/7 |
| `cargo test -p egggit --locked --lib process::tests` | 32 passed, 0 failed |
| `cargo nextest run -p codegg --locked --profile ci -E '<the 14 recorded failures>'` | **18 passed, 0 failed** |
| `cargo test -p codegg --locked --lib -- --list` | the three live-language-server tests are **absent**; the four rejection tests remain |
| `cargo nextest run -p codegg --locked --profile ci -E 'binary(scratch_cache_root_contract) or binary(linux_native_scratch_containment) or test(/scratch::tests::/)'` | **28 passed, 0 failed** (the scratch allocator's own qualifications, its resolution contract, and the native containment fixture) |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | **clean** (0 errors) — after the four lints in §3.8 and the two in `crates/egggit` were fixed. `verify.sh quick` runs this step |
| `cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci --no-fail-fast` | on `e0036d340`: **12512 tests run: 12512 passed, 0 failed, 7 skipped** (`Summary [ 434.172s]`) — the live Eggwork qualification included (§4.2). On `17350a5df`: 12496 run, 12496 passed, 0 failed, 7 skipped (`Summary [ 432.319s]`). On `d24808d4e`: 12474 run, 12474 passed, 0 failed, 7 skipped (`Summary [ 440.750s]`). On the pre-rebase tree: 11804 run, 11804 passed, 0 failed, 7 skipped |
| `cargo nextest run --locked --profile ci --no-fail-fast -p codegg --lib --test <the branch's changed integration targets>` (the narrowed local slice, §3.20.1) | on `aba3a559a`: **7561 tests run: 7561 passed, 4 skipped, 0 failed** (`Summary [ 141.813s]`, exit 0) — the `codegg` library test binary plus the twelve integration binaries covering the files this branch changed |
| `cargo nextest run --locked --profile ci --no-fail-fast -p codegg-core -p codegg-protocol -p egggit -p egglsp` | on `aba3a559a`: **2492 tests run: 2492 passed, 1 skipped, 0 failed** (`Summary [ 66.337s]`, exit 0) — the four crates this branch changed, in full |
| `cargo nextest run --locked --profile ci --no-fail-fast -p codegg --features server,plugins,lsp-test-support --lib --test <the feature-gated targets this branch changed>` | on `aba3a559a`: **7541 tests run: 7541 passed, 4 skipped, 0 failed** (`Summary [ 164.214s]`, exit 0) — the canonical feature set, narrowed to the binaries covering the branch's changed surface: the `codegg` library binary and the nine `server`/LSP-gated integration binaries. `projection_transport_real::real_tui_rollback_invariants_on_writer_closed` — the inherited race of §3.15 — **passes** here |
| `cargo nextest run --locked --profile ci --no-fail-fast -p egglsp --features lsp-test-support` | on `aba3a559a`: **1078 tests run: 1078 passed, 0 skipped, 0 failed** (`Summary [ 15.700s]`, exit 0) — the `egglsp` crate's own fake-LSP harness, which this branch changed |
| The same three local commands on the **five-times-merged** tree, `4e4023ed8` | `cargo check --workspace --all-targets --locked` → **exit 0** (`Finished dev profile in 1m 39s`); `scripts/verify.sh quick` → **"Quick verification passed."**; the count re-derivation → **ALL MATCH**; the narrowed slice → **7561 run, 7561 passed, 4 skipped** (`Summary [ 149.642s]`, exit 0); the four crates → **2492 run, 2492 passed, 1 skipped** (`Summary [ 73.439s]`, exit 0); the feature-enabled tier → **7541 run, 7541 passed, 4 skipped** (`Summary [ 153.252s]`, exit 0) for `codegg` and **1078 run, 1078 passed, 0 skipped** (`Summary [ 9.062s]`, exit 0) for `egglsp` |
| `cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci -E 'binary(eggwork_remote_execution_live)'` | on `17350a5df`: **16 run, 16 passed, 0 failed** (`Summary [ 8.212s]`) — the live qualification's own binary, run on its own so its inclusion is proved rather than inferred from a total |
| `cargo nextest run -p codegg --locked --profile ci -E 'binary(eggwork_remote_execution_live)'` | **14 passed, 0 failed** with the build tree in the failing state (helper 0775, target directory 0775) — the qualification's own non-vacuity proof (§3.11) |
| `cargo nextest run -p codegg --locked --profile ci -E 'test(/scratch/)'` with **no** `XDG_CACHE_HOME` | **33 passed, 0 failed** — the ambient cache path against this host's repaired `~/.cache` (0755) |

The two `crates/egggit` regression tests were proved **non-vacuous**: with the
pre-fix behaviour restored temporarily, both fail — the transient test with
`Err(BackendUnavailable { capabilities: [RepositoryAlias] })`, which is the
exact error the recorded sweep reported, and the persistent test because the
refusal came after one attempt instead of the bounded three. The fix was then
restored and both pass.

The scratch allocator's regression test was proved **non-vacuous** the same
way: with `ensure_private_cache_root`'s body restored to the pre-fix
`create_dir_all` + `ensure_private_parent`, `a_cache_root_the_product_creates_is_private`
fails with `WrongOwner` on the root the allocator had just created. The fix was
then restored and the test passes.

The live-Eggwork fixture's non-vacuity was proved **in the qualification
itself**, not only in a unit test: with the build tree restored to the state
that produced the five failures — the helper group-writable and the target
directory group-writable — the *same* binary produced **8 passed / 5 failed**
before the fix and **14/14 passed** after it, with the build tree unchanged by
the run. The five failures and the five passes are the same five tests with
the same refusal message, so the mode of the helper — and nothing else — was
the cause (§3.11).

The authoritative workspace sweep is recorded in §4.1.

### 4.1 The authoritative sweep

`XDG_CACHE_HOME="$(mktemp -d)" cargo nextest run --workspace --locked --profile ci --no-fail-fast`,
run on the committed tree with no concurrent edits and no Git commands in the
worktree:

- **The five-times-merged tree, `4e4023ed8`: 15841 tests run: 15841 passed, 8
  skipped, 0 failed, 0 timed out** (`Summary [ 523.804s]`, exit 0) — the
  **hosted** run `39642334455` (§4.3), on dedicated runners. This is the
  authoritative full-suite evidence for the tree this milestone ships.
- **The four-times-merged tree, `aba3a559a`: 15841 tests run: 15841 passed, 8
  skipped, 0 failed, 0 timed out** (`Summary [ 523.966s]`, exit 0) — the
  **hosted** run `39632391908` (§4.3), on dedicated runners. The count is **70**
  higher than the twice-merged tree's 15771 because `main`'s M007 sweep and its
  TUI-subagent workstream brought their own tests with them; nothing on the
  branch's side changed.
- **The same tree, locally, narrowed to the changed surface:** 7561 run, 7561
  passed, 4 skipped on the `codegg` library test binary and the branch's twelve
  integration binaries, and 2492 run, 2492 passed, 1 skipped on the four crates
  this branch changed. The **full workspace sweep was not run locally on this
  tree** — §3.20.1 says why, and §13 says what that means.

- **The tree carrying the Windows scratch fix (§3.18): 15771 tests run: 15771
  passed, 8 skipped, 0 failed, 0 timed out** (`Summary [ 469.705s]`, exit 0) —
  the same count as the twice-merged tree, because the fix changes a test, not
  the product.
- **The twice-merged tree, `e0036d340`: 15771 tests run: 15771 passed, 8 skipped,
  0 failed, 0 timed out** (`Summary [ 477.297s]`, exit 0). The count is **16**
  higher than the first reconciliation's tree because `main`'s M006 census
  brought its own tests with them; nothing on the branch's side changed.
- **The merged tree, `17350a5df`: 15755 tests run: 15755 passed, 8 skipped,
  0 failed, 0 timed out** (`Summary [ 484.380s]`, exit 0). The count is **22**
  higher than the fresh rebase's tree because `main`'s nine commits brought
  their own tests with them; nothing on the branch's side changed.
- **The fresh rebase's tree, `d24808d4e`: 15733 tests run: 15733 passed,
  8 skipped, 0 failed, 0 timed out** (`Summary [ 476.302s]`, exit 0), and
  `scripts/verify.sh quick` → **"Quick verification passed."** The count is
  729 higher than the pre-rebase tree's because `main` brought 103 commits
  with it.
- **15004 tests run: 15004 passed, 8 skipped, 0 failed, 0 timed out**
  (`Summary [ 505.752s]`, exit 0) on the pre-rebase tree `81697139`. The count is
  one higher than the earlier run on `9b346776` because the live-Eggwork
  fixture added one regression test (§3.11).
- Two earlier runs of the same command on this tree were **not** green, and
  both are recorded here rather than dropped:
  - The first recorded **14 timeouts** in `security::workflow::report::tests`
    and **1 failure** in `agent_loop_harness`
    (`test_task_tool_denied_tools_passthrough`, "Timeout waiting for request
    after 100 attempts"). This host's load average was **38–81** during that
    run — other sessions were saturating the machine — and the same tests had
    **passed in 9.9 s and 10.1 s** in the earlier green sweep of §4.1 on
    `9b346776`. Re-run in isolation on the same tree they pass again: the
    whole `security::workflow::report` module, **56 tests, 0 failed, 8.979 s**,
    and the harness test, **1 passed, 0.706 s**.
  - The second recorded **1 timeout**
    (`single_daemon_lifecycle plain_entrypoint_reaches_tui_startup_boundary_without_prestarted_daemon`)
    while the load average was **86–110**; re-run in isolation it passes in
    **8.246 s**. The same run recorded 12 "slow" tests, one of them at
    **86.6 s**.
  - These are the timing-sensitive tests §10 already names as a standing
    property of the `ci` profile's 120 s kill threshold, and they are **not**
    introduced by this milestone: the code they exercise is untouched by the
    changes recorded here. The `ci` profile's threshold was **not** widened to
    make a run green.
- The eight skipped tests are the repository's own `#[ignore]` declarations:
  four tool-advisor qualifications, the manual object-view performance
  qualification, and three opt-in real-process runs (a local Eggsearch
  compatibility smoke and two pinned-Eggsearch-child runs). They were skipped
  in every earlier sweep as well. Nothing was disabled, filtered, or excluded
  to reach this result — the sweep ran the whole workspace under the `ci`
  profile with `--no-fail-fast`.
- An earlier run of the same command on tree `9b346776` ran with **this
  workstation's `~/.cache` still at mode 0777** — world-writable, and correctly
  refused by the product — and with the sweep's own private cache root, and
  was green: 15003 passed, 0 failed. The green result is therefore a property
  of the product and of the sweep's own environment, not of a repaired host.
  The host's cache root was repaired to 0755 *after* that run; the final run
  above was made with the repaired host and the private cache root.
- The earlier sweep of `9b346776` recorded **44 failures across 15 binaries**,
  every one of them the typed refusal `WrongOwner`. The families that failed
  then — `scratch`, `core::daemon`, `core::daemon_sessions`,
  `tool_program_m015/016/017`, `session_family control_m004`,
  `async_background_c006`, `agent_loop_harness`, `identity_live_execution_audit`,
  `identity_m002_live_audit_hooks`, `workspace_root_execution_authority`,
  `linux_native_scratch_containment`, `python_script::executor`,
  `managed_process`, `tool::bash` — all pass now.
- The three live-language-server tests are **absent** from this sweep (they
  sit behind `lsp-real-server-tests`), and the four sibling rejection tests
  that use the same tool are present and pass.

### 4.2 The feature-enabled tier, and the live Eggwork qualification

`cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci --no-fail-fast`
— the third tier of `scripts/verify.sh full` — was run on the final tree:

- **The five-times-merged tree, `4e4023ed8`: 7541 tests run: 7541 passed, 4
  skipped, 0 failed** (`Summary [ 153.252s]`, exit 0), with
  `cargo nextest run --locked --profile ci --no-fail-fast -p egglsp --features
  lsp-test-support` → **1078 run, 1078 passed, 0 skipped**
  (`Summary [ 9.062s]`, exit 0). The same canonical feature set, the same
  narrowing, and the same coverage of the inherited projection-transport race
  of §3.15.
- **The four-times-merged tree, `aba3a559a`: 7541 tests run: 7541 passed, 4
  skipped, 0 failed** (`Summary [ 164.214s]`, exit 0), with
  `cargo nextest run --locked --profile ci --no-fail-fast -p egglsp --features
  lsp-test-support` → **1078 run, 1078 passed, 0 skipped**
  (`Summary [ 15.700s]`, exit 0). The **feature set is the canonical one** —
  `server,plugins,lsp-test-support` — but the run was **narrowed** to the
  binaries covering this branch's changed surface: the `codegg` library test
  binary and the nine `server`/LSP-gated integration binaries, rather than all
  ~290 integration binaries (§3.20.1). The inherited projection-transport race
  of §3.15 is covered here: `projection_transport_real::
  real_tui_rollback_invariants_on_writer_closed` **passes**.

- **The tree carrying the Windows scratch fix (§3.18): 12512 tests run: 12512
  passed, 0 failed, 7 skipped** (`Summary [ 435.238s]`, exit 0) — the same
  count as the twice-merged tree, because the fix changes a test, not the
  product.
- **The twice-merged tree, `e0036d340`: 12512 tests run: 12512 passed,
  0 failed, 7 skipped** (`Summary [ 434.172s]`, exit 0). The count is **16**
  higher than the first reconciliation's tree, entirely from `main`'s own
  tests.
- **The merged tree, `17350a5df`: 12496 tests run: 12496 passed,
  0 failed, 7 skipped** (`Summary [ 432.319s]`, exit 0). The live-Eggwork
  binary's own **16** tests are inside that total, and they were additionally
  run on their own — **16 run, 16 passed** (`Summary [ 8.212s]`) — so the
  qualification's inclusion is proved rather than inferred from a count. The
  count is **22** higher than the fresh rebase's tree, entirely from `main`'s
  own tests.
- **The fresh rebase's tree, `d24808d4e`: 12474 tests run: 12474 passed,
  0 failed, 7 skipped** (`Summary [ 440.750s]`, exit 0). All 15 live-Eggwork
  qualifications pass, including the two `main` added for Eggwork M002 and this
  branch's fixture-trust contract test (§3.11).
- The **first** run of the same command on that tree was **not** green: 12474
  run, 12473 passed, **1 failed** —
  `real_tui_rollback_invariants_on_writer_closed`. The defect was inherited
  from `main` (the test file and `src/server/ws.rs` are byte-identical to
  `main`'s) and is a race in the test's own wait condition; it failed **7 of 12**
  isolated runs. It is closed by waiting for the counter the assertion checks
  (§3.15), and the re-run above is green.
- **11804 tests run: 11804 passed, 0 failed, 7 skipped** (`Summary [ 603.669s]`,
  exit 0) on the pre-rebase tree `81697139`. The count is one higher than the
  run below because the live-Eggwork fixture added one regression test (§3.11).
- The earlier run of the same command on the same tree was **not** green:
  **11803 run, 11798 passed, 5 failed**, all five in one binary,
  `eggwork_remote_execution_live`, and all five with the same cause — the live
  node did not advertise the isolation capability the daemon requires, so the
  run was refused before the workspace was uploaded:
  `eggwork: node 'live-node-1' does not satisfy required isolation capability 'isolation.landlock.workspace-rw.v1'; refusing before workspace upload`
  (`src/scheduler/eggwork.rs:1512`).
- **The cause is now established and fixed** (§3.11): the pinned node refuses a
  sandbox helper it cannot trust, and the helper's mode — and the mode of the
  directory above it — followed the *process umask*, because neither cargo nor
  `tempfile` narrows the modes they create. On this host (`umask 0002`) that
  produced a group-writable helper, the node dropped
  `isolation.landlock.workspace-rw.v1`, and five live qualifications failed on
  a tree, a fixture, and a code base that were all identical to a green run.
- The fix is in the **fixture**, not in the product and not in the pinned
  upstream check: the qualification now stages the node and the helper in a
  private directory whose modes it sets itself. The node's own refusal is
  untouched and remains a fail-closed refusal.
- This is the **live** qualification, not a unit test. `ci.yml` excludes it
  from an unrelated-PR run — `cargo nextest run --workspace --locked --profile ci -E 'not binary(eggwork_remote_execution_live)'`
  — and provisions it separately (`scripts/prebuild-eggwork-fixtures.sh`,
  `CODEGG_EGGWORK_TEST_NODE`, `CODEGG_EGGWORK_FIXTURES_PREBUILT=1`), because it
  needs a provisioned node rather than a mocked one. The prebuild script only
  has to *produce* the artifacts; the qualification stages and trusts them.

### 4.3 The hosted runs

The hosted evidence this milestone waited for, on the two heads that carried
it. `352ed719c` is the fresh rebase's head; `979ba14c6` is the head after
the first reconciliation with `main` (§3.16).

| Run | Head | Workflow | Event | Result |
|---|---|---|---|---|
| `39551117669` | `352ed719c` | `CI` — the default sweep | `workflow_dispatch` | **success** (30m48s) — all 44 steps green; `Workspace tests`: **15733 tests run: 15733 passed, 8 skipped** (`Summary [ 523.092s]`) |
| `39550735458` | `352ed719c` | Linux M004 process and IPC qualification | `push` | **success** (30m10s) — jobs `restricted-userns-fails-closed` **success**, `process-ipc` **success**, `git-cost-pilot` **skipped** |
| `39550735444` | `352ed719c` | Linux C006 protected mount qualification | `push` | **success** (9m11s) |
| `39550735415` | `352ed719c` | Windows native probe contract | `push` | **success** (8m17s) |
| `39550735467` | `352ed719c` | Linux Git object snapshot CoW qualification | `push` | **success** (3m36s) |
| `39550735512` | `352ed719c` | Command authorization macOS target compile (one-shot) | `push` | **success** (3m6s) |
| `39567534361` | `979ba14c6` | `CI` — the default sweep | `workflow_dispatch` | **success** (56m05s) — all steps green; `Workspace tests`: **15755 tests run: 15755 passed, 8 skipped** (`Summary [ 441.370s]`) |
| `39567537372` | `979ba14c6` | Linux M004 process and IPC qualification | `workflow_dispatch` | **success** |
| `39567540916` | `979ba14c6` | Linux C006 protected mount qualification | `workflow_dispatch` | **success** |
| `39567544367` | `979ba14c6` | Linux Git object snapshot CoW qualification | `workflow_dispatch` | **success** |
| `39567547799` | `979ba14c6` | Windows native probe contract | `workflow_dispatch` | **success** |
| `39567330945` | `979ba14c6` | Command authorization macOS target compile (one-shot) | `push` | **success** |
| `39580104073` | `2ab5a3335` | `CI` — the default sweep | `pull_request` | **success** — `Workspace tests`: **15771 tests run: 15771 passed, 8 skipped** |
| `39580165234` | `2ab5a3335` | `CI` — the default sweep | `workflow_dispatch` | **success** |
| `39580103924` | `2ab5a3335` | Linux M004 process and IPC qualification | `pull_request` | **success** |
| `39580168070` | `2ab5a3335` | Linux M004 process and IPC qualification | `workflow_dispatch` | **success** |
| `39580097658` | `2ab5a3335` | Linux M004 process and IPC qualification | `push` | **success** |
| `39580104060` | `2ab5a3335` | Linux C006 protected mount qualification | `pull_request` | **success** |
| `39580170650` | `2ab5a3335` | Linux C006 protected mount qualification | `workflow_dispatch` | **success** |
| `39580103890` | `2ab5a3335` | Linux Git object snapshot CoW qualification | `pull_request` | **success** |
| `39580173312` | `2ab5a3335` | Linux Git object snapshot CoW qualification | `workflow_dispatch` | **success** |
| `39580103981` | `2ab5a3335` | Windows native probe contract | `pull_request` | **success** |
| `39580175684` | `2ab5a3335` | Windows native probe contract | `workflow_dispatch` | **success** |
| `39580097823` | `2ab5a3335` | Command authorization macOS target compile (one-shot) | `push` | **success** |
| `39580103958` | `2ab5a3335` | Windows process launcher | `pull_request` | **failure** — the branch's own Windows scratch test; §3.18. **Fixed on the branch's tree** |
| `39580103920` | `2ab5a3335` | Desktop E2E | `pull_request` | **failure** — **pre-existing on `main`**; §3.18 |
| `39632391908` | `aba3a559a` | `CI` — the default sweep | `pull_request` | **success** — `Workspace tests`: **15841 tests run: 15841 passed, 8 skipped** (`Summary [ 523.966s]`) |
| `39632392133` | `aba3a559a` | Windows process launcher | `pull_request` | **success** — and the branch's own Windows scratch test now passes there: `scratch::tests::the_session_root_lives_under_the_resolved_cache_root ... ok`, **21 passed, 0 failed** (§3.18) |
| `39632391912` | `aba3a559a` | Linux M004 process and IPC qualification | `pull_request` | **success** |
| `39632385340` | `aba3a559a` | Linux M004 process and IPC qualification | `push` | **success** |
| `39632391992` | `aba3a559a` | Linux C006 protected mount qualification | `pull_request` | **success** |
| `39632392003` | `aba3a559a` | Linux Git object snapshot CoW qualification | `pull_request` | **success** |
| `39632391967` | `aba3a559a` | Windows native probe contract | `pull_request` | **success** |
| `39632385341` | `aba3a559a` | Command authorization macOS target compile (one-shot) | `push` | **success** |
| `39642391968` | `aba3a559a` | Desktop E2E | `pull_request` | **failure** — **pre-existing on `main`**: the same `apps/desktop/e2e/specs/m004-session.e2e.ts:169`, `[data-testid="permission-list"]` "still not existing after 120000ms". It reproduces on `main` at `84aa47528` and `48a5ca7d2` and on the M006/M007 branches (§3.18) |
| `39642334455` | `4e4023ed8` | `CI` — the default sweep | `pull_request` | **success** — `Workspace tests`: **15841 tests run: 15841 passed, 8 skipped** (`Summary [ 523.804s]`) |
| `39642334201` | `4e4023ed8` | Windows process launcher | `pull_request` | **success** |
| `39642334496` | `4e4023ed8` | Windows native probe contract | `pull_request` | **success** |
| `39642334419` | `4e4023ed8` | Linux M004 process and IPC qualification | `pull_request` | **success** |
| `39642326885` | `4e4023ed8` | Linux M004 process and IPC qualification | `push` | **success** |
| `39642334661` | `4e4023ed8` | Linux C006 protected mount qualification | `pull_request` | **success** |
| `39642334322` | `4e4023ed8` | Linux Git object snapshot CoW qualification | `pull_request` | **success** |
| `39642326845` | `4e4023ed8` | Command authorization macOS target compile (one-shot) | `push` | **success** |
| `39642334182` | `4e4023ed8` | Desktop E2E | `pull_request` | **failure** — **pre-existing on `main`**, the same spec, line, and 120 s timeout as on every earlier head (§3.18) |
| — | all | GitGuardian Security Checks | — | **success** |

**The hosted default sweep's count matches the local one exactly, on both
heads** — 15733 on `352ed719c` and 15755 on `979ba14c6` — so each hosted
result is a property of the product and not of the runner.

**The M004 native tier is qualified on this head**, which is the evidence this
workstation cannot produce: the host has Landlock ABI < 6. All ten
qualification steps passed — install Bubblewrap; record native runner
capabilities; enable user namespaces; build the trusted sandbox helper; C007
private-index publication under containment; the M004 managed-process host PID
and IPC denial probes; the M004 constrained PTY live-owner qualification; the
M004 daemon-authorized PTY routing and descendant cleanup; the M004
daemon-owned constrained LSP duplex and procfs qualification; and the
protected-mount and Git regressions (**20 passed**). The step-level test
results were **17 passed / 0 failed** (552.47 s), **11 passed**, and **20
passed**. The tier was **re-qualified on the first reconciliation's head**
`979ba14c6` as well (`39567537372`, **success**), so the qualification is not
carried by a single head.

**One fact about *how* the CI runs were started, and the correction it
requires.** On the first two heads, the `pull_request` event did not create a
`CI` run, and every `CI` run on this branch was a `workflow_dispatch`. That
observation was real, and the explanation offered alongside it — that the
cause "is not in the repository's workflow file and is not diagnosed" — was
**incomplete, and is now corrected**.

Both earlier heads were **`CONFLICTING`** pull requests, and both were
reconciled only after the hosted runs had been started. The `pull_request`
event fired on the **third** head, `2ab5a3335`, the moment the branch carried
`main`'s tip and the pull request became **mergeable** — and it fired for the
whole set: the `CI` job (`39580104073`), the M004 native tier
(`39580103924`), the C006 protected mount qualification (`39580104060`), the
Git CoW qualification (`39580103890`), the Windows native probe contract
(`39580103981`), the Windows process launcher (`39580103958`), and the
Desktop E2E run (`39580103920`).

**The mechanism.** A `pull_request` workflow checks out the synthetic
`refs/pull/N/merge` ref, and GitHub cannot create that ref for a pull request
that conflicts — there is no merge commit to check out. The ref exists now:
`git ls-remote origin refs/pull/149/merge` →
`c9da671b853c8f75cd7f8ef74bc1736d4c71a50d`, a synthetic merge of the head into
`84aa47528`. While the pull request conflicted, it did not exist, and no
`pull_request` workflow could start. The absence of those runs was a symptom
of the **conflict**, not a property of `ci.yml`'s trigger block — which is
byte-identical to `main`'s, and which carries no `if:` condition.

This also settles the second question the earlier record left open: the
`workflow_dispatch` runs are not a weaker substitute for `pull_request` runs.
On the third head, **both** exist and exercise the same job, the same steps,
and the same configuration — the dispatched run against the branch head, the
`pull_request` run against the synthetic merge.

## 5. Invariant review

- **One migration number per migration.** Holds: one `migrate_v83`, one
  `migrate_v84`, one `migrate_v85`, and `STORAGE_LAYOUT_VERSION` tracks the
  highest wired migration. Proved by the catalog invariant guard.
- **One ADR identifier per decision.** Holds: the ADR integrity guard passes,
  and passes its own self-test.
- **A persistent object-store failure stays a typed refusal.** Holds: the
  fail-closed regression test passes and is non-vacuous.
- **The default sweep is host-independent.** Holds for the live-language-server
  case, for the per-user cache root, and for the live Eggwork fixture's trust
  chain: the sweep's cache root is supplied by the canonical runners, the
  allocator creates the root it depends on privately rather than at the umask's
  discretion, the refusal of an unsafe cache root is a tested contract, and the
  live qualification stages its node and helper with modes it sets itself
  instead of inheriting the operator's umask. It does **not** hold in general:
  the sweep still contains tests whose runtime depends on machine speed, and
  the nextest profile's 120 s kill threshold remains a boundary for them. That
  is a standing property of the profile, not a defect this milestone
  introduced; see §10.
- **No accepted decision's substance changed.** Holds: the command-authority
  ADR's status, date, text, authority, non-goals, and security limits are
  unchanged; only the identifier and the unambiguous cross-references moved.

## 6. Failure and recovery review

- A conflict resolution that produced markers was detected by scanning every
  commit of the branch for markers and by the compiler, and was repaired by a
  targeted rewrite whose correctness is an **empty tree diff**. The repair did
  not change the tree; it changed which commit carries the correction.
- The object-store fix's failure behaviour was reviewed deliberately: the
  bounded attempt count is the *same* bound the code already used for "the
  source changed during snapshot". Only the class of event that reaches it
  widened — from "the fingerprint did not match" to "the fingerprint could not
  be read". A store that stays unreadable still refuses, after the bound.
- The LSP gate's failure behaviour: a host without a language server now
  *skips* those three tests instead of hanging; a host with one runs them only
  when the feature is enabled.
- The canonical gate caught a defect in the tree this milestone had already
  committed: the closure record named the command ADR's **former** filename in
  the `plans/adrs/ADR-….md` path form, which the ADR guard reads as a live
  link. The reference now follows the repository's citation pattern — live
  paths in full, historical identities by number or filename. The guard and its
  self-test were re-run and pass. It is recorded here because a gate that
  fails on the tree it is meant to certify is the gate working, not the gate
  misbehaving.

## 7. Migration and compatibility review

- The layout version moves 84 → 85. A database already at 84 gains only the
  C007 publication journal (one table, one index, additive). A database at 83
  gains the M005 network-intent preference and then the journal. Both are
  additive; no existing table is altered and no stored data is rewritten.
- No protocol change. One bounded product change: a per-user cache root that
  the product **creates** is now created with mode `0700` instead of the
  process umask's default. A cache root that already exists is unaffected —
  it is validated, never re-permissioned — so no existing directory's
  permissions change, and an unsafe one is still refused. No decision's
  substance changes.
- The branch's history was rewritten by the rebase. Every commit subject is
  preserved; every SHA changed. The pre-rebase tip remains reachable through
  `backup/pre-rebase-032b-c001` for provenance.

## 8. Security review

- The renumbering does not touch a decision's authority, ceilings, or
  non-goals. The command-authority ADR's accepted text is unchanged.
- The object-store fix does not relax a containment boundary. The private
  object view still refuses a repository whose `.git` is a symlink, whose
  object store is a symlink, or whose object store carries an `alternates`
  file — those remain hard refusals in `trusted_object_directory`. Only the
  *transient read* path changed, and it changed from "refuse" to "retry, then
  refuse".
- The LSP gate does not reduce coverage of the product's own logic: the four
  sibling tests that exercise the same code paths without a server remain in
  the default sweep, and the three gated tests are still runnable through the
  feature the repository already defines.
- The scratch allocator's cache-root change does **not** relax the boundary it
  sits behind. A cache root another local user could write to is still refused,
  with the same typed `WrongOwner` error and the same path; the change only
  affects the mode of a directory the product is creating for the first time,
  and the product still never re-permissions a directory it did not create.
  The two refusal tests pin that behaviour.
- The audit record's network boundary contract (C023) is untouched by this
  milestone.
- The live-Eggwork fixture's staging does **not** relax the upstream trust
  check. The pinned node still refuses a helper that is not executable, is
  owned by neither root nor the invoking user, is group- or world-writable,
  or has a group- or world-writable directory above it; the product's executor
  still refuses a node that cannot isolate the workspace. The staging only
  ensures the fixture hands the node an artifact that satisfies the check on
  any host, and the node's own TLS directory — which holds the client key — is
  now narrowed to `0700` as well.

## 9. Documentation and operations

- `architecture/overview.md` — the "Verified counts" table and its dated
  re-measurement note.
- `plans/adrs/linux-command-adr-id-migration.md` — the identity history and
  the 2026-10-20 reconciliation.
- `plans/adrs/README.md`, the Linux isolation and command-authorization
  roadmaps, the affected implementation plans, and the affected closure
  records — corrected identifiers, with the historical narrative intact.
- `plans/subsystems/agent-tool-use-operability-post-audit-corrective-addendum.md`
  — the corrected dangling ADR path, with a provenance note.
- `scripts/check_adr_integrity.py` — the self-test fixture follows the
  renumbered identity.
- `architecture/security.md` — the scratch allocator's cache-root contract: a
  root the product creates is private, and an existing unsafe root is refused
  rather than tightened.
- `architecture/testing.md` — the sweep's private cache root, and why the
  canonical runners supply one; and the live Eggwork fixture's trust contract
  (§3.11).
- `AGENTS.md` — the broad runner's command now carries its private cache root.
- `scripts/verify.sh`, `.github/workflows/ci.yml` — the canonical runners
  supply the sweep's private cache root.
- `scripts/prebuild-eggwork-fixtures.sh`, `crates/eggwork-test-node/src/main.rs`
  — the prebuild produces the artifacts; the qualification stages and trusts
  them (§3.11).

### Host state on this workstation

`/home/sugarwookie/.cache` was mode `0777` — world-writable — which the
product refuses by design, so an installed `codegg` on this machine could not
allocate scratch at all. The directory was repaired to `0755` after the
authoritative sweep had been run and recorded **with the unsafe mode still in
place**, so the sweep's result is evidence about the product and not about the
repair. The repair is a host action; nothing in the repository depends on it.
With the repaired host, the ambient (no `XDG_CACHE_HOME`) path was qualified
separately: the 33 scratch-related tests and the two cache-root contract tests
pass with the host's own `~/.cache` as the resolved root.

This host's **umask is `0002`**, which is what made the live-Eggwork fixture's
trust chain host-dependent (§3.11). It is a host property, not a repository
one, and the fix removes the dependency rather than the exposure: the fixture
sets the modes it needs on any host.

## 10. Unresolved findings

- **low — the branch's history contains two commits whose
  `scripts/check_adr_integrity.py` carried unresolved conflict markers and was
  therefore not valid Python.** They are `a59718fc` and `8a634091` (the latter
  is the rebase's replay of the former); each is immediately followed by its
  own fix (`23c07f84`, `235a34fa`). No gate ever runs on an intermediate
  commit, no tip was affected, and the final tree's guard is valid and passes
  its self-test. They were **not** rewritten: a second full-history rewrite
  would churn every downstream SHA — including the commits this milestone's own
  closure cites — for no CI benefit, and the branch's ADR-reconciliation work
  has a documented pattern of marker-bearing intermediate states that a
  two-commit rewrite would not make consistent. This is recorded rather than
  hidden.
- **low — the sweep still contains timing-sensitive tests** whose runtime
  depends on machine speed, against the `ci` profile's 120 s kill threshold.
  The three live-language-server timeouts that were observed are closed. On
  this tree, two workspace-sweep runs were disturbed by **other sessions
  saturating this host** (load average 38–81 and 86–110) and produced 14
  timeouts plus one failure, and one timeout respectively; every one of them
  passes again in isolation on the same tree (§4.1), and the same tests had
  passed in 9.9 s and 10.1 s in the earlier green sweep. The threshold was
  **not** widened to make a run green, and no test was changed. The remaining
  exposure is a property of the profile, recorded in `.config/nextest.toml`,
  and is not introduced by this milestone.
- **closed — the hosted CI runs recorded in the C007 and C023 closures
  qualified the *code* those closures describe, not the rebased head.** The
  rebase rewrote every SHA, and the recorded runs predate it. This is now
  discharged: the rebased head `352ed719c` was observed in a hosted run — the
  default sweep **39551117669**, **success**, 15733 tests run and 15733
  passed (§4.3) — together with the five native workflows. The earlier
  statement, that the rebased head "has not yet been observed in a hosted
  run", was true when it was written and is not true now.
- **closed — the hosted `CI` run for this pull request did not exist on the
  first two heads, and the reason was the pull request's conflict, not the
  repository's workflow file.** The earlier statement — that the
  `pull_request` event "did not create a `CI` run for this pull request, on
  this branch, on any of its heads" and that the cause was "not diagnosed" —
  was **incomplete, and is corrected here**. A `pull_request` workflow checks
  out the synthetic `refs/pull/N/merge` ref, and GitHub cannot create that ref
  for a conflicting pull request. Both earlier heads conflicted; the ref does
  not exist for them, and no `pull_request` workflow could start. On the third
  head, which is mergeable, the ref exists and the `pull_request` event fired
  for the whole set of workflows, `CI` included (§4.3). The lesson is the
  ordering: **a branch that conflicts with `main` cannot obtain hosted
  `pull_request` evidence at all**, so the reconciliation with `main` is a
  prerequisite for the evidence and not merely a tidiness step.
- **closed — the branch's own Windows scratch test failed on the hosted
  Windows runner, and the defect is fixed.** Run `39580103958` failed
  `scratch::tests::the_session_root_lives_under_the_resolved_cache_root`
  because the test compared a canonicalized path against a non-canonicalized
  one, and on Windows those spell the same directory differently. The test is
  this branch's own and does not exist on `main`; the fix canonicalizes both
  sides, keeps the assertion's claim unchanged, and carries the comparison's
  discriminating power in-tree so it is checked on every platform. Non-vacuity
  is proved locally (§3.18).
- **low — the Desktop E2E fails, and the failure is pre-existing on `main`.**
  Run `39580103920` fails `m004-session.e2e.ts:169` on a
  `[data-testid="permission-list"]` that never appears. The same workflow fails
  on `main` itself, at `84aa47528` and at `48a5ca7d2`, and on the M006 and
  M007 branches — same spec, same step, same timeout. It is **not** a
  regression of this branch, and it is recorded rather than fixed here:
  `main`'s Desktop E2E is not this milestone's work (§3.18).
- **closed — the branch's hosted evidence now qualifies the tree it ships.**
  The gap this bullet recorded was real: after the second reconciliation the
  hosted runs qualified the tree *before* that change, and the twice-merged
  tree `e0036d340` was verified locally only. It is now discharged. The
  four-times-merged tree `aba3a559a` was observed in hosted runs — the default
  sweep **39632391908**, **success**, **15841 tests run: 15841 passed, 8
  skipped** (§4.1, §4.3), together with the Windows process launcher
  (**39632392133**, **success**, and the branch's own Windows scratch test now
  passes there), the Linux M004 tier, the C006 protected-mount tier, the Git
  CoW tier, the Windows native probe contract, and the macOS target compile.
  The **five-times-merged tree `4e4023ed8`**, which is the tree this milestone
  ships, was then observed in hosted runs of its own — the default sweep
  **39642334455**, **success**, **15841 tests run: 15841 passed, 8 skipped**,
  and all seven native workflows **success** (§4.3). The three reconciliations
  that followed the second one (§3.19, §3.20, §3.21) are recorded with their
  own counts and their own conflict resolutions.
- **low — the four-times-merged tree's *local* full-workspace sweep was not
  run.** This linked worktree's build cache was cold, so the sweep means
  compiling and linking roughly 290 integration test binaries from scratch; the
  hosted `CI` job runs the identical command on dedicated runners and is the
  authoritative full-suite evidence (§3.20.1). The local suite was narrowed to
  the changed surface — 7561 tests on the `codegg` library binary and the
  branch's twelve integration binaries, and 2492 on the four crates this branch
  changed — and that narrowing is recorded rather than presented as a full
  sweep. §13 states the gap.
- **low — `main` advanced twice while this milestone was in flight, and each
  advance was reconciled by a merge rather than a rebase.** The reconciliations
  are bounded, rule-driven, and each one is recorded (§3.16, §3.17), but the
  honest statement is that a branch carrying work `main` has not seen will be
  `dirty` against `main` until it is reconciled with whatever `main`'s tip is
  at that moment. Chasing a continuously-moving `main` is not a bounded task;
  the procedure this milestone established — merge, resolve by an explicit
  rule, re-derive the counts, re-run every gate — is the reusable part, and
  it is written down.
- **low — `main`'s `src/server/ws.rs` carries a deprecated `AtomicBool::fetch_update`
  call**, which the feature-enabled build reports as a warning. The file is
  byte-identical to `main`'s, the warning is not produced by any gate
  (`verify.sh full`'s clippy step runs without `--features server`, so it never
  compiles that module), and it is recorded rather than changed: editing
  another line's server code during a rebase stabilisation would be an
  unrelated change to a file this milestone does not own.
- **low — the branch and the *Linux Sandbox Admission — Post-Review Corrective
  Addendum* packet both call their first package "C001", and the two differ in
  two recorded places** (§3.10): the disposition of the three
  load-sensitive LSP tests, and the path of the closure record. The evidence
  this milestone produced is the evidence that packet's C001 asks for. Merging
  the two planning structures, or folding this record into that packet's
  closure path, is left to the owner rather than decided here.

## 11. Roadmap disposition

C001 is closed. The branch-and-CI stabilization roadmap has no further
milestone: its completion definition (§11 of the roadmap) is met — the branch
is rebased onto `main` with no conflict markers in the tip, no duplicate
identifier allocations, and no content that either side lost; the canonical
gate passes; and the sweep's result no longer depends on the host for the
defects that were found.

The roadmap's status becomes `closed; C001 closed`.

## 12. Registry updates

- Register the **Branch and CI stabilization** subsystem in the registry's
  active-roadmap table with status `closed; C001 closed`, and its C001 row in
  the closure table with this record.
- **Unblock audit:** this milestone unblocks nothing that was blocked. C005
  remains blocked on its own owner decisions (accepted ADR-0044, the separate
  M003/C010 admission decision, the unqualified tool-program child-job route,
  and the machine-readable supported-host policy) — none of which this
  milestone touched. ADR-0044 remains **proposed**; no host is admitted; the
  C010 no-admission disposition is unchanged. The Linux sandbox workstream's
  own milestones and blockers are unchanged.
- The Linux trusted Git cost corrective's registry row is updated to record
  that the branch it names has been rebased onto `main` and that its
  C007/C023 hosted evidence qualifies the pre-rebase source.
- **The fresh rebase's registry rows (2026-10-21).** Both of this milestone's
  rows — the active-roadmap row and the closure-table row — carry the fresh
  rebase's facts: the new merge base `cb0e79c0`, the second ADR collision and
  ADR-0046, the file-level no-loss proof, the three closed cross-file
  pairings, and the inherited race's fix with its non-vacuity numbers. The
  registry's own reconciliation is recorded in §3.12.1: **64** of `main`'s
  rows gained, **9** stale inherited rows superseded, **2** duplicate
  placements de-duplicated, and no branch row lost.
- **The reconciliation's registry rows (2026-10-21).** `main`'s nine-commit
  advance changed four registry rows in place, and the merge preserved both of
  this milestone's own C001 rows — the active-roadmap row at line 50 and the
  closure-table row at line 648 — unchanged. The registry is **996** lines
  after the merge, the branch's own count. Both C001 rows carry the
  reconciliation's facts and the hosted evidence's run identifiers (§3.16,
  §4.3).
- **The second reconciliation's registry rows (2026-10-21).** `main`'s M006
  census changed four registry rows in place, and the merge preserved both of
  this milestone's own C001 rows unchanged again. Both C001 rows carry the
  second reconciliation's facts — the four re-derived count rows, the twice-
  merged tree's gates, and the second hosted observation (§3.17, §4.3).

## 13. What was not verified

Stated plainly, because a closure that hides its gaps is worse than one that
names them:

- **The rebased head *was* observed in a hosted CI run, and the earlier gap is
  closed.** Run **39551117669** on head `352ed719c` concluded **success** —
  15733 tests run, 15733 passed, 8 skipped — together with the five native
  workflows. The same was then re-observed on the head after the first
  reconciliation, `979ba14c6`: the default sweep **39567534361**, **success**,
  **15755 tests run: 15755 passed, 8 skipped**, and all five native workflows
  **success** (§4.3). This bullet was the milestone's last open gap; it is now
  history, and it is kept as a record of what the gap was rather than deleted.
- **The twice-merged head `e0036d340` — the tree that carries `main`'s tip at
  `84aa47528` — has not been observed in a hosted run.** The second
  reconciliation changed the tree (it brought `main`'s M006 census in), so the
  hosted runs qualify the tree *before* that change. The twice-merged tree was
  verified locally and completely at the time: the workspace sweep (**15771 run,
  15771 passed, 8 skipped**), the feature-enabled tier (**12512 run, 12512
  passed, 7 skipped**), `scripts/verify.sh quick` → **"Quick verification
  passed."**, and `cargo check --workspace --all-targets --locked` → **exit 0**.
  That head has since been superseded by two further merges, and the tree that
  ships — `aba3a559a` — **is** observed in a hosted run (§4.3). This bullet is
  kept as a record of the gap that applied to an intermediate head.
- **The Windows scratch fix (§3.18) is now verified on Windows.** This gap is
  closed: hosted Windows process launcher run **39632392133** on `aba3a559a`
  concluded **success**, and the specific test that failed on the earlier head
  — `scratch::tests::the_session_root_lives_under_the_resolved_cache_root` —
  reports **ok** there (**21 passed, 0 failed**). The same workflow was then
  re-observed **success** on the final head — run **39642334201** on
  `4e4023ed8`. The local proof that the fix is non-vacuous still stands
  alongside it.
- **The four-times-merged tree's full workspace sweep was not run locally.**
  The hosted `CI` job runs the identical command over the identical workspace
  on dedicated runners — **15841 tests run: 15841 passed, 8 skipped** — so the
  full-suite claim for this tree rests on the hosted run, not on a local one.
  The local suite was narrowed to the changed surface (§3.20.1) and its result
  is reported as exactly that, never as a full sweep.
- **The feature-enabled tier was run narrowed.** The canonical feature set —
  `server,plugins,lsp-test-support` — is the one that was compiled, but only the
  binaries covering the branch's changed surface were built and run, rather than
  all ~290 integration binaries. On this tree it is **7541 run, 7541 passed, 4
  skipped** for `codegg` and **1078 run, 1078 passed** for `egglsp`. The tier's
  *full* form — every integration binary under those features — was last green
  at **12512 run, 12512 passed** on the twice-merged head.
- **No native Windows or macOS *test* run.** The Windows evidence is the
  native **probe contract** workflow (a compile-and-probe check on a Windows
  runner); the macOS evidence is a **target compile** on macOS. Neither is a
  native test suite. The tree was verified by running tests on Linux only.
- **The inherited projection-transport race (§3.15) was fixed on the branch's
  tree rather than on `main`'s** — so `main` still carries that race until the
  change is merged.
- **The M004/M005 tier was re-qualified on the rebased head** by the hosted
  runner (§4.3), which this workstation cannot do: its Landlock ABI is below
  6. That gap is closed; what remains is the reconciled head's hosted run
  above.
- **Every published count is now measured against a re-runnable source** (§3.6).
  The three rows this milestone first carried forward are re-derived, and two of
  their published values were stale in `main` as well. The table was measured
  again on 2026-10-21 against the rebased tree: nine further rows moved, two of
  them because the *measure* was wrong rather than the value.
- **The two historical commits with the invalid guard script were recorded,
  not rewritten** (§10).
- **The parallel planning packet's C001 is not merged into this one** (§3.10);
  its two records now describe the same evidence under two names.
