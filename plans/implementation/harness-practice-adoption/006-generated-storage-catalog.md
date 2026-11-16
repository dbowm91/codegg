# Harness Practice Adoption Milestone 006 — Generated Storage Catalog

Status: closed

Repository baseline: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`
(branch `feat/auto-20261109-f8abad0d`)

Source roadmap:

- `plans/subsystems/harness-practice-adoption-roadmap.md` §7 M006, §8

Long-term requirements:

- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
  — restart-safe migrations, coordinated with the layout version
- `plans/001-terminology-and-domain-model.md#3-deployment-terms`
- `plans/003-planning-process.md`
- `architecture/storage.md`, `architecture/session.md`

Applicable ADRs:

- **None.** Documentation and a test. No schema change, no migration, no
  production behaviour.

Primary class: infrastructure

## 1. Objective

The storage surface — every table, column, index, the migration that introduced
each, and the layout version — is **emitted by running the real migration
chain**, committed as a catalog, and checked on every sweep.

## 2. Why this milestone is ready

- No hard dependency.
- The migration chain is single and ordered in
  `crates/codegg-core/src/session/schema.rs`, and `STORAGE_LAYOUT_VERSION` in
  `crates/codegg-core/src/storage/mod.rs` is already tied to the highest
  migration by `scripts/check_project_catalog_invariants.py`.
- The test helper for a migrated database exists: `isolated_pool()` runs the
  migrations inside the pool it returns, and tests are already forbidden from
  calling `migrate()` separately.
- The generated-asset pattern and the catalog test pattern are established by
  M004; this milestone reuses the shape.

## 3. Current implementation evidence

**The storage surface is documented by hand.** `architecture/storage.md`
describes the user-scoped catalog and the project store, the migration chain,
and the layout version. Nothing ties the described tables to the migration that
creates them.

**The chain is at v94.** `STORAGE_LAYOUT_VERSION` is `94` and the highest
migration is `migrate_v94`. `scripts/check_project_catalog_invariants.py`
already enforces that the constant tracks the highest migration, so the catalog
has an authoritative version to agree with.

**Migrations are additive and ordered.** Each `migrate_vNN` takes a transaction
and creates or alters schema. That is exactly the property a per-migration
attribution needs: running the chain incrementally and diffing after each step
attributes every object to the step that introduced it.

**No table carries a declared owner.** The architecture document groups tables
by concern in prose. A table added without a documentation update is invisible
today.

## 4. Invariants that must not regress

- **The catalog is produced by running the migrations**, not by reading
  `schema.rs`. A parser over Rust source would miss a table created by a
  conditional path and would break on a refactor.
- **The generator changes nothing.** It runs against an in-memory or isolated
  database; it never opens a user's store and never writes a file outside the
  catalog under update mode.
- **Every table has a declared owner.** An unowned table fails the guard. This
  is the one hand-maintained input, and it is a *mapping*, not a description:
  `table → owning module`.
- **The catalog's version equals `STORAGE_LAYOUT_VERSION`.** The existing
  invariant guard is extended to assert it.
- **Deterministic output.** Two runs on one baseline are byte-identical: sorted
  tables, sorted columns in declaration order, sorted indexes.
- **No row data and no sample values.** The catalog describes structure only.
- **No production change.** No migration, no schema, no constant changes.

## 5. Scope

### In scope

- A test-only generator that runs the chain incrementally and emits the catalog.
- A committed catalog at `architecture/storage-catalog.md`.
- An update mode (`CODEGG_UPDATE_STORAGE_CATALOG=1`) and a comparison mode.
- A declared `table → owning module` map.
- An extension to `scripts/check_project_catalog_invariants.py` and a new
  structural guard with a self-test.

### Explicitly out of scope

- A schema diagram, an ER graph, or foreign-key analysis beyond declared
  references.
- Row counts, sample rows, or query plans.
- Changing any migration, index, or column.
- A second binary target. As with M004, the generator is a test.
- The user-scoped catalog's *content*; the catalog describes schema, not data.

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None. This milestone reads the chain and describes it.

### Protocol and DTOs

None.

### Runtime and concurrency

None. The generator runs migrations in a private database.

### Frontend or operator surface

None.

### Security and authorization

- The generator must not touch a real store. A test asserts the generator's
  database path is temporary and removed, and that no path under the user's data
  directory is opened.

### Documentation and static guards

- `architecture/storage-catalog.md` — generated, with a header naming the
  generator and the update command.
- `architecture/storage.md` — keeps its prose and links to the catalog.
- `scripts/check_project_catalog_invariants.py` — extended to assert the
  catalog's version.
- `scripts/check_storage_catalog.py` — the structural guard, with `--self-test`.

## 7. Ordered work packages

### Work package A — Incremental attribution

Intent: know which migration introduced each object, from the migrations
themselves.

Required changes:

- Run the chain step by step against a private database, capturing
  `sqlite_master` (tables, indexes) and `pragma table_info` (columns, types,
  nullability, primary-key position) after each step.
- Attribute a table or index to the first step at which it appears; attribute a
  column to the step at which it appears in its table; record a column that
  changes type or nullability as `altered in vNN`.
- The layout version is read from the crate constant, not inferred from the last
  filename.

Acceptance evidence:

- Every table in the final schema has exactly one introducing step.
- A test asserts a known table's introducing step matches its migration.
- A test asserts an altered column is reported as altered rather than as
  introduced twice.

### Work package B — The owner map and the catalog

Intent: a table cannot exist undocumented.

Required changes:

- A declared map from table name to owning module. It is the only
  hand-maintained input.
- The renderer emits, per table: name, owning module, introducing migration,
  columns (name, type, nullability, primary-key position), and indexes.
- `architecture/storage-catalog.md` with a generated header.
- A comparison test with the update mode.

Acceptance evidence:

- The catalog regenerates byte-identically twice.
- Adding a migration that creates a table makes the test fail and names the
  table and the missing owner entry.
- Adding the owner entry and regenerating makes it pass.

### Work package C — Guards

Intent: the version cannot drift and the owner map cannot go stale.

Required changes:

- Extend `scripts/check_project_catalog_invariants.py`: the catalog's declared
  layout version must equal `STORAGE_LAYOUT_VERSION`, and the catalog must
  exist.
- `scripts/check_storage_catalog.py`: every table in the catalog has an owner
  and an introducing migration; no duplicate table; no duplicate column within a
  table; the header carries the generated marker; every entry in the owner map
  names a table that exists (so a dropped table does not leave a stale entry).
- `--self-test` injects: a table with no owner, an owner naming a missing table,
  a duplicated column, a mismatched layout version, and a hand-edited header —
  and proves each is rejected.

Acceptance evidence:

- All five injections are rejected.
- Both guards are green.

### Work package D — Documentation reduction

Intent: the hand-written document stops restating the schema.

Required changes:

- `architecture/storage.md` keeps the store topology, the pooling rules, the
  migration-safety contract, and the reason for the layout version; it loses any
  per-table enumeration and links to the catalog.
- `architecture/overview.md`'s schema row names the catalog as its source.

Acceptance evidence:

- The document contains no table enumeration.
- Every table it still mentions resolves in the catalog.

## 8. Failure, cancellation, restart, and contention semantics

- **A migration fails mid-chain.** The generator fails naming the step. It must
  not emit a catalog for a partial schema.
- **A table appears in two steps.** Impossible for a `CREATE TABLE`; if an
  `ALTER TABLE` renames, the rename is reported as an alter, not as a second
  introduction.
- **A step is non-idempotent.** Not applicable: the chain is run once, in order,
  on a fresh database.
- **Update mode in CI.** The test asserts update mode is not set in CI.
- **Restart.** Not applicable.

## 9. Compatibility and migration

- No schema change and no migration, so no compatibility surface.
- The catalog is additive documentation.
- A future migration must regenerate it; the comparison test forces that in the
  same change.

## 10. Required tests

### Focused unit tests

- The renderer is deterministic across two runs.
- Attribution: a table's introducing step, an altered column, an added index.
- An unowned table fails the comparison with the table's name.

### Integration tests

- The comparison test passes on the committed catalog.
- The comparison test fails after an injected migration and names the table.

### Restart and recovery tests

- Not applicable.

### Contention and cancellation tests

- Not applicable. The test asserts update mode is refused under CI.

### Security and negative tests

- The generator opens no path under the user's data directory.
- The catalog contains no row data and no sample value.

### Migration and compatibility tests

- The layout-version assertion in `check_project_catalog_invariants.py`.

## 11. Required verification commands

```bash
# the crate and the migration chain
cargo test -p codegg-core
# A lib test, not a `--test` target: the generator must reach the private
# `migrate_and_record`, so `--test storage_catalog` selects nothing.
CODEGG_UPDATE_STORAGE_CATALOG=1 cargo test -p codegg-core storage_catalog
cargo test -p codegg-core storage_catalog

# the guards
python3 scripts/check_project_catalog_invariants.py
python3 scripts/check_storage_catalog.py
python3 scripts/check_storage_catalog.py --self-test

# formatting and linting
cargo fmt --check
cargo clippy -p codegg-core --tests -- -D warnings

# the routine set
scripts/verify.sh quick
```

The closure record states which commands ran.

## 12. Documentation updates

- `architecture/storage-catalog.md` — new, generated.
- `architecture/storage.md` — enumeration removed, link added.
- `architecture/session.md` — if it enumerates tables, it links to the catalog.
- `architecture/overview.md` — the schema row names its source.
- `plans/subsystems/harness-practice-adoption-roadmap.md` §12 — status only.

## 13. Acceptance criteria

1. The committed catalog is byte-identical to a fresh generation on the closure
   baseline, twice.
2. Every table has an owning module and an introducing migration.
3. The catalog's layout version equals `STORAGE_LAYOUT_VERSION`.
4. A new migration that creates a table makes the comparison test fail and name
   the table; regenerating with an owner entry makes it pass.
5. The structural guard rejects all five of its self-test injections.
6. No migration, schema, or constant changed, proven by the diff.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- the chain cannot be run incrementally without changing production code — the
  attribution must come from the real chain or not exist;
- a step's effect cannot be observed from `sqlite_master` and `pragma`
  (for example a data backfill that changes no structure) — report it as a
  migration with no structural effect rather than inventing a description;
- the owner map would need to be derived, which would make it a second parser of
  the schema;
- the generator needs to open a real store.

## 15. Closure evidence required

- The committed catalog, its table count, and the two-run determinism result.
- The attribution table for a known table and a known altered column.
- The injected-migration failure and the regenerated pass, with the diff.
- Both guards' output, and the structural guard's `--self-test` with all five
  injections rejected.
- The diff proving no migration or schema changed.
- The named non-claim: the catalog describes structure, not data, and it is not
  a migration plan.

## 16. Handoff notes

- **Run the chain; do not parse the source.** A parser is a second
  implementation of the migrations and will be wrong first.
- **The owner map is a mapping, not prose.** One line per table. If it starts
  describing behaviour, it has become the document it replaced.
- **Do not add a second binary.** The generator is a test with an update mode,
  exactly as M004's is.
- **Deterministic ordering matters more than pretty grouping.** A diff a
  reviewer can read is the whole value.
- **A migration with no structural effect is a real answer.** Say so; do not
  omit the step.
