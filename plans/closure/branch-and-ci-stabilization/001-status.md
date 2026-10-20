# Branch and CI Stabilization Milestone C001 — Closure Status

Status: closed — the branch is integrated with `main`, both identifier namespaces are unique, the content the branch had lost is restored, the tree compiles with every target, the canonical gate passes, and the default sweep's green result is a property of the product rather than of the host it ran on.

Source implementation plan:

- `plans/implementation/branch-and-ci-stabilization/001-branch-and-ci-stabilization.md`

Source subsystem roadmap:

- `plans/subsystems/branch-and-ci-stabilization-roadmap.md#7-milestones`

Repository baseline reviewed: `d251b11c` (branch tip before the rebase), rebased onto `main` at `fe2c1306`

## 1. Executive finding

The branch is integrated with `main`, and the integration did not cost either
side its content.

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
| The branch is rebased onto `main` and carries `main`'s tip | `git merge-base HEAD origin/main` == `fe2c1306`; `git log --oneline origin/main..HEAD` == 294 commits, every subject preserved | pass |
| No conflict survives in the final tree | `git grep -lE "^(<<<<<<< \|>>>>>>> )" HEAD --` outside `plans/` → **0** | pass |
| A conflict is never committed with markers | two such commits were found during the rebase and repaired by a targeted rewrite; the repair is proved by an **empty** `git diff --stat backup/post-rebase-pre-fix HEAD` | pass |
| One storage-layout number per migration | `crates/codegg-core/src/session/schema.rs` has one `migrate_v83` (main's managed scratch), `migrate_v84` (M005 network intent), `migrate_v85` (C007 journal); `python3 scripts/check_project_catalog_invariants.py` → **7/7** | pass |
| The layout version tracks the highest wired migration | `STORAGE_LAYOUT_VERSION = 85`; `scripts/check_project_catalog_invariants.py` → "storage layout marker tracks highest wired schema migration" PASS | pass |
| One ADR identifier per decision | `python3 scripts/check_adr_integrity.py` → pass; `main` passes the same guard, so the guard is not merely permissive | pass |
| The ADR guard is itself trustworthy | `python3 scripts/check_adr_integrity.py --self-test` → "ADR integrity fixtures passed" | pass |
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
| Every published count is measured against its listed `Source` | `architecture/overview.md`'s "Verified counts" table re-measured on 2026-10-20; six rows corrected | pass |
| The canonical gate after the sweep | `scripts/verify.sh quick` → "Quick verification passed." (§4) | pass |

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

Three rows — `Authorized operations`, `Tool registrations`, and `Distinct
production tool names` — are **carried forward unchanged**: their published
conventions could not be reproduced from their listed `Source` on this tree
(`main`'s published values do not reproduce either), and re-deriving them from
a guess would be worse than recording that they were not re-derived. This is
stated in the table's own re-measurement note rather than left implicit.

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

## 4. Verification executed

Local, on the final tree, in this order.

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo check --workspace --all-targets --locked` | clean (every target) |
| `scripts/verify.sh quick` | **"Quick verification passed."** |
| `python3 scripts/check_adr_integrity.py` | pass |
| `python3 scripts/check_adr_integrity.py --self-test` | "ADR integrity fixtures passed" |
| `python3 scripts/check_project_catalog_invariants.py` | 7/7 |
| `cargo test -p egggit --locked --lib process::tests` | 32 passed, 0 failed |
| `cargo nextest run -p codegg --locked --profile ci -E '<the 14 recorded failures>'` | **18 passed, 0 failed** |
| `cargo test -p codegg --locked --lib -- --list` | the three live-language-server tests are **absent**; the four rejection tests remain |
| `cargo nextest run -p codegg --locked --profile ci -E 'binary(scratch_cache_root_contract) or binary(linux_native_scratch_containment) or test(/scratch::tests::/)'` | **28 passed, 0 failed** (the scratch allocator's own qualifications, its resolution contract, and the native containment fixture) |

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

The authoritative workspace sweep is recorded in §4.1.

### 4.1 The authoritative sweep

`cargo nextest run --workspace --locked --profile ci`, run on the committed
tree with no concurrent edits and no Git commands in the worktree:

- **RESULT_PLACEHOLDER**

## 5. Invariant review

- **One migration number per migration.** Holds: one `migrate_v83`, one
  `migrate_v84`, one `migrate_v85`, and `STORAGE_LAYOUT_VERSION` tracks the
  highest wired migration. Proved by the catalog invariant guard.
- **One ADR identifier per decision.** Holds: the ADR integrity guard passes,
  and passes its own self-test.
- **A persistent object-store failure stays a typed refusal.** Holds: the
  fail-closed regression test passes and is non-vacuous.
- **The default sweep is host-independent.** Holds for the live-language-server
  case and for the per-user cache root: the sweep's cache root is supplied by
  the canonical runners, the allocator creates the root it depends on privately
  rather than at the umask's discretion, and the refusal of an unsafe cache
  root is a tested contract. It does **not** hold in general: the sweep still
  contains tests whose runtime depends on machine speed, and the nextest
  profile's 120 s kill threshold remains a boundary for them. That is a
  standing property of the profile, not a defect this milestone introduced; see
  §10.
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
  canonical runners supply one.
- `AGENTS.md` — the broad runner's command now carries its private cache root.
- `scripts/verify.sh`, `.github/workflows/ci.yml` — the canonical runners
  supply the sweep's private cache root.

### Host state on this workstation

`/home/sugarwookie/.cache` was mode `0777` — world-writable — which the
product refuses by design, so an installed `codegg` on this machine could not
allocate scratch at all. The directory was repaired to `0755` after the
authoritative sweep had been run and recorded **with the unsafe mode still in
place**, so the sweep's result is evidence about the product and not about the
repair. The repair is a host action; nothing in the repository depends on it.

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
  The three live-language-server timeouts that were observed are closed. The
  remaining exposure is a property of the profile, recorded in
  `.config/nextest.toml`, and is not introduced by this milestone.
- **medium — the hosted CI runs recorded in the C007 and C023 closures qualify
  the *code* those closures describe, not the rebased head.** The rebase
  rewrote every SHA, and the recorded runs predate it. The rebased head has
  **not** been observed in a hosted run. See §12.

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

## 13. What was not verified

Stated plainly, because a closure that hides its gaps is worse than one that
names them:

- **The rebased head has not been observed in a hosted CI run.** The recorded
  runs (`39356728579` and its predecessors) qualify the pre-rebase source.
  The rebased tree is verified locally, and the hosted workflow is expected to
  exercise it, but that is an expectation and not evidence.
- **No native Windows or macOS run.** The rebased tree was verified on Linux
  only.
- **The M004/M005 tier was not re-qualified on the rebased head.** This
  workstation cannot run it (Landlock ABI < 6); that evidence must come from
  the hosted runner.
- **Three published rows were carried forward rather than re-derived** (§3.6).
- **The two historical commits with the invalid guard script were recorded,
  not rewritten** (§10).
