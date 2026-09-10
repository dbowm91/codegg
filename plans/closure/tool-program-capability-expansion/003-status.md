# Tool Program Capability Expansion M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-program-capability-expansion/003-operation-scoped-lsp-git-read-adapters.md`

Source subsystem roadmap:

- `plans/subsystems/tool-program-capability-expansion-roadmap.md#M003--operation-scoped-GitLSP-read-adapters`

Repository baseline reviewed: `79b70f55540bb98a78640f36c11f5f38276a4d4b`

Implementation commits or pull requests:

- (this commit) — feat(tool-programs): M003 operation-scoped Git/LSP
  read adapters (hidden `ProgrammaticOnly` `git_read`/`lsp_read`
  contracts delegating `GitExecutionService` /
  `LspTool::execute_scoped_read`, `ProgrammaticOnly` snapshot
  admission, allow/deny + delegation + mutation-negative +
  workspace/cache/replay/disclosure tests, docs).

## 1. Executive finding

M003 is closed. Tool Programs can perform a bounded Git read subset
(`status`, `diff`, `log`, local `branches`) and a bounded LSP read
subset (`diagnostics`, `documentSymbol`, `workspaceSymbol`, `hover`,
`goToDefinition`, `findReferences`) through hidden `ProgrammaticOnly`
adapters that delegate the canonical owners with identical
path/workspace/provenance rules; every mutation-capable operation is
structurally unavailable (no schema field can name it — strict
`deny_unknown_fields` parsing plus an `operation` enum limited to the
read subset); the ordinary model-visible tool surface does not grow
(adapters are `Hidden` with `expose_in_definitions = false`,
undiscoverable via `tool_search`, absent from all palettes); no
duplicate backend exists (Git reads execute typed
`codegg_git::GitOperation`s through `GitExecutionService`; LSP reads
execute through the new `LspTool::execute_scoped_read` typed entry
point that is also the single implementation behind the model-facing
`lsp` read arms). The multiplexed `git`/`lsp` tools stay `DirectOnly`.
No unresolved M003 finding remains. With M001–M003 all closed, the
expansion roadmap's completion definition is satisfied and the
subsystem closes with this record.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Allow/deny effect table for multiplexed Git/LSP (package A) | `tests/tool_program_git_lsp_palette.rs`: `adapter_contracts_are_hidden_programmatic_only_reads`, `multiplexed_git_and_lsp_stay_direct_only`, `git_read_schema_admits_only_the_read_subset`, `lsp_read_schema_admits_only_the_read_subset`, `candidate_census_no_other_tool_promoted_by_m003` | pass | Table also recorded in `architecture/tool_programs.md` Expansion M003 |
| Bounded read subset on canonical backends; stop if duplication needed (package B) | `git_read_delegates_the_canonical_execution_service` (adapter display byte-identical to `GitExecutionService` stdout for the same typed op); `lsp_read_matches_canonical_tool_for_every_allowed_read` (adapter and canonical `lsp` observe identical status+display for all six reads) | pass | No candidate required duplicated logic; subset is status/diff/log/branches + six LSP reads |
| Hidden programmatic-only adapters/contracts with schemas + workspace context (package C) | Contract unit tests in `src/tool/git_read.rs` / `src/tool/lsp_read.rs` (ProgrammaticOnly/ReadOnly/Idempotent, cache disabled, retry none, output schema, hidden); `resolve_contract_snapshot` admits `ProgrammaticOnly` (`src/tool/tool_program_context.rs`); `ToolRegistry::with_options` registers both with workspace root + shared LSP service; `disclosure_for` returns `Hidden` | pass | `git_read` has no `workdir` input — ctx cwd is the repo root; `lsp_read` root defaults to ctx cwd |
| Broker/program fixtures for allowed reads + exhaustive mutation-name/input negatives (package D) | `program_git_{status,diff,log,branches}_*` positives; `program_git_read_rejects_every_mutation_shaped_input` (24 op names + 6 field names); `program_lsp_read_rejects_every_preview_shaped_input` (21 op names + 6 field names); caller-policy triad (`agent_caller_is_denied_the_program_only_adapters`, `program_caller_is_denied_the_multiplexed_tools`, `unverified_program_authority_is_rejected_for_adapters`); manifest/snapshot gates | pass | 32/32 in the new suite |
| Cache/replay/state-version/provenance + disclosure invariance (package E) | `adapter_cache_is_disabled_as_version_dependent_declaration`, `program_git_rerun_observes_changed_repository_state` (dirty-after-clean re-observed, no stale memo), `adapter_ledger_reserve_complete_replay_roundtrip`, `programmatic_outcome_mapping_is_truthful_for_adapters`, `program_lsp_unavailable_fails_closed_never_success`, `adapters_are_hidden_and_ordinary_definitions_unchanged`, `adapters_are_undiscoverable_via_tool_search` | pass | Git provenance `LocalTrusted`, LSP `LocalUntrusted` (canonical shaping) |

## 3. Production implementation evidence

- `src/tool/git_read.rs` (new, ~370 lines): `GitReadTool` with
  `ProgrammaticOnly`/`ReadOnly`/`Idempotent`, cache disabled, retry
  none, versioned output schema (`operation`, `truncated`,
  `results`); strict `deny_unknown_fields` input (`operation` enum +
  optional `base_ref`/`max_count` only); typed dispatch to
  `Status`/`Diff`/`Log`/`BranchList` via `GitExecutionService`;
  `base_ref` alphabet/length validation (alphanumeric start, 128
  chars, no shell metacharacters; argv path never invokes a shell);
  log clamp 1–50 (default 20); 64 KiB display cap with truncation
  metadata; `Native`/`codegg/git_read`/`LocalTrusted` provenance;
  `expose_in_definitions = false`; 4 unit tests.
- `src/tool/lsp_read.rs` (new, ~440 lines): `LspReadTool` with the
  same contract shape and `egglsp`-shaped output schema; strict input
  (`operation` enum of six + `file_path`/`line`/`column`/`symbol`
  only); per-operation field validation (1-indexed positions,
  200-char symbol cap); delegation through
  `LspTool::execute_scoped_read` with the program workspace root as
  `allowed_root`; canonical-mirror provenance
  (`native`/`egglsp`/`LocalUntrusted`); `expose_in_definitions =
  false`; 5 unit tests.
- `src/tool/lsp.rs` (refactor, no behavior change):
  `ScopedLspRead` request type + `LspTool::execute_scoped_read`
  typed entry point holding the six read arms verbatim; the
  model-facing `execute()` arms delegate to it. Single
  implementation, two entry points. Full `tool::lsp` lib suite
  (204 tests) plus `tests/lsp` (34) and `tests/tool_execution`
  (164) pass unchanged.
- `src/tool/mod.rs`: `git_read`/`lsp_read` modules; both registered
  in `with_options` (git adapter rooted at the workspace root; LSP
  adapter sharing the one registry `LspService` instance with the
  model-facing tool, always registered so disabled-backend reads
  fail closed at execution like the M002 unavailable seam).
- `src/tool/disclosure.rs`: `git_read`/`lsp_read` classify `Hidden`
  (never in definitions, never returned by `tool_search`).
- `src/tool/tool_program_context.rs`:
  `resolve_contract_snapshot` admits `ProgrammaticOnly` alongside
  `DirectOrProgrammatic` (manifest resolution already did);
  `DirectOnly` remains rejected everywhere.
- `tests/tool_program_git_lsp_palette.rs` (new, 32 tests): full §2
  A–E coverage as listed above.
- Docs: `architecture/tool_programs.md` (palette 6 → 8 rows +
  Expansion M003 section with table, delegation map, eligibility
  extension; M001/M002 census notes updated),
  `architecture/tool_broker.md` (program-capable policy note),
  `architecture/git.md` + `architecture/lsp.md`
  (programmatic-availability notes),
  `architecture/agent-tool-surface.md` (Hidden row),
  `AGENTS.md` (palette + manifest/callability rules).

Before/after contracts: no existing contract changed. `git`/`lsp`
remain `DirectOnly` (pinned by
`multiplexed_git_and_lsp_stay_direct_only`); `resolve_manifest`
semantics for existing tools unchanged; `resolve_contract_snapshot`
widens from exactly-`DirectOrProgrammatic` to
`DirectOrProgrammatic | ProgrammaticOnly` (pinned by
`contract_snapshot_admits_programmatic_only_and_hash_covers_policy`,
including digest-weakening invalidation).

## 4. Verification executed

All local (no hosted `CI / verify` claimed).

### Commands run

```bash
cargo test --test tool_program_git_lsp_palette --test tool_contract_guards --test tool_surface_minimization --test tool_program_runtime --test tool_program_cache --test tool_program_read_palette --test tool_program_diff_palette --test tool_program_search_palette
cargo test --test tool_broker_integration --test tool_program_context_artifacts --test search_runtime_isolation
cargo test -p codegg --lib -- tool::
cargo test -p codegg --lib -- git_service git:: lsp::disclosure
cargo test -p egggit -p codegg-git
cargo test --test lsp --test tool_execution --test git_closure_matrix
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
python3 scripts/check_tool_broker_boundary.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check-core-boundary.sh
scripts/verify.sh quick
```

### Results

- `tool_program_git_lsp_palette`: 32 passed (new M003 suite).
- Palette/contract companions: `tool_contract_guards` 11,
  `tool_surface_minimization` 13, `tool_program_runtime` 13,
  `tool_program_cache` 14, `tool_program_read_palette` 21,
  `tool_program_diff_palette` 23, `tool_program_search_palette` 25
  passed (M001/M002/contract/disclosure behavior intact).
- `tool_broker_integration` 25, `tool_program_context_artifacts` 9,
  `search_runtime_isolation` 9 passed.
- `codegg --lib tool::`: 561 passed (includes the 9 new
  `git_read`/`lsp_read` unit tests and the full `lsp` suite over the
  refactored arms).
- Focused lib filters (`git_service`, `git::`, `lsp::disclosure`):
  33 passed; `egggit` 358 + `codegg-git` 75 passed.
- `lsp` 34, `tool_execution` 164, `git_closure_matrix` 54 passed
  (model-facing LSP/git behavior unchanged by the arm refactor).
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  pass, zero warnings.
- Static guards: tool-broker boundary (the adapter delegates via the
  typed `execute_scoped_read` method, never the `Tool` trait),
  scheduler-bypass, execution-ownership (no new spawn site),
  daemon-cwd, core-boundary — all pass.
- `scripts/verify.sh quick`: passed (fmt, builtin-agents check,
  core-boundary, sandbox contract, execution-ownership,
  `cargo check --workspace --all-targets --locked`).

Plan §11 lists `cargo test --workspace lsp` and `cargo test
--workspace git`: no such workspace crates exist (both are modules
of the `codegg` binary/crate tree), so the focused suites above
plus the `egggit`/`codegg-git` crate suites were used as the
justified substitute, exactly as in the M001/M002 closures; nothing
was skipped without one.

## 5. Invariant review

| Plan invariant | Evidence | Result |
|---|---|---|
| No Git write/worktree mutation or LSP edit/preview-apply is programmatically available | Exhaustive op-name negatives (24 git + 21 LSP) plus unknown-field negatives; multiplexed tools pinned `DirectOnly` at contract, manifest (`DirectOnly` rejection), snapshot (resolution error), and broker (`CallerDenied` for Program callers) | pass |
| Adapter code contains no independent backend logic or subprocess/shell fallback | Git display byte-identical to canonical service output; LSP adapter output identical to canonical tool output for all six reads; `git_read` spawns nothing (service-owned); `lsp_read` holds no protocol code (single `execute_scoped_read` implementation); broker-boundary guard green | pass |
| Workspace/execution context is explicit | `git_read` has no `workdir` input (ctx cwd is the repo root); `lsp_read` validates every path against the program root; two-workspace test observes per-workspace branches; escape/`..` negatives denied; daemon-cwd guard green | pass |
| Output bounded/structured | Log clamp (≤50), base_ref/symbol/position bounds, 64 KiB git display cap with `truncated` in display+value+provenance, canonical LSP caps inherited, broker artifact boundary above | pass |
| Hidden adapters do not expand the model-visible surface | `Hidden` disclosure, `expose_in_definitions = false`, absent from definitions/`tool_search`/all palettes/plan-mode; ordinary definitions still contain `git`/`lsp` | pass |
| Broker/manifest/effect authority remains canonical | Every program test executes via `ToolBroker::execute` with a verified grant; unverified authority rejected; `ProgrammaticOnly` denied to `Agent`; manifest+snapshot+digest gates cover the new policies | pass |

## 6. Failure and recovery review

- Unsupported `operation` values: broker schema-enum rejection
  (`BrokerError::Execution`) or adapter allowlist denial (typed
  `InfrastructureError` programmatic failure) — never `Success`.
- Mutation-shaped fields (`mutation`, `recover`, `subcommand`,
  `new_name`, `content`, `patch`, …): strict parsing rejects before
  any backend is touched.
- Invalid `base_ref`/positions/symbol queries: typed failures with
  actionable messages; `into_programmatic_outcome` maps to `Err`.
- Outside-repository git root / unavailable LSP server (no language
  server installed, server restart, indexing): typed
  `InfrastructureError` programmatic failures, never synthesized
  results; reruns re-observe live state (cache disabled) while
  ledger replay serves the recorded result and divergence fails
  closed (pinned).
- Cancellation: broker deadline/cancellation precheck and
  `BrokerAdapter` propagation unchanged; no new async path added
  (adapters are straight await chains through the canonical
  backends).
- Contention: concurrent reads mutate nothing; adapters hold no
  interior mutability; cache/ledger concurrency behavior unchanged.
- Oversized output: 64 KiB adapter cap + broker `max_output_bytes`
  + artifact spillover apply in order.

## 7. Migration and compatibility review

- Direct `git`/`lsp` input schemas, string outputs, and
  model-facing behavior unchanged (arm refactor is behavior
  preserving; `lsp` 34 + `tool_execution` 164 + lib `tool::lsp`
  suites pass).
- Additive hidden contracts only; historical program manifests
  without the adapters remain valid; new contract hashes/versioning
  apply normally (digest-weakening test pins invalidation).
- No storage, protocol, config, or `Tool`-trait migration. No new
  feature flag, CI lane, or binary-size gate per verification policy.
- Rollback: reverting the binary removes the adapters; persisted
  program records referencing them fail admission on drift rather
  than mis-executing.

## 8. Security review

- Authority: every program `git_read`/`lsp_read` call carries the
  verified grant and frozen manifest; broker validates caller
  policy, authority, input schema, and contract snapshot before any
  backend dispatch. `Agent` callers are denied the adapters;
  `Program` callers are denied the multiplexed tools.
- Read surface: no new read backend — `GitExecutionService`
  (hardened env policy, 8 MiB stream caps, URL-credential redaction)
  and `LspService`/`LspOperations` are unchanged; `base_ref` cannot
  smuggle flags (leading-dash/alphabet/length rules) and argv never
  passes through a shell.
- Path policy: `git_read` accepts no paths at all (workspace root
  only); `lsp_read` validates every `file_path` against the program
  root (absolute escapes and `..` traversals denied; error messages
  name only the requested display path).
- No principal, credential, or reasoning content enters source,
  manifest, cache key, or ledger beyond existing conventions (cache
  is disabled for both adapters; ledger stores the same call shapes
  as other reads).
- DoS bounds: §5 row 4; no subprocess, network, or lock introduced
  by the adapters themselves.

## 9. Documentation and operations

- Updated: `architecture/tool_programs.md` (palette + Expansion
  M003 section + M001/M002 census notes),
  `architecture/tool_broker.md` (program-capable policy note),
  `architecture/git.md` + `architecture/lsp.md`
  (programmatic-availability notes),
  `architecture/agent-tool-surface.md` (Hidden row), `AGENTS.md`
  (palette + manifest/callability rules). No skill catalog change
  (no skill enumerates the palette).
- Operator diagnostics: unchanged broker error taxonomy
  (`CallerDenied`, `InputTooLarge`, typed `InfrastructureError`
  with existing actionable messages); `/tool-backends` and
  `backend_report` unchanged (adapters are hidden diagnostics-wise
  except by name).
- Static guards: all green; no new CI lane added per verification policy.

## 10. Unresolved findings

There are no unresolved critical, high, medium, or low M003 findings.

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None | — | — |

## 11. Roadmap disposition

Milestones M001–M003 are all closed, satisfying the roadmap's
completion definition (Tool Programs have materially broader useful
read capability through existing authority, external trust is
preserved, and no mutating or duplicate backend path was
introduced). The subsystem roadmap therefore closes with this
record. Dependency audit against `plans/registry.md`:

- No registered plan lists TP expansion M003 as a hard or interface
  dependency (the roadmap dependency graph is terminal at M003;
  the only blocked items are the unrelated Architecture
  convergence M009 and Runtime safety C002 evidence items) → no
  plan moves `blocked` → `ready`. No corrective follow-up required.
- Deferred reads left out of the minimal subset (git `show`/`blame`,
  LSP `declaration`/`callHierarchy`, …) remain intentionally
  unregistered: a future milestone may admit them through the same
  adapter pattern with fresh evidence, but no such plan is
  dependency-ready today.

## 12. Registry updates

- `plans/registry.md`: expansion row `active / M003 ready` →
  `closed / M001–M003 closed`; dependency-ready table: M003 row
  removed (no dependency-ready plans remain in this subsystem);
  execution order §6 reworded (M001–M003 closed; expansion
  complete); closure control points: M003 closed row added.
- `plans/subsystems/tool-program-capability-expansion-roadmap.md`:
  Status `active` → `closed`; M003 `ready` → `closed` with closure
  link.
- `plans/implementation/tool-program-capability-expansion/003-*.md`:
  `ready for handoff` → `implemented` with closure link.
