# TUI Editor Helix Parity Milestone 013 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/tui-editor-helix-parity/013-lsp-read-delivery-and-reachability.md`
Source subsystem roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 (M013)
Repository baseline reviewed: `84440fe7f`, re-verified at `a8778e87c` (W1/W2 were
implemented against this tree)
Implementation commit: **none — uncommitted.** No commit was requested for this
milestone; the evidence below describes the working tree at baseline `a8778e87c`
plus this change. It is labelled as such rather than attributed to a commit.

---

## 1. Executive finding

M013 is **closed**. Both defects the plan recorded at baseline are fixed, the
read surface is twelve operations deep instead of six, the Helix leaves are
bound to a surface that actually answers, and the read-only/write-only boundary
is drawn in code rather than in a comment.

Four findings are worth recording beyond the checklist, because each is a
decision a later milestone would otherwise have to re-litigate.

1. **Six more "correct code never reached" defects, all the same class.** The
   plan recorded two. Implementation found six more, and the pattern is the
   reason they are listed together rather than individually:

   - **`CoreRequest::LspReadGet` had no session-locator arm.** The daemon's
     session-locator match carried `LspPreviewApply` and `LspCompletionGet`
     under a comment saying the M006-B reads were "deliberately untouched".
     With no locator, no project resolved, so every M006-B read failed
     `authorization_scope_required` *before* the resolver ever ran. A socket
     client is a team principal, so **the entire M006-B read surface was dead
     in production** while its 14 tests were green — they called the daemon as
     `LocalOwner` through bare `handle_request`, which bypasses scope
     resolution entirely. The plan's own defect 1 (an undriven publisher) and
     this one share a cause: tests that do not traverse the production route.
     No guard script covers session-locator arm completeness
     (`grep -rn 'via_session|ViaSession|session locator' scripts/` returns
     nothing), which is why it went unnoticed; a guard here is the obvious
     follow-up.
   - **Warm-only was a claim about two servers.** The gate resolved a client for
     the session root; the query then re-resolved through `get_or_create_client`
     — its own root from the file, its own server id from the language. Those
     could disagree, and the re-resolution could start a server. Fixed by
     threading the resolved key from the gate into the query through
     `*_for_warm_client` entry points. This is why nine `egglsp` operations were
     refactored rather than just the daemon.
   - **Diagnostics were keyed by `file_name()`.** `src/lib.rs` and
     `tests/lib.rs` collapsed into one entry, and the publisher walked
     `client_keys()` globally, so two open projects recorded each other's
     files. Now keyed by the full workspace-relative path and scoped to the
     session's root.
   - **Point-read server errors were all mapped to `OutsideWorkspace`.** Every
     `Err(_)` from a warm-server call became "that file is not in your
     workspace", which is both false and unactionable.
     `LspReadDenial::ServerFailed` already existed for this and was unused.
   - **Three more capabilities were not warm-only at all.** See §3.
   - **Capital leaves were dead on half of all terminals.** See §4.

2. **Three of the six new capabilities could not have been exposed as the plan
   assumed.** The plan's §14 stop condition says to probe behaviour rather than
   infer it from signatures. Doing so found that `signature_help_typed`,
   `format_preview_typed` and `rename_preview_typed` were real, working
   implementations that were **not warm-only**: each routed through
   `require_capability`, `capability_decision_for_file` or
   `ensure_file_open_from_disk`, and all three of those reach
   `get_or_create_client`. Worse, `ensure_file_open_from_disk` itself resolves a
   client, *and* every helper it calls to open or refresh a document
   (`open_file`, `update_file`, `set_managed_document`) resolves one again. The
   refactor therefore reaches three layers deeper than the operation does. None
   of this was visible from the operation signatures.

3. **`Unsupported` is a status, not a denial and not `NotReady`.** A warm server
   that never advertised `renameProvider` has still *answered*. Mapping that to
   `ServerFailed` tells the user their working language server broke, and
   mapping it to `NotReady` sends a client into a retry loop that can never
   succeed. `LspReadStatus::Unsupported` carries the real
   `ServerCapabilities` key (via `LspReadOperation::capability`), so a client
   can point at the server's own documentation rather than a protocol-invented
   label.

4. **`is_point` was two predicates wearing one name.** Document symbols and
   semantic tokens rode inside `is_point` with a line exemption bolted on, and
   a whole-document format preview would have been told it needed a cursor it
   does not have. Split into `is_point` / `is_file_scoped` / `is_workspace`.

---

## 2. Both recorded defects, before and after

### Defect 1 — the diagnostics publisher was undriven

**Before.** `publish_changed_diagnostics` and `DIAGNOSTICS_POLL_INTERVAL` in
`src/core/daemon_lsp.rs` had **zero callers**, and the only production write to
`daemon.lsp_diagnostics` sat inside that uncalled function. Every diagnostics
read therefore returned `NotReady` forever — which is indistinguishable from a
broken integration.

**After.** `CoreDaemon::start_lsp_diagnostics_driver()` is a daemon-owned tokio
task, started at both serving-daemon sites in `src/main.rs` immediately after
`attach_idle_wake()` — the same `self: &Arc<Self>` + explicit-spawn precedent
that `CoreDaemon::new` and `with_deps` cannot provide (they do not return an
`Arc`, and tests construct bare daemons).

Three decisions inside it:

- **It enumerates `SessionRuntimeRegistry`, not the session table.**
  `SessionRuntimeRegistry::list_sessions()` was `#[allow(dead_code)]` until this
  driver. Walking the session table instead would include sessions from a
  previous run, whose root may since have been deleted, and would walk them
  every 750 ms forever.
- **It holds a `Weak`.** The task ends when the daemon does.
- **It does not skip ambiguous sessions.** Several warm clients can each have
  diagnostics; only an *absence* of warm clients means there is nothing to do.
  The immediate first interval tick is skipped so a driver started at boot does
  not publish before anything has connected.

Ownership of the cadence by the daemon rather than a frontend is ADR-0011:
a frontend-triggered poll would put the "may start a server" decision on
whichever client happens to be open.

### Defect 2 — ambiguity mapped to `NotReady`

**Before.** The read path called
`find_existing_client_for_root_hint(Some(&root), None)`, which returns `Err`
when more than one client shares a root, and that `Err` was mapped flatly to
`NotReady`. Any polyglot workspace had F3–F9 silently dead, reporting "no
server" while two healthy servers were running.

**After.** `resolve_warm_client(service, root, target, server_id)` resolves in
four steps, each with a stated reason:

1. An explicit `server_id`, if named.
2. An extension match — but only when **exactly one** warm client claims that
   extension. `servers_for_extension` returns *all* claimants;
   `find_server_for_extension` had been "the first match", which is a guess, and
   a wrong-server LSP answer is worse than an honest refusal.
3. The single warm client, when there is exactly one.
4. Otherwise `Ambiguous`, carrying the candidate names and **no answer**.

`Ambiguous` is a distinct status, not a readiness state: retrying cannot make
an ambiguity go away, and a client that treated it as `NotReady` would poll
forever against a workspace that is entirely healthy. A **named-but-cold**
server is `Cold`, not `Ambiguous` — "no server for what you asked for" and
"several servers" are different facts, and the first must not tell a user to
choose when there is nothing to choose from.

`server_id` is additive and `#[serde(default)]`; `PROTOCOL_VERSION` stays 2,
following this repository's convention for additive changes. It grants nothing:
it picks *which* already-warm server answers.

---

## 3. The probe, and what it changed about W3

Plan §14 requires probing each capability's behaviour rather than inferring it
from signatures. Result: **all six are real implementations, none is a stub,
and three are not warm-only.**

| Capability | Real? | Warm-only before? |
|---|---|---|
| `implementation` | yes, capability-gated, real `textDocument/implementation` | yes (M013 W2 refactor) |
| `declaration` | yes | yes (M013 W2 refactor) |
| `document_highlights` | yes | yes (M013 W2 refactor) |
| `signature_help_typed` | yes | **no** — `require_capability` |
| `format_preview_typed` | yes, bounded diff, re-reads disk for staleness | **no** — `require_capability` + `ensure_file_open_from_disk` + two `get_or_create_client` for generation |
| `rename_preview_typed` | yes, `prepareRename` best-effort then `rename` | **no** — `capability_decision_for_file` + `ensure_file_open_from_disk` |

Exposing the last three without fixing them would have put three ways to start a
language server on a read path that is documented as never starting one.

`egglsp` now has **fifteen** `*_for_warm_client` entry points — eight in
`navigation.rs`, three in `completion.rs`, three in `rename.rs`, one in
`formatting.rs`. M013 added fourteen of them (nine in W2, five in W3); the
fifteenth, `completion_for_warm_client`, predates this milestone.

The fix is the W2 pattern applied one layer deeper: keyed bodies plus
`*_for_warm_client` entry points in `completion.rs`, `formatting.rs` and
`rename.rs`, and keyed `ensure_file_open_from_disk_for_key` / `open_file_for_key`
/ `update_file_for_key` / `set_managed_document_for_key` in `service.rs`.
`rename_raw_and_preview` lost its last caller once `rename_preview_typed` went
keyed and was **deleted** rather than kept as an `allow(dead_code)` wrapper;
`capability_decision_for_file` was deleted for the same reason. A file-path form
that only exists to start servers is not an API worth keeping.

---

## 4. W4 — the Helix leaves

Bound into the M005 leaf tables (`src/tui/editor.rs`: `SPACE_LEAVES`,
`GOTO_LEAVES`), not into a second prefix mechanism.

| Key | Effect |
|---|---|
| `space k` | hover |
| `space s`, `space f` | document symbols |
| `space S` | workspace symbols, queried with the token at the cursor |
| `space d` | refresh and open the diagnostics panel |
| `space h`, `gr` | references |
| `gd` | definition |
| `gi` | implementation |
| `]d` / `[d` | next / previous diagnostic, cursor moves |
| `gy` | **unavailable**: no language server implements type definition here |
| `space F` | **unavailable**: no multi-document file picker |
| `space r`, `space a`, `=` | **unbound**: applying an LSP-driven mutation has no accepted ADR |

**The two halves of a leaf run at different authority levels.** The prefix
resolver holds `&mut EditorState` and can decide *what* was asked for; only the
App holds a core client, task registry and toasts, so it owns *how* the request
runs. The leaf parks its effect on `EditorLspState::pending_leaf` and
`handle_editor_key` drains it on the very next statement. One hop, not a queued
channel: a leaf can never leave a request armed, and a scope change discards it
with the rest of the LSP state. This is what `PrefixAction::Lsp` exists for, and
why it is a fourth arm rather than a bool or an `EditorCommand` — an LSP leaf
moves no cursor and edits no text.

**Those names are the ones this milestone was reviewed against; the change
that shipped it carries the generalized ones.** `PrefixAction::Lsp` and
`EditorLspLeafEffect` became `PrefixAction::Effect` and
`EditorPendingEffect`, and the type moved out of `src/tui/editor_lsp.rs` into
`src/tui/editor_effect.rs`. That is M010's W1, folded into the same commit
because M013's leaf rows are the effect type's only users: splitting the
rename from its first use would have left one commit that does not compile.
The record keeps the original names because they are what the milestone was
reviewed against; the code carries the generalized ones, and `EditorPendingEffect`
is where the clipboard family and the pickers will live.

**Capital leaves were dead on half of all terminals.** `PrefixLeaf::glyph`'s own
doc always claimed the dispatch reconciled `Char('S')`+`NONE` with
`Char('s')`+`SHIFT`. The lookup gated on `NONE` alone, so `space S` and
`space F` would have been live on one terminal and silently dead on the next —
exactly the M002 failure mode this milestone exists to end.
`leaf_lookup_char` now upper-cases under `SHIFT`, which makes the doc true, and
a test drives both spellings.

**Degradation is a status row, not a toast.** `EditorLspLeafEffect::Unavailable`
carries the reason a key is unbound. `NotReady`, `Ambiguous` and `Unsupported`
each render their own line; none falls through to "No results", which would read
as a clean workspace.

**F2–F9 are untouched**: the function-key `match` in `handle_editor_key` returns
before `editor_buffer_key` runs, and a leaf is a *second* key after a prefix, so
the two cannot collide.

### One diagnosis that was wrong, and was reverted

While wiring the App-level tests, `gy` appeared to resolve to nothing through
`handle_editor_key` while it resolved perfectly through `editor_buffer_key`. The
hypothesis was that App-level Normal-mode key bindings were consulted *between*
the arming key and the second key, stealing it; a guard skipping that block
while a prefix was armed was written and appeared to fix it.

It did not. The real cause was in the test fixture: the App's default
`input_mode` is not Normal, so `editor_buffer_key` was typing `k` into the
document instead of resolving the leaf. With Normal mode set, `gy` resolved with
or without the guard. The guard was **removed** rather than kept "just in case" —
an unverified behaviour change in the App's key route is not a cheap thing to
carry, and the check that would have justified it (disable the guard, re-run)
would have failed.

The test-coverage gap it exposed is real and is now covered: **every previous
leaf test drove `editor_buffer_key` directly**, skipping the App route that
decides Normal-mode bindings, the F-key intercept and the LSP panel. A leaf
table that resolved perfectly in a unit test could still have been dead for a
user. `a_leaf_resolves_through_the_app_route_not_only_the_buffer_route` and
`a_prefixed_key_is_reached_only_after_its_arming_key` now drive
`handle_editor_key` on the editor route.

### Not delivered, and why

- **`space S` needs a query.** A workspace-symbol read with no query is not a
  request the daemon can answer, so the leaf uses the token under the cursor —
  the same source F7 uses, and the only query a keypress can carry.
- **`space h` is the references read, not Helix's highlight overlay.** There is
  no highlight-overlay authority on the read surface yet.
- **`space D` / `[D` workspace diagnostics were not bound.** The daemon
  publishes per-project diagnostics, but no server capability backs a separate
  "workspace diagnostics" request, so it would be a second name for the panel
  `space d` already opens.
- **Impaired change navigation (`]g`/`[g`/`]G`/`[G`) remains deferred** with no
  reachable data source, and stays asserted unbound.

---

## 5. Acceptance criteria (plan §13)

| # | Criterion | Evidence |
|---|---|---|
| 1 | Warm server → diagnostics read `Ready` with real contents | `tests/lsp_read_delivery_m013.rs::a_warm_sessions_diagnostics_reach_the_stream_without_a_frontend_asking`, `::the_resync_authority_becomes_ready_once_the_driver_has_run` |
| 2 | Two warm servers → distinct ambiguity, not `NotReady` | `::several_warm_servers_are_ambiguous_not_not_ready` |
| 3 | Explicit `server_id` selects the correct server | `::an_explicit_server_id_resolves_what_the_extension_cannot`, `::the_targets_extension_selects_one_of_two_warm_servers` |
| 4 | Cold root → `NotReady`, no process started | `::a_cold_root_still_answers_not_ready_and_starts_no_process`, `::a_named_but_cold_server_is_not_ready_and_not_ambiguous` |
| 5 | Every Helix LSP binding produces a result or states why it cannot | 14 dispatch/stepping tests in `src/tui/commands/editor.rs` (two of them through the App route); `SPACE_LEAVES`/`GOTO_LEAVES`/`OPEN_BRACKET_LEAVES`/`CLOSE_BRACKET_LEAVES` |
| 6 | F2–F9 continue to work unchanged | Function-key `match` is upstream of `editor_buffer_key`; `::a_leaf_that_parked_nothing_leaves_no_read_error` covers the adjacent path; existing F-key tests unmodified |
| 7 | No LSP **write** path exists | `FormatPreview`/`RenamePreview` return previews only; `space r`/`space a`/`=` unbound; `rename_preview_for_warm_client` and `format_preview_for_warm_client` have no apply sibling on any read path |

---

## 6. Verification

All results below are **local** (`scripts/verify.sh`, invoked directly). No CI
run was made for this milestone.

| Command | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | clean |
| `cargo test -p codegg --lib --no-fail-fast` | **6697 passed, 2 failed, 4 ignored** (101.99 s) — see note below |
| `cargo test -p egglsp --lib` | 1011 passed, 0 failed |
| `cargo test -p codegg-protocol --lib` | 252 passed, 0 failed |
| `cargo test -p codegg-core --lib` | 858 passed, 0 failed |
| `cargo test -p codegg-core --lib authorization` | 21 passed, 0 failed |
| `cargo test -p codegg-client` | 33 + 9 + 16 passed, 0 failed |
| `cargo test -p egglsp --features lsp-test-support --test scenario_engine` | 5 passed, 0 failed |
| `cargo test -p codegg --features lsp-test-support --test lsp_read_delivery_m013` | 7 passed, 0 failed |
| `cargo test -p codegg --features lsp-test-support --test lsp_read_surface_trajectory` | 14 passed, 0 failed |
| `cargo test -p codegg --features lsp-test-support --test lsp_completion_trajectory` | 12 passed, 0 failed |
| `python3 scripts/check_tui_editor_key_authority.py` | exit 0 |
| `python3 scripts/check_tui_editor_prefix_state.py` | exit 0 |
| `python3 scripts/check_tui_editor_text_authority.py` | exit 0 |
| `python3 scripts/check_authorization_matrix.py` | exit 0 |
| `python3 scripts/check_audit_coverage.py` | exit 0 |
| `python3 scripts/check_daemon_cwd_usage.py` | exit 0 |
| `python3 scripts/check_scheduler_bypass.py` | exit 0 |
| `python3 scripts/check_execution_ownership.py` | exit 0 (via `verify.sh quick`) |
| `scripts/verify.sh quick` | **passed** |

### `verify.sh full` — it ran, and it is **not** green

`scripts/verify.sh full` was run on this tree. Its `quick` and `clippy` phases
passed. Its `cargo nextest run --workspace --locked --profile ci` phase ran
**14600 tests: 14531 passed, 69 failed, 6 skipped** (348.775 s) and the script
exited **100**.

**No green full sweep is claimed.** The 69 failures are real and they are
recorded as a residual, not written up as a pass.

They are also **not this milestone's**. The failing tests are in 11
`tests/`-integration binaries — `lsp`, `tool_execution`,
`command_routing_execution_ownership`, `bash_background_m002_routes`,
`git_execution_origin_matrix`, `identity_live_execution_audit`,
`identity_m002_live_audit_hooks`, `session_family`, `tool_program_git_lsp_palette`,
`tool_surface_minimization`, `workspace_isolation` — and **not one of those
files is touched by this change**. Every failure is one of two pre-existing
causes:

1. **A tool constructed without a workspace root now refuses.** The message is
   `"no workspace root was resolved for this call (the tool was constructed
   without a workspace root); refusing to substitute the process working
   directory"`. This is the `workspace-authority-and-session-navigation`
   workstream's deliberate contract: a registry built without a root must not
   produce cwd-rooted tools (`ToolError::MissingWorkspaceRoot`,
   `ToolRegistry`'s `UnrootedTool`).
2. **`/tmp` is a symlink on macOS.** `workspace root '/tmp' must be an
   absolute, non-symlink path` — the same platform defect already fixed in
   `src/core/daemon.rs`'s fixtures during W1, still present in
   `tests/bash_background_m002_routes.rs` and others.

**Proven, not assumed.** The comparison was made by stashing this milestone's
entire change (`git stash push -u`), re-running the same 11 binaries at the
baseline `a8778e87c`, and diffing the failing test **names**:

- baseline: **69** failing tests
- with this change: **69** failing tests
- `comm` in both directions: **empty**

The two sets are identical name for name. **This milestone introduced zero new
test failures.**

**The residual is already owned.** `plans/closure/workspace-authority-and-session-navigation/001-status.md`
records the same break from the other side: *"Failure accounting across the
milestone: **47** lib tests broke when the cwd fallbacks were removed; **44**
were fixed by naming a root; **3** were proven pre-existing."* The **lib**
tests were repaired in that workstream; the **`tests/` integration** binaries
were not, and the 69 that remain are that same repair, one line per helper
away. The fix pattern is known and documented there — name a root, e.g.
`.with_workspace_root(...)` or the `make_tool_with_root` helper that already
exists in `tests/lsp.rs`.

M013 did **not** apply that repair. It is a different workstream's contract,
its own closure record, and its own test files; folding 69 test repairs from
another workstream into an LSP-delivery milestone would blur exactly the
boundary this milestone's closure is supposed to draw. It is registered as a
residual in `plans/registry.md` instead.

### The two failures in the lib suite

`tui::commands::interactive_terminal::tests::a_live_terminal_surface_repaints_with_no_further_user_input`
and `::a_hidden_terminal_surface_does_not_arm_the_loop`, both panicking on a
fixture assertion (`the box is showing a terminal` / `the fixture must start
with a mounted surface`).

These are **not** caused by this change and are **not** claimed to be fixed.
They are the same two load-sensitive terminal tests already characterised that
way in the M008 closure: they fail in the full lib suite under concurrent cargo
contention and pass in isolation. No part of this milestone touches
`interactive_terminal`. They are recorded as a residual, not written up as a
green run.

### One environmental failure that was *not* a residual

Three tests (`python_script::executor::*`, `managed_process::*`, one daemon
human-shell test) failed on a first full-suite run with
`trusted sandbox helper codegg-sandbox-helper could not be resolved: No such
file or directory`. That is a **missing binary in the shared `target/debug`
directory**, not a code fault: `cargo test --lib` builds no bin targets, and
another concurrent session had removed or replaced the helper. After
`cargo build --bin codegg-sandbox-helper` they passed. Recorded because the
honest cause is "the helper binary was absent", and a closure that reported
those three as a code failure — or silently dropped them — would be wrong in
opposite directions.

---

## 7. Closure evidence required (plan §15)

- **Implementation commits:** none — uncommitted, labelled as such in the header.
  Not requested for this milestone.
- **§11 output labelled local vs CI:** see §6; every row is local. CI is a strict
  subset of `verify.sh quick`, so a green CI run would not imply the provider and
  client/desktop boundary guards either.
- **Explicit before/after for both defects:** §2.
- **The cold-root assertion that no process started:**
  `tests/lsp_read_delivery_m013.rs::a_cold_root_still_answers_not_ready_and_starts_no_process`
  asserts `NotReady` against a root with no warm client and that the fake
  language-server process count does not rise.
- **The updated `lsp-ide` skill diff:** `.opencode/skills/lsp-ide/SKILL.md`. It
  previously stated "The current diagnostics change publisher has no production
  driver, so do not promise ongoing push updates". That sentence is now false
  and was replaced with the driver description, the four-step resolution order,
  the structural warm-only rule, and the full leaf table including the three
  unbound keys. A skill that documents a fixed defect as current is worse than
  no skill.
- **Previously-deferred items in `architecture/lsp.md`, now delivered or still
  deferred:**

| Item | Outcome |
|---|---|
| Diagnostics publisher driver | **delivered** (`start_lsp_diagnostics_driver`) |
| Ambiguity disambiguation | **delivered** (`resolve_warm_client`, `Ambiguous`) |
| Warm-only enforced structurally | **delivered** (`*_for_warm_client` across 12 operations) |
| `implementation`, `declaration`, `document_highlights`, `signature_help` | **delivered** |
| Format / rename preview | **delivered as read-only previews** |
| `typeDefinition` | **still deferred** — `egglsp` does not implement it; offering it would be a promise the daemon cannot keep, so the leaf reports why instead |
| LSP write authorization (rename/code-action apply, `=` format) | **still deferred** — needs an accepted ADR; the previews are the delivery, the apply is not |
| Impairied change navigation `]g`/`[g`/`]G`/`[G` | **still deferred** — no reachable data source; asserted unbound |
| `space D`/`[D` workspace diagnostics | **not delivered** — no server capability backs it; would duplicate `space d` |

---

## 8. Two test-fixture defects fixed outside M013's scope

`session_create_through_daemon` and `turn_submit_rejects_when_active` in
`src/core/daemon.rs` hardcoded `Path::new("/tmp")` as a workspace root where
every other caller of `seed_test_context` uses `tempfile::tempdir()`. On macOS
`/tmp` is a symlink to `/private/tmp`, so workspace registration rejected it
with `InvalidRootPath("/tmp")` **deterministically**. Both now use a tempdir.

This is outside M013's functional scope and is recorded rather than folded in
silently. It was fixed because those two tests failed on every macOS run and
would have muddied every verification sweep this milestone reports — a
deterministic, unrelated failure inside the suite being used as evidence is
worse than no evidence.

---

## 9. Known limits carried out of this milestone

- **`verify.sh full` is red on 69 pre-existing integration-test failures**, not
  on this milestone's account. Proven name-for-name against the baseline (§6)
  and registered in `plans/registry.md` as a residual owned by the
  `workspace-authority-and-session-navigation` workstream, whose closure record
  already documents the same break and repaired it for the lib tests only. The
  repair is one line per helper. **No green full sweep is claimed.**
- **`LspPreviewApply` is unchanged.** It existed before M013 and still has no
  accepted authorization ADR. M013 did not widen or narrow it; it only made sure
  no new write path reached it.
- **`SessionRuntimeRegistry::list_sessions()` lost its `#[allow(dead_code)]`**,
  because the diagnostics driver is now its production caller.
- **A guard for session-locator arm completeness does not exist**, and the
  missing `LspReadGet` arm is why. This is the highest-value follow-up from this
  milestone: the defect was invisible to tests because the tests bypassed scope
  resolution, and invisible to guards because no guard looks at that match.
- **`o`/`O` still never insert a line** (pre-existing: `enter_insert` maps
  `OpenLineBelow`/`OpenLineAbove` to `_ => cursor`). Recorded, not fixed; it is
  M010's or a successor's.
- **`src/tui/components/help_overlay.rs` remains unreferenced dead code** and
  the shipped `/help` still lists no editor chords. Carried from the M001
  closure and untouched here.
- **`plans/closure/workspace-authority-and-session-navigation/001-status.md:135`**
  still describes terminal-panel work as "uncommitted in this worktree". That
  was already stale before this milestone (the work landed in `18c3dca92`) and
  was left alone rather than silently corrected from a neighbouring closure.
- **One status divergence in the registry was corrected in this commit, and it
  was not M013's.** The `M009` row still said `**Blocked:** M008 … must close`,
  while the roadmap's M009 row had long recorded *"ADR-0034 acceptance only;
  the M008 blocker is discharged"* and M008 is closed. Two status cells
  disagreeing is a planning defect in its own right, and the registry is the
  authoritative control surface, so it was brought into line with the roadmap —
  M009 remains blocked on ADR-0034 acceptance alone, which is unchanged
  substance. The roadmap's M009 row was **not** edited, because it was the
  correct one.

## Registry narrative

Moved verbatim from the `plans/registry.md` row for **TUI editor Helix keybinding parity** on 2026-10-23, so that the registry stays an index.

Depends on: M005 closed (hard, for the leaf table), M002 closed (hard). Class: **invariant (delivery) + enabling capability (reach)**. **Corrective insertion, not a successor** — two defects verified at baseline `84440fe7f` make the existing F2–F9 path return `NotReady` in ordinary configurations. (1) `publish_changed_diagnostics` and `DIAGNOSTICS_POLL_INTERVAL` in `src/core/daemon_lsp.rs` have **zero callers**, and the only production write to `daemon.lsp_diagnostics` sits inside that uncalled function, so diagnostics are permanently `NotReady`. (2) The read path calls `find_existing_client_for_root_hint(Some(&root), None)`, which returns `Err` when **multiple** clients share a root, and that `Err` is mapped flatly to `NotReady` — so any polyglot workspace has F3–F9 silently dead. Fix is a daemon-owned publisher cadence plus optional `server_id` with explicit → extension-matched → unique → **typed ambiguity** resolution. Warm-only reads (ADR-0012 §3) and no app-side startup (ADR-0011) are invariants, not negotiable. Extends `LspReadOperation` with **read** variants plus rename/format **preview** only; write apply stays deferred pending an authorization ADR. **Must precede M010** — binding LSP leaves first ships keys that open always-`NotReady` panels. **M013 is closed** (`plans/closure/tui-editor-helix-parity/013-status.md`): the daemon now owns the diagnostics cadence (`start_lsp_diagnostics_driver`, ADR-0011), reads resolve explicit `server_id` → unique extension claimant → unique warm client → typed `Ambiguous`, and warm-only is **structural** rather than a pre-gate check — the resolved key is threaded into every query through `*_for_warm_client`, which required refactoring fourteen `egglsp` operations and the service's document-sync helpers, not just the daemon. `LspReadOperation` grew from six to twelve (declaration, implementation, document highlight, signature help, format **preview**, rename **preview**) plus a `new_name` field and an `Unsupported` status for a live server that lacks a capability — distinct from `NotReady`, because retrying cannot conjure an `advertised` provider. **Six further "correct code never reached" defects were found and fixed while implementing this, the same class as the two the plan recorded:** `CoreRequest::LspReadGet` had **no session-locator arm**, so every M006-B read failed `authorization_scope_required` before the resolver ran and the whole read surface was dead in production while its 14 tests passed (they called the daemon as `LocalOwner`, which bypasses scope resolution); no guard script covers that match, so a guard here is the top follow-up. Also: warm-only was a claim about two servers; diagnostics were keyed by `file_name()` so `src/lib.rs` and `tests/lib.rs` collided and two open projects recorded each other's files; point-read server errors were all mapped to `OutsideWorkspace`; signature-help, format-preview and rename-preview reached `get_or_create_client` three layers down through `ensure_file_open_from_disk` → `open_file`/`update_file`/`set_managed_document`; and capital leaves (`space S`) were dead on every terminal that reports a capital as a shifted lower-case glyph, despite `PrefixLeaf::glyph`'s own doc claiming otherwise. Helix leaves are bound in the M005 leaf tables with visible degradation; `space r`, `space a` and `=` stay **unbound** because applying an LSP-driven mutation has no accepted ADR. *(M015, 2026-10-19: that reason was wrong — ADR-0008 is accepted — and the two previews are now bound; what the editor lacked was the bridge to a reviewable artifact. See the M015 row and roadmap Amendment A2.)* **Residual registered, not discharged by M013.** `scripts/verify.sh full` runs but is **not green**: its nextest phase reports 69 failing tests in 11 `tests/`-integration binaries, and the script exits 100. These are **pre-existing and not M013's** — the change was stashed entirely and the same 11 binaries re-run at baseline `a8778e87c`, giving the **same 69 failing test names, identical name for name**, so M013 introduced zero new failures. Two causes: a tool constructed without a workspace root now refuses (`ToolError::MissingWorkspaceRoot` — the `workspace-authority-and-session-navigation` contract), and `/tmp` is a symlink on macOS so `workspace root '/tmp' must be an absolute, non-symlink path`. The owning workstream already recorded the same break and repaired it for the **lib** tests — *"47 lib tests broke … 44 were fixed by naming a root"* — but never repaired the **`tests/` integration** binaries. The repair is one line per helper (`.with_workspace_root(...)`, or the existing `make_tool_with_root` in `tests/lsp.rs`). It is left to that workstream rather than folded into an LSP-delivery milestone. |

## Registry narrative

Moved verbatim from the `plans/registry.md` row for **TUI editor Helix keybinding parity** on 2026-10-23, so that the registry stays an index.

Both blockers discharged: the M005 edge by the M005 closure on 2026-10-18, and the M013 edge by the M013 closure on 2026-10-19 (`plans/closure/tui-editor-helix-parity/013-status.md`). Capability class. Planned 2026-10-19 against the live Helix keymap and the tree at `a8778e87c`. **M010 now owns the pickers and the clipboard family, not the LSP leaves** — M013 bound `space k`/`s`/`S`/`d`/`h`/`f`, `gd`/`gi`/`gr`, `gy` and `space F` as visible unavailability notices, and `]d`/`[d` diagnostic stepping. **One M013 binding is reclaimed:** `space f` is bound to `DocumentSymbols`, a **duplicate of `space s`**, because the workspace file browser was not reachable from the editor route at the time; the browser (`Dialog::Files`) exists and the route is what M010 fixes, so `space f` becomes the file picker — a **breaking change to a live binding**, recorded in the `CHANGELOG.md` under `### Changed`. **`space s` and `space S` are as the code has them** (document and workspace symbol pickers); this cell's earlier prose left them ambiguous. The plan binds `space p`/`P`/`Y`/`R` and `space ?`, and binds the seven leaves with no substrate to a **visible statement** rather than leaving them silently dead. The M013 pending-effect mechanism is **generalized** — it was LSP-typed, and a file picker is not an LSP fact. **Closed 2026-10-19 (Amendment A5).** The space menu landed whole: `space f` is the workspace file browser (reclaimed from the duplicate of `space s` — a **breaking change to a live M013 binding**, recorded in the `CHANGELOG.md` under `### Changed`), `space ?` is the editor's own `:` command palette, `space p`/`space P`/`space R`/`space Y` are the clipboard family (`space Y` takes **only** the main selection where `space y` takes the whole set), and seven leaves (`space F`, `space b`, `space j`, `space D`, `space c`, `space C`, `space '`) are bound to a **visible statement** of why they cannot run — no space key is silently dead. **The picker browses the *tab's* workspace, not the document's:** the attachment carries a `workspace_id` and a relative path and **no root**, and the daemon resolves the listing from the tab's canonical project/workspace binding (ADR-0022, C005), so `space f` checks that binding **before** opening and refuses with a reason rather than opening an empty browser. The seeding decision moved into a new `App::open_file_picker`, which `Dialog::Files` now delegates to, so `/files` and `space f` cannot disagree — and `/files` was **fixed** on the way, since it used to show a tree belonging to another workspace under this one's name. **A new guard polices the leaf tables**: `scripts/check_tui_editor_leaf_authority.py` (in `verify.sh quick` + CI `verify` job) fails on a duplicated glyph in one table, a glyph that is not printable ASCII, one glyph claimed by two tables of the same prefix, a leaf behind a guard-block chord, and a table no prefix binds; it reads the guard-block chords **through** the key-authority guard rather than re-parsing the dispatch chain. **The plan's own rule 4 was wrong as written** — it forbade a leaf glyph equal to a guard-block chord's *glyph*, and the guard block's glyphs are `s` and `r`, with `space s` live — so it is implemented as the **chord** a leaf can resolve to; and the plan's rule 3 is implemented as the prefix's **union** across its tables. **The M013 leaf test lost exactly one row, by design:** the plan required those tests to pass unmodified *and* required the reclaim, and the `f` row asserted the old binding, so the row was **removed** — `space f` is no longer an LSP read at all — and the other thirteen M013 leaf tests are unmodified. |
