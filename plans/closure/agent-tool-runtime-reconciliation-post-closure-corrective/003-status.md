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

C003 is **conditionally closed**, not strictly closed. The complete workspace CI-profile run on current main exposed four reproducible TUI failures introduced by upstream commits `845d77db` and `667005a6`, outside this branch's runtime diff. Exact-ref hosted CI and Desktop E2E are being requested after push; their results will be appended before handoff. The failures block a green full-suite claim and are not silently attributed to this tool-runtime corrective.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| C001 and C002 accepted before qualification | Separate accepted closure records; current replay ledger above | Pass |
| All predecessor and corrective source guards execute in quick | Current-main `scripts/verify.sh quick` ran broker adapter, descriptor snapshot, descendant ceiling, disclosure wire, retry admission and fixture runner; each guard accepted checked-in source | Pass |
| Every guard rejects a forbidden mutation | `scripts/test_tool_runtime_guards.py` mutated each of the five source boundaries; all five guards rejected their negative fixture | Pass |
| Hosted CI uses the same explicit guard order | `.github/workflows/ci.yml` runs the six tool-runtime commands; workflow YAML parses | Pass locally; exact hosted result pending below |
| Current-main integration preserved unrelated work | Rebased from `a7b116af` onto fetched `667005a6`; the two upstream commits changed TUI paths, with no conflicts. The complete TUI changes and tests remain present. | Pass |
| C001/C002 focused runtime behavior after final rebase | Seven focused binaries passed, 173 tests total | Pass |
| Full workspace and optional-feature qualification | Current-main broad suite has four reproducible upstream TUI failures; the required optional feature suite also exposed the upstream poolless shell test failure on the preceding `a7b116af` base. Details in sections 4 and 10. | Conditional |
| Exact-ref hosted CI and Desktop E2E | Requested after push for the implementation revision; record run IDs/URLs and result below | Pending at record creation |

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

No test cancellation or partial hosted run will be counted as green. Hosted run outcomes must be attached by exact SHA after they complete.

## 7. Compatibility and migration

No protocol or storage migration was added. The additive `parent_authority` payload, existing model tool schemas and tool names are unchanged. No provider-native tool-search allowlist was enabled. The final integration base is current main `667005a6`; the rebase completed cleanly and retained the upstream TUI commits.

## 8. Security and trust-boundary review

The new static checks inspect code ownership and fail when a guard input disappears. Mutation fixtures prove both accepted source and representative forbidden edits. They are narrow source checks, not substitutes for the C001/C002 behavioral fixtures, broker admission, permission checks or sandbox tests. No secret-bearing data is printed by the guards or fixtures.

No new security finding was identified in the C003 implementation. The outstanding full-suite failures are correctness/qualification findings in upstream TUI behavior and are not security claims.

## 9. Documentation and operations

Updated `architecture/testing.md`, `architecture/tool_broker.md`, `architecture/agent-tool-surface.md`, `plans/registry.md`, and the corrective addendum. The registry now records C001/C002 replay SHAs through current main `667005a6`, the C003 conditional disposition, and the four upstream test failures. Workflow YAML was parsed locally. Exact hosted URLs and statuses are to be added after dispatch.

## 10. Unresolved findings

- **Medium, qualification blocker — upstream TUI panel regressions.** On `667005a6`, these tests fail in the full suite and independently: `tui_agents_panel::the_agents_command_loads_its_panel_instead_of_recursing`, `tui_agents_panel::the_agents_panel_takes_the_keyboard`, `tui_chat_surfaces::the_focused_panel_advertises_its_own_keys`, and `tui_chat_surfaces::the_session_panel_advertises_its_own_keys`. The mainline TUI implementation came in `845d77db` and `667005a6`; none of these files are changed by the tool-runtime corrective. Owner is the TUI workstream. C003 makes no speculative fix to unrelated TUI code.
- **Medium, optional-feature qualification gap — poolless shell output.** On base `a7b116af`, `core::daemon::tests::poolless_standalone_human_shell_uses_registered_workspace` failed with `server,plugins,lsp-test-support`, including in isolation, but passed in the default-feature isolated run. Its test source is from upstream `9f9bde93`; the exact `667005a6` optional-feature suite has not been completed. No tool-runtime source was implicated.
- Hosted `CI / verify` and Desktop E2E exact-ref outcomes remain operational evidence and must be recorded before recommending integration. No PR was open for this branch during the audit. The branch will be pushed, then hosted workflows will be requested through available dispatch triggers.

## 11. Roadmap disposition

C003 is conditionally closed at implementation `3528ef6e` on current-main base `667005a6`. C001 and C002 remain accepted and closed. C003's code and guard acceptance criteria are met, while strict all-suite and hosted closure are withheld because of the named upstream failures and pending exact-ref hosted evidence.

A registry/reference search found no registered downstream plan with a hard or soft dependency on C003 or this corrective addendum. No plan is newly unblocked. The specified three-plan sequence is complete as far as current dependencies permit; no other eligible plan exists in this corrective directory.

## 12. Registry updates

- C001 remains closed; C002 remains closed; C003 is conditionally closed in this plan, subsystem addendum and registry.
- The implementation and closure replay SHAs for C001/C002 are updated through `667005a6` in the addendum and registry; historical closure records remain untouched.
- The four current-main TUI failures and optional-feature shell failure are recorded as unresolved qualification findings with upstream ownership.
- No downstream plans were unblocked; no unrelated corrective was started.
- Append exact hosted CI and Desktop E2E workflow run URLs, exact SHA and final statuses here after dispatch.
