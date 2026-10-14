# Agent Tool Runtime Post-Closure Corrective C003 — Closure Record

Status: conditionally closed

Plan: `plans/implementation/agent-tool-runtime-reconciliation-post-closure-corrective/003-guard-ci-and-integration-qualification.md`

Source roadmap: `plans/subsystems/agent-tool-runtime-reconciliation-post-closure-corrective-addendum.md#4-milestones-and-dependencies`

Integration baseline: current `origin/main` `667005a66e491efae6a28fca43931e05e94214f8`, fetched and rebased before final qualification. C001 and C002 hard dependencies are accepted.

Implementation and closure SHA ledger:

| Work | Original / audit SHA | Replayed on `b9ba494c` | Replayed on `a7b116af` | Replayed on current `667005a6` |
|---|---|---|---|---|
| C001 retry admission | `8d13bfd6` | `e914cc7c` | `411e8474` | `27d88df5` |
| C001 closure record | `d458c4d8` | `c4f6a3f8` | `6841c151` | `37e8e9f9` |
| C002 descendant authority | `d213150c`, `7ced77b0` | `f20721ba`, `77266575` | `94e1ea42`, `c4275def` | `ecf63e07`, `f99a267e` |
| C002 closure record | `4941a845` | `72be3323` | `08a11dee` | `94fc159e` |
| C003 guards, workflow wiring and docs | `6a134a84` | `6a134a84` | `3528ef6e` | `3528ef6e` |

C003 implementation commit `3528ef6e` is based on `667005a6`. This record and the status updates are documentation-only and follow the implementation commit. Historical M001–M004 closure records were not edited. C001 and C002 closure records remain historical evidence; current-main replay SHAs are added to this ledger and to the active roadmap/registry.

## 1. Executive finding

C003 installed and enforced the tool-runtime guard set in `scripts/verify.sh quick` and hosted CI, added offline positive/negative fixtures for every guard, documented the guard contract and updated the Desktop E2E workflow with manual dispatch. The branch was rebased onto current main without conflicts or lost commits. Current-main quick verification, workspace Clippy and all seven focused tool-runtime integration binaries pass.

C003 is **conditionally closed**, not strictly closed. The complete workspace CI-profile run on current main exposed four reproducible TUI failures introduced by upstream commits `845d77db` and `667005a6`, outside this branch's runtime diff. Exact-ref hosted CI failed in upstream Clippy code. Desktop E2E passed its build, renderer, host and boundary checks; six WebDriver scenarios passed and the deterministic M004 transcript scenario failed because the expected first assistant narration was absent after the turn completed. These findings block a green full-qualification claim and are not silently attributed to this tool-runtime corrective.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| C001 and C002 accepted before qualification | Separate accepted closure records; current replay ledger above | Pass |
| All predecessor and corrective source guards execute in quick | Current-main `scripts/verify.sh quick` ran broker adapter, descriptor snapshot, descendant ceiling, disclosure wire, retry admission and fixture runner; each guard accepted checked-in source | Pass |
| Every guard rejects a forbidden mutation | `scripts/test_tool_runtime_guards.py` mutated each of the five source boundaries; all five guards rejected their negative fixture | Pass |
| Hosted CI uses the same explicit guard order | [CI run 38482051870](https://github.com/dbowm91/codegg/actions/runs/38482051870), exact SHA `453ecd1b943c227b6ea39477f22b63eef7aad338` | All tool-runtime guards, mutation fixtures and format passed; Clippy failed on upstream `src/tui/components/messages.rs:4043`; workspace tests were skipped |
| Current-main integration preserved unrelated work | Rebased from `a7b116af` onto fetched `667005a6`; the two upstream commits changed TUI paths, with no conflicts. The complete TUI changes and tests remain present. | Pass |
| C001/C002 focused runtime behavior after final rebase | Seven focused binaries passed, 173 tests total | Pass |
| Full workspace and optional-feature qualification | Current-main broad suite has four reproducible upstream TUI failures; the required optional feature suite also exposed the upstream poolless shell test failure on the preceding `a7b116af` base. Details in sections 4 and 10. | Conditional |
| Exact-ref hosted CI and Desktop E2E | [CI run 38482051870](https://github.com/dbowm91/codegg/actions/runs/38482051870) and [Desktop E2E run 38482058408](https://github.com/dbowm91/codegg/actions/runs/38482058408), both at SHA `453ecd1b943c227b6ea39477f22b63eef7aad338` | CI failed in upstream Clippy; Desktop passed six WebDriver scenarios and failed one transcript assertion |

## 3. Production and infrastructure changes

C003 makes no new production runtime or storage change. Commit `3528ef6e` adds `scripts/check_tool_retry_admission.py`, adds positive/negative mutation fixtures in `scripts/test_tool_runtime_guards.py`, wires those plus the four predecessor guards into canonical quick and `.github/workflows/ci.yml`, adds `workflow_dispatch` to `.github/workflows/desktop-e2e.yml`, and reconciles `architecture/testing.md`, `architecture/tool_broker.md`, `architecture/agent-tool-surface.md`, and the C001 plan status.

The quick order now includes all repository guards, then M001 broker adapter, M002 descriptor snapshot, M003 descendant ceiling, M004 disclosure wire, C001 retry admission and the offline mutation fixture runner. Missing guard inputs fail closed. Fixtures need no network or credentials.

## 4. Verification executed on current-main base `667005a6`

Commands used Rust 1.89 on macOS arm64 with `CARGO_BUILD_JOBS=2`.

- `bash scripts/verify.sh quick` — passed. This includes `cargo fmt --check --all`, all repository guards, all five runtime guards, all five negative mutation fixtures, and `cargo check --workspace --all-targets --locked`.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — passed.
- `cargo nextest run -p codegg --locked --profile ci -E 'binary(agent_tool_m002_discovery) | binary(agent_tool_m003_descendant) | binary(provider_tool_disclosure_wire) | binary(agent_loop_harness) | binary(fake_eggsearch_mcp) | binary(tool_broker_integration) | binary(subagent)'` — 173 passed across seven binaries.
- `cargo nextest run --workspace --locked --profile ci --no-fail-fast` — 13,581 passed, four failed. The four failing assertions are listed in section 10 and reproduce when those tests are run alone.
- `cargo nextest run -p codegg --locked --profile ci -E 'test(the_agents_command_loads_its_panel_instead_of_recursing) | test(the_agents_panel_takes_the_keyboard) | test(the_focused_panel_advertises_its_own_keys) | test(the_session_panel_advertises_its_own_keys)'` — the same four tests failed independently.
- `ruby -e 'require "yaml"; ARGV.each { |f| YAML.load_file(f) }' .github/workflows/ci.yml .github/workflows/desktop-e2e.yml` — both workflow files parsed.
- `git diff --check` — passed.

Additional prior-base qualification: on `a7b116af`, the full workspace `verify.sh full` reached 1,074 tests before the known asset-refresh coalescing test hit its 120-second nextest timeout; a standalone run of that test passed. The newly merged current-main revision supersedes that base, and the current-base no-fail-fast run completed with the four TUI failures above.

The required `server,plugins,lsp-test-support` nextest run was attempted on `a7b116af`. A no-fail-fast run completed with 10,565 passed and one failed: upstream `core::daemon::tests::poolless_standalone_human_shell_uses_registered_workspace`. It also failed in isolation with those features, while the corresponding test passed in isolation with default features. The current `667005a6` delta does not touch `src/core/daemon.rs`. This optional-feature result is retained as evidence; it is not described as passing on the final base.

## 5. Invariant review

- AgentLoop has one dispatch owner for a model tool call; a batch no longer retries an ambiguous, non-idempotent external effect.
- The broker remains the only production tool-call boundary and admits explicit retry-safe behavior only under the ToolBroker contract.
- Descendant calls bind to the exact child session/workspace and inherited per-tool effect/name envelope at broker admission.
- Functional nested `task` is present before surface resolution when authorized and is narrowed before the child loop starts.
- The guards assert these ownership boundaries and fail on missing source or a forbidden mutation; they do not establish live provider interoperability or enable native tool search.
- No C003 code changed sandbox authority, provider allowlists, persistence schema, permissions, or child grants.

## 6. Failure, recovery and concurrency

C001's fake-MCP timeout-after-dispatch and native ambiguous-effect regressions prove one dispatch rather than caller-owned replay. C002's root→child→grandchild test, sibling/session rejection and effect-drift broker tests prove positive nesting and denial before execution. Their details remain in the accepted C001/C002 closure records.

C003's full workspace failures are in TUI panel behavior and panel key advertising. They reproduce independently and are outside the C003 code diff. The suite reports no C001/C002 regression among the focused binaries. On the prior current-main base, the optional-feature standalone shell test also failed independently; that test was introduced by mainline commit `9f9bde93` and is outside this branch's diff.

No test cancellation or partial hosted run will be counted as green. The exact-ref Desktop E2E run remains in progress; its result must be appended after completion.

## 7. Compatibility and migration

No protocol or storage migration was added. The additive `parent_authority` payload, existing model tool schemas and tool names are unchanged. No provider-native tool-search allowlist was enabled. The final integration base is current main `667005a6`; the rebase completed cleanly and retained the upstream TUI commits.

## 8. Security and trust-boundary review

The new static checks inspect code ownership and fail when a guard input disappears. Mutation fixtures prove both accepted source and representative forbidden edits. They are narrow source checks, not substitutes for the C001/C002 behavioral fixtures, broker admission, permission checks or sandbox tests. No secret-bearing data is printed by the guards or fixtures.

No new security finding was identified in the C003 implementation. The outstanding full-suite failures are correctness/qualification findings in upstream TUI behavior and are not security claims.

## 9. Documentation and operations

Updated `architecture/testing.md`, `architecture/tool_broker.md`, `architecture/agent-tool-surface.md`, `plans/registry.md`, and the corrective addendum. The registry records C001/C002 replay SHAs through current main `667005a6`, the C003 conditional disposition, and qualification findings. Workflow YAML was parsed locally. Hosted run links and final outcomes are recorded above.

## 10. Unresolved findings

- **Medium, qualification blocker — upstream TUI panel regressions.** On `667005a6`, these tests fail in the full suite and independently: `tui_agents_panel::the_agents_command_loads_its_panel_instead_of_recursing`, `tui_agents_panel::the_agents_panel_takes_the_keyboard`, `tui_chat_surfaces::the_focused_panel_advertises_its_own_keys`, and `tui_chat_surfaces::the_session_panel_advertises_its_own_keys`. The mainline TUI implementation came in `845d77db` and `667005a6`; none of these files are changed by the tool-runtime corrective. Owner is the TUI workstream. C003 makes no speculative fix to unrelated TUI code.
- **Medium, optional-feature qualification gap — poolless shell output.** On base `a7b116af`, `core::daemon::tests::poolless_standalone_human_shell_uses_registered_workspace` failed with `server,plugins,lsp-test-support`, including in isolation, but passed in the default-feature isolated run. Its test source is from upstream `9f9bde93`; the exact `667005a6` optional-feature suite has not been completed. No tool-runtime source was implicated.
- **Medium, Desktop transcript qualification gap — missing narration around tool call.** On hosted exact SHA `453ecd1b`, the M004 WebDriver trajectory completed the permission denial and final response but did not render `Examining your request.` in the final transcript. The completed-turn status and final response rendered. Investigate provider-event aggregation and public session transcript projection before strict closure; no assertion was removed.
- [Hosted `CI / verify`](https://github.com/dbowm91/codegg/actions/runs/38482051870) ran on exact SHA `453ecd1b943c227b6ea39477f22b63eef7aad338`; all guards, fixtures and format passed, then upstream Clippy failed at `src/tui/components/messages.rs:4043`. Workspace tests were skipped. No PR was open for this branch during the audit.
- [Hosted Desktop E2E](https://github.com/dbowm91/codegg/actions/runs/38482058408) ran on the same exact SHA. Boundary, daemon build, renderer and host checks passed. Six WebDriver scenarios passed; the M004 deterministic live-turn test failed because the completed transcript lacked the expected first assistant narration. The uploaded `desktop-e2e-logs` artifact is linked from the run.

## 11. Roadmap disposition

C003 is conditionally closed at implementation `3528ef6e` on current-main base `667005a6`. C001 and C002 remain accepted and closed. C003's code and guard acceptance criteria are met, while strict all-suite and hosted closure are withheld because of the named upstream CI/TUI failures and the Desktop transcript qualification gap.

A registry/reference search found no registered downstream plan with a hard or soft dependency on C003 or this corrective addendum. No plan is newly unblocked. The specified three-plan sequence is complete as far as current dependencies permit; no other eligible plan exists in this corrective directory.

## 12. Registry updates

- C001 remains closed; C002 remains closed; C003 is conditionally closed in this plan, subsystem addendum and registry.
- The implementation and closure replay SHAs for C001/C002 are updated through `667005a6` in the addendum and registry; historical closure records remain untouched.
- The four current-main TUI failures and optional-feature shell failure are recorded as unresolved qualification findings with upstream ownership.
- No downstream plans were unblocked; no unrelated corrective was started.
- Exact hosted CI and Desktop E2E run URLs, SHA and final statuses are recorded above.

## 13. Re-qualification on current main `f7ac5228` (merge `6521560e`)

This section appends new integration evidence without rewriting sections 1–12.
Historical `667005a6` qualification and hosted runs `38482051870`/`38482058408`
remain the evidence for that base.

- Merge parents: branch tip `8a08a623` + `origin/main` `f7ac5228`; merge commit
  `6521560e` on `planning/agent-tool-runtime-reconciliation`. `origin/main` had
  moved 34 commits since `667005a6` (command authorization + Linux isolation
  planning). No force-push; no lost commits.
- Conflict disposition (exact, no unrelated history loss):
  - `src/tool/backend.rs`, `src/tool/broker.rs`,
    `src/scheduler/tool_program_executor.rs`, 13 test initializers: kept BOTH
    `parent_tool_authority` (C002/M003 descendant ceiling) AND
    `execution_authorization` (main command-authorization M002). Broker
    `execution_context_for_attempt` forwards both; `From<ToolExecutionContext>`
    carries both; test `make_ctx` initializers carry both `None`s.
  - `src/agent/tool_batch.rs` `build_tool_execution_context`: kept BOTH the
    delegation-ceiling mint (`parent_tool_authority`) and the
    `execution_authorization` (`snapshot`/`receipt`/`command_preflight`) threading.
  - `src/agent/tool_batch.rs` batch dispatch: kept HEAD no-`mcp__` fork per the
    M001/C001 guard (`check_mcp_broker_adapter.py` forbids `mcp_tool_calls` and
    execution-routing `starts_with("mcp__")`); dropped main's direct
    `McpService::call_tool` fork with its caller-owned lock retry rather than
    reintroducing a broker bypass. Kept main's 4-tuple
    `(idx, tc, receipt, command_preflight)` plus the `child_allowed_tools`
    binding, so both authorities reach the broker context.
  - `plans/registry.md`: kept the HEAD Windows row and added main's command
    authorization + Linux isolation rows.
- Narrow rebase fixes (only touched by the struct-field addition, no authority
  change): `src/tool/mcp_adapter.rs` test helper and
  `tests/agent_tool_m003_descendant.rs` fixture gain
  `execution_authorization: None`.
- Verification on the merged tree (macOS arm64, Rust 1.89, `CARGO_BUILD_JOBS=2`):
  - `python3 scripts/check_mcp_broker_adapter.py`,
    `check_tool_descriptor_snapshot.py`, `check_descendant_tool_ceiling.py`,
    `check_tool_disclosure_wire.py`, `check_tool_retry_admission.py`,
    `scripts/test_tool_runtime_guards.py` — all pass, including all five
    negative mutation fixtures.
  - `bash scripts/verify.sh quick` — passes (fmt, all guards, workspace
    `cargo check --all-targets`).
  - `cargo fmt --all -- --check` — passes.
  - `cargo check --workspace --all-targets --locked` — passes.
  - Focused seven binaries (`agent_tool_m002_discovery`,
    `agent_tool_m003_descendant`, `provider_tool_disclosure_wire`,
    `agent_loop_harness`, `fake_eggsearch_mcp`, `tool_broker_integration`,
    `subagent`) with `--no-fail-fast` — 172 passed, 1 failed:
    `agent_loop_harness::shell_escalation_ask_waits_for_explicit_approval_before_dispatch`.
    The same test fails identically on a clean `origin/main` worktree at
    `f7ac5228`, so the merge introduces no new failure; ownership is the
    upstream command-authorization workstream.
  - `cargo clippy --workspace --all-targets --locked -- -D warnings` — fails on
    upstream `too_many_arguments` in `src/live_execution_audit.rs:104,135` and
    `src/tool/bash.rs:282`, all from main's command-authorization code; this
    branch's diff touches none of those functions. No unrelated fix attempted
    per C003 non-goals.
  - The four prior TUI panel tests fail identically in isolation on the merged
    tree (0 passed, 4 failed), same upstream ownership as sections 4/10.
  - Full `cargo nextest run --workspace --locked --profile ci --no-fail-fast`
    was attempted locally and exceeded a 20-minute timeout without completing;
    no green claim is made from it. The targeted re-runs above are the exact
    merged-tree evidence.
  - Hosted `CI / verify` and Desktop E2E were NOT re-run on merge SHA
    `6521560e`; no green is claimed for unrun hosted workflows. Prior exact-SHA
    hosted evidence (`453ecd1b`) stands for the `667005a6` base only.
- Unresolved findings carried forward (all upstream, none blocking the
  tool-runtime invariant itself):
  - Medium — workspace Clippy `too_many_arguments` on main-owned audit/bash
    functions (see above).
  - Medium — `shell_escalation_ask` AllowOnce dispatch gap, reproduced on clean
    `origin/main` `f7ac5228`; owned by command authorization.
  - Medium — four TUI panel regressions, reproduced in isolation on the merged
    tree; owned by TUI workstream.
  - Medium — no hosted CI/Desktop E2E on the merge SHA; strict closure withheld
    until exact-SHA hosted runs are green.
- Disposition: C003 REMAINS conditionally closed at merge `6521560e`. C001/C002
  remain closed. The tool-runtime guard/invariant acceptance criteria are met on
  the merged tree; strict all-suite and hosted closure are withheld for the
  named upstream failures. Merging this branch does not worsen `main`: every
  merged-tree failure reproduces on clean `origin/main`.
- Unblock audit (registry blocked-work + subsystem dependency graphs, same
  commit): no registered plan lists this C003 or corrective addendum as a hard
  or interface dependency. The blocked items (Linux M001/C002/C003/M003–M008,
  search M001/M002, tool-advisor M004/M005) wait on unrelated native/backend
  gates. Nothing is newly unblocked; no corrective follow-up was registered.

## 14. Second re-qualification on main tip `ec43e495` (merge `32098022`)

Sections 1–13 stand. This section records the follow-up merge and the first
exact-SHA hosted evidence on a merge head.

- Merge parents: docs head `669dc74d` + `origin/main` `ec43e495` (21 commits
  since `f7ac5228`: Windows C041–C048 successor line incl. PR #115, TUI
  focused-panel/picker fixes, scheduler completion-signal broadcast, desktop
  dependency pins). Clean auto-merge, no conflict resolution; the standing
  `6521560e` disposition (both authorities threaded, no `mcp__` broker bypass,
  both registry rows) carries forward untouched. Merge commit `32098022`,
  pushed to `planning/agent-tool-runtime-reconciliation`.
- Verification on the merged tree (macOS arm64, Rust 1.89):
  - All five tool-runtime guards + all five negative mutation fixtures — pass.
  - `cargo fmt --all -- --check` — passes.
  - `cargo check --workspace --all-targets --locked` — passes.
  - Focused seven binaries, `--no-fail-fast` — 172 passed, 1 failed: the known
    upstream `shell_escalation_ask` AllowOnce gap (command-authorization owner).
  - The four TUI panel tests blocking §4/§10 — now **4/4 pass** in isolation.
    Fixed upstream by `d68608c1` (focused-panel key advertisement) and
    `a9897f8b` (`/agents` picker regression-test update), both absorbed here.
  - `cargo clippy --workspace --all-targets --locked -- -D warnings` — still
    fails on the same three upstream `too_many_arguments` sites
    (`src/live_execution_audit.rs:104,135`, `src/tool/bash.rs:282`).
  - `python3 scripts/check_adr_integrity.py` — fails on the upstream
    ADR-0027/ADR-0028 duplication (Linux/command-authorization vs new
    Windows-successor ADRs) plus two dangling windows-sandbox roadmap paths.
    This also reds `verify.sh quick` at that step and all hosted CI, on main
    itself as well as here. Out of C003 scope; no unrelated fix attempted.
- Hosted evidence on exact merge SHA
  `32098022b5198ef7644efff7cb5f15775557b23c`:
  - [CI / verify `38521963856`](https://github.com/dbowm91/codegg/actions/runs/38521963856)
    (`workflow_dispatch` on the branch ref) — **failure at the ADR identity
    and link guard** with the byte-identical upstream collision quoted above.
    Earlier steps (agent schema, core boundary, sandbox contract) passed; the
    tool-runtime guard steps, format, Clippy and workspace tests show
    `skipped` because the ADR step precedes them in workflow order. No green
    is claimed; the tool-runtime guards remain locally green (above) but are
    still unrun on any hosted green-CI SHA.
  - [Desktop E2E `38521966606`](https://github.com/dbowm91/codegg/actions/runs/38521966606)
    (`workflow_dispatch` on the branch ref) — **success**: all steps green
    including `Built-app WebDriver trajectory`, the step that failed on run
    `38482058408`. The M004 deterministic live-turn transcript assertion
    (`Examining your request.` + `E2E deterministic turn complete.`) passes
    with this branch's code on the new base, so the §10 transcript gap is
    **closed**. (WebdriverIO fails the step on any spec failure; the
    per-spec logs were inspected and the run conclusion is success.)
- Gap ledger update: of the §13 blockers, the TUI-4 and Desktop-transcript gaps
  are closed; remaining for strict closure are (1) the ADR-0027/0028 collision
  (Windows-sandbox owners — gates all hosted CI), (2) the three Clippy sites
  (command-authorization owners), (3) `shell_escalation_ask` (same owner),
  (4) full workspace + `server,plugins,lsp-test-support` nextest green on a
  final SHA, and (5) a green hosted CI run covering the tool-runtime guard
  steps. The §11 ledger table's `667005a6` column remains the last full
  replay; C001/C002 code is byte-preserved through `6521560e` and `32098022`
  (no replay SHAs — nothing to replay).
- Disposition: C003 REMAINS conditionally closed at `32098022`. Merging this
  branch still does not worsen `main`: every remaining merged-tree failure
  reproduces on clean `origin/main`.
- Unblock audit (same commit): no registered plan depends on this C003; the
  blocked set is unchanged (Linux, search, tool-advisor gates). Nothing newly
  unblocked; no follow-up registered.

## 15. Upstream-gate remediation round (fix commit `d2a5d8e7`, CI `38602010489`)

Sections 1–14 stand. At the owner's direction, C003 carried three narrow
cross-workstream fixes that were blocking every hosted run, then re-qualified.

- Fix commit `d2a5d8e7` (branch `planning/agent-tool-runtime-reconciliation`):
  - ADR collision: Windows-successor ADRs filed as 0027/0028 (PR #115) renamed
    to ADR-0029/ADR-0030 via `git mv`; Linux keeps 0027, command-syntax keeps
    0028. Titles, internal cross-refs, the Windows roadmap authority section,
    the ADRs README (with dated collision record), and living registry rows
    updated. Historical closures and closed implementation plans untouched.
    `check_adr_integrity` passes.
  - Clippy: `#[allow(clippy::too_many_arguments)]` on `emit_command_execute`,
    `emit_command_execute_with_result`, and `BashTool::emit_command_audit`
    (one argument per event field by design; matches seven existing sites).
    Workspace Clippy `-D warnings` is clean.
  - `shell_escalation_ask`: root cause was the loop snapshotting WorkspaceWrite
    while the harness registers a bare `BashTool::new()` — the new
    containment/drift gates correctly refuse. The test exercises approval
    gating, not containment, so it now runs FullHost on both loop snapshot
    (`set_sandbox_profile`) and tool (`with_sandbox_profile`). Companion
    narrowing: the BashTool drift/availability gates apply only to configured
    executors (comparing against a never-made claim was fail-always, not
    fail-closed); the managed-process boundary still fails closed, and
    production registry behavior (always configured) is unchanged.
- Local evidence on `d2a5d8e7`: `verify.sh quick` passes (incl. ADR
  integrity), workspace Clippy clean, focused seven binaries **173/173**,
  TUI panel 4/4.
- Hosted [CI `38602010489`](https://github.com/dbowm91/codegg/actions/runs/38602010489)
  on exact SHA `d2a5d8e7`: past ADR identity, all guards (incl. the five
  tool-runtime guards + fixtures), format, and Clippy — then workspace tests
  fail (fail-fast) at
  `tool::terminal::tests::terminal_custom_restrictions_only_narrow_canonical_policy`
  (4147 passed before the stop). The allowlist/`env rm -rf /` expectation no
  longer matches the emptied default blocklist.
- Local optional-feature suite (`server,plugins,lsp-test-support`,
  `--no-fail-fast`) on `d2a5d8e7`: 10,600 passed, 22 failed, 1 timed out,
  5 skipped. Failing families: `tool_execution` hard-deny tier (2),
  denied-pattern (1), escalation signals (2); `tool::terminal` allowlist (1);
  `git_network_integration` (7), `git_recovery_integration` (4),
  `git_closure_matrix` (1); `work_orders` worktree isolation (2);
  `worktree` create/remove (1); `identity_live_execution_audit`
  single-host trajectory (1); `poolless_standalone_human_shell` (1, known
  §10 gap); `eggpool` generic-cancellation (1 — passes in isolation, load
  flake). The single timeout did not reproduce on re-run and matches the
  known asset-refresh 120 s load flake recorded in §4.
- Attribution (provable non-involvement of this round's diff): the
  `tool.execute()` path passes no authorization context, so the narrowed
  bash gate cannot fire there; the live-audit edit is comment-only; git,
  worktree, poolless, and eggpool paths execute none of the changed code.
- Escalation required before strict closure — shell-policy contract
  contradiction: closed shell-policy C001 requires tool-layer hard-deny to
  stay authoritative (`default_blocked_commands` non-empty), while closed
  command-authorization 005 explicitly records "the default blocked literal
  set is empty" as accepted design. Updating the stale tests and restoring
  enforcement are each a unilateral settlement of another workstream's
  decided trade-off, so neither was attempted here. The git/worktree
  cluster sits in the Linux Git M001 blocked area; the identity-audit
  trajectory needs command-auth triage. These need owner decisions, not a
  C003 improvisation.
- Follow-up (successor corrective C002, closed scoped): the shell-policy
  family above is now green via
  `plans/closure/shell-command-policy-ci-corrective/002-status.md`
  (unsafe destructive invocations removed, explicit-deny markers,
  current-vocabulary escalation table, managed-execution deny/ceiling
  tests; hosted CI `38614071155` green through Clippy with no shell-policy
  failure). Remaining workspace-red families for C003 strict closure are
  git/worktree (Linux Git owners), the identity-audit trajectory
  (command-auth owners), and the known poolless gap.
- Disposition: C003 REMAINS conditionally closed. Per instruction, the merge
  to `main` is withheld until the evidence above turns green.
- Unblock audit (same commit): nothing depends on this C003; blocked set
  unchanged. Nothing newly unblocked; no follow-up registered.

## 16. Strict-closure gap research on `b3c0fef7` (document-and-escalate)

Sections 1–15 stand. At the owner's direction this section records the exact
remaining red list, its root cause, and the owner fix spec. No code or test
change is made here; C003 REMAINS conditionally closed and the main merge
stays withheld.

- Baseline: branch head `b3c0fef7` (C002 code `85fc876d` + closure docs).
  `origin/main` is still `ec43e495` — zero drift since merge `32098022`, so
  no re-merge is needed and the §14 merge disposition stands.
- Green since §15: the entire shell-policy family (C002 successor,
  conditionally→scoped-closed; hosted CI `38614071155` green through Clippy
  with no shell-policy failure). The §15 shell contract contradiction is
  resolved on the advisory reading with no default blocklist.
- Exact remaining red on `b3c0fef7`, default features, `--no-fail-fast`
  (shell suites fully green in the same runs):
  - `git_closure_matrix::clean_dry_run_returns_preview_only` — also the
    hosted-`38614071155` fail-fast stop. Signature:
    `clean_preview: Policy(RequiresAdditionalAuthorization { capabilities:
    [DestructiveRepositoryChange] })` (`tests/git_closure_matrix.rs:587`).
  - `git_network_integration` (7): `clean_preview_lists_untracked_files`,
    `clean_removes_untracked_files`, `push/fetch/pull_to/from_local_bare_remote`,
    `reset_hard_moves_head_and_drops_changes`, `reset_soft_keeps_working_tree`.
    Representative signature:
    `push: Policy(BackendUnavailable { capabilities: [RemoteHelper, Network,
    Credential] })` — against a *local* bare remote
    (`tests/git_network_integration.rs:370`).
  - `git_recovery_integration` (4): `recovery_abort_in_progress_clears_state`,
    `recovery_continue_after_resolution_succeeds`,
    `recovery_continue_with_unresolved_conflicts_fails`,
    `recovery_skip_during_rebase_skips_step`.
  - `worktree::test_create_and_remove_worktree` (1),
    `work_orders_m007::f_worktree_isolation_and_truthfulness` (1),
    `work_orders_m002::concurrent_git_mutation_gets_distinct_managed_worktrees` (1).
  - `identity_live_execution_audit::single_host_trajectory_is_ordered_correlated_and_secret_free` —
    signature: `stage: Policy(BackendUnavailable { capabilities:
    [CleanSmudgeFilter] })` (`tests/identity_live_execution_audit.rs:506`).
  - Optional-feature suite (`server,plugins,lsp-test-support`, from §15,
    not re-run here): `poolless_standalone_human_shell` (known §10 gap),
    `eggpool` generic-cancellation (passes in isolation — load flake), plus
    the known asset-refresh 120 s load flake.
- Root cause (source-level, no clean-main rebuild required): main commit
  `ae025ef3 feat(git): reconcile typed execution policy` introduced the
  capability gates. `GitMutationExecutor::run_subprocess`
  (`src/git_mutations.rs:704`) funnels every git argv — including read-only
  `git clean -n -d` built by `clean_preview`
  (`src/git_network_ops.rs:743`) — through `env.apply`, which keys on the
  subcommand name without honoring dry-run/read-only flags, and demands
  backends (`RemoteHelper/Network/Credential`, `CleanSmudgeFilter`) even
  for local-bare and staging operations. The branch's only touches near
  this code are the `6521560e` merge threading (`parent_tool_authority` /
  `execution_authorization: None` in `src/core/daemon.rs`,
  `tests/identity_live_execution_audit.rs`, tool-program initializers) —
  all semantically-None struct-init additions that cannot produce a
  `BackendUnavailable`/`RequiresAdditionalAuthorization` outcome. The
  failures are upstream-owned by construction.
- Escalation — git-policy contract contradiction (same shape as the §15
  shell contradiction, now with a concrete fix spec for the owning
  workstreams — command authorization + Linux Git):
  1. Either the git execution policy becomes dry-run/read-only aware
     (`-n`/`--dry-run` previews, `clean_preview`, local-bare transports,
     and smudge staging do not demand destructive/network backends), or
  2. the git/identity tests explicitly grant the new capabilities — but
     that concedes previews require destructive authorization, which
     destroys preview UX and should be an explicit owner decision, not a
     C003 improvisation. Unilateral action by C003 on either side would
     settle another workstream's decided trade-off, exactly as in §15.
- Strict-closure checklist (unchanged owners, ordered):
  1. Command-auth + Git owners resolve the contradiction above (all 16
     default-feature reds hang off it).
  2. `poolless_standalone_human_shell` owner fix (pre-existing §10 gap).
  3. Confirm `eggpool`/asset-refresh flakes by isolated re-run at the
     final SHA.
  4. Local full workspace + optional-feature green on the final SHA.
  5. Hosted `CI / verify` green on the exact final SHA (fail-fast: any
     single red fails the run) plus Desktop E2E re-run (last green
     `38521966606` predates the fix commits; renderer-affecting code is
     untouched since, but the exact-SHA rule requires it).
  6. C003 §17 strict-closure record, registry flip, then merge.
- Unblock audit (head `b3c0fef7`): nothing depends on this C003; blocked
  set unchanged. Nothing newly unblocked; the only new record is this
  section and the C002 scoped closure it cites.
