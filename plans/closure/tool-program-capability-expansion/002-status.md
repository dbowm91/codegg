# Tool Program Capability Expansion M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-program-capability-expansion/002-external-search-programmatic-read-seam.md`

Source subsystem roadmap:

- `plans/subsystems/tool-program-capability-expansion-roadmap.md#M002--external-repository-search-programmatic-seam`

Repository baseline reviewed: `761667af48733bea590fe4bb9b2418065119ff23`

Implementation commits or pull requests:

- (this commit) — feat(tool-programs): M002 external repo-search
  programmatic read seam (`repo_search` structured contract + explicit
  nondeterministic/external cache/retry/provenance declaration,
  runtime-threading/bounds/policy/cancellation/replay tests,
  network-census negatives, docs).

## 1. Executive finding

M002 is closed. Tool Programs can call canonical `repo_search` through
the existing broker against the daemon-owned `SearchRuntimeContext` and
receive bounded structured results (upstream eggsearch JSON value plus
framed `external_untrusted` display) with `Mcp`/`ExternalUntrusted`
provenance; disabled/builtin-without-eggsearch/unavailable backends fail
closed as typed `InfrastructureError` programmatic failures;
cancellation propagates before dispatch; `max_results` is capped at 30
and display at `max_repo_search_output_chars` with truncation metadata;
contract/manifest hashes invalidate on any policy weakening; existing
direct callers are unchanged. The executable eligibility matrix (explicit
caller policy + read-side effect + output schema + bounds +
daemon-owned runtime context + explicitly nondeterministic trust +
no-retry/disabled-cache + truthful replay, no hidden globals, no
credential choice, no mutation surface) admits exactly `read`, `glob`,
`grep`, `list`, `diff` (M001, cache-enabled local) plus `repo_search`
(M002, cache-disabled external) and rejects every other nearby candidate
with a recorded disposition — no other network tool (`websearch`,
`webfetch`, `repo_fetch`, `repo_map`, `codesearch` alias,
`batch_fetch`, `security_search`, `research_search`,
`evidence_bundle`), multiplexed surface (`git`, `lsp`), mutation, or
process execution was promoted. Recorded program call results are
attributable to execution time: successive reruns against a changed
backend are allowed to differ (pinned by test), while ledger replay
serves the recorded result and divergence fails closed. No unresolved
M002 finding remains. The plan's stop-condition escape hatch (leave
`repo_search` unsupported if the contract cannot express safe semantics)
was not needed: the current contract model expresses the seam with
`ReadOnly` + `Idempotent` + no-retry + disabled-cache + permissive
object output schema plus documented nondeterminism.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Eligibility matrix applied to repo_search with nondeterminism/trust decisions (package A) | `tests/tool_program_search_palette.rs`: `repo_search_satisfies_external_eligibility_matrix` (DirectOrProgrammatic/ReadOnly/schema/no-retry/cache-disabled), `candidate_census_only_repo_search_admitted_among_external_reads` (17 deferred with reasons), `other_network_tools_rejected_from_program_manifest` (9 network tools rejected), `repo_search_input_schema_admits_no_arbitrary_url_or_credential` (no url/api_key/token/credential/env); `src/tool/repo_search.rs::contract` admission-rationale comment | pass | `ToolCategory::ReadOnly` alone never admits; local vs external matrix distinguished by cache declaration |
| Runtime-context threading into program registry/manifest (package B) | `program_registry_uses_configured_search_runtime` (mock-enabled program success vs disabled program `InfrastructureError`), `isolated_default_reports_unavailable_never_global` (no-context registry yields actionable eggsearch-unavailable) | pass | Same `ToolRegistry::with_options` search_runtime serves direct and program calls; no production change needed beyond the contract |
| Caller policy with conservative retry/cache and ledger replay semantics (package C) | `repo_search_cache_disabled_as_nondeterministic_declaration`, `repo_search_cache_keys_remain_workspace_scoped_if_used`, `repo_search_call_ledger_reserve_complete_replay_roundtrip`, `rerun_may_observe_different_results_replay_serves_recorded`, `manifest_allows_repo_search_and_hash_covers_contract`, `programmatic_outcome_mapping_is_truthful_for_search` | pass | No manifest/ledger schema migration; hash invalidation structural |
| Program search fixture, unavailable-backend/policy/SSRF/trust/size/replay tests (package D) | Mock eggsearch `repo_search` service; success/provenance/equivalence/direct-compat tests; unavailable/builtin/disabled negatives; DirectOnly denial for websearch; unverified-authority rejection; missing-query typed error; max-results cap proof (args == 30); 40 KiB truncation metadata; credential-smuggling negative; `BrokerAdapter` pre-dispatch cancellation with zero backend calls | pass | 25/25 in the new suite |
| Docs/diagnostics expose external-untrusted behavior (package E) | `architecture/tool_programs.md` palette table 5 → 6 rows + Expansion M002 section; `architecture/search_backend.md` programmatic-availability note; `AGENTS.md` palette + M002 rule | pass | Diagnostics render caller policy dynamically; no hardcoded palette to update |

## 3. Production implementation evidence

- `src/tool/repo_search.rs` (contract added, execution paths untouched):
  `contract()` override returning `DirectOrProgrammatic`/`ReadOnly`/
  `Idempotent`, no-retry, cache disabled (`enabled: false`, `ttl_secs: 0`,
  `max_entries: 0`), permissive object output schema (upstream eggsearch
  JSON is backend-versioned; broker output validation accepts any object
  and `value = None` text-only responses skip schema validation as
  before); `contract_output_schema()` helper on the inherent impl;
  `execute`/`execute_structured`/provenance dispatch unchanged; 2 new
  unit tests (contract shape, digest-weakening invalidation).
- `tests/tool_program_search_palette.rs` (new, 25 tests): full §2 A–E
  coverage as listed above.
- `tests/tool_program_diff_palette.rs`: M001 census updated — `repo_search`
  removed from the deferred list (now owned by the M002 census) and from
  the manifest-negative matrix (now positively admitted).
- `tests/tool_contract_guards.rs`: palette pin extended to
  `["read", "glob", "grep", "list", "diff", "repo_search"]`.
- Docs: `architecture/tool_programs.md` (palette + Expansion M002 section
  with matrix, promotion, census; M001 census note updated),
  `architecture/search_backend.md` (programmatic-availability note),
  `AGENTS.md` (palette + M002 rule).
- Deliberately absent (out of scope per plan §5): `websearch`/`webfetch`/
  `research` tools, search backend redesign, persistent indexing,
  programmatic arbitrary URLs, Git/LSP adapters (M003).

Before/after `repo_search` contract: legacy default (`DirectOnly`,
`NonIdempotent`, no cache/retry, no output schema, string-only result
convention) → explicit (`DirectOrProgrammatic`, `ReadOnly`,
`Idempotent`, no-retry, cache disabled, permissive object output schema,
structured bounded value with truncation metadata, `ExternalUntrusted`
provenance via the existing `execute_structured` path).

## 4. Verification executed

All local (no hosted `CI / verify` claimed).

### Commands run

```bash
cargo test --workspace repo_search --no-fail-fast
cargo test --test tool_program_runtime
cargo test --test tool_program_cache
cargo test --workspace search_backend --no-fail-fast
cargo test --test tool_program_search_palette --test tool_program_diff_palette --test tool_contract_guards --test tool_program_read_palette
cargo test --test tool_broker_integration --test tool_program_context_artifacts --test tool_surface_minimization --test search_runtime_isolation
cargo test -p codegg --lib tool::repo_search
cargo test -p codegg --lib search_backend
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
python3 scripts/check_tool_broker_boundary.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_daemon_cwd_usage.py
scripts/verify.sh quick
```

### Results

- `tool_program_search_palette`: 25 passed (new M002 suite).
- `tool_program_runtime`: 13 passed; `tool_program_cache`: 14 passed
  (existing runtime/cache behavior intact with `repo_search` added).
- `--workspace repo_search` filter: all matching suites passed (203
  result lines, zero failures — includes the 2 new `tool::repo_search`
  lib tests via the lib target line).
- `--workspace search_backend` filter: all matching suites passed (203
  result lines, zero failures — includes 82 `search_backend` lib
  tests).
- `tool_program_diff_palette`: 23 passed; `tool_contract_guards`: 11
  passed; `tool_program_read_palette`: 21 passed (M001/contract/palette
  behavior intact with `repo_search` promoted).
- `tool_broker_integration`: 25 passed; `tool_program_context_artifacts`:
  9 passed; `tool_surface_minimization`: 13 passed;
  `search_runtime_isolation`: 9 passed.
- `codegg --lib tool::repo_search`: 2 passed (new contract unit tests).
- `codegg --lib search_backend`: 82 passed.
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  pass, zero warnings.
- Static guards: tool-broker boundary, scheduler-bypass,
  execution-ownership, daemon-cwd all pass (no new spawn site, no broker
  bypass, no protected-module CWD use; `RepoSearchTool::default()`
  isolated default retained only for legacy context-free construction).
- `scripts/verify.sh quick`: passed (fmt, builtin-agents check,
  core-boundary, sandbox contract, execution-ownership,
  `cargo check --workspace --all-targets --locked`).

Plan §11 lists `cargo test --workspace repo_search` and
`cargo test --workspace search_backend`: no such workspace crates exist
(modules of the `codegg` binary), so the workspace-wide filter form
above plus the focused suites were used as the justified substitute;
nothing was skipped without one.

## 5. Invariant review

| Plan invariant | Evidence | Result |
|---|---|---|
| Program calls go through broker; no mutable/global search installation | All program tests execute via `ToolBroker::execute` with a verified grant; `BrokerAdapter` cancellation path tested; unverified-authority program call rejected (`CallerDenied`); disabled context never invokes MCP | pass |
| External results remain `ExternalUntrusted` | Provenance asserts `ExternalUntrusted`/`mcp` on program success; display framing asserts `trust=external_untrusted` + `tool=repo_search`; never `LocalTrusted` | pass |
| SSRF/network/backend policy unchanged | Disabled/builtin/unavailable negatives assert existing messages; `max_results` cap asserted at adapter args (30); no `url` in input schema; credential-ish fields proven absent from backend args; program authority cannot choose provider credentials | pass |
| Program authority cannot choose hidden provider credentials | Input-schema negative + backend-args negative (`api_key`/`env` never forwarded) | pass |
| Result count/payload bounded | max-results cap test + 40 KiB truncation test with `truncated` metadata in display, value path, and provenance | pass |
| No claim that repeated external search is deterministic | Contract declares cache disabled; rerun-difference test pins that successive executions may differ; ledger test pins replay-serves-recorded with divergence failing closed | pass |
| Broker/ledger remains canonical | Manifest/ledger/contract-snapshot tests use production `resolve_manifest`/`resolve_contract_snapshot`/`ToolProgramLedger`; no new executor or bypass | pass |

## 6. Failure and recovery review

- Missing `query`: broker input-schema validation rejects before dispatch
  (typed `BrokerError::Execution`, never `Success`).
- Backend unavailable (no MCP handle), disabled backend, builtin backend:
  typed `InfrastructureError` programmatic failures (`Err` from
  `into_programmatic_outcome`), never `Success`; direct callers observe
  the same messages as before.
- Cancellation: `BrokerAdapter` observes the cancelled token before
  dispatch (`InterpreterError::Cancelled`) with zero backend calls;
  broker deadline/cancellation precheck unchanged.
- Replay: ledger semantics unchanged; search completions persist and replay
  like other reads; replay serves the recorded execution-time result and
  divergence fails closed (pinned). Rerun (new execution) may return
  different results and is never labeled deterministic.
- Contention: concurrent reads mutate nothing; tool holds no interior
  mutability (`&self` only, `SearchRuntimeContext` is `Clone` + shared
  `Arc` MCP handle); cache/ledger concurrency behavior unchanged.
- Oversized output: backend display cap + broker artifact boundary apply;
  truncation sets `truncated` in display and provenance.

## 7. Migration and compatibility review

- Direct `repo_search` input schema unchanged (`query` required, same
  filters); string output identical (framed evidence); structured result
  is additive via the existing `execute_structured` adapter convention;
  legacy callers unaffected.
- No storage, protocol, config, or Tool-trait migration. No new feature
  flag, CI lane, or binary-size gate per verification policy.
- Rollback: reverting the binary restores the legacy `DirectOnly`
  search; persisted program records reference contracts by digest and fail
  admission on drift rather than mis-executing.

## 8. Security review

- Authority: every program `repo_search` call carries the verified grant
  and frozen manifest; broker validates before any backend dispatch,
  including replay-adjacent paths.
- Network surface: no new network path — the adapter, backend policy,
  and MCP transport are unchanged; `repo_search` takes no URL, so there
  is no SSRF surface to widen; `webfetch` (arbitrary-URL) stays
  `DirectOnly` (pinned).
- Credentials: programs supply only query/filters; `[search.eggsearch.env]`
  values never enter source, manifest, cache key, ledger, or diagnostics
  (context `Debug` redacts values; backend-args negative test pins
  non-forwarding).
- No principal, credential, or reasoning content enters source, manifest,
  cache key, or ledger beyond existing conventions.
- DoS bounds: input/output caps above; no subprocess or lock introduced;
  backend timeout enforced by the adapter (`eggsearch_timeout_ms`) inside
  the broker timeout.

## 9. Documentation and operations

- Updated: `architecture/tool_programs.md` (palette + Expansion M002
  section + M001 census note), `architecture/search_backend.md`
  (programmatic-availability note), `AGENTS.md` (palette + M002 rule).
  No skill catalog change (no skill enumerates the palette).
  `tool_broker.md` needs no change (no pipeline or trait change).
- Operator diagnostics: unchanged broker error taxonomy (`CallerDenied`,
  `InputTooLarge`, typed `InfrastructureError` with existing actionable
  messages); search failures surface the existing typed tool errors.
- Static guards: all green; no new CI lane added per verification policy.

## 10. Unresolved findings

There are no unresolved critical, high, medium, or low M002 findings.

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None | — | — |

## 11. Roadmap disposition

Milestone closed; M003 is unblocked. Dependency audit against
`plans/registry.md`:

- TP expansion M003 (operation-scoped Git/LSP read adapters) lists TP
  expansion M002 closure as its sole remaining blocker (M001 already
  closed) → moves `blocked` → `ready` (dependency-ready); its plan
  already exists at
  `plans/implementation/tool-program-capability-expansion/003-operation-scoped-lsp-git-read-adapters.md`.
- No other registered plan lists TP expansion M002 as a dependency, so
  no further status moves. No corrective follow-up required.

## 12. Registry updates

- `plans/registry.md`: expansion row `active / M002 ready` → `active /
  M003 ready`; dependency-ready table: M002 row removed, M003 row added
  (ready on the M001 eligibility contract + M002 seam);
  execution order §6 reworded (M002 closed; M003 may proceed);
  blocked work: M003 row removed; closure control points: M002 closed
  row added.
- `plans/subsystems/tool-program-capability-expansion-roadmap.md`:
  Status stays `active`; M002 `ready` → `closed` with closure link;
  M003 `blocked` → `ready`.
- `plans/implementation/tool-program-capability-expansion/002-*.md`:
  `ready for handoff` → `implemented` with closure link.
- `plans/implementation/tool-program-capability-expansion/003-*.md`:
  `blocked` → `ready for handoff`.
