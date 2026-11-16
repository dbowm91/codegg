# Harness Practice Adoption Roadmap

Status: conditionally closed — M004–M007 closed; M001–M003 carry named outstanding evidence in their closure records

Repository baseline reviewed: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`
(branch `feat/auto-20261109-f8abad0d`)

Long-term references:

- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`
  — durable replay beyond a bounded in-memory buffer, snapshot/event
  equivalence, and replay after reconnect and daemon restart
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
  — versioned operations, bounded payloads, restart-safe migrations
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#6-session-and-interaction-terms`
- `plans/001-terminology-and-domain-model.md#12-audit-and-observability-terms`
- `plans/003-planning-process.md`
- `architecture/testing.md`, `architecture/session.md`, `architecture/tool.md`,
  `architecture/config.md`, `architecture/storage.md`

Provenance:

This line adopts six **practices** observed in `deepseek-ai/deepseek-harness`
(`dsh`), an MIT-licensed TypeScript agent harness in developer preview. **No
dsh source, dependency, protocol, package boundary, or vocabulary is
adopted.** Each milestone restates one practice against a CodeGG contract that
already exists, and where the two disagree CodeGG's decision is kept and the
disagreement is recorded in the plan's §5 rather than resolved by copying.

The practices were selected by one criterion: CodeGG has a *named* gap the
practice closes, and closing it cannot make an existing path worse. The
practices CodeGG already implements — and the ones it deliberately refuses —
are in §3, because a roadmap that omits them invites a later agent to
"adopt" something the tree already has.

Related ADRs:

- **None required as written.** Two milestones carry a named stop condition
  that would require one: **M002** if the ledger cannot be expressed as an
  additive session-store row (a new `CoreRequest` variant or a field outside
  ADR-0053's decided projection set is a protocol decision), and **M003** for
  the same reason on the deliverable record. Each plan states the condition in
  its §14 rather than leaving the boundary implicit.

## 1. Purpose and ownership boundary

This workstream closes six gaps that a comparison against `dsh` made visible.
None of them is a defect report: every gap is a capability or a piece of
evidence CodeGG does not have, not a behaviour it gets wrong.

It owns:

- the **replay tier** — a test-only provider that serves a recorded model
  transcript to the shipped agent loop, so a turn can be re-run with no key and
  no network (M001);
- the **request ledger** — a durable record of the identity of what each model
  request actually sent, so a turn is reconstructable after the fact (M002);
- the **deliverable record** — a model-declared, harness-verified statement of
  what a turn hands the user (M003);
- the **generated surface catalogs** — the tool surface, the configuration
  surface, and the storage surface documented by a generator from source with a
  freshness gate (M004–M006);
- the **configuration provenance surface** — the effective value of every
  configuration key with the layer that won it (M005);
- the **defect memory** — a dated record per escaped defect naming the guard
  that now prevents it (M007).

It consumes, and does not redefine:

- the agent loop's turn/step structure, its compaction policy, and its
  progress-recovery ladder — owned by `architecture/agent.md`;
- the tool boundary: `ToolBroker`'s ten-stage pipeline, the tool contract
  vocabulary, and the scheduler's admission authority — owned by
  `architecture/tool_broker.md` and `architecture/scheduler.md`;
- the canonical session projection and its decided field set (ADR-0053);
- the configuration discovery order and its per-field merge strategies —
  owned by `architecture/config.md`;
- the migration chain and `STORAGE_LAYOUT_VERSION` — owned by
  `architecture/storage.md` and `architecture/session.md`;
- the test-family layout, the nextest profiles, and the Rust-aware scanner
  rules — owned by `architecture/testing.md`.

The governing rule for the whole line:

> Every claim the harness makes about itself is either **produced** by the
> harness or **checked** against it. A claim that is neither — a hand-written
> tool count, a "what the model saw" reconstructed from today's assets, a
> deliverable asserted in prose — is the defect this line exists to remove.

## 2. Work classification

### Invariants

- **Replay never ships.** The replay provider is compiled out of every
  production target, is unreachable from configuration, the protocol, an
  environment variable, or any CLI path, and a guard fails if it becomes
  reachable. This mirrors the existing `capture-test-support` constraint
  rather than inventing a weaker one.
- **Replay mocks exactly one boundary.** The model adapter is scripted;
  everything downstream — the tool registry, the broker, the scheduler, the
  store, the projection — is the real implementation. A replay that also
  stubs a tool proves the bridge moves bytes, not that the tool works.
- **The ledger does not change what is sent.** Its digest is computed over the
  values that are dispatched, not over a reconstruction of them. A ledger that
  derives its own answer is a second source of truth.
- **One writer per record.** Each durable record this line adds has exactly one
  production writer, named in a guard, the way the queue-delivery and
  idle-continuation call sites are named today.
- **A generated catalog cannot disagree with its source.** The tool catalog is
  produced by constructing the real registry; the config catalog by the visitor
  that performs the real merge; the storage catalog by running the real
  migration chain. A generator that re-implements the source has the same
  defect as a hand-written document.
- **A guard only guards if the regression fails it.** Every guard this line
  adds carries a `--self-test` that injects the defect and proves rejection.
- **A postmortem names a guard that exists.** A record whose guard path does
  not resolve, or whose guard has no self-test, fails the guard that checks
  postmortems.

### Capabilities

- **M003** is the only user-visible milestone: the model can declare what a
  turn delivers and the declaration is verified before it is shown.
- **M005** is operator-visible: `codegg config show --provenance` answers
  "which layer set this key?".

Neither changes an existing surface's behaviour. M003 adds a tool and a
turn-tail card; M005 adds a read-only subcommand.

### Infrastructure

- **M001** (replay tier), **M002** (request ledger), **M004** and **M006**
  (generated catalogs). None of these is a completed capability until a
  consumer exists, and each plan names its consumer.

### Polish

- **M007** (defect memory) is documentation and a guard, sequenced last by
  construction because it records the guards the other milestones add.

## 3. Non-goals

These are recorded because each one was considered and refused. An agent
picking up this line must not treat a refusal as an oversight.

- **Parallel tool execution and per-call concurrency classification.** CodeGG
  already runs a bounded parallel batch with a semaphore, a batch-scaled
  admission deadline, and a repair for the long-poll head-of-line wedge
  (`src/agent/tool_batch.rs`, `src/agent/policy.rs`). Adopting dsh's
  `isConcurrencySafe` classifier would be a **downgrade**.
- **An ordered, extensible tool policy waterfall.** `ToolBroker` is a fixed
  ten-stage pipeline that already covers caller policy, authority, permission,
  and path policy before dispatch. Replacing a fixed order with an extensible
  one trades a proof for a convention.
- **Output spill with locators.** The context ledger already stores bounded
  artifacts and serves them through `context_read` with an explicit next
  offset.
- **Model-authored programs with a full runtime (dsh "Code Mode").** Tool
  Programs are parse-only by invariant, and
  `scripts/check_tool_program_parse_only.py` rejects a `pyo3` dependency, an
  import, a process spawn, or an FFI link. This is a deliberate capability
  boundary, not a gap.
- **Compile-time scope carriers and per-agent registration shadowing.** CodeGG
  builds a registry per turn from `ToolRegistry::with_options`. dsh's
  `ScopeKey`/`Scoped<T>` primitive buys plugin-registry ergonomics CodeGG's
  static registry does not need.
- **Restricted tools that are indistinguishable from nonexistent ones.** dsh
  hides a restricted tool so thoroughly that a caller cannot tell it was
  restricted. CodeGG deliberately does the opposite: `DisabledTool` and
  `UnrootedTool` keep a stable name and return a typed, actionable refusal,
  while `expose_in_definitions` hides them from the model. Losing that
  diagnosis would be a regression.
- **A wall-clock benchmark gate.** CodeGG's standing rule is deterministic
  operation counts (`src/tui/render_cost.rs`). A timing gate on a shared
  machine is the regression that rule exists to prevent.
- **A per-file 100% coverage gate.** Rust line coverage cannot be attributed
  per file without an instrumented build. A scoped floor is a separate
  decision with its own cost argument; it is not adopted here.
- **An "everything is a plugin" kernel.** CodeGG's WASM plugin runtime plus a
  static registry is a different tradeoff. A kernel rewrite is a large
  regression risk with no measured defect behind it.
- **Remote execution providers (SSH, computer-use, browser-use).** Product
  capabilities, not harness practices, and CodeGG's daemon is a local
  singleton.

## 4. Current state

Each gap below is stated as repository evidence, not as a deficiency of
intent. Paths are named; line numbers are not, because they move.

**No replay tier.** `crates/codegg-providers` exposes a `Provider` trait whose
entire streaming surface is `stream(&ChatRequest) -> EventStream`, which is a
clean seam for a scripted adapter. The only test-support seam in the crate is
`capture-test-support`, which redirects one provider's origin to a loopback
capture server and is compiled out of production. It can *record* an exchange;
nothing replays one. There are no recorded session fixtures in the tree, and no
runner that drives the agent loop from a fixture.

**No request ledger.** The session store persists messages. It persists no
assembled system prompt and no admitted tool schema, so the exact request a
turn sent is *reproducible in principle* — the prompt is rebuilt from assets
and configuration each turn — but it is not *recorded*, and it changes
silently when an asset or a configuration key changes. The context ledger
(`architecture/context-ledger.md`) records artifact packing, not request
identity. A tool-definition fingerprint exists for cache stability; it is not
persisted per request.

**No declared deliverables.** There is no tool by which the model declares what
a turn hands the user. The TUI holds per-session changed files as frontend
state, fed by Git events; that is a different claim — what the repository
changed, not what the turn delivered — and it is not durable.

**Hand-written surface documentation.** `architecture/agent-tool-surface.md`
enumerates the model-facing tools by hand. `architecture/config.md` documents
the four configuration layers and the per-field merge strategies by hand.
`architecture/storage.md` documents a migration chain that
`scripts/check_project_catalog_invariants.py` already ties to
`STORAGE_LAYOUT_VERSION`. Two generated assets exist and prove the pattern:
`scripts/generate_builtin_agents.py` and `scripts/generate_subagent_names.py`,
each with a `--check` mode. The three largest surfaces do not use it.

**No configuration provenance.** Discovery order and merge strategy are
documented and guarded (`scripts/check_config_merge_coverage.py` fails when a
field is not merged), but there is no way to ask which layer supplied an
effective value. The CLI's command set has no `config` subcommand.

**No defect memory.** `plans/closure/` records milestone evidence
forward-looking. `agentfindings.md` is git-ignored scratch. Nothing durably
records, for a defect that escaped to a release, which guard now prevents it
and why the evidence that existed at the time did not catch it.

## 5. Target architecture

One rule, applied seven times:

> **A claim about the harness is produced by the harness or checked against
> it.** Recording is produced; a catalog is produced; a provenance answer is
> produced; a deliverable is checked; a postmortem is checked against the guard
> it names.

After this workstream:

- A turn can be re-run from a committed fixture with no key and no network, and
  the durable result is compared rather than eyeballed.
- A turn's request identity is readable from storage, and the difference
  between two turns' requests is a query rather than a rebuild.
- A turn's deliverables are a verified record: every declared path existed,
  was inside the workspace, and was written by that turn.
- The tool, configuration, and storage surfaces are generated from the code
  that implements them, and a change to any of them fails a `--check` until the
  document is regenerated.
- The effective value of any configuration key names the layer that won it.
- Every escaped defect names a guard, and the guard's self-test is the proof
  that the record is not aspirational.

Each milestone is independently landable and independently reversible. None
changes the behaviour of an existing path; the two that add a surface (M003,
M005) add it beside the existing one.

## 6. Dependency graph

All seven milestones touch disjoint ownership boundaries. There is **no hard
dependency** in this line: no milestone consumes another's output, and none
requires an interface to stabilise first.

| Milestone | Hard dependencies | Interface dependencies | Soft edges |
|---|---|---|---|
| M001 recorded-session replay | none | `Provider` trait; the test-family layout | M002's acceptance test is cheapest on top of M001 |
| M002 durable request ledger | none | the session store's migration chain; the agent loop's dispatch seam | — |
| M003 present deliverables | none | `validate_path`; the turn's changed-file evidence | — |
| M004 generated tool-surface catalog | none | `ToolRegistry::with_options`; the generated-asset `--check` pattern | establishes the generator shape M005 and M006 reuse |
| M005 config catalog and provenance | none | the merge visitor in `codegg-config` | prefers M004's generator shape |
| M006 generated storage catalog | none | the migration chain; `STORAGE_LAYOUT_VERSION` | prefers M004's generator shape |
| M007 escaped-defect postmortems | none | the guard suite | records the guards M001–M006 add |

Classification: every edge above is **soft** — a shared shape or a preferred
order, never a semantic prerequisite.

**Sequencing by consequence, not by dependency.** M001 first: it is the
cheapest of the seven and every later milestone's evidence gets easier once a
turn can be replayed. M002 second: it is the largest storage change and M001 is
its acceptance test. M003 third: it is the only user-visible milestone, and it
is independent of the storage work. M004 fourth: it proves the generator shape
the next two reuse. M005 fifth: the config surface has the most drift risk
today. M006 sixth: the storage surface drifts slowest and its generator is the
most mechanical. M007 last by construction: it records guards that do not exist
yet.

## 7. Milestones

### M001 — Recorded-session replay

- **Class:** infrastructure (test evidence).
- **Objective:** a committed fixture drives the shipped agent loop through one
  complete turn with no API key and no network, and the durable result is
  compared.
- **Deliverable boundary:** a test-only `Provider` implementation in
  `crates/codegg-providers` behind a feature no production target enables; a
  new root test family; a fixture-format guard.
- **Exit conditions:** three scenarios (plain turn, tool call, multi-turn)
  pass keyless; the replay provider is unreachable from configuration, the
  protocol, the environment, and the CLI, and a guard proves it; perturbing the
  prompt assembly makes a scenario fail.
- **Plan:** `plans/implementation/harness-practice-adoption/001-recorded-session-replay.md`

### M002 — Durable request ledger

- **Class:** infrastructure.
- **Objective:** each model request attempt records the identity of what was
  sent — system-prompt digest, tool-definition digest, message range, model and
  effort selection — durably, with the body retained only when capture is on or
  the attempt failed.
- **Deliverable boundary:** one additive migration in the session store, one
  write at the agent loop's dispatch seam, one reader.
- **Exit conditions:** a turn writes exactly one row per attempt; a retried
  attempt writes a second; the digest matches the dispatched value; a turn
  cancelled mid-prepare writes no row; no secret material is stored.
- **Plan:** `plans/implementation/harness-practice-adoption/002-durable-request-ledger.md`

### M003 — Present: declared turn deliverables

- **Class:** capability.
- **Objective:** the model declares files as a turn's deliverables; the harness
  verifies each one and records the declaration; the frontend renders it at the
  turn's tail.
- **Deliverable boundary:** one read-only tool, one durable record, one
  projection or response field, one TUI card.
- **Exit conditions:** a path outside the workspace is refused; a path that
  does not exist is refused; a path the turn did not write is refused with a
  reason; a verified declaration is durable across a restart and visible in the
  TUI.
- **Plan:** `plans/implementation/harness-practice-adoption/003-present-turn-deliverables.md`

### M004 — Generated tool-surface catalog

- **Class:** infrastructure (documentation truth).
- **Objective:** the model-facing tool surface is emitted by constructing the
  real registry, and a `--check` guard fails when the document and the registry
  disagree.
- **Deliverable boundary:** one generator, one committed catalog, one guard
  with a self-test, and the architecture document pointing at the catalog
  instead of restating counts.
- **Exit conditions:** adding or removing a tool changes the generator's output
  and fails `--check`; a backend-gated tool appears as gated rather than being
  omitted; the generator needs no workspace root and no network.
- **Plan:** `plans/implementation/harness-practice-adoption/004-generated-tool-surface-catalog.md`

### M005 — Configuration catalog and provenance

- **Class:** capability (operator-visible).
- **Objective:** `codegg config show --provenance` names the winning layer for
  every effective key, and the configuration surface is documented by the
  visitor that performs the merge.
- **Deliverable boundary:** provenance capture in `codegg-config`, one read-only
  CLI subcommand, one generated catalog, one guard.
- **Exit conditions:** the dump's values equal `Config::load()` for every key;
  the catalog lists exactly the keys the merge visits; a key added to the schema
  but not to the merge fails a guard; provenance names a real path for each of
  the four layers.
- **Plan:** `plans/implementation/harness-practice-adoption/005-config-catalog-and-provenance.md`

### M006 — Generated storage catalog

- **Class:** infrastructure (documentation truth).
- **Objective:** the storage surface — tables, columns, indexes, and the layout
  version — is emitted by running the real migration chain, with a freshness
  guard.
- **Deliverable boundary:** one test-only generator, one committed catalog, one
  guard with a self-test.
- **Exit conditions:** the catalog's layout version equals
  `STORAGE_LAYOUT_VERSION`; every table has a declared owning module and an
  undeclared table fails the guard; the generator is deterministic across two
  runs on the same baseline.
- **Plan:** `plans/implementation/harness-practice-adoption/006-generated-storage-catalog.md`

### M007 — Escaped-defect postmortems

- **Class:** polish (process).
- **Objective:** every defect that escapes to a release has a dated record
  naming the guard that now prevents it, and a guard fails when the record
  names something that does not exist.
- **Deliverable boundary:** one convention document, one template, at least one
  seeded record from a defect already fixed, one guard with a self-test.
- **Exit conditions:** the guard rejects a missing section and a dangling guard
  path; the convention states its boundary against `plans/closure/`; the seeded
  record's guard exists and self-tests.
- **Plan:** `plans/implementation/harness-practice-adoption/007-escaped-defect-postmortems.md`

## 8. Cross-cutting requirements

**Storage.** One migration, owned by M002, additive and restart-safe, with
`STORAGE_LAYOUT_VERSION` tracking it and
`scripts/check_project_catalog_invariants.py` green. M003's record reuses an
existing table if one fits; a new table is a second migration and must be
justified in its plan. No milestone deletes or rewrites an existing row.

**Protocol.** None as written. Two stop conditions: M002 and M003 each stop and
report if their record cannot ride an existing request family or an existing
projection field. Adding a `CoreRequest` variant or extending ADR-0053's
decided set is an owner decision, not an implementation detail.

**Security.** Three live consequences, not assumptions:

- **M001** fixtures are committed data. They must contain no credential, no
  token, and no absolute path from a developer's machine; the fixture guard
  rejects credential-shaped strings and absolute home paths.
- **M002** records what was sent. The digest is over the dispatched value, but
  the *body* is redacted-at-write under the same rule the delegated transcript
  already uses, and bodies are absent unless capture is enabled or the attempt
  failed.
- **M003** resolves model-supplied paths. Every path goes through
  `validate_path` against the turn's `workspace_root`; a path outside it, a
  symlink that escapes it, or a path that is not a regular file is refused with
  a typed error. The tool holds no write authority.

No authorization boundary changes. No milestone adds a capability to the
command catalog except M005's read-only subcommand, which is a CLI surface and
not a daemon `Command`.

**Concurrency.** M002's writer is at the dispatch seam, inside the turn that
owns the request, so it needs no lock beyond the store's existing transaction.
M001 runs under the nextest `ci` profile, one process per test, because the
replay provider must not observe another test's registry. M003's verification
reads the turn's own evidence; it does not race a concurrent writer.

**Observability.** Each milestone's claim is enforced by a guard with a
self-test, registered where the defect would otherwise fail silently.
`scripts/verify.sh quick` runs formatting, the generated-asset checks, the guard
scripts and a workspace `cargo check` — **it does not run tests**. The three
catalog *comparison* gates (M004–M006) are therefore tests and run in the nextest
sweep (CI and `verify.sh full`); each is paired with a cheap structural guard
that `quick` can run without invoking `cargo`. The replay-absence check, the
single-writer guard and the postmortem guard run in `quick`.

**Docs.** `architecture/testing.md` gains the replay tier and its recording
rule; `architecture/session.md` gains the request ledger and its retention;
`architecture/tool.md` gains `present` and its contract;
`architecture/agent-tool-surface.md`, `architecture/config.md` and
`architecture/storage.md` link to their generated catalogs instead of restating
counts; `architecture/overview.md`'s verified-counts rows follow their listed
sources.

## 9. Verification strategy

Three layers, each cheaper than the next:

1. **The artifact itself.** A replay scenario that passes keyless; a ledger row
   read back and compared; a `present` refusal for each bad path shape; a
   `--check` that regenerates and compares.
2. **The guard with a self-test.** Every new guard injects its defect and
   proves rejection. A guard without a self-test is a claim, not a hook.
3. **A perturbation.** For the two milestones whose claim is "this detects
   change", the plan requires the change to be made and the detection observed:
   perturb the prompt assembly and watch M001's scenario fail; add a tool and
   watch M004's `--check` fail; add a schema key without a merge and watch
   M005's guard fail.

Layer 3 is the one that separates evidence from decoration, and each plan names
its perturbation explicitly in §10.

## 10. Risks and decision points

| Id | Decision | Resolution taken here | Reopens if |
|---|---|---|---|
| D1 | Are replay fixtures committed, or recorded locally? | **Committed and bounded.** A tier that needs a key is not a tier. Fixtures are small by construction: one scenario, one turn, no binary content. | A scenario cannot be expressed under the size bound; then the scenario is dropped, not the bound. |
| D2 | Does the ledger store request bodies? | **Digests always; bodies only when capture is enabled or the attempt failed.** A body per request is unbounded growth for a question the digest already answers. | A debugging class appears that a digest cannot answer. |
| D3 | Does the tool catalog include schemas? | **No.** Names, category, effect class, defer-loading, and the hiding gate. Schemas are large, change often, and already have one authority. | A consumer needs a schema diff, which is a different artifact. |
| D4 | Is a postmortem required for every defect? | **Only for defects that escaped to a release.** A record per internal defect is a tax on ordinary work. | Recurrence is observed in defects that never shipped. |
| D5 | Does M003 add a projection field? | **Only if ADR-0053's decided set already has a fitting field.** Otherwise stop and report. | The owner decides to extend the projection. |

The largest risk in this line is **scope drift into dsh's design**. §3 exists
for that reason: the refusal list is normative, and a plan that widens into a
rejected practice is a stop condition in that plan's §14.

## 11. Completion definition

The line is complete when all seven milestones have accepted closure records
and:

- a replay scenario runs keyless in the routine suite, and a prompt-assembly
  perturbation is observed to fail it;
- a turn's request identity is readable from storage after a daemon restart,
  and the digest matches the dispatched value;
- a `present` declaration for a path outside the workspace, a missing path, and
  a path the turn did not write each fail with a distinct typed error, and a
  verified declaration survives a restart;
- the tool, configuration, and storage catalogs regenerate byte-identically on
  the closure baseline, and each `--check` fails on an injected change;
- `codegg config show --provenance` names the winning layer for every key and
  agrees with `Config::load()`;
- at least one escaped defect has a postmortem whose named guard exists and
  self-tests.

## 12. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 recorded-session replay | conditionally closed | `plans/implementation/harness-practice-adoption/001-recorded-session-replay.md` | `plans/closure/harness-practice-adoption/001-status.md` | Criterion 4 is **met** on this tree (corrected 2026-11-13). Residue: `architecture/testing.md` has no replay subsection, WP-D's live recording needs a provider credential, and the tree is uncommitted |
| M002 durable request ledger | conditionally closed | `plans/implementation/harness-practice-adoption/002-durable-request-ledger.md` | `plans/closure/harness-practice-adoption/002-status.md` | Re-measured insert cost, call-site coverage, and the byte-identity criterion remain outstanding |
| M003 present deliverables | conditionally closed | `plans/implementation/harness-practice-adoption/003-present-turn-deliverables.md` | `plans/closure/harness-practice-adoption/003-status.md` | Store-reopen test, open-in-editor affordance, and an `agent_loop` integration test remain outstanding |
| M004 generated tool-surface catalog | closed | `plans/implementation/harness-practice-adoption/004-generated-tool-surface-catalog.md` | `plans/closure/harness-practice-adoption/004-status.md` | none |
| M005 config catalog and provenance | closed | `plans/implementation/harness-practice-adoption/005-config-catalog-and-provenance.md` | `plans/closure/harness-practice-adoption/005-status.md` | none |
| M006 generated storage catalog | closed | `plans/implementation/harness-practice-adoption/006-generated-storage-catalog.md` | `plans/closure/harness-practice-adoption/006-status.md` | none |
| M007 escaped-defect postmortems | closed | `plans/implementation/harness-practice-adoption/007-escaped-defect-postmortems.md` | `plans/closure/harness-practice-adoption/007-status.md` | none |

All seven plans are implemented and each milestone's status above is carried from
its closure record. M001–M003 are conditionally closed on the named evidence
conditions; M004–M007 are closed.
