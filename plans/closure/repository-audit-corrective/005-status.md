# Repository Audit Corrective C005 — Verification Surface and Dead Code — Closure Status

Status: **conditionally closed** — the guard, the lint surface, and the
routing abstraction are landed and evidenced; the 615-line disabled body is
**not** removed, because the plan's own §14 stop condition fires.

Source implementation plan:

- `plans/implementation/repository-audit-corrective/005-verification-surface-and-dead-code.md`

Source subsystem roadmap:

- `plans/implementation/repository-audit-corrective/README.md` — the
  repository-audit corrective workstream (C001–C007). There is no separate
  `plans/subsystems/` roadmap for this workstream; the registry's
  "Blocked work" section carries the C005 row.

Repository baseline reviewed: `8590c17c5` ("docs(plans): register the
repository audit corrective workstream")

Implementation commits or pull requests:

- `fix/c005-audit-corrective` — the whole of this milestone's implementation.
  Not pushed, not merged; the sibling milestones C001 and C002 land on their
  own branches and are reconciled at merge time.

---

## 1. Executive finding

The milestone's infrastructure boundary is **mostly** complete, with one
part deliberately not closed:

- **The broker-boundary guard is repaired, dispositioned, and wired.** It was
  red at the baseline, in neither `scripts/verify.sh quick` nor CI, and
  reported six findings of which five were false positives produced by a
  single root cause (the pattern could not tell intra-tool delegation from a
  boundary crossing). The patterns now distinguish the two by the *method*
  they call, the one genuine finding is a **recorded, owned exception** rather
  than a broker re-route, and the guard runs in both `verify.sh quick` and
  CI with a `--self-test` that proves it still rejects a genuine violation.
- **The macOS lint result is clean.** `cargo clippy --workspace --all-targets
  --locked -- -D warnings` failed at the baseline with ten findings across
  three targets; every one was Linux-only code the macOS build does not
  compile. Each was fixed by **narrowing the `cfg`**, with no lint
  suppression anywhere. The platform matrix is documented: macOS is a
  **gate**, not a reported platform.
- **The caller-free routing abstraction is removed**, not retained, because
  the one relevant roadmap states the opposite direction.
- **The 615-line `#[cfg(any())]` body is NOT removed.** The exhaustive
  branch-coverage analysis (below) found **three** branch groups the live
  `FocusManager` dispatch does not cover, which is exactly the plan's §14
  stop condition: *"The live `FocusManager` dispatch does **not** cover a
  branch the legacy function handled. Stop: the legacy body is then not pure
  history, and removing it would change behaviour. Record what is missing."*
  The body is therefore **retained**, and what is missing is recorded here
  and in the unresolved-findings table.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| WP1 — repair the guard's patterns so intra-tool delegation is not a finding | `scripts/check_tool_broker_boundary.py` (rewritten); `python3 scripts/check_tool_broker_boundary.py` → exit 0 | pass | Rule adopted: a **registry dispatch** (`ToolRegistry::execute_capture`) is a crossing everywhere outside the registry's own module; a **tool-instance dispatch** (`Tool::execute` / `Tool::execute_structured`) is intra-tool delegation inside `src/tool/` and a crossing outside it. The broker's own four-argument `ToolBroker::execute(registry, name, args, ctx)` is never matched. |
| WP2 — disposition the one genuine finding | `src/permission/reviewer.rs` unchanged; the exception is recorded in the guard with a named owner | pass | **Recorded exception**, not a broker re-route — see §3 and §8. |
| WP3 — wire the guard in | `scripts/verify.sh` `run_guards()`; `.github/workflows/ci.yml` guard step | pass | Both, plus the `--self-test`. Positions: immediately after `check_execution_ownership.py` in both files — the tool-call analogue of the execution-ownership guard. |
| WP4 — resolve the lint-gate platform asymmetry | eight `cfg` narrowings across five files; macOS clippy exit 0 | pass | See §4 for the itemised before/after. |
| WP4 — record the platform matrix and the per-platform expectation | `architecture/testing.md` §"The lint gate's platform matrix"; `plans/registry.md` §Verification policy | pass | macOS is a **gate**, expected green. |
| WP5 — remove the 615-line disabled body | branch-coverage analysis in §3.2 | **not met — stop condition** | Three uncovered branch groups. Body retained; findings recorded. |
| WP6 — dispose the caller-free routing abstraction | `src/agent/router.rs` (488 → 237 lines); §3.3 | pass | Removed, with the roadmap citation. |
| WP7 — register the `research/` residual finding | `plans/registry.md` §Deferred unregistered product work | pass | Registered with a named owner. |
| §10 — a test that the reviewer's call is routed as the disposition requires, and its ceiling is unchanged | `tests/security_permission/reviewer_broker_boundary.inc` — 4 tests, all pass | pass | |
| §10 — a test that the dialog dispatch is unchanged | `tests/tui_render/dialog_dispatch_coverage.inc` — 4 tests, all pass | pass | The branch table's covered half, made executable. |

## 3. Production implementation evidence

### 3.1 The broker-boundary guard

**The rule adopted.** The guard no longer matches a method name everywhere; it
matches the *call shape* and asks where the caller lives:

1. **Registry dispatch** — `ToolRegistry::execute_capture(name, input, ctx)`.
   It looks a tool up *by name* and runs it. Outside the registry's own module
   (`src/tool/mod.rs`) this is **always** a boundary crossing: the registry is
   the thing the broker wraps, so a registry call bypasses the broker's
   caller policy, grant-scope verification, input bounds, deadline check and
   output validation.
2. **Tool-instance dispatch** — `Tool::execute(input)` /
   `Tool::execute_structured(input, ctx)`. Inside `src/tool/` this is
   *intra-tool delegation*: a tool calling its own implementation, a wrapper
   delegating to the inner tool it owns, or one tool calling a sibling tool
   it owns. The broker cannot intercept it — there is no registry lookup and
   no grant to check — and forbidding it would forbid tool composition
   outright. Outside `src/tool/` a production caller has no legitimate way to
   hold a tool instance, so there it is a boundary crossing.

**The five former false positives, and their disposition.** All five are
intra-tool delegation inside `src/tool/`, and all five are now *not findings
by rule*, not by an allowlist entry:

| Former finding | Receiver | Disposition |
|---|---|---|
| `src/tool/extension.rs:43` | `self.execute_structured(input, None)` — the extension tool's own default implementation | intra-tool delegation (rule 2) |
| `src/tool/extension.rs:120` | `self.execute_structured(input, None)` — the same, on the other extension path | intra-tool delegation (rule 2) |
| `src/tool/git_read.rs:145` | `self.inner.execute_structured(input, ctx)` — the wrapper delegating to its inner tool | intra-tool delegation (rule 2) |
| `src/tool/verify.rs:419` | `self.execute_structured(input, None)` — the verify tool's own implementation | intra-tool delegation (rule 2) |
| `src/tool/verify.rs:439` | `self.bash.execute_structured(...)` — one tool calling a sibling tool it owns | intra-tool delegation (rule 2) |

**The one genuine finding — `src/permission/reviewer.rs:789`.**
`self.registry.execute_capture(name, args.clone(), Some(ctx))`, inside
`RegistryReviewerInvestigator::investigate`. **Disposition: a recorded
exception with a named owner — NOT a broker re-route.**

*Why not route it through the broker.* `ToolBroker::execute` requires
`BrokerAuthority::Verified { grant }`, and `validate_pre_execution` refuses
`BrokerAuthority::Unverified` outright
(`src/tool/broker.rs`, invariant 2 in `architecture/tool_broker.md`). The
reviewer's grant would therefore have to come from the very `ApprovalRouter`
the reviewer is servicing — i.e. the reviewer would have to approve its own
investigation. Two independent repository contracts agree:

- `scripts/check_approval_reviewer.py` forbids the reviewer module from
  naming `ApprovalRouter` **at all** (`FORBIDDEN_PATTERNS`), and forbids
  `PermissionPending` registration and any human-approval wait. The
  reviewer's own doc comment states the same isolation contract: it
  *"cannot recursively invoke `ApprovalRouter` or another reviewer"*.
- `architecture/approval_reviewer.md` owns the subsystem; the guard
  `scripts/check_approval_reviewer.py` is its isolation guard.

The reviewer is constructed inside `resolve_automatic_escalation` — i.e.
*inside* the permission-check path that services the router. A broker route
would either recurse into the router (forbidden) or make the reviewer its
own approval issuer (an authorization change, which the plan's §14 lists as a
stop condition).

*Named owner.* **M006 Automatic approval reviewer** —
`src/permission/reviewer.rs`, `architecture/approval_reviewer.md`, isolation
guard `scripts/check_approval_reviewer.py`.

*How the exception is recorded.* In the guard itself, so it is visible where
the rule lives: `RECORDED_EXCEPTIONS` in
`scripts/check_tool_broker_boundary.py` carries the owner, the reason, the
enclosing function (`investigate`) and a call budget of **one**. The guard
resolves the enclosing function of every dispatch and reports a violation for
any registry dispatch in that file that is **not** in `investigate()`, or that
exceeds the budget. Widening the exception therefore requires editing the
guard on purpose.

*Authority-ceiling comparison — unchanged.* The exception changes nothing
about the context the reviewer builds. Every field is asserted from outside
the reviewer by `tests/security_permission/reviewer_broker_boundary.inc`:

| Ceiling | Value | Where it is set | Where it is asserted |
|---|---|---|---|
| Effect class | `read_only` | `reviewer.rs:784` `ctx.max_effect_class = Some("read_only")` | `the_reviewers_authority_ceiling_is_unchanged` |
| Backend policy | `native_only` | `reviewer.rs:785` `ctx.backend_policy = Some("native_only")` + `ToolExecutionContext::with_backend(ToolBackendKind::Native)` | same test |
| Timeout | `15_000` ms | `reviewer.rs:786` `ctx.timeout_ms = Some(15_000)` | same test |
| Output truncation | `REVIEWER_MAX_TOOL_OUTPUT_CHARS` = 8 192 chars | `truncate_bounded` (`reviewer.rs:1343`) | `the_reviewers_output_is_truncated_to_its_budget` |
| Caller class | `approval-reviewer` | `reviewer.rs` ctx build | `the_reviewers_authority_ceiling_is_unchanged` |
| Permission mode | `automatic-reviewer` | `reviewer.rs` ctx build | same test |
| Tool palette | `read`, `glob`, `grep`, `list`, `diff`, `git_read` | `REVIEWER_ALLOWED_TOOLS`, checked **before** the call | `the_reviewer_refuses_a_tool_outside_its_palette` |
| Argument budget | `REVIEWER_MAX_INVESTIGATION_ARG_CHARS` = 4 096 | checked before the call | unchanged |

*The route is pinned at runtime, not only in prose.* The test
`the_reviewers_call_is_a_registry_dispatch_the_broker_refuses` runs the same
call twice: through the reviewer it succeeds and the tool runs; through
`ToolBroker::execute` with an unverified authority it is refused with
`BrokerError::CallerDenied` **before** the tool runs. The two routes are not
interchangeable, which is precisely why the exception is the right
disposition.

### 3.2 The 615-line disabled body — branch-coverage analysis

`src/tui/app/mod.rs:4603-5217` holds `handle_dialog_key_legacy`: a 615-line
body behind `#[cfg(any())]`, therefore **never compiled**, with no callers.
The live route is `App::handle_dialog_key` (`src/tui/app/mod.rs:4536`) →
`FocusManager::handle_key` (`src/tui/components/component/focus.rs:164`) →
the top mounted component's `handle_key`.

The doc comment claims the convergence is complete. **It is not.** The
analysis below is exhaustive over the legacy body's branches; every branch is
listed, not sampled.

| # | Legacy branch (absolute line in `src/tui/app/mod.rs`) | Live counterpart | Covered? |
|---|---|---|---|
| 1 | 4611 — `Dialog::ProjectPicker` → `handle_project_picker_key` | `handle_dialog_key` (`mod.rs:4541`) → `handle_project_picker_key` | ✓ |
| 2 | 4619 — `Dialog::Keybind` → `handle_keybind_key` | `handle_dialog_key` (`mod.rs:4546`) | ✓ |
| 3 | 4624 — `Dialog::Terminal` → `handle_terminal_key` | `handle_dialog_key` (`mod.rs:4551`) | ✓ |
| 4 | 4629 — `Dialog::Model` + `Tab` → `model_dialog.next_tab()` | `FocusManager::handle_key` intercepts `Tab` for its focus ring and never forwards it to the component; `ModelDialog` does not override `set_focused`, so the call is a no-op. `ModelDialog::handle_key`'s own `Tab` arm (`model.rs:1032`) is therefore **unreachable through the FocusManager** | **✗ NOT COVERED** |
| 5 | 4634-4656 — `Dialog::ShellShow` + `Char('i'\|'a'\|'r'\|'k'\|'e')` → `ShellInclude` / `/shell-ask` / `ShellRerun` / `ShellKill` / `/shell-expand` | `DialogType::ShellShow` is an `InfoDialog`; `InfoDialog::handle_key` handles `Up`/`Char('k')` (scroll up), `Down`/`Char('j')` (scroll down), `Char('a')` **only** for `InfoType::ProjectInit`, `Enter`, `Esc`. None of `i`/`a`/`r`/`e` is handled, and `k` scrolls up instead of killing | **✗ NOT COVERED** |
| 6 | 4694-4722 — `Cancel`: theme picker cleared; MCP action mode exited or dialog closed; Session bulk mode toggled or preview cleared + closed; Connect back-step or closed; `Context`/`Cost`/`Usage`/`ShellShow`/`Terminal`/`Stats`/`TaskList`/`MemoryResults`/`DoctorReport` closed; else `import_dialog = None` + closed | Each component's `Esc` → `TuiMsg::CloseDialog` or its own refinement (MCP action mode, Session bulk, Connect back-step, Theme revert-on-preview). The **Import** arm's `import_dialog = None` is not in the live `close_dialog` (`mod.rs:10118` cancels the task and the request instead) | ✓ (the Import sub-clause differs — see finding 3) |
| 7 | 4732-4806 — `NavigateUp`: Model, Agent, Session, Tree, Theme, Question, Permission, Mcp, Template, Connect (SelectTls cycles the TLS policy backwards) | Each component's `Up` (`model.rs:1040`, `agent.rs`, `tree.rs`, `theme.rs`, `question.rs`, `permission.rs`, `mcp.rs`, `template.rs`, `connect.rs:18`); `ConnectStep::SelectTls` cycles the TLS policy through `connect.rs:791` | ✓ |
| 8 | 4790-4864 — `NavigateDown`: the same set, `SelectTls` cycles forwards | the same, `connect.rs:791` | ✓ |
| 9 | 4866-5030 — `Send`: Model (add / select), Agent (select), Session (bulk toggle / activate), Theme (commit), Question (submit), Permission (confirm), Mcp (action dispatch), Share (copy URL), Import (`handle_import_send`), Template (apply), Connect (send) | `TuiMsg::SelectModel` / `SelectAgent` / `SelectSession` / `ThemeCommit` / `SubmitQuestionAnswers` / permission `commit` / `McpAction` / `CopyShareUrl` / `SubmitImportPreview` / `SelectTemplate` / `SubmitConnect`, each handled in `src/tui/app/input.rs`. The MCP action set is **identical** — "Configure OAuth", "Browse Resources", "Disconnect", "Reconnect", "Connect", "Remove", "Wait", "Configure" (`input.rs:288-341`) | ✓ |
| 10 | 5031-5171 — `Char(c)`: Model (add-model fields / Configure `a`,`d` / filter), Agent (filter), Session (bulk `a`,`d`,`A`,`D`,` ` / `s` / `h` / `b` / filter), Tree (`e` expand, `f` fork), Question (answer), Import (`set_input`), Template (filter), Connect (insert char) | Model `model.rs:1061-1079`; Agent `agent.rs`; Session `session.rs:980-1033` (`s` sort, `h` archived, `b` bulk, bulk `a`/`d`/`A`/`D`/` `); Tree `tree.rs` (`e`, `f`); Question `question.rs`; Template `template.rs`; Connect `connect.rs:127`. **Import: the live `ImportDialog::handle_key` handles only `Esc` and `Enter`** | ✓ except **Import → ✗ NOT COVERED** |
| 11 | 5172-5190 — `Backspace`: Model (pop base URL / API key / provider / name / filter), Agent, Session, Question, Import (`backspace`), Template, Connect | Model `model.rs:1080`; Agent; Session `session.rs:1034`; Question `question.rs`; Template `template.rs`; Connect `connect.rs:121`. **Import: not handled live** | ✓ except **Import → ✗ NOT COVERED** |
| 12 | 5193-5197 — `Delete`: Question | `question.rs` `KeyCode::Delete` | ✓ |
| 13 | 5200-5204 — `Left`: Question | `question.rs` `KeyCode::Left` | ✓ |
| 14 | 5207-5211 — `Right`: Question | `question.rs` `KeyCode::Right` | ✓ |

**Three uncovered branch groups.** They are the plan's §14 stop condition,
and the body is therefore **retained**. Each is a **live-path gap**, not a
regression introduced here:

1. **The Model dialog's `Tab`.** The Model dialog has two tabs
   (`SelectModel` / `Configure`), and the Configure tab is where a custom
   model is added. `FocusManager::handle_key` consumes `Tab` for its focus
   ring before any component sees it, and `ModelDialog` does not implement
   `set_focused`, so the tab cannot be switched from the keyboard. The
   `ModelDialog::handle_key` `Tab` arm (`model.rs:1032`) is dead code.
2. **The `/shell-show` modal's five verbs.** The modal's own footer
   (`src/tui/commands/shell.rs:1451`) still reads
   *"i include | a ask | r rerun | k kill | e expand | j/k scroll | Esc close"* —
   but `InfoDialog::handle_key` implements none of `i`/`a`/`r`/`e`, and `k`
   scrolls **up** where the footer says it kills. The verbs exist as slash
   commands (`/shell-include`, `/shell-ask`, `/shell-rerun`, `/shell-kill`,
   `/shell-expand`), and `TuiMsg::ShellInclude` / `ShellAsk` / `ShellRerun` /
   `ShellKill` have handlers in `src/tui/app/input.rs:1276-1305` with **no
   producer** — the legacy body was the producer's counterpart.
3. **The Import dialog's text input.** The `/import` slash command opens the
   dialog (`src/tui/app/mod.rs:6602`), and `open_dialog` pushes a **clone**
   into the focus manager (`src/tui/app/modal.rs:457`). The live
   `ImportDialog::handle_key` handles only `Esc` and `Enter`, so a typed
   character never reaches the clone; and the app reads the **App-owned**
   `dialog_state.import_dialog` in `handle_import_send`, so even the
   clone's `handle_paste` would not reach the preview. The legacy body is the
   only implementation of that input.

**Why the body was not removed anyway.** The plan's §14 gives the stop
condition and its reason: *"the legacy body is then not pure history, and
removing it would change behaviour. Record what is missing."* Removing a
body that is `#[cfg(any())]` cannot change runtime behaviour — but it would
destroy the **only** written record of three UI behaviours, one of which the
live UI still advertises on screen. The branch table above is the replacement
for the comment, and it is a better record than the comment was, because it
names the live counterpart for every branch.

**What would unblock the removal.** A decision, in one of two shapes:
(a) implement the three branches in the live dispatch — which is a **live
behaviour change** and therefore outside this milestone's scope; or (b)
retire the corresponding UI surfaces (drop the `/shell-show` modal's verb
footer, drop the Model dialog's Configure tab or give it a real focus ring,
and either implement or drop the Import dialog's text input). Once either
shape lands, the body is dead history and can be deleted, and the body is
**behaviour-neutral** to delete in the meantime — the branch table is the
handover.

### 3.3 The caller-free routing abstraction

`src/agent/router.rs` held two things. The **live** half — `ModelRouter`,
`route_model`, `classify`, `TaskComplexity` — is used by the turn's request
preparation (`src/agent/request_preparation.rs:828-838`) and constructed at
`src/agent/loop.rs:251`; it is **retained**. The **dead** half —
`TaskEnvelope`, `classify_envelope`, `route_from_envelope` — had **no**
production caller anywhere in `src/`, `crates/` or `tests/`; only the file's
own test module referenced them, and `route_from_envelope` had no caller at
all. It is **removed**.

The two enums that existed only to populate the envelope — `MutationRisk` and
`TaskBreadth` — are removed with it: they had no other referent, so leaving
them would have left the same dead code the milestone exists to remove. The
vestigial `#[allow(dead_code)]` on the retained `ModelRouter` is also
removed: `router` is a `pub mod` in a `pub mod agent` in a `pub` library, so
every public item in it is externally reachable and the suppression covered
nothing.

**The roadmap citation.** `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md:274`
states: *"No persistent repository index or background model router is
introduced."* The one relevant roadmap therefore states the **opposite**
direction from the envelope abstraction: an envelope-shaped routing
abstraction with no caller is not a planned capability awaiting a consumer.
This is the plan's §14 condition — *"The caller-free routing abstraction
turns out to be a **planned capability** that the registry does not record.
Stop and record the gap; do not delete a planned capability"* — and it does
**not** fire: the abstraction is not planned. No registry row exists for it,
and the registry's active and deferred sections do not mention it.

Result: `src/agent/router.rs` went from 488 lines to 237 lines.

### 3.4 The lint-gate platform asymmetry

Ten findings at the baseline, in three targets, all of them code no macOS path
calls. Every one was fixed by narrowing the `cfg`; **there is no
`#[allow(...)]` anywhere in the change.**

| Site | Finding | Fix |
|---|---|---|
| `src/interactive_process.rs:1469` | unused variable `shutdown` | the parameter is cfg-split by name — `#[cfg(target_os = "linux")] shutdown` / `#[cfg(not(target_os = "linux"))] _shutdown` — the pattern `run_inner` already used for `helper_override`. Only the Linux sandboxed-interactive path observes the token. |
| `src/interactive_process.rs:1131` | `needless_return` | the `#[cfg(not(target_os = "linux"))]` arm became the function's tail expression instead of a `return`, so the `#[cfg(target_os = "linux")]` arm is the tail on Linux. Behaviour identical on both platforms. |
| `src/managed_process.rs:870` | unused `mut` on `request` | `#[cfg(target_os = "linux")] let mut request = request;` / `#[cfg(not(target_os = "linux"))] let request = request;` — the request is mutated only by the Linux `OfflineRequired` tightening. |
| `src/managed_process.rs:1977` | unused variable `close_unapproved_fds` | cfg-split parameter, the same pattern as `shutdown` above. Closing the unapproved descriptors is a Linux `close_range` operation. |
| `src/managed_process.rs:235` | `sandbox_interactive_entries` never used | `#[cfg(target_os = "linux")]`. Its only callers are `spawn_sandboxed_interactive_child` (Linux) and `src/lsp/m004.rs:429` (a Linux-gated `launch`). The plain `sandbox_entries` variant stays cross-platform because the managed launch uses it. |
| `src/managed_process.rs:2715` | `spec_with` never used | `#[cfg(target_os = "linux")]` — the three tests that use it assert the Linux network-boundary contract. |
| `src/security/sandbox.rs:781` | `SandboxPathIdentity::capture` never used | `#[cfg(target_os = "linux")]`. Its only caller is the Linux protected-mount view. The `#[cfg(not(unix))]` variant is removed with it. |
| `src/lsp/m004.rs:106` | `new` never used | the non-Linux `#[cfg(test)]` constructor is **removed**: its only callers are Linux-gated test modules. |
| `src/lsp/m004.rs:115` | `new_with_network_intent` never used | the non-Linux constructor is **removed**: the only production caller (`src/agent/turn_runtime.rs:331`) is Linux-gated and yields `None` on every other target. |
| `src/bin/codegg-sandbox-helper.rs:30` | unused import `Seek` | `use std::io::Seek;` is now `#[cfg(target_os = "linux")]`, gated with the Linux seccomp filter builder that uses it. `Write` stays `#[cfg(unix)]`. |

The last one was **masked** at the baseline: the lib's failure stopped the
build before the bin target was reached, so a `-D warnings` run never saw it.
It surfaced only after the lib was fixed.

**The Linux result is unchanged and green.** Every finding was a site the
Linux build compiles and the macOS build does not, and every fix is a
narrowing that leaves the Linux compilation path byte-for-byte equivalent:
the `shutdown` and `close_unapproved_fds` parameters keep their Linux names
and their Linux uses; the `request` binding keeps its `mut` on Linux; the
`next_tab`-adjacent code is untouched; `sandbox_interactive_entries`,
`spec_with` and `SandboxPathIdentity::capture` keep their Linux bodies under
a `target_os = "linux"` gate; and the two removed `src/lsp/m004.rs`
constructors were `#[cfg(not(target_os = "linux"))]`, so Linux never compiled
them. No Linux-only test or workflow was touched.

### 3.5 The `research/` residual finding

**Registered, not fixed.** Five standalone Rust crates — 3 448 lines — sit
outside the workspace: `gix-bench` (387), `two-hop-native-diagnostic` (155),
`mxc-c033-harness` (661), `mxc-c035-harness` (1 684) and `mxc-v1-harness`
(561). They have their own lockfiles, are not workspace members, and so are
compiled by neither `cargo clippy --workspace --all-targets --locked` nor
`cargo fmt --all`.

This is a **lint-reach gap, not dead code**. Every crate is a live research
harness with a recorded closure, and nothing in this milestone may delete
them. Four are compiled by **Windows-only** workflows (`windows-c044-two-hop-native`,
`windows-mxc-c033-requalification`, `windows-mxc-c035-helper-corrective`,
`windows-mxc-c036-capture`, `windows-mxc-c037-handle-boundary`) — those run
`cargo test` / `cargo check` / `cargo run`, **not** clippy, so the crates are
compiled on CI but never linted anywhere; `gix-bench` is built by **no
workflow at all**, only by the manual commands recorded in
`research/linux-trusted-git-cost-corrective/c003-gix-vs-cli-benchmark.md`.

**Named owners:** the Windows native-sandbox line
(`research/windows-native-sandbox/`) and the Linux trusted-Git cost line
(`research/linux-trusted-git-cost-corrective/`). **Decision needed:** whether
these harnesses join a clippy/fmt gate of their own, or whether their
evidence value is explicitly accepted as "compiled but unlinted". Until that
decision lands, the honest statement is that **no lint result exists for
those 3 448 lines on any platform**. Recorded in `plans/registry.md` §"Deferred
unregistered product work".

## 4. Verification executed

### Commands run

```bash
# the guard
python3 scripts/check_tool_broker_boundary.py
python3 scripts/check_tool_broker_boundary.py --self-test

# the lint gate, on macOS (this machine)
cargo clippy --workspace --all-targets --locked -- -D warnings

# the new tests
cargo test --test security_permission -- reviewer_broker_boundary
cargo test --test tui_render -- dialog_dispatch_coverage

# the counts
python3 scripts/measure_overview_counts.py
```

### Results

| Command | Result |
|---|---|
| `python3 scripts/check_tool_broker_boundary.py` | **exit 0** — "OK: No direct tool execution calls outside the broker boundary." plus the recorded exception in force, printed with its owner and reason. |
| `python3 scripts/check_tool_broker_boundary.py --self-test` | **exit 0** — six assertions, all reported "ok": compliant tree accepted (broker call, intra-tool delegation); injected tool-instance dispatch outside `src/tool/` rejected; injected registry dispatch rejected; recorded exception covers its single call; widened recorded exception rejected; unmodified tree still passes. |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` (macOS, before) | **exit 1** — 9 distinct findings across the lib (7) and the lib test (2), plus a 10th in the bin test that the lib's failure masked. Itemised in §3.4. |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` (macOS, after) | **exit 0** — "Finished `dev` profile … in 7m 49s", zero errors, zero warnings. |
| `cargo test --test security_permission -- reviewer_broker_boundary` | **pass** — 4 passed, 0 failed, 325 filtered out. |
| `cargo test --test tui_render -- dialog_dispatch_coverage` | **pass** — 4 passed, 0 failed. |
| `python3 scripts/measure_overview_counts.py` | `CI guard scripts` **41** (was 40), `Integration test files` **329** (was 327), `Guard scripts (all)` **80** (unchanged), `Storage layout version` **88** (unchanged). |

**Not run, and why.** `scripts/verify.sh full` and a workspace-wide test
sweep were **not run locally**: this milestone's testing discipline shares
one machine with three sibling milestones and forbids
`cargo nextest run --workspace`, `cargo test --workspace`, `cargo test`
without `--test`, and `verify.sh full`. The hosted CI `verify` job is the
authority for the workspace sweep and for the Linux lint result, and **no
hosted CI run is claimed by this record**. `scripts/verify.sh scoped` and
`scripts/verify.sh quick` results are recorded in the branch report.

### The guard's cost

The newly-wired guard adds **≈1.2 s** to `verify.sh quick` (measured: 0.52 s,
0.60 s, 1.20 s for three consecutive runs of the guard; 0.57 s for the
`--self-test`). That is inside the verification policy's budget, so §14's
budget stop condition does not fire.

## 5. Invariant review

| Source-plan invariant | Evidence | Result |
|---|---|---|
| The broker boundary contract is unchanged; no tool call is moved on or off the broker | No production code in `src/tool/`, `src/agent/` or `src/permission/` changed its call routing. The reviewer's call is still a registry dispatch. The guard *enforces* the existing contract; it defines no new one. | pass |
| The permission reviewer's behaviour is unchanged except for how its tool call is routed | Nothing in `src/permission/reviewer.rs` changed at all. The route is unchanged too (it was already a registry dispatch) — what changed is that the guard now records it rather than failing on it. Ceiling asserted field-by-field in §3.1. | pass |
| CI must not become weaker | CI gained two steps (the `--self-test` and the guard) and lost none. No existing check was relaxed or removed. | pass |
| Every guard added must be non-vacuous, proved by injection | `check_tool_broker_boundary.py --self-test` injects a tool-instance dispatch outside `src/tool/`, a registry dispatch, and a widened exception, and shows each rejected; it also shows a compliant tree accepted and the unmodified tree still passing. | pass |
| The documented verification commands stay accurate | `architecture/testing.md` §"The lint gate's platform matrix" replaces the stale "known macOS-local gate failures" section; `architecture/tool_broker.md`, `architecture/tool.md`, `architecture/overview.md`, `plans/registry.md` and `architecture/count_audit_history.md` all carry the change. | pass |
| The `research/` crates keep building | Nothing under `research/` was touched. The Windows workflows that build them are untouched. | pass |
| Historical closure records are untouched | No file under `plans/closure/` was modified — including `plans/closure/memory-skill-promotion/002-status.md` and `003-status.md`, which name the parked guard's red state. | pass |

## 6. Failure and recovery review

- **Failure.** A guard that fails is a **build failure**, not a runtime
  failure. The plan forbids an `|| true` escape, a failure suppression, or an
  allowlist entry that silently tolerates the class the guard exists to
  catch. The change adds none: the guard exits 1 on any finding, and the one
  exception is a **scoped, owned, counted** record — a second dispatch in the
  same file, or one in a different function, is a finding.
- **Cancellation.** No runtime change.
- **Restart.** No durable state change.
- **Contention.** The guard is stateless and reads the tree; it holds no lock
  and is safe to run concurrently with another instance of itself.
- **Malformed or unauthorized input.** The guard is a static analyser: it
  reads `.rs` files under `src/`, skips unreadable files, and reports a
  malformed line as no dispatch rather than crashing. A malformed tree makes
  it report fewer findings, not more — the one class of failure that matters
  for a static guard is a *silent* pass, which is why the `--self-test`
  exists.

## 7. Migration and compatibility review

- **Schema migration:** none. `STORAGE_LAYOUT_VERSION` is unchanged at 88.
- **Backward compatibility:** the removed routing types were `pub` in a `pub`
  module of the library crate. They had no in-repository caller, and the
  repository's own roadmap states the opposite direction from that
  abstraction, so they were removed rather than retained. This is a
  source-level API change for any *external* consumer of `codegg::agent::router`;
  the crate is an application, not a published library, and no consumer
  exists in this repository.
- **Protocol negotiation:** unchanged.
- **Configuration behaviour:** unchanged. The reviewer's `ReviewerConfig` and
  the lint gate's configuration are untouched.
- **Rollback limitations:** the change is a single conventional commit on one
  branch; reverting it restores the baseline exactly, including the red
  guard and the red macOS lint result.
- **Legacy-path status:** the `handle_dialog_key_legacy` body is **retained**,
  still `#[cfg(any())]`, still uncalled. It is not a supported path and was
  never one.

## 8. Security review

- **Authorization enforcement.** Unchanged. The broker's pre-execution
  pipeline is untouched, and the reviewer's recorded exception does **not**
  create a new path into the broker — it records an existing, deliberately
  narrow registry dispatch.
- **The exception's blast radius.** One file, one function, one call. The
  guard enforces that scope. The exception's owner is a named subsystem with
  its own isolation guard, and the reviewer's ceiling is asserted from
  outside in the test, so a future widening of the ceiling fails the test
  rather than passing quietly.
- **Privilege boundaries.** The reviewer still runs under `read_only` /
  `native_only` / 15 s / 8 192-char truncation, and its tool palette is still
  the six read-only tools. Nothing in this milestone widens it.
- **Secret handling.** Unchanged. The reviewer's output truncation and the
  broker's artifact spillover are untouched.
- **Path validation.** Unchanged.
- **Denial-of-service bounds.** Unchanged. The guard's own cost is ~0.6 s.
- **Audit behaviour.** Unchanged; the broker's trusted-audit seam is
  untouched.
- **A note on what the exception is not.** It is not a bypass introduced by
  this milestone: the registry dispatch existed at the baseline, is
  constrained by the reviewer's own palette and ceiling, and is enforced by a
  second guard (`scripts/check_approval_reviewer.py`) that forbids the
  reviewer from reaching the approval router at all.

## 9. Documentation and operations

| Document | Change |
|---|---|
| `architecture/tool_broker.md` | new §"The broker boundary guard" — the rule, the recorded exception with its owner and reason, and the `--self-test`. Links the guard; does not restate its implementation. |
| `architecture/tool.md` | §"ToolContracts and the Canonical Broker" gains the guard's static-guard note and a pointer to the exception. |
| `architecture/core.md` | unchanged — the broker boundary is not a `src/core/` transport-facade concern, and the guard's blast radius does not include `src/core/`. The doc's own "Static guard" note convention is followed in `tool_broker.md` instead. |
| `architecture/testing.md` | §"Known macOS-local gate failures" replaced by §"The lint gate's platform matrix" — the matrix, the per-platform expectation, the ten itemised findings, and the masked tenth. |
| `architecture/overview.md` | Verified counts: `CI guard scripts` 40 → **41**, `Integration test files` 327 → **329**. |
| `architecture/count_audit_history.md` | third amendment: why each of the two rows moved, and that the sibling branches' own re-measurements are expected to conflict at merge time. |
| `plans/registry.md` | the C005 row in §Blocked work records the branch outcome and the unblock audit; §Verification policy gains the platform matrix and the broker guard's place in the documented surface; §Deferred unregistered product work gains the `research/` residual finding with its named owners. |
| `plans/implementation/repository-audit-corrective/005-...md` | unchanged — the plan is the contract, not the record. |

**Operator diagnostics.** A red broker-boundary guard now names the file, the
line, the offending text, and — for the reviewer's file — the recorded
exception it did not match. A red lint gate names the site.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| **medium** | The live `FocusManager` dispatch does not cover three branches `handle_dialog_key_legacy` handled: the Model dialog's `Tab`; the `/shell-show` modal's `i`/`a`/`r`/`k`/`e` verbs (which the modal's own footer still advertises, and where `k` scrolls up instead of killing); and the Import dialog's text input. | Three live UI affordances are unreachable or mislabelled. The `/shell-show` modal advertises five verbs it does not implement. The Model dialog's Configure tab cannot be reached from the keyboard. The `/import` dialog cannot be typed into. | A decision outside this milestone's scope: implement the three branches in the live dispatch (a live behaviour change), or retire the corresponding UI surfaces. The exhaustive branch table in §3.2 is the handover. **Until then the 615-line body stays.** Owner: the TUI line. |
| **medium** | The five `research/` crates (3 448 lines) are compiled by Windows-only workflows that do not lint them, and `gix-bench` is built by no workflow at all. | No lint result exists for 3 448 lines of Rust on any platform. A defect in those harnesses would not be caught by any gate. | The owners named in §3.5 decide between a `research/` clippy/fmt gate and an explicit "compiled but unlinted" acceptance. Registered in `plans/registry.md` §Deferred unregistered product work. |
| **low** | The macOS lint result is enforced locally, not hosted. | A macOS regression that only manifests on a clean machine would not be caught by CI. | A hosted macOS lane is a separate decision with its own cost; explicitly out of scope for this milestone and recorded as such. |
| **low** | `scripts/check_execution_subject_ownership.py` fails on `crates/egggit/src/refs.rs` at the baseline. | Pre-existing and **proven unrelated** to this workstream — byte-identical output on a pristine `origin/main` tree. It is in neither `verify.sh quick` nor CI. | Not this milestone's. Recorded here so the red result is not mistaken for a regression. |
| **low** | `TuiMsg::ShellInclude` / `ShellAsk` / `ShellRerun` / `ShellKill` have handlers with no producer. | The live shell verbs are reachable only as slash commands; the single-key paths have no producer. | Follows from the first finding; the same decision resolves it. |

## 11. Roadmap disposition

**Milestone conditionally closed**, with one named condition:

- **Closed and evidenced:** the broker-boundary guard (repaired,
  dispositioned, wired, non-vacuous); the macOS lint surface (clean, by
  `cfg` narrowing, no suppression); the platform matrix (documented);
  the caller-free routing abstraction (removed, with the roadmap citation);
  the `research/` residual finding (registered with a named owner); the
  documented verification surface (updated in every place it is stated).
- **Conditionally closed:** the 615-line disabled body. The plan's §14 stop
  condition fires on it, so the body is **retained** and the three missing
  branches are recorded. The condition to discharge it is a decision on
  those three branches, not more work on the body. **The body is
  behaviour-neutral to delete in the meantime** — it is `#[cfg(any())]` and
  uncalled — and §3.2's branch table is the handover that makes deleting it
  safe once the decision lands.
- **Not claimed:** any hosted CI run. The branch's local evidence is
  `verify.sh quick`, the clippy gate, and the two new test members; the
  workspace sweep and the Linux lint result are the hosted CI job's to
  produce.

## 12. Registry updates

- `plans/registry.md` §Blocked work — the C005 row now records the branch
  outcome: the guard repaired and wired, the reviewer's recorded exception,
  the macOS lint clean, the router removed, and the 615-line body **not**
  removed because §14 fires. It stays in §Blocked work because **C001–C004
  are not closed in this tree** — C001 and C002 land on sibling branches that
  have not been merged here.
- `plans/registry.md` §Verification policy — the platform matrix and the
  broker guard's place in the documented surface.
- `plans/registry.md` §Deferred unregistered product work — the `research/`
  residual finding with its named owners and the specific evidence.
- **Unblock audit of §Blocked work:** **nothing is unblocked by this
  milestone alone.** C006 explicitly does not wait on C005 ("C002, C004 and
  C005 do not block it"). C007 waits on C001–C006, so this milestone
  discharges one of its five prerequisites — but C007 stays blocked on
  C001–C004 and C006. No other row names C005. The workstream's own
  dependency (C001–C004 closing) is unchanged by this branch.
- `architecture/overview.md` §Verified counts and
  `architecture/count_audit_history.md` — the two moved rows and their
  reconciliation.
