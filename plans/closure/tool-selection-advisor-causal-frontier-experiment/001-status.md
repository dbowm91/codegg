# Tool-Selection Advisor Causal Frontier Experiment M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-causal-frontier-experiment/001-typed-contracts-state-and-benchmark.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-causal-frontier-experiment-roadmap.md#m001--typed-causal-contracts-state-projection-and-benchmark-preregistration`

Repository baseline reviewed: `82e289db`

Implementation commits or pull requests:

- `152613de` — tool-advisor causal-frontier M001: typed contracts, state snapshot, benchmark + gates

## 1. Executive finding

M001 is complete and closes positively. The milestone delivered the full
trusted substrate for causal tool-menu experiments with no runtime
visibility change: a closed 17-fact / 14-outcome typed ontology, an additive
native causal-contract seam, a deterministic host-owned state snapshot, 20
static native pilot contracts, a frozen 168-case stateful benchmark across
13 families, and frozen M002 selection gates. Every acceptance criterion in
the source plan is satisfied with test and receipt evidence below. No causal
filter affects provider definitions, broker authorization, or disclosure in
M001.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Closed typed state/effect ontology (§2) | `src/tool_advisor/causal_frontier.rs`: `CausalStateFact` (17), `CausalOutcome` (14); `ontology_sizes_frozen`, `fact_wire_strings_stable`, unknown-variant rejection tests | pass | Exact vocabulary of plan §2.1/§2.2 |
| Additive native causal-contract seam (§3) | `Tool::causal_contract` default `None` (`src/tool/mod.rs`); `ToolRegistry::causal_contract_of`; `default_tool_trait_seam_is_none` | pass | Existing `Tool::contract`, broker, retry, cache, permission untouched |
| Missing contract is safe (§3) | `default_tool_trait_seam_is_none`, `uncontracted_tools_have_no_contract` (bash, tool_search, skill, MCP/external) | pass | `None` = not classifiable, never denied |
| Deterministic host-owned snapshot (§4) | `CausalStateSnapshot::from_inputs`, `snapshot_deterministic_and_fingerprint_stable`; fingerprint binds ontology + facts + host revisions + surface fp | pass | `snapshot_binds_surface_fingerprint`, `snapshot_binds_host_revisions` |
| Snapshot bounded, no raw payload (§4) | Counts capped, ids truncated; `snapshot_contains_no_raw_prompt_or_output`, `snapshot_ids_and_counts_bounded`, key-allowlist assertion | pass | Secret-like ledger strings provably absent |
| Each fact has one canonical host source (§10) | `apply_goal` / `apply_work_plan` / `apply_ledger` / `set_failed_tests` / `set_security_findings` / `apply_preview_registry` / `set_context_read_available`; 17 per-fact tests + `unmet_acceptance_without_typed_evidence_sets_no_fact` | pass | No embedding/LLM classifier; descriptions never interpreted |
| Bounded pilot native contracts (§5) | `native_causal_contract` + per-tool `causal_contract()` overrides for 20 tools; `pilot_contracts_validate`, `registry_seam_matches_native_table` | pass | Generic tools carry outcomes only; multiplexed `bash` stays uncontracted, `git` uses conservative union |
| Contract integrity (§6) | `bind_causal_contract` (name + impl id/version + schema fp + ontology + payload); contradiction/empty-group/empty-provenance/version tests; name-mismatch, schema-sensitivity, determinism tests | pass | Catalog fp `0ac06de8…` frozen in prereg |
| Frozen stateful benchmark ≥160 cases (§7) | `assets/tool-advisor/causal-frontier-v1.jsonl` fp `f60dad17…`: 168 cases, 13/13 required families; `benchmark_loads_and_validates` | pass | Gold current-step satisfiable + premature unsatisfiable re-derived against live contracts |
| Dev/qualification split frozen (§8) | Prereg dev 112 / qual 56, family-balanced, disjoint, fingerprinted; `verify_against` in `preregistration_matches_recomputation` | pass | Qual untouched until M002 selection frozen |
| M002 gates frozen (§9) | `CausalM002Gates::m001_frozen` + prereg `metric_formulas`/`tie_breaking`; exact gate-value assertions | pass | 1.00 / 0 / 1.00 / ≥0.50 / ≤4 / ≤5 ms |
| Hidden/denied never in frontier (§10) | `benchmark_frontier_universe` + `benchmark_withheld_never_in_frontier_universe`; `withheld` sets in every case | pass | Upstream authority stays authoritative |
| Surface fingerprint bound (§10) | `snapshot_binds_surface_fingerprint` | pass | — |
| No runtime visibility change (§12) | No disclosure/broker/request-preparation edits (`git show --stat 152613de`); `scripts/verify.sh quick` green | pass | M002 owns first offline measurement |

## 3. Production implementation evidence

Implementation `152613de` (25 files, +~2500/​+110 lines):

- New module `src/tool_advisor/causal_frontier.rs` (~2500 lines incl. 48
  tests): ontology, contract types + validation + canonical fingerprinting,
  `CausalStateInputs` host extractors over `Goal` / `WorkPlan`+`WorkItem` /
  `ContextLedgerState` / `PreviewArtifactRegistry`, `CausalStateSnapshot`,
  benchmark loader/validator, preregistration receipt + verifier, M002 gates.
- `Tool::causal_contract` additive default (`src/tool/mod.rs`) plus
  `ToolRegistry::causal_contract_of` registry seam. No change to
  `Tool::contract`, `ToolContractCatalog`, broker, permission, disclosure, or
  request preparation.
- One-line `causal_contract()` overrides delegating to
  `native_causal_contract` in 20 pilot tools: read, glob, grep, lsp, write,
  edit, git, commit, goal_get, goal_update_progress, work_plan_get,
  work_plan_update_item, lsp_preview_apply, test, verify, task, context_read,
  research, webfetch, websearch. `bash`, deferred/specialist, MCP/plugin, and
  external tools remain uncontracted and discoverable.
- Frozen assets: `assets/tool-advisor/causal-frontier-v1.jsonl` (168 cases;
  fp `f60dad170f3a2527…`) and
  `assets/tool-advisor/causal-frontier-m001-preregistration.json` (catalog
  fp `0ac06de84189b093…`, dev 112 / qual 56, formulas, tie-breaking, gates).
  The prereg contract payloads are cross-checked against live recomputation
  by test, so the checked-in mirror cannot drift from code.
- `architecture/tool-advisor.md`: causal-frontier M001 subsection (ontology,
  seams, invariants, freeze receipts).

Planned but absent by design (owned by later milestones): M002 offline
frontier evaluation, M003 effect-path refinement, M004 observe-mode runtime
integration, M005 active disclosure. No provider-definition or palette
change exists anywhere in this workstream yet.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg --lib tool_advisor::causal_frontier
cargo test -p codegg --lib tool::contract
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

### Results

All local. Hosted CI runs on push (see §12); this record is written
against local evidence plus the cited receipts.

- `tool_advisor::causal_frontier`: 48 passed / 0 failed. Covers ontology
  closure, 17 per-fact host-source tests, snapshot determinism/secrecy/
  binding, contract validation/fingerprinting/binding, registry-seam parity
  (default registry sweep + directly constructed session-gated tools),
  benchmark load/validate/gold-consistency/withheld-exclusion, and
  prereg-against-recomputation (catalog fp, benchmark fp, split coverage,
  gate values, contract payloads).
- `tool::contract`: 13 passed / 0 failed (existing contract suite
  unaffected by the additive seam).
- `scripts/verify.sh quick`: passed (agent schema, core-boundary,
  client/desktop boundaries, sandbox, execution-ownership, TUI authority,
  HTTP-route disposition, audit coverage, scheduler bypass, provider-wire
  guards, workspace check).
- `cargo fmt --all -- --check`: clean. `git diff --check`: clean.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: all
  M001 code clean. One remaining error is pre-existing toolchain drift in
  untouched `src/main.rs:3677` (`clippy::never_loop` on the daemon
  singleton acquire loop; file byte-identical to baseline `82e289db`).
  Fixing daemon-startup control flow is out of M001 scope; recorded as a
  low finding in §10. Hosted CI is the arbiter (see §12).

### Hosted CI reconciliation (factual correction, post-push)

- Run `36964201769` (closure commit `7165f9a2`) failed exactly one step:
  hosted Workspace Clippy (rust 1.99.0) flags the same pre-existing
  `never_loop` at `src/main.rs:3677`. The parent main run `36958183677`
  failed identically, confirming the drift predates M001.
- Follow-up `62653a85` removes the vacuous `loop`/`break` (direct match;
  every other arm exits the process, so no iteration was possible — no
  behavior change). Local `cargo clippy --workspace --all-targets --locked
  -- -D warnings` is now fully clean.
- Run `36964618785` (on `62653a85`, includes this M001 implementation):
  **success**. The ordinary hosted `verify` gate is green with M001 landed.

## 5. Invariant review

- `ResolvedToolSurface` remains the only per-turn capability ceiling: no
  surface, disclosure, or request-preparation code was touched.
- Causal filtering changes visibility only: M001 builds no filter at all;
  contracts are inert metadata until M002 evaluates them offline.
- Required/never-reduce tools remain visible: benchmark marks
  `tool_search` required and `read` never-reduce; validation rejects any
  case placing them in a premature set.
- Uncontracted tools stay discoverable: `bash`, specialist, MCP/plugin, and
  synthetic external tools carry no contract; the unknown-uncontracted
  family exercises fallback discoverability with an external tool as gold.
- Hidden/denied/disabled tools cannot be restored: every case carries
  `withheld` identities disjoint from `eligible`, and frontier construction
  goes through `benchmark_frontier_universe` (eligible only).
- No remote telemetry, no model weights, no new inference runtime, no chain
  of thought parsing or storage.
- State projection holds host-owned bounded facts only (§2 table + secrecy
  tests).
- Contract metadata is versioned, deterministic, and fingerprinted
  (ontology v1, schema v1, canonical JSON, bound fingerprints).

## 6. Failure and recovery review

- Contradictory contracts (`requires_all`/`requires_any` ∩ `forbids`),
  empty `requires_any` groups, empty/oversize provenance, and unsupported
  schema versions are rejected by `validate()` with typed errors and covered
  by tests. Unknown enum values fail at serde parse time.
- A contract served under the wrong tool name yields a different bound
  fingerprint (name-mismatch test); the catalog is name-keyed and the
  registry seam is parity-tested per tool.
- Input-schema changes alter bound fingerprints (schema-sensitivity test)
  without invalidating the payload-level catalog freeze; any contract-content
  edit changes the prereg catalog fp and requires a new experiment version
  per split discipline.
- Malformed benchmark lines fail `load_causal_benchmark` with the case id
  or line index; duplicate ids, family violations, and gold-set violations
  are typed errors, all exercised through the frozen asset itself.

## 7. Migration and compatibility review

Additive only. The `Tool` trait gains a defaulted method; all existing
implementors compile unchanged and behave identically (default `None`).
No storage migration, no protocol change, no config change, no migration of
existing advisor artifacts. The two new assets are new files consumed only
by the new module's tests. Rollback is a clean revert of `152613de` with no
durable side effects.

One deliberate integrity note: the prereg `contract_catalog_fingerprint`
is payload-level (name + impl marker + ontology + payload) rather than
schema-bound, so unrelated input-schema refactors do not invalidate the
frozen receipt; per-tool schema binding is still enforced structurally by
`bind_causal_contract` and its sensitivity test.

## 8. Security review

No authorization, permission, sandbox, or execution path was modified. The
snapshot secrecy tests prove ledger strings (including secret-shaped
values), artifact-handle contents, and command text never enter snapshots;
only presence/counts and bounded typed ids do. Provenance strings are
bounded (512 chars). No secrets are logged. Benchmark and prereg assets
contain no repository text beyond tool names and synthetic state labels.

## 9. Documentation and operations

- `architecture/tool-advisor.md`: new "Causal frontier experiment (M001
  foundation)" section (ontology, seams, invariants, freeze receipts).
- Source plan stays the handoff contract; this closure record is the gate.
- Operator action: none. M002 implementors consume the frozen benchmark +
  prereg; the qualification partition (56 cases) must not be inspected for
  contract debugging before M002 selection freezes.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low (resolved) | Local and hosted `cargo clippy --workspace --all-targets` reported pre-existing `clippy::never_loop` at untouched `src/main.rs:3677` (hosted run `36958183677` on the parent commit failed identically). | None on M001 evidence; the lint blocked the hosted gate. | Resolved by mechanical follow-up `62653a85` (loop removal, no behavior change; isolated commit outside M001 scope). Local clippy fully clean; hosted run `36964618785` green. No toolchain-drift corrective needed. |
| low (resolved) | Hosted CI result pending at closure-write time (local evidence only). | Positive closure assumed ordinary hosted CI green per plan §11. | Reconciled: hosted `CI / verify` run `36964618785` success with M001 landed. Assumption holds. |

No medium or higher findings. No corrective pass required.

## 11. Roadmap disposition

M001 closed positively. Consequences:

- M002 (offline causal-admissibility frontier) is dependency-ready: its
  hard dependency (positive M001) is satisfied and its frozen inputs
  (benchmark fp `f60dad17…`, catalog fp `0ac06de8…`, dev 112 / qual 56)
  exist. M002 moves blocked → ready in the same commit as this record.
- M003 stays blocked/optional on positive M002 (A). M004 stays blocked on
  positive M002. M005 stays blocked on positive M004. No other registered
  plan lists M001 as a dependency; nothing else is unblocked.
- The subsystem roadmap M001 row moves ready → closed; M002 row moves
  blocked → ready.

## 12. Registry updates

- `plans/registry.md` subsystem row: causal-frontier experiment
  `M001 ready; M002-M005 dependency-gated` → `M001 closed (positive);
  M002 ready; M003-M005 dependency-gated`.
- `plans/registry.md` plan rows: M001 `ready` → `closed` (this record,
  implementation `152613de`); M002 `blocked` → `ready` (positive M001;
  frozen inputs cited above). M003/M004/M005 rows unchanged.
- `plans/subsystems/tool-selection-advisor-causal-frontier-experiment-roadmap.md`:
  M001 status ready → closed; M002 status blocked → ready.
- `plans/implementation/tool-selection-advisor-causal-frontier-experiment/001-*.md`:
  status ready-for-handoff → closed (pointer to this record).
- `plans/implementation/tool-selection-advisor-causal-frontier-experiment/002-*.md`:
  status blocked-on-positive-M001 → ready for handoff.
- Hosted CI: ordinary `verify` run `36964618785` success (on `62653a85`,
  includes this M001 implementation). Prior run `36964201769` failed only
  on the pre-existing hosted `never_loop` drift, resolved by `62653a85`.
