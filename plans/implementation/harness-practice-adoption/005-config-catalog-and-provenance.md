# Harness Practice Adoption Milestone 005 — Configuration Catalog and Provenance

Status: closed

Repository baseline: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`
(branch `feat/auto-20261109-f8abad0d`)

Source roadmap:

- `plans/subsystems/harness-practice-adoption-roadmap.md` §7 M005, §8

Long-term requirements:

- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
  — one catalog per surface; no second copy
- `plans/000-long-term-specification.md#28-observability`
- `plans/001-terminology-and-domain-model.md#3-deployment-terms`
- `plans/003-planning-process.md`
- `architecture/config.md` — the document this milestone makes generated

Applicable ADRs:

- **None.** A read-only CLI surface over the existing loader, plus generated
  documentation. No new authority, no protocol, no storage, no dependency.

Primary class: capability

## 1. Objective

`codegg config show --provenance` names, for every effective key, the layer that
won it — and the configuration surface is documented by a catalog derived from
the schema's own type, so a key cannot exist in one and not the other.

## 2. Why this milestone is ready

- No hard dependency.
- The loader is a single pipeline with a documented order:
  resolve paths → load each file → merge → migrate → validate. A provenance
  surface reads that pipeline; it does not reimplement it.
- The merge's shape is already classified and guarded: field-by-field for some
  sections, whole-value replacement for others, and
  `scripts/check_config_merge_coverage.py` already fails the build when a
  section is added without merge coverage.
- The CLI has a `Commands` enum and an `OutputFormat` (`Text` | `Json`) that a
  subcommand can reuse.
- The catalog has a compile-time exhaustiveness property available without
  reflection: a Rust struct literal that names every field of `Config` fails to
  compile when a field is added, so a fully-populated `Config` is a faithful key
  source.

## 3. Current implementation evidence

**Four layers, two precedence shapes.** `architecture/config.md` documents the
discovery order — `CODEGG_TUI_CONFIG`, system config, global config, project
config — and the merge strategies: field-by-field for `provider`, `server`,
`watcher`, `search`, `discovery`, `eggwork` and eleven optional sections, and key
replacement for `agent`, `mcp`, `commands`, `mode`, `model_profile` and
`model_routers`. `crates/codegg-config/src/paths.rs` states the same two shapes
in a comment and names the guard that keeps them covered.

**The guard exists and is load-bearing.**
`scripts/check_config_merge_coverage.py` fails the build if a section is added
whose fields are silently discarded by the merge. That guard is the reason this
milestone's catalog can be trusted: the merge is proven exhaustive before the
catalog describes it.

**There is no way to ask which layer won.** The `Commands` enum has `Doctor`,
`Auth`, `Daemon`, `Edit`, `Exec`, `Completions`, `Acp` and `ToolAdvisor`. There
is no `config` subcommand, and no code path records the origin of a value.

**The documented surface is hand-written.** `architecture/config.md` enumerates
the sections and strategies by hand. Nothing ties the enumeration to
`crates/codegg-config/src/schema.rs`.

**A generated-asset pattern exists.** `scripts/generate_builtin_agents.py` and
`scripts/generate_subagent_names.py` each own a `generated.rs` and offer
`--check`. As with M004, the *pattern* is reused and the mechanism is not: this
milestone's input is a Rust type, so the comparison is a test.

## 4. Invariants that must not regress

- **One loader.** The provenance surface calls the real pipeline. A parallel
  loader that re-merges documents is a second source of truth, and the two will
  disagree the first time a strategy changes.
- **Provenance describes the value that was used.** `config show` prints the
  values `Config::load()` produced, not a re-merge's. A test asserts equality
  key-for-key.
- **The catalog is derived from the type, not from prose.** A field added to
  `Config` breaks the build until the catalog's source value is updated.
- **A merge whose semantics are not "last layer containing the key" is declared.**
  The provenance rule is correct for last-wins and union-of-keys merges. Any
  merge that concatenates, intersects, or transforms must be listed as an
  exception with its own attribution rule; an undeclared exception is a wrong
  answer.
- **Secrets are never printed.** `config show` prints a value for every key
  except the ones the schema marks as credentials; for those it prints
  `(set)` or `(unset)` and the provenance layer. This is not optional
  formatting — a dump that prints an API key is a disclosure defect.
- **The subcommand is read-only.** It writes no file, reloads no watcher, and
  takes no lock beyond reading the store.
- **No behaviour change.** The loader's results are identical with and without
  provenance capture, proven by a test that loads the same documents both ways.

## 5. Scope

### In scope

- A provenance engine that attributes each effective key to a layer.
- `codegg config show [--provenance] [--format json]`.
- A generated catalog at `architecture/config-catalog.md`.
- A test-only fully-populated `Config` value whose construction is
  compile-time-exhaustive.
- An extension to the existing merge-coverage guard and a new provenance guard.

### Explicitly out of scope

- Changing any merge strategy, default, or discovery rule.
- A write path. `config set` is not in this milestone.
- A per-key catalog of descriptions. The catalog lists keys, defaults and
  strategies; the prose in `architecture/config.md` keeps the *why*.
- A daemon surface. This is a CLI surface over the config crate; it adds no
  `CoreRequest` variant.
- Reloading the watcher or invalidating caches. The command reads and exits.
- Adding a reflection or schema-generation dependency.

## 6. Required production changes

### Core/domain

- A `ConfigLayer` identity (`EnvOverride`, `System`, `Global`, `Project`) with
  its path, already available from `resolve_config_paths`.
- A provenance engine in `codegg-config`:
  1. resolve the paths;
  2. load each document;
  3. fold the layers one at a time through the **real** `merge_configs`,
     serializing the accumulated `Config` after each fold;
  4. attribute a key to the last layer whose raw document contains that key
     path, and cross-check against the fold in which the serialized value last
     changed.
  The cross-check is the mechanism: the raw-document rule gives the answer, the
  value-diff detects a merge whose semantics are not last-wins.
- A declared strategy table: one entry per `Config` section, either
  `last_containing` or a named exception. The merge-coverage guard is extended
  to require every section in `Config` to have an entry.

### Storage and migrations

None.

### Protocol and DTOs

None. The output is a CLI document; `OutputFormat::Json` reuses the existing
enum.

### Runtime and concurrency

- The engine runs once at command start, in the CLI process. No daemon
  interaction, no lock, no watcher.
- A layer file that fails to parse is reported as a layer with an error, not
  skipped: "the project layer is broken" is the most useful answer a provenance
  dump can give.

### Frontend or operator surface

- `Commands::Config { command: ConfigSubcommand }` with
  `Show { provenance: bool, format: OutputFormat }`.
- Text output groups by layer, then by key path, and prints the effective value
  with its winning layer; shadowed values are printed beneath it.
- JSON output is a stable shape: an array of `{ path, value, layer, shadowed }`
  entries, bounded and deterministically ordered.

### Security and authorization

- Credential-shaped keys print `(set)` / `(unset)` and never the value. The
  list of such keys is derived from the schema's credential fields, not from a
  name heuristic.
- The command requires no capability beyond reading the config files the process
  already reads.

### Documentation and static guards

- `architecture/config.md` keeps its prose and links to the generated catalog.
- `architecture/config-catalog.md` is generated.
- `scripts/check_config_merge_coverage.py` is extended with the strategy-table
  requirement.
- `scripts/check_config_provenance.py` with `--self-test`.

## 7. Ordered work packages

### Work package A — The catalog source and the catalog

Intent: a key list that cannot drift from the type.

Required changes:

- A test-only constructor that builds a `Config` with **every** field set to a
  distinguishable sentinel, using a struct literal. Adding a field to `Config`
  without adding it here is a compile error — that is the exhaustiveness
  property, and the plan says so in the code's own comment.
- A renderer that walks the serialized sentinel config and emits, per key path:
  the key, its default from `Config::default()` (or `—` when the default omits
  it), and its section's strategy from the declared table.
- `architecture/config-catalog.md` with a generated header naming the generator
  and the update command.
- A comparison test with `CODEGG_UPDATE_CONFIG_CATALOG=1` to rewrite and a
  default mode to compare.

Acceptance evidence:

- The catalog regenerates byte-identically twice.
- Adding a field to `Config` without updating the sentinel constructor fails to
  compile.
- Adding a field to the sentinel constructor changes the catalog and fails the
  comparison until regenerated.

### Work package B — The provenance engine

Intent: the answer, using the real merge.

Required changes:

- Fold the layers one at a time through `merge_configs`, serializing after each
  fold.
- Attribute each leaf key to the last layer whose raw document contains that
  path.
- Compare that attribution with the fold in which the serialized value last
  changed. A disagreement is either a declared exception or a defect; the
  engine reports it as an internal error naming the key and the two layers, and
  never guesses.
- Declare the strategy table, with exceptions for any merge that is not
  last-wins.

Acceptance evidence:

- A three-layer fixture where a field-by-field section has different keys set by
  different layers attributes each key to the right layer.
- A three-layer fixture where a whole-value-replace section is set by two layers
  attributes every key in it to the later layer.
- A deliberately non-last-wins merge, introduced in a test-only section, is
  reported as an undeclared exception rather than silently mis-attributed.

### Work package C — The subcommand

Intent: the operator surface.

Required changes:

- `Commands::Config` with `Show`.
- Text and JSON renderers over the engine's output, with credential keys
  redacted to `(set)`/`(unset)`.
- A broken layer prints its parse error and its path, and still prints the
  layers that did parse.

Acceptance evidence:

- `config show` values equal `Config::load()` values for every key, asserted
  key-for-key in a test.
- `config show --provenance` names a real path for each of the four layers when
  all four exist.
- A credential key prints `(set)` and its value appears nowhere in the output.

### Work package D — Guards

Intent: the table stays exhaustive and the engine stays honest.

Required changes:

- Extend `scripts/check_config_merge_coverage.py`: every `Config` section must
  have a strategy entry, and an exception must name the attribution rule it
  uses.
- `scripts/check_config_provenance.py` verifies: the catalog exists and carries
  its generated header; every section in the catalog has a strategy; the
  subcommand's help text is registered; and no credential-shaped key appears
  with a value in a captured output fixture.
- `--self-test` injects: a section missing from the strategy table, a catalog
  row with no strategy, a credential key with a value, and a hand-edited header
  — and proves each is rejected.

Acceptance evidence:

- All four injections are rejected.
- Both guards are green.

## 8. Failure, cancellation, restart, and contention semantics

- **A layer file is unreadable or malformed.** The layer is reported with its
  path and error; the other layers still produce a dump. The exit status is
  non-zero so a script cannot mistake a partial dump for a complete one.
- **No layer files exist.** The dump reports the defaults and states that no
  layer was found. It does not print an empty document.
- **A layer sets a key to the same value as a previous layer.** The key is
  attributed to the later layer, because it is present there. The value is
  unchanged, which is what the operator asked about.
- **A merge is not last-wins and is not declared.** The engine reports an
  internal error naming the key; it does not pick one layer.
- **Restart and contention.** Not applicable: the command reads files and exits.

## 9. Compatibility and migration

- The catalog is additive documentation.
- The subcommand is additive; no existing command's output changes.
- `OutputFormat` is reused, so the JSON shape follows the existing convention
  for `exec --format json`.
- No configuration key, default, or discovery rule changes, so a user's existing
  configuration behaves identically.
- The merge-coverage guard gains a requirement; an existing section without a
  strategy entry is a build failure, which is the point.

## 10. Required tests

### Focused unit tests

- The sentinel constructor's serialization contains every section.
- The attribution rule for a field-by-field section across three layers.
- The attribution rule for a whole-value-replace section across three layers.
- An undeclared non-last-wins merge is reported, not guessed.

### Integration tests

- `config show` equals `Config::load()` key-for-key.
- `--provenance` names a real path per layer.
- A broken layer is reported and the exit status is non-zero.

### Restart and recovery tests

- Not applicable; the command reads and exits.

### Contention and cancellation tests

- Not applicable.

### Security and negative tests

- A credential key prints `(set)`/`(unset)` and its value is absent from both
  text and JSON output.
- A config file containing a secret does not leak it into the dump.

### Migration and compatibility tests

- The catalog comparison test is the migration gate: a schema change must
  regenerate it.

## 11. Required verification commands

```bash
# the crate
cargo test -p codegg-config
CODEGG_UPDATE_CONFIG_CATALOG=1 cargo test -p codegg-config --test config_catalog
cargo test -p codegg-config --test config_catalog

# the CLI
# The CLI surface tests are the bin target's `cli_surface_tests` module; the
# `--lib -- cli_surface` form this plan originally gave selects zero tests.
cargo test -p codegg --bin codegg -- cli_surface_tests
cargo run -- config show --provenance --format json | head

# the guards
python3 scripts/check_config_merge_coverage.py
python3 scripts/check_config_provenance.py
python3 scripts/check_config_provenance.py --self-test

# formatting and linting
cargo fmt --check
cargo clippy -p codegg-config -p codegg --tests -- -D warnings

# the routine set
scripts/verify.sh quick
```

The closure record states which commands ran.

## 12. Documentation updates

- `architecture/config-catalog.md` — new, generated.
- `architecture/config.md` — links to the catalog; keeps the discovery order and
  the merge rationale; loses any hand-maintained key enumeration.
- `architecture/overview.md` — the config row names the catalog as its source.
- `docs/` user-facing configuration page, if one enumerates keys, links to the
  catalog instead.
- `plans/subsystems/harness-practice-adoption-roadmap.md` §12 — status only.

## 13. Acceptance criteria

1. `codegg config show` prints the same value as `Config::load()` for every key,
   asserted in a test.
2. `--provenance` names a real path for each layer that exists, and the layer
   named for each key is the one whose document contains that key.
3. A field-by-field section with keys from two layers attributes each key to its
   own layer.
4. A whole-value-replace section attributes every key to the later layer.
5. A credential key prints `(set)`/`(unset)` and its value appears in neither
   output format.
6. The catalog regenerates byte-identically, and adding a `Config` field without
   updating the sentinel constructor fails to compile.
7. Both guards are green and the provenance guard rejects all four injections.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- the provenance engine cannot reuse the real `merge_configs` and would need a
  second merge implementation — that is the defect this milestone exists to
  avoid;
- a merge's semantics cannot be expressed as an attribution rule, and the
  exception list would have to grow without bound;
- the catalog cannot be generated without a reflection dependency — the
  sentinel-constructor approach exists for that reason, and a new dependency is
  an owner decision;
- printing a value would disclose a credential and the schema does not mark the
  key as one — stop and report the unmarked key rather than adding a name
  heuristic;
- the subcommand needs a daemon request to answer.

## 15. Closure evidence required

- The committed catalog, its section count, and the two-run determinism result.
- The `config show` equality test output.
- The provenance fixtures' attribution tables for both strategy shapes.
- The undeclared-exception test output.
- The credential-redaction test output for both formats.
- Both guards' output, and the provenance guard's `--self-test` with all four
  injections rejected.
- The named non-claim: the catalog lists keys and strategies, not descriptions,
  and it is not a validator.

## 16. Handoff notes

- **Do not write a second merge.** The value of this milestone is that the
  answer comes from the code path that produced the value.
- **The sentinel constructor is the exhaustiveness proof.** It is a test-only
  `Config` literal; keep it a literal. A `..Default::default()` would defeat the
  compile-time check, which is the entire mechanism.
- **Declare exceptions rather than widening the rule.** If a merge is not
  last-wins, name it. A silently mis-attributed key is worse than no provenance
  at all, because it is trusted.
- **Redact by schema, not by name.** `api_key` is a name; the schema's credential
  marker is the fact.
- **A broken layer is a result, not a crash.** Print it, keep going, exit
  non-zero.
