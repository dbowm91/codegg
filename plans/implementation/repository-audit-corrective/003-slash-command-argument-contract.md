# Repository Audit Corrective Milestone C003 — Slash-Command Argument Contract

Status: closed — closure record at `plans/closure/repository-audit-corrective/003-status.md`

> **Scope re-derivation (2026-10-23, implementation).** WP1 re-derived the
> affected set from the code over all 161 dispatcher arms and found **twelve**
> defective arms, not the six this plan recorded. The plan's six are among
> them. Six more were found: `/open`, `/memory-list`, `/habits` and
> `/security-review` used the whole typed line as their argument; `/loop`
> split the line and took the **first** part — the command name — as the
> duration; and `/review-change` stripped `"/review "`, a spelling that is
> not its own. The plan's §14 stop condition fired; the scope was
> re-derived, re-registered in the roadmap and the registry, and the
> derivation and the eleven behaviour changes are recorded in the closure
> record. The plan's claim that "`/loop` splits the line and takes the
> second part" was falsified by the re-derivation: it takes the second part
> as the *message* and the command name as the *duration*.


Repository baseline: `3b29c5b88` (`origin/main`)
Audit evidence was gathered at `04536eeeb`; every file this workstream cites is
byte-identical at that commit and at the baseline above, so the findings carry
forward unchanged.

Source roadmap: `plans/subsystems/repository-audit-corrective-roadmap.md#milestone-c003--slash-command-argument-contract`

Long-term requirements: `plans/000-long-term-specification.md`, `plans/002-long-term-roadmap.md`, `architecture/tui.md`

Applicable ADRs: none required. The command registry, the key-ownership model, and the argument contract are unchanged in shape; only the extraction is made single-sourced.

Primary class: **capability**

## 1. Objective

A registered command's handler receives the argument the user typed, with the command name removed. No command is reachable with an argument that still contains its own name.

## 2. Why this milestone is ready

It has no hard or interface dependency, and it owns one defect class in one ownership boundary: the argument handed from the command registry to the command handlers.

It is registered as ready because the command registry and the key-ownership model are established and stable, and because the fix is bounded to the extraction of an already-typed string — it does not change which commands exist, what they are named, or what they do.

## 3. Current implementation evidence

### What the command palette holds

The palette's `query` is the **entire typed line**, including the command name:

- `src/tui/app/mod.rs` sets `self.dialog_state.command_palette.query = trimmed.to_string();` when a command is submitted, where `trimmed` is the whole typed line.
- A test in the same file sets `app.dialog_state.command_palette.query = "/approval".into();` — the command name and no arguments.

So a handler that reads `query` directly receives `"/search foo"`, not `"foo"`.

### Two competing idioms in one dispatcher

`dispatch_builtin_command` is a single match over the `BuiltinSlashAction` enum with **161 arms**. Two argument idioms coexist inside it:

- arms that consume the `raw_input: Option<&str>` parameter of the dispatch function;
- arms that re-read `self.dialog_state.command_palette.query` and parse it themselves.

A count over the dispatcher's body puts roughly 101 arms in the first group and 73 in the second. The two groups are not disjoint — some arms read the query *and* consume the parameter.

Working arms in the second group strip the command name first. For example, one arm does `query.strip_prefix("/agents").map(|s| s.trim()).unwrap_or("")` to obtain the subcommand arguments, and another does `query.strip_prefix("/agent")`. Stripping is the established convention.

### Six arms do not strip

These read the query raw, so the command name is part of the argument the handler acts on:

| Command | Arm | What the handler receives instead of the argument |
|---|---|---|
| `/search` | `B::Search` | the whole line, e.g. `"/search foo"` — and that string is what is searched for |
| `/task-del` | `B::TaskDel` | the whole line, e.g. `"/task-del 5"` — used as the task id |
| `/memory-search` | `B::MemorySearch` | the whole line, e.g. `"/memory-search notes"` — used as the query |
| `/memory-remember` | `B::MemoryRemember` | the whole line, e.g. `"/memory-remember buy milk"` — used as the text |
| `/memory-forget` | `B::MemoryForget` | the whole line, e.g. `"/memory-forget 42"` — used as the id |
| `/habit-dismiss` | `B::HabitDismiss` | the whole line, e.g. `"/habit-dismiss 7"` — used as the id |

Two further points about the enumeration, so the implementer does not chase the wrong number:

- An earlier survey of this defect class reported ten affected commands. On inspection, `/loop` is **not** among them: its arm splits the line on a space and takes the second part, which is correct. The confirmed count is **six**. The implementer must re-derive the set from the code rather than trusting a fixed list, because the list is the defect.
- The command's **alias** matters as much as its name. A command reachable by more than one spelling must have its arguments extracted against the spelling the user actually typed, not against the canonical name.

### Why each arm looked correct in isolation

Every one of the six arms is locally plausible: it reads a string, trims it, and uses it. The defect is only visible at the level of the dispatcher, where two idioms sit side by side and 161 arms make the inconsistency invisible on review. This is the same structural cause as the nested-runtime defect C001 removes, and it is why the fix is to make the extraction **single-sourced**, not to patch six arms.

### A related predicate defect on the same surface

`Dialog::is_open()` in `src/tui/app/types.rs` matches against 50 of the `Dialog` enum's 53 variants. `None` is correctly excluded — it means no dialog is open. `PanelManager` and `Control` are omitted, so a session that has one of those two dialogs open reports no dialog open.

## 4. Invariants that must not regress

- A command with **no** argument must behave exactly as it does today, including the commands whose arms currently read an empty query.
- A command's **argument** is the text after the command name, with surrounding whitespace trimmed. A command whose argument is empty must reach the same code path it reaches today.
- The **command registry** — names, aliases, categories, and the palette's ordering — is unchanged.
- The **key-ownership model** is unchanged. This milestone does not touch key bindings, and the recorded chord counts in the key-authority guards remain their contract.
- A command's **visible result** is unchanged, including the toast text and the dialog a command opens.
- The `/lsp-*` commands' results are unchanged; C001 owns their runtime handling and this milestone must not alter what they return.
- A command that is dispatched with an explicit argument — as the unit tests do — must continue to work, including the tests that call `dispatch_builtin_command(action, None)`.

## 5. Scope

**In scope**

- The argument extraction at the command registry's entry point, and the dispatcher's use of it.
- The six confirmed arms, and any other arm found to be defective by the implementer's own re-derivation.
- The alias handling, so that a command invoked by a non-canonical spelling yields its argument.
- `Dialog::is_open()`, so that every dialog variant is covered.
- A test per affected command, asserting the argument the handler receives.
- A guard that fails when an arm reads the palette query directly instead of the extracted argument.

**Out of scope**

- Splitting the 161-arm dispatcher, or restructuring `src/tui/app/mod.rs`. The structural cause is recorded in the roadmap's §4 and is explicitly not in scope.
- Converting the dispatcher to `async`, or adopting the `spawn_tui_task` completion pattern. C001 records the same deferral.
- Adding, removing, renaming, or re-aliasing any command.
- Changing any command's behaviour, output, dialog, or key binding.
- The `Dialog::is_open` semantics of `None`, which is correct as it stands.

## 6. Required production changes

### Core

- The argument must be extracted **once**, at the point where the typed line is resolved to a registry entry, and the extracted argument must be the only thing the handlers receive.
- The extraction must resolve the **spelling the user typed** against the registry entry's name **and aliases**, so that `/agent` and `/agents` and any other alias each yield their own argument.
- The dispatch function's existing `raw_input: Option<&str>` parameter is the natural carrier. The implementer must decide whether `raw_input` continues to carry the whole line — in which case each arm still needs a consistent extraction — or becomes the **already-extracted argument**, in which case the arms that read the palette query are removed. The second is preferred: it makes a wrong arm impossible rather than merely absent today.
- If `raw_input` becomes the extracted argument, the unit tests that call `dispatch_builtin_command(action, None)` and the tests that set `command_palette.query` to a full command line must be reconciled with the new contract. **This is a contract change to an internal function, not to a user-visible behaviour**, and the reconciliation is part of the milestone.

### Storage

None.

### Protocol

None.

### Runtime

None.

### Frontend

- A user typing `/search foo` gets a search for `foo`.
- A user typing `/task-del 5` deletes task `5`.
- Every command that takes an argument behaves as a user would expect, and the palette's own completion and validation behaviour is unchanged.

### Security

- No change to any authorization boundary. The implementer must confirm that no arm's behaviour depended on the argument **containing** the command name — a handler that matched on the full line, or used the command name as part of its input, would change behaviour if the argument is extracted. That is a real risk in this milestone and must be checked exhaustively, not sampled.

### Docs

- `architecture/tui.md` must record the argument contract: where the argument is extracted, that it excludes the command name and its alias, and that handlers receive only the argument.

## 7. Ordered work packages

### WP1 — Re-derive the affected set

Intent: establish the true set of arms that receive a wrong argument.

Required changes: enumerate every arm of the dispatcher, classify each by which idiom it uses, and for each arm in the query-reading group determine whether it strips the command name — and whether it strips the right one for every alias. The result is a table of every arm with its extraction behaviour.

The earlier survey's count of ten is **not** to be used as the input to this step; the survey's list included `/loop`, which is correct as it stands. The implementer must produce the set from the code.

Acceptance evidence: the table covers all 161 arms and every entry states the extraction behaviour and the affected spellings.

### WP2 — Introduce a single extraction point

Intent: make a wrong arm impossible.

Required changes: implement the extraction at the command-registry entry point, resolving the typed spelling against the entry's name and aliases, and pass the extracted argument to the dispatcher. Remove the arms' direct reads of the palette query.

Acceptance evidence: after this change, no arm of the dispatcher reads `command_palette.query`.

### WP3 — Migrate the arms

Intent: make every arm receive the extracted argument.

Required changes: update the arms that read the query to consume the extracted argument instead, preserving each arm's current semantics for the argument it intends to use. For the six confirmed defective arms this changes the value the handler acts on, which is the point.

Acceptance evidence: the WP1 table is re-derived against the new code and shows a single consistent contract.

### WP4 — Fix `Dialog::is_open`

Intent: every dialog variant is covered.

Required changes: add the two missing variants to the predicate. The implementer must check whether the predicate's callers relied on the omissions — a caller that used `is_open` to mean "a modal dialog is open" may have been relying on `PanelManager` and `Control` being excluded, and that reliance must be found and handled deliberately rather than discovered by a test failure.

Acceptance evidence: every variant is covered, and the callers' behaviour is unchanged or the change is recorded.

### WP5 — Tests

Intent: pin the argument each handler receives.

Required changes: one test per affected command, asserting the argument the handler receives, plus a test per alias of a command with aliases. The test must fail against the baseline.

Acceptance evidence: the tests are shown failing against the baseline implementation.

### WP6 — Add the guard

Intent: prevent a new arm from reading the palette query directly.

Required changes: a guard that fails when an arm of the command dispatcher reads `command_palette.query` directly, and that fails when a `Dialog` variant is missing from the `is_open` predicate. It must use `scripts/rust_test_scan.py` for any Rust parsing, provide `--self-test` that injects a violating arm and proves the guard rejects it, and be wired into `scripts/verify.sh quick` and CI.

Acceptance evidence: the guard runs in `verify.sh quick`; its `--self-test` passes; it fails on an injected violation.

### WP7 — Record the contract

Intent: prevent the inconsistency from being reintroduced.

Required changes: `architecture/tui.md` gains the argument contract. It states the rule and links to the guard; it does not restate the guard's implementation.

Acceptance evidence: the section exists and is linked from the guard's docstring.

## 8. Failure, cancellation, restart, contention semantics

- **Failure.** A command whose argument cannot be extracted — an unknown spelling, an alias that is not registered — must reach the same handling it reaches today, whatever that is. The extraction must not introduce a new failure mode for a spelling that previously worked.
- **Cancellation.** No change. A command that opens a dialog and is then cancelled still cancels.
- **Restart.** No durable state changes.
- **Contention.** The extraction runs on the keypress path and must stay cheap. It must not introduce a lock, and it must not re-resolve the registry on every keystroke beyond what the palette already does.

## 9. Compatibility and migration

- **User-visible:** the six commands change behaviour, and that change is the point. Every other command's behaviour is unchanged.
- **Internal:** `dispatch_builtin_command`'s parameter contract changes. It is a crate-internal function, so there is no external consumer, but the unit tests that call it directly are consumers and must be reconciled. The reconciliation is a mechanical update, and the closure must state how many call sites changed.
- No persisted state, configuration, or protocol changes.

## 10. Required tests

**Focused**

- One test per affected command, asserting the argument the handler receives.
- One test per alias of a command that has aliases, asserting the argument is extracted against the typed spelling.
- A test that a command with no argument still receives an empty argument, and behaves as it does today.
- A test that a command dispatched with an explicit argument still works.

**Integration**

- A test that types the command line through the palette's submission path — not by calling the dispatcher directly — and asserts the resulting behaviour. The defect lives in the composition of the palette and the dispatcher, so a direct dispatcher call does not reproduce it.

**Security**

- A test that no arm's behaviour changed as a side effect of the extraction, for the arms whose handling depended on the argument's previous content. The implementer must enumerate those arms from the WP1 table and cover them.

**Migration**

- None.

**Placement.** The `tui_render` family is the right home for the TUI command-surface tests. A new member is `tests/tui_render/<name>.inc`, bound in `tests/tui_render/mod.rs` with `#[path = "<name>.inc"] pub mod <name>;`. The family target is already declared in the root `Cargo.toml`; a new family would need that declaration and is not expected here.

**Vacuity.** Every new test must be shown failing against the baseline. A test that passes before the change is not evidence for this milestone.

## 11. Required verification commands

```bash
# Scoped: quick's guards plus only the tests this change can affect.
scripts/verify.sh scoped HEAD~1

# The affected family.
cargo test --test tui_render

# The new guard, including its non-vacuity proof.
python3 scripts/check_command_argument_contract.py --self-test

# Rust-aware test inventory (ground truth; never regex-scan Rust).
python3 scripts/rust_test_scan.py --validate

# Full workspace, before the closure record is written.
bash scripts/verify.sh full
```

`scripts/verify.sh full` runs `cargo clippy --workspace --all-targets --locked -- -D warnings`. On macOS this currently fails for reasons unrelated to this milestone (C005 owns that asymmetry); the implementer must record the failure set before and after and state that the change added and removed no lint finding. Linux must stay green.

The key-authority guards carry **recorded chord counts** as part of their contract. This milestone must not change any chord, and the implementer must confirm the counts are unchanged rather than assuming it.

## 12. Documentation updates

- `architecture/tui.md`: the argument contract.
- `docs/`: no user-facing change. The six commands behave as a user would already expect; the change removes the surprise.

## 13. Acceptance criteria

Externally observable:

1. `/search foo` searches for `foo`.
2. `/task-del 5` deletes task `5`.
3. `/memory-search notes` searches for `notes`.
4. `/memory-remember buy milk` remembers `buy milk`.
5. `/memory-forget 42` forgets `42`.
6. `/habit-dismiss 7` dismisses `7`.
7. A command invoked through an alias receives its argument.
8. Every command that takes no argument behaves exactly as it does today.
9. `Dialog::is_open()` returns true for every dialog variant that is open.
10. The new guard runs in `verify.sh quick` and its `--self-test` passes.

## 14. Stop conditions

The implementer must report rather than improvise if any of the following holds.

- The extraction would change a command's behaviour **beyond** the six confirmed commands. That is a discovery that the defect is wider than the survey found, and the milestone's scope must be re-derived and re-registered before the change lands.
- An arm's correct behaviour depends on the argument **containing** the command name, and cannot be expressed on the extracted argument. Stop: that arm needs a decision, not a mechanical migration.
- Removing the palette-query reads would require converting the dispatcher to `async`, or would require the `spawn_tui_task` completion semantics. Stop: both are C001's recorded deferrals and neither may be absorbed here.
- `Dialog::is_open()`'s callers rely on the two omitted variants being excluded, and the reliance cannot be preserved while covering them. Stop and report the callers.
- The key-authority guards' recorded chord counts change. Stop: that means a key binding moved, which is out of scope.

## 15. Closure evidence required

- The WP1 table, covering all 161 arms, and its re-derivation after the change.
- The list of commands whose behaviour changed, and confirmation that it is exactly the set derived in WP1.
- The demonstration that each new test fails against the baseline.
- The `Dialog::is_open` callers' review, and the disposition of each.
- The guard's `--self-test` output and its presence in `verify.sh quick`.
- The `verify.sh scoped` and `verify.sh full` results, labelled local versus CI, with the macOS lint set stated before and after.
- The key-authority guards' chord counts, confirmed unchanged.
- The registry update, and the unblock audit of the registry's Blocked work section.

## 16. Handoff notes

- The command **alias** is the part most likely to be missed. Stripping the canonical name when the user typed an alias yields an argument that still starts with the alias, which is the same defect in a different place. The extraction must resolve the typed spelling.
- Do not trust a fixed list of affected commands. The survey that produced this plan's list also listed a command that was correct; the set comes from WP1.
- The 161 arms are the reason this defect survived. A patch to six arms fixes today's symptom and leaves the mechanism in place. The milestone's deliverable is the single extraction point, and the six-arm fix is its consequence.
- The `Dialog::is_open` omission is on the same surface and is cheap to fix here. It is included because leaving it would be an inconsistency in a milestone whose whole subject is one.
- This milestone closes no other milestone. C005 and C006 depend on it.
