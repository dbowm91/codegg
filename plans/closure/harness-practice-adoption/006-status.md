# Harness Practice Adoption Milestone 006 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/harness-practice-adoption/006-generated-storage-catalog.md`

Source subsystem roadmap:

- `plans/subsystems/harness-practice-adoption-roadmap.md#m006--generated-storage-catalog`

Repository baseline reviewed: `7642b9e38` (`feat-auto-20261109-f8abad0d`), plus
uncommitted working-tree work

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`, on top of HEAD `7642b9e38`.
  `architecture/storage-catalog.md`,
  `crates/codegg-core/src/session/schema/storage_catalog.rs`,
  `docs/storage-table-owners.toml` and `scripts/check_storage_catalog.py` are
  untracked additions; `architecture/storage.md`, `architecture/overview.md`,
  `crates/codegg-core/src/session/schema.rs` (the module declaration) and
  `scripts/check_project_catalog_invariants.py` carry uncommitted edits. Nothing
  below is attributable to a SHA.

The source plan still reads `Status: ready for handoff`, and the roadmap's §12
status table still reads `not started` for M006. **Both are stale and are not
this record's to change** — the plan `Status:` line and the roadmap table are
the parent's.

## 1. Executive finding

The storage surface is now emitted by **running the migration chain**, and both
layers ran green here:

- **The authoritative comparison** — `cargo test -p codegg-core --lib
  storage_catalog`: **6 passed, 0 failed**, 3.65 s. It includes
  `catalog_matches_the_committed_file`, so the committed 3,292-line catalog is
  byte-identical to a fresh generation on this baseline.
- **The cheap structural guard** — `python3 scripts/check_storage_catalog.py`:
  green, **122/122 checks**; `--self-test` rejects all **7** cases.
- **The layout invariant** — `check_project_catalog_invariants.py`: **8/8**,
  including "storage catalog exists and declares the layout version".

The catalog reads: **layout version 95, 95 migrations, 122 tables, 223
indexes.**

**The headline finding stands, and it is the reason the milestone exists.** The
hand-written documentation claimed far less: `architecture/overview.md` recorded
"database tables **95 statements / 91 tables**" (at `:367` and again at
`:402-403`), and `architecture/storage.md` described the surface in prose with no
mechanical tie to the migrations. The real chain yields **122 tables and 223
indexes**. The gap is not a typo: a table's creation can live in a module-owned
schema constant that a migration calls, and no single-file `CREATE TABLE` count
can attribute a table to the migration that introduced it. `CREATE TABLE`
literals appear in `schema.rs` **104** times and in **ten** other
`codegg-core` modules (`collaboration/policy.rs`, `collaboration/store.rs`,
`snapshot/checkpoint.rs`, `work_order/store.rs`, `session_control.rs`,
`migration.rs`, `work_plan/store.rs`, `work_plan/repository_binding.rs`,
`approval.rs`, `session/store.rs`). Running the chain is the only way to know
what schema a given layout version actually has.

One precision about the briefing's wording: a **tree-wide** `CREATE TABLE` grep
*would* see those constants. What it cannot do is say which migration created
which object — that attribution is the catalog's actual product, and it requires
stepping the chain. §7 records this as a correction to the briefing, not a
defect in the milestone.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| The catalog is produced by running the migrations, not by parsing `schema.rs` | `crates/codegg-core/src/session/schema/storage_catalog.rs` steps `super::migrate_and_record(&pool, version)` one version at a time (`:178`) and reads `sqlite_master` plus `PRAGMA table_info` | pass | No Rust parser |
| The generator changes nothing and opens only a private database | `open_private_pool` creates `storage-catalog.sqlite` inside a tempdir; test `the_generator_opens_only_a_private_removed_temporary_database` | pass | No user store touched |
| Attribution: each table and index to the step that introduced it | `Attribution { introduced, columns, indexes }` (`:44-53`), `introduced_label` (`:318`); test `a_known_table_is_attributed_to_the_migration_that_created_it` | pass | |
| An altered column is reported as altered, not introduced twice | `altered` computation at `:211-222`; test `an_altered_column_is_reported_as_altered_not_introduced_twice` | pass | **The real chain at v95 has zero altered columns**, so this branch is exercised only by the test — see §5 |
| Every table has a declared owner | `docs/storage-table-owners.toml`, **122 entries**; test `an_unowned_table_fails_naming_the_table_and_the_entry_to_add` | pass | Criterion 2 |
| An owner entry naming a table that no longer exists is rejected | test `an_owner_entry_for_a_missing_table_is_rejected` | pass | A dropped table cannot leave a stale entry |
| The catalog's layout version equals `STORAGE_LAYOUT_VERSION` | catalog header "Layout version: **95**"; `check_project_catalog_invariants.py` check "storage catalog exists and declares the layout version" | pass | Criterion 3 |
| The committed catalog is byte-identical to a fresh generation | `catalog_matches_the_committed_file` | pass | Criterion 1 |
| The generator is deterministic | The comparison test plus the generator's `BTreeMap`-ordered snapshots and sorted output (`Snapshot { tables: BTreeMap<…>, indexes: BTreeMap<…> }`) | pass | Criterion 1's two-run property |
| The catalog carries a generated header naming its generator and update command | lines 1-2: `AUTO-GENERATED by crates/codegg-core/src/session/schema/storage_catalog.rs` and `Regenerate: CODEGG_UPDATE_STORAGE_CATALOG=1 cargo test -p codegg-core storage_catalog` | pass | |
| The structural guard checks owner, introducing migration, duplicates, header, owner-map staleness | `scripts/check_storage_catalog.py`, 122/122 checks | pass | |
| The structural guard rejects its self-test injections | `--self-test`: 7/7 | pass | Criterion 5 — the plan asked for 5; there are 7 |
| `architecture/storage.md` loses its per-table enumeration and links to the catalog | `architecture/storage.md` modified; the enumeration is replaced by the link | pass | The document keeps store topology and the migration-safety contract |
| `architecture/overview.md`'s schema row names the catalog as its source | `overview.md:596` — "Database tables (`CREATE TABLE`) \| 122 tables / 223 indexes (generated) \| [storage-catalog.md]" | pass | |
| No migration, schema or constant changed by this milestone | No `migrate_vNN` function added; `STORAGE_LAYOUT_VERSION` is 95, which is M002's bump, not this milestone's | pass | Criterion 6 |

## 3. Production implementation evidence

**No production behaviour changed.** The generator is a `#[cfg(test)]` module
(`crates/codegg-core/src/session/schema/storage_catalog.rs`, 625 lines) reached
through the crate's lib test target. It adds no binary, no feature and no
runtime path — the plan's explicit stop condition.

**How attribution works.** The generator snapshots the schema after each
migration step:

1. `open_private_pool` creates a fresh `storage-catalog.sqlite` in a tempdir.
2. For each version `1..=STORAGE_LAYOUT_VERSION`, it calls
   `super::migrate_and_record(&pool, version)` — the **real** migration
   function — and captures `Snapshot { tables, indexes }` from `sqlite_master`
   plus `PRAGMA table_info` per table.
3. A table or index is attributed to the first step at which it appears
   (`:194-196`, `:229-234`).
4. A column is attributed to the step at which it appears in its table, and a
   later step that changes its type or nullability records it as
   `altered` (`:211-222`).
5. The layout version is read from the crate constant, not inferred from the
   last filename.

**The owner map.** `docs/storage-table-owners.toml` holds **122** `table →
module` entries and is the one hand-maintained input. Its header states the two
rules: the generator refuses to render a catalog naming a table with no entry,
and `scripts/check_storage_catalog.py` refuses an entry naming a table the chain
no longer creates. That is a *mapping*, not a description — which is what keeps
it from becoming the document it replaced.

**What the artifact says.** `architecture/storage-catalog.md` is 3,292 lines
with three sections: a migration table (`| Migration | Structural effect |`), a
per-table section (name, owning module, introducing migration, columns with type,
nullability and primary-key position, indexes), and a summary. Its summary is
authoritative: **layout 95, 95 migrations, 122 tables, 223 indexes.**

**The plan's literal command was wrong, and the replacement is recorded.** The
plan's §11 says
`CODEGG_UPDATE_STORAGE_CATALOG=1 cargo test -p codegg-core --test storage_catalog`.
There is **no `storage_catalog` integration target** in either
`crates/codegg-core/Cargo.toml` or the root `Cargo.toml` — the generator must
reach the crate-private `migrate_and_record`, so it can only be a lib test. The
correct forms, which are what the catalog's own header and the guard both name,
are:

```bash
CODEGG_UPDATE_STORAGE_CATALOG=1 cargo test -p codegg-core --lib storage_catalog
cargo test -p codegg-core --lib storage_catalog
```

This is a correction to the plan's text, not a deviation from its intent.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-core --lib storage_catalog
python3 scripts/check_storage_catalog.py
python3 scripts/check_storage_catalog.py --self-test
python3 scripts/check_project_catalog_invariants.py
grep -rln "CREATE TABLE" crates/codegg-core/src/ --include=*.rs
python3 scripts/check_comment_budget.py
```

### Results

- `cargo test -p codegg-core --lib storage_catalog` — **6 passed, 0 failed**,
  1,010 filtered out, 3.65 s:
  `catalog_matches_the_committed_file`,
  `a_known_table_is_attributed_to_the_migration_that_created_it`,
  `an_altered_column_is_reported_as_altered_not_introduced_twice`,
  `an_unowned_table_fails_naming_the_table_and_the_entry_to_add`,
  `an_owner_entry_for_a_missing_table_is_rejected`,
  `the_generator_opens_only_a_private_removed_temporary_database`.
- `check_storage_catalog.py` — green:
  `catalog: 122 tables, 223 indexes, layout version 95` and
  `[PASS] the generated storage catalog is structurally sound. 122/122 checks passed.`
- `check_storage_catalog.py --self-test` — green, **7/7**: a valid catalog
  passes (control); a table with no owner is rejected; an owner entry naming a
  missing table is rejected; a duplicated column is rejected; a mismatched
  layout version is rejected; a hand-edited header is rejected; a catalog owner
  that disagrees with the map is rejected.
- `check_project_catalog_invariants.py` — **8/8**, including "storage layout
  marker tracks highest wired schema migration" and "storage catalog exists and
  declares the layout version".
- `grep -rln "CREATE TABLE" crates/codegg-core/src/ --include=*.rs` — eleven
  files, ten of them outside `schema.rs`; `schema.rs` alone holds **104**
  `CREATE TABLE` occurrences against **122** actual tables. This is the
  measurement behind §1's finding.
- `check_comment_budget.py` — **red, exit 1, 44 violations**, none of them in
  `storage_catalog.rs`, `architecture/storage-catalog.md` or
  `docs/storage-table-owners.toml`. Recorded because it is the parent's to bank
  and because it blocks `quick`.

### Not run

- `cargo test -p codegg-core` in full — **not run.** The plan lists it; this
  record ran the focused `storage_catalog` filter, which is what the milestone's
  own evidence needs. A full `codegg-core` sweep is `verify.sh full` territory
  and was excluded by this session's rules.
- `CODEGG_UPDATE_STORAGE_CATALOG=1 cargo test -p codegg-core --lib storage_catalog`
  — **not run**, deliberately: the comparison already passes, so a regeneration
  would be a no-op with a write risk.
- `cargo fmt --check`, `cargo clippy -D warnings`, `scripts/verify.sh quick`: not
  run. `quick` is red for comment budget and for M001's corpus guard, both
  independent of this milestone.

## 5. Invariant review

- **The catalog is produced by running the migrations.** Step-by-step
  `migrate_and_record`, `sqlite_master` and `PRAGMA table_info`. No source
  parser, so a refactor that moves a `CREATE TABLE` into a helper changes
  nothing about the output.
- **The generator changes nothing.** A private tempdir database, a single
  connection, and a test asserting the database is temporary and removed.
- **Every table has a declared owner.** 122 entries for 122 tables; both
  directions guarded.
- **The catalog's version equals `STORAGE_LAYOUT_VERSION`.** 95 in both, and
  `check_project_catalog_invariants.py` asserts it.
- **Deterministic output.** `BTreeMap` snapshots, sorted tables, declaration-order
  columns, sorted indexes; no timestamp, no path, no environment value.
- **No row data and no sample values.** The generator reads structure only; the
  artifact has no data section.
- **No production change.** No `migrate_vNN`, no constant, no schema edit. The
  layout version is 95 because **M002** added `migrate_v95`, which is why this
  catalog is at 95 and not at the plan's written 94 — the plan was authored
  before M002 landed, and the catalog regenerating at 95 is the mechanism
  working, not a discrepancy.
- **The altered-column branch is currently unexercised by the real chain.**
  `grep -c "altered in v" architecture/storage-catalog.md` returns `0` at layout
  95, while the generator can emit it and a test proves it. That is an honest
  gap in real-data coverage, not a missing feature.

## 6. Failure and recovery review

- **A migration fails mid-chain** — the generator returns the step's error and
  emits no catalog for a partial schema. There is no partial-write path: the
  render happens in memory and the file is written once.
- **A table appears in two steps** — impossible for `CREATE TABLE`; an
  `ALTER TABLE … RENAME` is reported as an alter, which is what the
  altered-column branch exists for.
- **A step is non-idempotent** — not applicable: the chain runs once, in order,
  on a fresh database.
- **Update mode in CI** — the plan requires the test to refuse it. The
  `tool_catalog` family has an explicit CI refusal; the storage side's guard is
  the structural one and the test asserts the comparison, so an update under CI
  would be caught as a dirty artifact rather than refused up front. This is a
  **weaker** posture than M004's and is recorded in §10.
- **Restart** — not applicable; no durable state beyond the committed file.

## 7. Migration and compatibility review

- No schema change and no migration, so no compatibility surface.
- The catalog is additive documentation.
- A future migration must regenerate it; `catalog_matches_the_committed_file`
  forces that in the same change.
- `scripts/check_project_catalog_invariants.py` gained the catalog's
  layout-version assertion, so the layout constant and the artifact cannot drift
  apart silently.

## 8. Security review

- The generator opens only a private tempdir database; a test asserts no path
  under the user's data directory is opened.
- The catalog contains no row data and no sample value — structure only — so a
  committed artifact cannot leak user content.
- No authorization surface: no tool, no capability, no operation.
- The owner map is a table→module mapping, which reveals module names only.

## 9. Documentation and operations

- `architecture/storage-catalog.md` — new, generated, 3,292 lines, with the
  generator and regenerate command in its header.
- `architecture/storage.md` — modified: keeps the store topology, the pooling
  rules, the migration-safety contract and the reason for the layout version;
  loses the per-table enumeration; links to the catalog.
- `architecture/overview.md` — modified: the schema row now reads
  "122 tables / 223 indexes (generated)" and cites the catalog.
- `docs/storage-table-owners.toml` — new, the one hand-maintained input.
- `scripts/check_storage_catalog.py` — wired into `scripts/verify.sh` and
  `.github/workflows/ci.yml`, both `--self-test` and plain.
- `scripts/check_project_catalog_invariants.py` — extended with the catalog's
  layout-version check.
- Operator action: after a migration, run
  `CODEGG_UPDATE_STORAGE_CATALOG=1 cargo test -p codegg-core --lib storage_catalog`,
  add the owner entry for any new table, and commit the diff.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The storage generator has no CI-refusal of update mode, unlike M004's `update_requested(update, ci)`. A `CODEGG_UPDATE_STORAGE_CATALOG=1` run inside CI would rewrite the artifact instead of failing | A CI run could mask a stale catalog by rewriting it. Bounded: the run would leave a dirty tree the job does not commit, and the comparison test would then pass on the rewritten file within that same run | Add the same `CI` refusal the tool catalog uses, and a test for it |
| low | `altered in vNN` has zero real occurrences at layout 95, so the altered-column path is proven only by a synthetic test | A defect in that branch would not be caught by the committed catalog | Accept as an honest gap; note it in the catalog's own prose so a reader does not assume the branch is dead |
| low | `architecture/storage-catalog.md`, its generator, the owner map and the guard are all untracked | The artifact has no history and the guard is in no reviewable diff | Commit with the milestone |
| low | The plan's §11 command names a `--test storage_catalog` target that does not exist | An operator copying the command gets "no test target named `storage_catalog`" | Correct the plan to the `--lib` form (already what the catalog header and the guard name) |
| low | The roadmap §12 table and the plan's own `Status:` line still say `not started` / `ready for handoff` | The roadmap understates delivered work | Parent owns both |

## 11. Roadmap disposition

**Closed.** Every acceptance criterion was met by a command run here:

1. The committed catalog is byte-identical to a fresh generation —
   `catalog_matches_the_committed_file` passed; the generator's ordered
   `BTreeMap` snapshots are what make the two-run property structural rather than
   hoped for.
2. Every table has an owning module and an introducing migration — 122/122 owner
   entries, 122/122 structural checks, and the attribution test.
3. The catalog's layout version equals `STORAGE_LAYOUT_VERSION` — 95 in both,
   guarded.
4. A new migration makes the comparison fail and names the table; regenerating
   with an owner entry makes it pass — both halves exist as tests
   (`an_unowned_table_fails_naming_the_table_and_the_entry_to_add`, and the
   comparison test). The end-to-end injected-migration run is **reported by the
   implementing session**, not reproduced here.
5. The structural guard rejects all five of its self-test injections — it rejects
   **seven**.
6. No migration, schema or constant changed — no `migrate_vNN` was added; the
   catalog's 95 is M002's layout version.

The headline defect this milestone exists to prevent — documentation describing
a schema the chain does not have — is measurably gone: the prose said 95
statements / 91 tables, the chain says 122 tables / 223 indexes, and the artifact
is compared against the chain on every sweep.

## 12. Registry updates

Required in `plans/registry.md` and
`plans/subsystems/harness-practice-adoption-roadmap.md`:

- §12's M006 row: `not started` → `closed`, closure record
  `plans/closure/harness-practice-adoption/006-status.md`, blocker column `—`.
- The registry's milestone table entry for M006, if `plans/registry.md` carries
  one.
- The count reconciliation already applied to `architecture/overview.md`
  (122 tables / 223 indexes, layout 95) needs no further roadmap change.
- **Not updated by this record:** the source plan's `Status:` line. It stays
  `ready for handoff` until the parent changes it.
