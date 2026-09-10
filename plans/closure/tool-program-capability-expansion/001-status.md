# Tool Program Capability Expansion M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-program-capability-expansion/001-deterministic-local-read-contract-expansion.md`

Source subsystem roadmap:

- `plans/subsystems/tool-program-capability-expansion-roadmap.md#M001--deterministic-local-read-contract-expansion`

Repository baseline reviewed: `9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6`

Implementation commits or pull requests:

- (this commit) — feat(tool-programs): M001 deterministic local read
  contract expansion (`diff` structured contract + execution-context
  workspace policy, eligibility matrix/census tests, program/cache/ledger
  evidence, docs).

## 1. Executive finding

M001 is closed. Tool Programs can call the local `diff` operation through
the existing broker against the program's authorized workspace and receive
bounded structured output (`path`, `has_changes`, `diff`, `truncated`, byte
counts) with deterministic `LocalTrusted` provenance; path escape, `..`
traversal, and symlink redirection fail closed (`Denied`); oversized input
fails deterministically at the broker input bound with a tool-level backstop
for direct callers; contract/manifest hashes invalidate on any policy
weakening; existing direct string callers are unchanged. The executable
eligibility matrix (explicit caller policy + read-side effect + output
schema + bounds + execution-context authority + determinism/trust +
conservative retry/cache + truthful replay, no hidden globals) admits
exactly `read`, `glob`, `grep`, `list`, `diff` and rejects every other
nearby candidate with a recorded disposition — no mutation, network, or
multiplexed surface was promoted. No unresolved M001 finding remains.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Eligibility checklist encoded as executable matrix/tests (package A) | `tests/tool_program_diff_palette.rs`: `admitted_local_read_palette_satisfies_eligibility_matrix` (all five admitted tools: policy/effect/schema/cache/retry/validate), `diff_contract_declares_bounded_structured_output`, `broker_catalog_exposes_diff_with_programmatic_contract`, `candidate_census_only_deterministic_local_reads_are_eligible` (28 deferred tools with reasons), `mutation_and_network_tools_rejected_from_program_manifest` | pass | `ToolCategory::ReadOnly` alone never admits; matrix is the gate |
| `diff` structured execution + explicit workspace/path context, direct behavior preserved (package B) | `src/tool/diff.rs::execute_with_context` shared by `execute` (string, context-free legacy path) and `execute_structured` (typed value + `codegg/diff`/`LocalTrusted` provenance); `effective_root` prefers broker `ToolExecutionContext.cwd` (workspace root for program calls), falls back to the tool default only for legacy direct callers; relative paths join to the effective root; `check_path_for_symlinks` + canonical prefix check; `line_range` window guard returns `(no changes)` instead of underflowing | pass | Direct input schema and messages unchanged; only huge-diff output is newly bounded |
| `DirectOrProgrammatic` + `ReadOnly` + conservative cache/retry; manifest/catalog follow (package C) | `DiffTool::contract`: `DirectOrProgrammatic`/`ReadOnly`/`Idempotent`, cache 60s/50 entries, retry none, output schema; `ToolBroker` catalog picks it up automatically; `resolve_manifest`/`resolve_contract_snapshot` admit `diff` with no code change; `manifest_allows_diff_and_hash_covers_contract` proves a `DirectOnly` weakening changes the canonical digest | pass | No manifest/ledger schema migration; hash invalidation is structural |
| Program fixture incl. replay/cache/ledger/provenance + path-escape negatives (package D) | Broker program success/structured/provenance tests, no-changes flag, direct/program equivalence, execution-context-root authority test, absolute/`..`/symlink denials, missing-path/missing-original determinism, oversized-original broker+tool bounds, 20k-line truncation metadata, `CallerDenied` (direct-only tool, unverified authority), manifest hash, workspace-scoped cache, ledger reserve/complete/replay roundtrip with divergence failing closed, `into_programmatic_outcome` truthfulness | pass | 23/23 in the new suite |
| Census of other local read tools with disposition, no unrelated promotions (package E) | Census test + `architecture/tool_programs.md` Expansion M001 section (§candidate census); admitted 5, deferred rest with per-tool reasons; production diff limited to `src/tool/diff.rs` | pass | `repo_search` stays M002-blocked; `git`/`lsp` stay M003-blocked |

## 3. Production implementation evidence

- `src/tool/diff.rs` (rewritten, ~2 pre-existing unit tests retained):
  `MAX_ORIGINAL_BYTES` (10 MiB) and `MAX_DIFF_DISPLAY_BYTES` (256 KiB)
  bounds; `contract()` override; `execute_structured()` with structured
  value + provenance; context-derived `effective_root`; symlink/traversal
  policy; truncation helper; `line_range` empty-window guard; 4 new unit
  tests (window guard, truncation bound, contract shape, root authority).
- `tests/tool_program_diff_palette.rs` (new, 23 tests): full §2 A–E
  coverage as listed above.
- Docs: `architecture/tool_programs.md` (palette table 4 → 5 rows +
  Expansion M001 section: matrix, promotion, census), `AGENTS.md`
  (palette + M001 eligibility-matrix rule).
- Deliberately absent (out of scope per plan §5): `repo_search` (M002),
  Git/LSP adapters (M003), write/edit/apply_patch/Bash/terminal promotion,
  `Tool`-trait refactor, storage migration.

Before/after `diff` contract: legacy default (`DirectOnly`,
`NonIdempotent`, no cache/retry, no output schema, string-only result,
process-CWD-relative path resolution) → explicit (`DirectOrProgrammatic`,
`ReadOnly`, `Idempotent`, cache 60s/50, retry none, versioned output
schema, structured bounded value with truncation metadata, `LocalTrusted`
provenance, execution-context workspace root).

## 4. Verification executed

All local (no hosted `CI / verify` claimed).

### Commands run

```bash
cargo test --test tool_program_diff_palette
cargo test --test tool_program_read_palette --test tool_contract_guards --test tool_program_runtime
cargo test --workspace diff --no-fail-fast
cargo test --test tool_broker_integration --test tool_program_cache --test tool_program_context_artifacts --test tool_surface_minimization
cargo test -p codegg --lib tool::diff
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
python3 scripts/check_tool_broker_boundary.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_daemon_cwd_usage.py
scripts/verify.sh quick
```

### Results

- `tool_program_diff_palette`: 23 passed (new M001 suite).
- `tool_program_read_palette`: 21 passed; `tool_contract_guards`: 11
  passed; `tool_program_runtime`: 13 passed (existing palette/contract/
  runtime behavior intact with `diff` added to the registry).
- `--workspace diff` filter: all matching suites passed (82 lib tests in
  the primary binary target; no failures anywhere).
- `tool_broker_integration`: 25 passed; `tool_program_cache`: 14 passed;
  `tool_program_context_artifacts`: 9 passed;
  `tool_surface_minimization`: 13 passed.
- `codegg --lib tool::diff`: 6 passed (2 pre-existing + 4 new).
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  pass, zero warnings.
- Static guards: tool-broker boundary, scheduler-bypass,
  execution-ownership, daemon-cwd all pass (no new spawn site, no broker
  bypass, no protected-module CWD use; the `DiffTool::new()` process-CWD
  default is retained only for legacy context-free direct construction).
- `scripts/verify.sh quick`: passed (fmt, builtin-agents check,
  core-boundary, sandbox contract, execution-ownership,
  `cargo check --workspace --all-targets --locked`).

Plan §11 lists `cargo test --workspace diff --no-fail-fast`: no `diff`
workspace crate exists, so the filter form above plus the focused suites
were used as the justified substitute; nothing was skipped without one.

## 5. Invariant review

| Plan invariant | Evidence | Result |
|---|---|---|
| Program calls go through broker; captured principal/workspace/path policy controls file access | All program tests execute via `ToolBroker::execute` with a verified grant; `BrokerAdapter` path unchanged; unverified-authority program call rejected (`CallerDenied`) | pass |
| No process CWD as program authority | `effective_root` uses broker `ToolExecutionContext.cwd` (workspace root); dedicated test resolves a relative path against the context root while the tool instance is rooted elsewhere | pass |
| Input/output bounds explicit | 10 MiB file + original caps (broker input bound + tool backstop), 256 KiB diff display with marker, `line_range` 1000-change window; truncation sets `truncated` in display, value, and provenance | pass |
| Effect `ReadOnly`; cache/retry/replay metadata truthful | Contract asserts `ReadOnly`/`Idempotent`/no-retry/cache-60s; only `Success` maps to a programmatic `Ok`; ledger replays recorded results and fails closed on divergence; cache keys are content+workspace scoped | pass |
| Contract/manifests version and invalidate correctly | Canonical digest covers caller policy + schemas; weakening test proves digest change; executor admission already compares frozen vs current digests | pass |
| No mutation/network promotion | Census + manifest negatives pin write/edit/apply_patch/bash/git/lsp/repo_search/websearch/tool_program as ineligible; ordinary model tool surface untouched (no disclosure change) | pass |

## 6. Failure and recovery review

- Missing/stale path or missing/invalid `original`: deterministic typed
  failure (`Denied` for policy violations, `InfrastructureError` for
  validation/IO), never `Success`; programmatic outcome maps to `Err`.
- Cancellation: broker deadline/cancellation precheck and
  `BrokerAdapter` cancellation propagation unchanged; no new async path
  added (`execute_with_context` is a straight await chain with one
  `spawn_blocking` file read).
- Replay: ledger semantics unchanged; diff completions persist and replay
  like other reads; no second filesystem read is claimed — replay serves
  the recorded result and divergence fails closed (pinned).
- Contention: concurrent reads mutate nothing; tool holds no interior
  mutability (`&self` only); cache/ledger concurrency behavior unchanged.
- Oversized input: broker `InputTooLarge` rejects before dispatch;
  tool-level `original` bound covers direct callers.

## 7. Migration and compatibility review

- Direct `diff` input schema unchanged (`path`/`original`/`line_range`);
  string output identical except newly bounded huge diffs (truncation
  marker) and the fail-closed symlink/empty-window cases.
- Structured result is additive via the existing `execute_structured`
  adapter convention; legacy `execute` callers unaffected.
- No storage, protocol, config, or Tool-trait migration. No new feature
  flag, CI lane, or binary-size gate per verification policy.
- Rollback: reverting the binary restores the legacy `DirectOnly` diff;
  persisted program records reference contracts by digest and fail admission
  on drift rather than mis-executing.

## 8. Security review

- Authority: every program `diff` call carries the verified grant and
  frozen manifest; workspace/path-policy checks run per call, including
  replay-adjacent paths (broker validates before any cache/replay claim).
- Path policy: absolute escapes, `..` traversal to real outside files,
  and symlink redirections all deny; relative paths cannot escape the
  effective root; error messages name only the requested display path.
- No principal, credential, or reasoning content enters source, manifest,
  cache key, or ledger beyond existing conventions (cache key is
  tool+input-hash+workspace).
- DoS bounds: input/output caps above; no subprocess, network, or lock
  introduced; file read stays in `spawn_blocking` with the broker timeout.

## 9. Documentation and operations

- Updated: `architecture/tool_programs.md` (palette + Expansion M001
  section), `AGENTS.md` (palette + eligibility rule). No skill catalog
  change (no skill enumerates the palette). `tool_broker.md`/`tool.md`
  need no change (no pipeline or trait change).
- Operator diagnostics: unchanged broker error taxonomy (`CallerDenied`,
  `Denied` values, `InputTooLarge`); diff failures surface the existing
  typed tool errors.
- Static guards: all green; no new CI lane added per verification policy.

## 10. Unresolved findings

There are no unresolved critical, high, medium, or low M001 findings.

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None | — | — |

## 11. Roadmap disposition

Milestone closed; M002 is unblocked. Dependency audit against
`plans/registry.md`:

- TP expansion M002 (`repo_search` seam) lists TP expansion M001 closure
  as its sole blocker → moves `blocked` → `ready` (dependency-ready);
  its plan already exists at
  `plans/implementation/tool-program-capability-expansion/002-external-search-programmatic-read-seam.md`.
- TP expansion M003 still waits on M002 (hard) in addition to the now
  closed M001 → stays `blocked`, blocker narrowed to M002 closure.
- No other registered plan lists TP expansion M001 as a dependency, so
  no further status moves. No corrective follow-up required.

## 12. Registry updates

- `plans/registry.md`: expansion row `active / M001 ready` → `active /
  M002 ready`; dependency-ready table: M001 row removed, M002 row added
  (ready on the M001 eligibility contract); execution order §6 reworded
  (M001 closed; M002 may proceed); blocked work: M002 row removed, M003
  blocker narrowed to M002; closure control points: M001 closed row added.
- `plans/subsystems/tool-program-capability-expansion-roadmap.md`:
  Status stays `active`; M001 `ready` → `closed` with closure link;
  M002 `blocked` → `ready`; M003 blocker narrowed to M002.
- `plans/implementation/tool-program-capability-expansion/001-*.md`:
  `ready for handoff` → `implemented` with closure link.
- `plans/implementation/tool-program-capability-expansion/002-*.md`:
  `blocked` → `ready for handoff`.
