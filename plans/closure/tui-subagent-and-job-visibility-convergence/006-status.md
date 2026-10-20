# TUI Subagent and Job Visibility Convergence — Milestone 006 — Closure Status

**Milestone:** C003 of `plans/subsystems/tui-subagent-and-job-visibility-convergence-corrective-addendum.md`

Status: **closed**

Document numbering: this subsystem numbers closure documents by **write
sequence**, not by milestone id — `003-status.md` closes **C005** and
`005-status.md` closes **C007**. This document is the sixth closure record and
closes **C003**.

Source implementation plan:

- **None.** C003 was implemented without one. This is governance gap **D-8** of
  the corrective addendum, the same gap C004 closed against. No plan was
  written retrospectively: a plan authored after the fact is not the
  specification the work was built against, and minting one here would convert a
  recorded governance defect into an invisible one. This record therefore closes
  C003 **directly against the addendum's deliverable boundary and stop
  condition**, which is the only specification that ever existed for it.

Source subsystem roadmap:

- `plans/subsystems/tui-subagent-and-job-visibility-convergence-corrective-addendum.md`
  §7 (C003), §1 (cause 3), §4 (D-6 is *not* the cause; the picker is D-6's
  subject only by way of §7)

Repository baseline: `e8a6b5ca2`.

Implementation: **committed at `e8a6b5ca2`** — C003 is *not* part of the
uncommitted working tree. Verified by reading `git show
e8a6b5ca2:src/tui/app/mod.rs`, which already contains the delivered behaviour.

Scope note: this record covers only C003. The working tree separately carries
C002's, C005's and C007's uncommitted implementation, and §5 records two fixes
made by this closure pass that touch files shared with those milestones.

## 1. Executive finding

C003 is closed. Bare `/agents` opens the agent dialog, the panel picker is gone
entirely, and panel placement remains reachable on `/panels`.

Two findings the implementation left behind:

1. **The panel picker was deleted, not bypassed.** `open_panel_picker` has
   **zero** occurrences anywhere in `src/`. This is the correct outcome — the
   addendum forbade "routing around it by relabelling the picker" — and it is
   stronger than the milestone asked for.
2. **The test file's module doc still described the retired picker.** It
   explained, in detail, that bare `/agents` opens "the Send agents to panel
   picker" and that the recursion test therefore walks the picker — while the
   tests immediately below it drove `/panels`. The tests were right and the
   prose was wrong. Corrected by this record; see §6.

No implementation defect was found. The delivered behaviour already satisfied
the milestone's boundary.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Verdict |
|---|---|---|
| Bare `/agents` shows agents | `src/tui/app/mod.rs`, `B::Agents` arm: the empty-`args` branch calls `self.open_dialog(Dialog::Agent)` | met |
| Not a question about panel placement | `open_panel_picker` has zero occurrences in `src/`; the picker is deleted, not relabelled | met |
| Panel *placement* stays reachable | `/panels` opens `Dialog::PanelManager`; `the_panel_manager_still_loads_and_focuses_the_agents_panel` selects and focuses `SidePanelId::Agents` | met |
| Subcommands not made harder to reach | `--all`/`-a`, `show <name>`, `diff <name>`, `validate`, `reload`/`rebuild` are unchanged in the same arm | met |
| `/agents` and `/agent` agree | `agents_and_agent_open_the_same_surface`; both open `Dialog::Agent` (`B::AgentSelect`, `src/tui/app/mod.rs:5545-5549`) | met |
| Hidden agents excluded | `Dialog::Agent` arm in `src/tui/app/modal.rs` filters `!a.hidden` when populating the visible list | met |

## 3. The stop condition was not hit

The addendum set one:

> **Stop condition:** if satisfying this requires building a new agents dialog,
> report scope before building rather than substituting the picker.

It was **not** hit. `Dialog::Agent` already existed and already did the job — it
is the surface `/agent` has always opened — so C003 needed no new dialog at all.
The milestone was satisfied by pointing an existing command at an existing
dialog and deleting the intervening detour. No scope report was owed and none is
filed.

This is worth stating plainly because it is the outcome that makes the milestone
cheap: the addendum's "if no real agents dialog exists, that is the work" clause
did not apply.

## 4. Non-vacuity proof

A test that has never failed is not evidence. The C003 behaviour was proven
non-vacuous by injecting a regression and confirming the test catches it.

**Injection 1 (compile failure, recorded because it is informative):** the first
attempt restored the pre-C003 call `self.open_panel_picker("agents")`. The crate
**did not compile**:

```
error[E0599]: no method named `open_panel_picker` found for mutable reference `&mut app::App`
    --> src/tui/app/mod.rs:5598:26
```

This is not counted as the non-vacuity proof, because a test that cannot be
compiled is not a test that failed. It does independently establish that
`open_panel_picker` is genuinely gone from the tree rather than merely unused.

**Injection 2 (the proof):** bare `/agents` was repointed at a different dialog —
`open_dialog(Dialog::Agent)` → `open_dialog(Dialog::Help)` in the empty-`args`
branch — which compiles and changes only the user-visible behaviour. Result
recorded in §5.

## 5. Verification executed

**All verification was local. No CI receipt exists for this work, and no runtime
observation was made — the TUI was never launched.**

```bash
cargo fmt --check                                                     # pass
cargo clippy --workspace --all-targets --locked -- -D warnings      # pass, no warnings
scripts/verify.sh quick                                              # pass ("Quick verification passed.")
cargo test --test tui_agents_panel                                   # 3 passed, 0 failed
cargo test --lib subagent                                            # 119 passed, 0 failed
cargo test -p codegg-protocol --lib projection                       # 94 passed, 0 failed
cargo test --test user_prompt_queue_box                              # 70 passed, 0 failed
cargo test --test tui_side_panels                                    # 31 passed, 0 failed
cargo test --test tui_sessions_pending_sections                      # 11 passed, 0 failed
cargo test --lib command_docs                                        # 2 passed, 0 failed
python3 scripts/check_tui_transport_authority.py                    # pass
python3 scripts/check_tui_subagent_name_pool_isolation.py           # pass
python3 scripts/check_execution_ownership.py                        # pass
python3 scripts/check_scheduler_bypass.py                            # pass
python3 scripts/check_adr_integrity.py                               # pass
python3 scripts/generate_subagent_names.py --check                   # pass
(cd apps/desktop && node scripts/check-bindings.mjs)                 # pass
```

`tui_agents_panel` reports **3**, against 4 at `e8a6b5ca2`: the delta is the
vacuous test this record deleted (§7). `cargo test --lib subagent` reports
**119**, against 118 before this session: C007's own count, unchanged by C003.
`codegg-protocol --lib projection` reports **94**, unchanged — C003 and C006
added no projection test; C006's existing test was reused rather than duplicated.

`apps/desktop/scripts/check-bindings.mjs` must be run **from `apps/desktop`**;
it resolves paths relative to cwd and fails with `ENOENT` from the repo root.
It is a change-triggered guard, not part of `verify.sh` or CI, and it asserts
only the fields it enumerates. C003 adds no projection field, so it is reported
here as a regression check, **not** as evidence of desktop parity.

### Non-vacuity results

| Injection | Target | Result |
|---|---|---|
| `open_panel_picker("agents")` restored | bare `/agents` branch | crate **fails to compile** — `error[E0599]: no method named open_panel_picker`. Establishes the picker is deleted, not merely unused; **not** counted as the test proof |
| `open_dialog(Dialog::Agent)` → `Dialog::Help` | bare `/agents` branch | `bare_agents_opens_the_agent_dialog` **FAILED** — `left: Help, right: Agent`; the other 3 passed |
| `self.agent_state.current_agent = idx;` deleted | `B::AgentSelect` | `agent_with_a_name_selects_that_agent` (first rewrite) **passed** — proving that rewrite vacuous too (§7) |
| bare `/agents` → `Dialog::Help`, run twice | bare `/agents` branch | `agents_and_agent_open_the_same_surface` **passed both times** — the vacuity proof for the deleted test (§7) |

Every injection was reverted and the suites re-run green afterwards.


## 6. The stale module doc, and what was changed

`tests/tui_agents_panel.rs` opened with:

> Bare `/agents` opens the "Send agents to panel" picker rather than jumping
> straight into the column, so the path continues through the box: the picker
> opens on the active panel, `j` walks the registry to Agents, and Enter commits.

Every clause of that was false after C003, and the tests directly below it —
`the_panel_manager_still_loads_and_focuses_the_agents_panel` drives `/panels` —
contradicted it. A reader arriving at the recursion test would have been told to
expect a picker walk and given a panel-manager walk.

Rewritten to describe the delivered behaviour: bare `/agents` opens
`Dialog::Agent`, the picker left that path, and placement lives on `/panels`.
The substantive recursion history (why the bespoke-panel test exists at all) was
preserved, because it still explains why the `/panels` path must not recurse.

The file is **committed and unmodified at `e8a6b5ca2`**, so this is a
correction to committed text rather than to this session's work.

## 7. A second thing: `agents_and_agent_open_the_same_surface` was vacuous

The test claimed "`/agents` and `/agent` are the same command wearing two
spellings; they must not disagree about what a user gets". **It could not fail,
and it was deleted rather than rewritten.**

Both spellings resolve through the completion popup: `handle_command_key` prefers
`completion_rows()[completion_sel].label` over the raw query
(`src/tui/app/mod.rs`, the `InputAction::Send` arm). Typing either string filters
the popup, "agent" ranks `/agents` above `/agent`, and **both therefore execute
the same command**. The assertion compared a value to itself.

Proven, not suspected: repointing bare `/agents` at `Dialog::Help` left
`agents_and_agent_open_the_same_surface` **green**, while
`bare_agents_opens_the_agent_dialog` failed in the same run.

### Why it was deleted instead of replaced

A replacement was written and then withdrawn. The assertion that genuinely
matters is that `/agent <name>` changes the selected agent — `B::AgentSelect`'s
selection branch, reached only through the leading-token fallback when the
completion popup matches no row for the argument form. Three attempts:

1. Asserting `current_agent == expected` **passed with the assignment deleted
   from `B::AgentSelect`** — the same trap as the test it replaced.
   `current_agent` is already `0` and the first agent is already index `0`.
2. Starting from a different index and targeting the last agent (10 agents in the
   fixture) then failed for a different reason: tracing the keystrokes shows the
   composer receives `/agent verifier` correctly and the palette query is
   `/agent verifier` at Enter, yet nothing is selected and no dialog opens. At
   `Enter` the popup's rows still win, its label resolves to no command, and the
   resolver falls through to "no matching command — send it as a prompt".
3. Reaching the branch from an integration test would need `completion_rows` or
   `dispatch_builtin_command`, both private to `src/tui/app/mod.rs`.

**The blocker is not a missing assertion, it is a product question**: should the
completion popup match a query carrying its own argument, or should `/agent
<name>` always take the leading-token fallback? That decides whether the popup
should offer `/agent` for the query `/agent verifier`, and it belongs to the
command-palette routing milestone, not to C003's closure.

Shipping a test that passed for the wrong reason, or quietly widening C003 to
settle popup routing, would both have been worse than deleting a test that could
never fail. The gap is recorded here instead of being papered over.

No production behaviour changed. `B::AgentSelect` selects correctly when reached;
what is unproven is which route a keystroke takes to reach it.

## 8. Invariant review

- **I1 — no client-side fabrication.** Holds and is untouched. The agent dialog
  reads `agent_state.agents`; nothing about subagent *rows* or their names is
  in scope for this milestone.
- **I5 — a subagent tab stays read-only.** Untouched. C003 opens an *agent*
  dialog (agent definitions), not a subagent run surface.
- **Panel recursion.** The original defect this file guards — a bespoke panel
  read as an unloaded hosted panel, causing `materialize_hosted_panel →
  open_panel_box → select_panel → materialize_hosted_panel` — is still guarded by
  `no_bespoke_panel_reads_as_an_unloaded_hosted_one`, and still reaches the same
  code through `/panels`.

## 9. What was **not** verified

- **No runtime observation.** The TUI was never launched. Every claim here is
  from source and from tests that drive the real key path (`/`, characters,
  Enter) — never from a screen.
- **No CI receipt.** Nothing in this record has been through hosted CI.
- **No visual check** that `Dialog::Agent` reads well at the user's terminal
  size. That is a design question this closure does not answer.
- **No claim about `/agents` with arguments.** The subcommand arms were read, not
  re-tested; C003 did not change them and no test was added or removed for them
  by this record.