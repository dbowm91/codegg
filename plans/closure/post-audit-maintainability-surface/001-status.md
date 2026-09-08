# Post-Audit Maintainability and Surface Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/post-audit-maintainability-surface/001-compatibility-surface-rationalization.md`

Source subsystem roadmap:

- `plans/subsystems/post-audit-maintainability-surface-roadmap.md#7-milestones`

Repository baseline reviewed: `a11614a1`

Implementation commits:

- `a11614a1` — feat(tools): rationalize compatibility surface (maintainability M001)

## 1. Executive finding

M001 is complete. A repository-local compatibility census across the
tool, provider, Git, search, compaction, config, and protocol surfaces
classified every candidate as canonical, compatibility (retained), or
removed with consumer evidence. Two removals landed: the unregistered
`multiedit` module (268 lines plus all coupled live
permission/risk/policy/timeout/agent/doc references) and the legacy
`TodoTool` duplicate (130 lines bypassing todo policy, events, and
persistence). The `codesearch` alias is explicitly retained: three
closed search-eggsearch closure records (002/004/005) accepted it as the
compatibility name, and current agent profiles, plan-mode gating, and
exposure lists rely on the literal name. Canonical evidence tools
(`repo_search`, `repo_fetch`, `repo_map`, `research`, `research_search`,
`batch_fetch`, `security_search`, `evidence_bundle`) were reconciled to
the same Read/ReadOnly permission/risk contract as the alias, closing a
real policy gap where the canonical tool was classified more
restrictively than its alias. No serialized, config, protocol, or
downstream-crate contract was broken: all C/D-class items were retained
with removal conditions. M002 is unblocked by this closure.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Compatibility census with A–F classification and canonical replacement | Section 3 inventory table; work-package-A searches over `src`, `crates`, `architecture`, `docs`, `assets`, `examples`, tests, and closure records | pass |
| Remove F-class dead/unregistered residue | `src/tool/multiedit.rs` deleted; `pub mod multiedit` removed; `TodoTool` struct/impl deleted from `src/tool/todo.rs` | pass |
| Migrate internal imports off removed paths | No internal importer of `multiedit` or `TodoTool` remains (`rg` clean except historical readers and removal notes); fallback branch constructs canonical `TodoWriteTool` | pass |
| `codesearch` dispositioned first with profile/config evidence | Retained: closure commitments 002/004/005 plus `assets/agents/research.toml` allow, generated builtin allow, `plan_allowed`, curated/minimal exposure lists | pass |
| Retained alias is a thin canonical delegate | `CodeSearchTool::execute`/`execute_structured` call `dispatch_repo_search[_structured]`; no independent backend/authority/persistence logic; input translation is the documented coding-profile contract | pass |
| Permission/risk reconciled atomically, no policy gap | `tool_category_for_name` + `classify_tool_risk` + builtin modes + `PERMISSION_TYPES` all cover canonical evidence names; parity tests added | pass |
| High-risk B/C/D disposition with rationale | Section 3 table: all serde/DTO/crypto/config-alias/provider-wire items retained; extraction-seam re-exports retained; agent-compaction shim retained (integration-test consumer) | pass |
| Focused unit/integration/migration tests | 3 registry tests + risk parity/negative tests + permission parity test; existing suites green (tool lib 524, preflight 71, checkpoint 22, agent 103) | pass |
| Docs reconcile surviving surface | `architecture/tool.md`, `permission.md`, `preflight.md`, `model_profile_task_state.md`, `AGENTS.md`, agent TOMLs/examples updated; historical records untouched | pass |
| M002 receives stable canonical-name input | Canonical inventory in section 3; registry unblocks M002 | pass |

## 3. Production implementation evidence

Final compatibility inventory (class: A model alias, B CLI/config,
C serialized, D Rust re-export, E test-only, F dead):

| Candidate | Class | Disposition | Canonical replacement | Evidence / removal condition |
|---|---|---|---|---|
| `codesearch` tool | A | compatibility (retained) | `repo_search` | Retained: search-eggsearch closures 002/004/005 intentionally accepted the alias; `research.toml` + builtin `research` allow it; plan-mode/exposure lists name it. Thin delegate verified. Remove only when M002 finalizes canonical disclosure AND profiles/config migrate off the literal name. |
| Legacy `TodoTool` default | F | removed | `TodoWriteTool` | No consumer besides the `with_options` fallback. Fallback now builds `TodoWriteTool::new` with default state + explicit-todo policy: identical effective behavior plus policy/event/persistence wiring. |
| `multiedit` module | F | removed | `edit` / `apply_patch` | Never in any registry constructor; only test construction. Deleted module + live refs (risk, modes, 5 agent TOMLs + regen, worker/convergence deny logic, timeout field/arm, snapshot gate). |
| `multiedit` historical-name readers | C | compatibility (retained) | n/a (readers, not tools) | Retained: `affected_paths` extract/restorable arms, session-import redaction, eggsentry classification, workflow-action mapping, TUI target rendering. Stored runs/transcripts may name it; readers contain no invocation path. Remove when stored-run migration drops the name. |
| `task` action `get` | A | compatibility (retained) | `status` (durable runs) | Retained: agent-run workstream plan preserves `spawn`+`get` names; `get` serves task-id lookup (`AgentTaskId`/legacy numeric) while `status` serves run-id control — distinct operations, not a pure alias. No change. |
| Provider/root re-exports (`src/provider`, `src/auth`, error, protocol_conversions) | D | compatibility (retained) | `codegg-providers::*`, `codegg-core::*` | Thin extraction seams per module contracts; ~50 internal consumers use root paths. Migrate only with a dedicated import-migration pass. |
| Git re-exports/facades (`git_mutations`, `git_service`, network/recovery) | D/B | canonical (retained) | `egggit` reads + `git_mutations` executor | Accepted Git ownership layers (convergence M003); facades delegate, no duplicate execution. |
| `src/agent/compaction.rs` shim | D | compatibility (retained) | `crate::context::compaction` | 7-line re-export WITH live consumers (`tests/compaction.rs`, `tests/provider_transcripts.rs`). Remove when those tests migrate to the canonical path. |
| `src/lsp` shim, `src/shell_session` | D/E | retained, deferred | `egglsp` / subsystem decision | LSP shim has ~30 consumers (extraction seam). `shell_session` has no production consumers but is a documented subsystem module, not transitional surface; removal needs a subsystem-level decision, not alias disposal. |
| Config serde aliases, `CommandIntentMode::Route`, provider credential fields, `InlineScript` parse arm | C | compatibility (retained) | canonical field names | User config must keep parsing. `InlineScript` still warns-and-skips in hooks. No migration milestone exists. |
| Protocol DTO compat fields, `YamlCompatibility`, legacy SSE function-call parser, provider wire aliases, crypto legacy decrypt | C/A | compatibility (retained) | canonical DTOs/parsers | Wire/storage compat cannot be removed by alias cleanup. |
| `CircuitBreaker::is_available` (deprecated) | D | compatibility (retained) | `call()` | Properly deprecated with named migration; only a test calls it. Pre-1.0 API removal needs downstream evidence, not alias disposal. |
| `PERMISSION_TYPES`, `plan_allowed`, exposure lists naming `codesearch` | B | reconciled, disclosure owned by M002 | canonical names | `PERMISSION_TYPES` extended with canonical evidence names. Plan-mode/exposure/curated lists intentionally left naming `codesearch`; M002 owns disclosure changes. |

Removed live references (verified by `rg`): module, `pub mod`,
`TodoTool` type, risk arms, mode lists, agent TOML denies (5 assets +
3 examples), generated builtins (regen verified), `requires_isolation` /
`is_read_only_agent` / `read_only_blocked_tools` / verifier-deny entries
(removal required: leaving the name would force universal isolation),
timeout field + dispatch arm, snapshot gate arm, preflight direct test,
and all presenting docs.

## 4. Verification executed (commands + results; local unless noted)

- `cargo test -p codegg --lib -- tool::` — 524 passed, 0 failed (includes
  3 new `compatibility_surface_tests` + risk parity/negative tests).
- `cargo test -p codegg --lib -- permission:: agent::` — parity test
  `codesearch_alias_matches_canonical_category` ok; all 103 agent tests
  ok (builtin deny-list updates consistent with regen).
- `cargo test --test preflight_integration` — 71 passed (multiedit test
  removed; edit/replace/apply_patch preflight coverage intact).
- `cargo test --test edit_checkpoint_integration` — 22 passed (retained
  multiedit affected-path reader pinned).
- `cargo test -p codegg-providers`, `-p codegg-git`, `-p codegg-core`
  snapshot filter — green (0 unit tests in providers/git roots; 23
  core snapshot-filtered pass).
- `rg 'TodoTool' src crates architecture docs README.md AGENTS.md tests`
  — only M001 removal notes remain.
- `rg 'multiedit' src crates architecture docs README.md AGENTS.md` —
  only historical readers, removal notes, and the negative test remain.
- `cargo fmt --all -- --check` — clean.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  — clean.
- `scripts/verify.sh quick` — passed (includes generated-agents check,
  core boundary, sandbox contract, execution ownership, locked
  workspace check).
- `python3 scripts/generate_builtin_agents.py --check` — passed.
- `bash scripts/check-core-boundary.sh`,
  `check_scheduler_bypass.py`, `check_tool_broker_boundary.py`,
  `check_daemon_cwd_usage.py` — all passed.
- No hosted `CI / verify` run: this milestone makes no daemon,
  scheduler, protocol, or release change; local `verify.sh quick` plus
  all-feature Clippy is the proportionate posture per the roadmap.

## 5. Invariant review

- Every retained compatibility path delegates to one canonical owner:
  `codesearch` → `dispatch_repo_search`; Git facades → accepted
  execution layers; re-exports → canonical crates. The deleted paths
  have no delegation because they have no consumers.
- No divergent semantics: canonical evidence tools now share the alias
  Read/ReadOnly classification; `TodoWriteTool` fallback preserves the
  old effective behavior while adding policy enforcement the duplicate
  lacked (strictly safer).
- Serialized/config/protocol compat untouched (all retained).
- Tool Program authority untouched (no broker/manifest/contract change;
  `tool_broker_boundary` guard green).
- Search/Git/provider ownership untouched (no backend merge; guards
  green).
- Closure records untouched (only new record added).
- Removing `multiedit` from child-isolation/read-only lists was
  behavior-preserving *because* the TOML denies were removed
  atomically: with no agent able to deny an unregistered name, leaving
  the string would have forced universal isolation.

## 6. Failure and recovery review

Behavior-preserving milestone; no new cancellation/restart mechanism.
No removed wrapper owned tasks, locks, stores, or recovery behavior:
`multiedit`/`TodoTool` were stateless tool impls. The retained
affected-paths/import readers keep restart-tolerant handling of stored
runs naming `multiedit`. The todo fallback constructs fresh in-memory
state exactly as before (no persistence, no session binding).

## 7. Migration and compatibility review

No migration required:

- `multiedit` was never registered, so no transcript, config, profile,
  plugin, or Tool Program could invoke it through a supported path.
  Removal condition checklist (§9 of the plan) holds for all seven
  conditions.
- `TodoTool` → `TodoWriteTool` fallback: same tool name
  (`todowrite`), same parameters, same default policy; only the
  policy-bypass is gone.
- `codesearch`, `get`, serde aliases, DTO fields, config aliases:
  retained, so no consumer migrates.
- Deferred: `shell_session` removal and deprecated pub-API removal need
  dedicated subsystem/API decisions and are recorded in section 10, not
  smuggled into this closure.

## 8. Security review

- Permission/risk reconciliation is monotonic for read-only tools: the
  canonical evidence tools move from Mutating/Unknown (prompt) to
  ReadOnly/Read (short-circuit Allow), matching their declared
  `Tool::category()` and the alias treatment. No mutating tool changed
  level. Discovery authority is unchanged.
- Session-import redaction for `multiedit` retained, so historical
  inputs with paths stay redacted on export.
- Eggsentry still classifies `multiedit`-named calls as filesystem
  writes in scanned records.
- Removed-alias negative coverage: `classify_tool_risk("multiedit")`
  is now `Unknown` (no shadow Write path), and no registry resolves
  `multiedit`.
- Auth logging, credential handling, sandbox, and destructive-command
  policy untouched.

## 9. Documentation and operations

- `architecture/tool.md`: todo table corrected; multiedit file-tree and
  integration-table entries removed; former "NOT Registered" section
  rewritten as "REMOVED in M001" with replacements and retained-reader
  list.
- `architecture/permission.md`: mode table, short-circuit list, and
  `PERMISSION_TYPES` inventory updated (19 → 27 names).
- `architecture/preflight.md`: integration table drops `multiedit`.
- `architecture/model_profile_task_state.md`: legacy-wrapper paragraph
  replaced with canonical-fallback statement.
- `AGENTS.md`: multiedit bullet replaced with M001 removal note.
- `assets/agents/*.toml` (5) + `examples/agents/*.toml` (3): stale
  `multiedit` allow/deny lines removed; `research.toml` keeps
  `codesearch = "allow"` (alias retained).
- `src/agent/builtins/generated.rs`: regenerated; `--check` green.
- Historical closure records and prior roadmaps: not edited.

## 10. Unresolved findings (severity: low)

1. (low) `plan_allowed`, curated/minimal exposure lists, and builtin
   mode allowed lists still advertise `codesearch` rather than
   canonical `repo_search`. Intentional: disclosure changes belong to
   M002, which now has the stable canonical inventory. No capability or
   policy gap: the alias delegates fully.
2. (low) `shell_session` module has no production consumers but is
   retained pending a subsystem-level removal decision.
3. (low) Deprecated `CircuitBreaker::is_available` retained pending
   pre-1.0 downstream-migration evidence; migration (`call()`) is named
   in the deprecation note.
4. (low) `PERMISSION_TYPES` remains a write-only documented inventory
   (no code reader); extended for truthfulness, removal out of scope.

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

M001 meets all exit conditions in
`plans/subsystems/post-audit-maintainability-surface-roadmap.md#7-milestones`:
every touched item has a canonical replacement with consumer/removal
rationale; adapters contain no independent execution behavior; obsolete
permission/risk/docs entries were removed atomically with the aliases;
no user-facing or serialized compat path was removed without evidence;
focused tests prove canonical paths retain behavior.

Recommendation: M002 is ready. Its input inventory is the section 3
table; known first candidates are the research/evidence disclosure
set (`research`, `research_search`, `batch_fetch`, `evidence_bundle`,
plus canonical `repo_search` vs retained `codesearch` advertisement).
M005 remains blocked on M002 (plus the M003 interface dependency).
M003/M004 are unaffected (soft/independent).

## 12. Registry updates

- `plans/registry.md`: M001 moved to closed with this closure record;
  M002 moved from blocked to ready (hard dependency satisfied);
  subsystem row updated to `M001 closed, M002 ready`; blocked-work
  entry for M002 removed; M005 entry retained (still hard-blocked on
  M002).
- `plans/subsystems/post-audit-maintainability-surface-roadmap.md`:
  M001 → closed with closure link; M002 → ready (unblocked by M001).
- `plans/implementation/post-audit-maintainability-surface/001-compatibility-surface-rationalization.md`:
  status → closed.
- `plans/implementation/post-audit-maintainability-surface/002-model-visible-tool-surface-minimization.md`:
  status → ready for handoff (dependency audit: sole hard dependency
  was M001; no other hard/interface dependency is outstanding).
