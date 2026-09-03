# Memory-to-Skill Promotion M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/memory-skill-promotion/002-user-triggered-skill-draft-and-preview.md`

Source subsystem roadmap:

- `plans/subsystems/memory-skill-promotion-roadmap.md#13-milestones`

Repository baseline reviewed: `583c2702ad45fa28401f5c10bc0a0b6e19fa73fb`

Implementation commits or pull requests:

- `583c2702ad45fa28401f5c10bc0a0b6e19fa73fb` — feat: add user-triggered skill draft and preview proposals
- M001 dependency revision: `2f029d8dd7de49876cf6527c835e586bd3d46e3c` (strictly closed in `plans/closure/memory-skill-promotion/001-status.md`)

## 1. Executive finding

M002 is complete as a user-triggered, pre-publication proposal boundary. A
`Ready` habit candidate can be explicitly promoted into one bounded portable
`SKILL.md` proposal that is validated through the shared skill parser,
persisted with digest/revision provenance, previewed/rejected by the user,
and never installed. No skill file is written, no asset refresh is invoked,
and no automatic drafting exists. All 13 acceptance criteria are met with
focused tests and `scripts/verify.sh quick` passing.

The implementation follows the preferred current-AgentLoop flow: no new
direct `Provider::stream()` caller was added. Subagent delegation is
intentionally stricter than the plan minimum — `skill_proposal` is
`DirectOnly`, so subagents cannot submit at all in M002 rather than through
an explicit delegation token.

## 2. Requirement-to-evidence matrix

| Requirement (plan §17) | Evidence | Result | Notes |
|---|---|---|---|
| 1. No automatic draft on Ready | `SkillPromotionStore::begin_request` is the only request constructor; TUI `/skill-promote` is the only production caller; no `AgentFinished` or background hook calls it | pass | `tests/habit_skill_promotion.rs::non_ready_habit_cannot_begin_request`, `submission_without_explicit_request_is_denied` |
| 2. Explicit request scoped to session/project/habit revision | `PromotionDraftRequest` carries session_id, project_namespace, habit_id, fingerprint, candidate_revision; 15-min TTL; single-use consumed flag | pass | `wrong_session_project_or_habit_is_denied`, `expired_request_cannot_be_replayed`, `request_is_revision_bound_and_consumed_after_one_submission`, `stale_candidate_revision_is_rejected` |
| 3. Bounded habit evidence only | `HabitPromotionContext` exposes habit_id, fingerprint, action skeleton (≤32), counts, ≤32 existing names, ≤16 bounded memory refs (≤512 chars each); prompt builder excludes session IDs, raw commands, tool output | pass | `promotion_context_is_structural_and_prompt_hides_session` |
| 4. Host submission boundary, no skill-path write | `skill_proposal` tool (`action=submit`) validates scope/freshness then `SkillPromotionStore::submit`; no filesystem skill root touched; no `AssetRefreshCoordinator` reference in promotion modules | pass | `proposal_restrictions_reject_allowed_tools_without_skill_root_changes`, grep shows no `AssetRefresh` in `promotion.rs`/`skill_proposal.rs` |
| 5. Portable parsing reused | `validate_portable_document` extracted in `src/skills/parser.rs`; `parse_candidate` portable branch delegates to it | pass | `proposal_parser_reuses_portable_discovery_rules`, `parser_rejection_matches_discovery_for_malformed_and_oversized` |
| 6. One SKILL.md; scripts/resources/plugins/MCP and allowed-tools rejected | Generated-restriction layer: frontmatter allowlist (name/description/license/metadata), allowed-tools error, `scripts/`/`resources/`/`package.json`/`mcp:`/`plugin:` sidecar rejection | pass | `proposal_restrictions_reject_allowed_tools_without_skill_root_changes`, `proposal_restrictions_reject_scripts_and_sidecars_but_allow_prose` |
| 7. Bounded name/description/body with digest/revision | `MAX_NAME_BYTES` 128, `MAX_DESCRIPTION_BYTES` 2048, `MAX_MARKDOWN_BYTES` 256 KiB, `MAX_DIAGNOSTICS` 32; `compute_content_digest` domain-separated SHA-256 with CRLF normalization; proposal `revision` starts at 1 | pass | `proposal_digest_is_deterministic_and_content_bound`, `proposal_scope_is_host_enum_not_path` |
| 8. Atomic, locked, path-safe, restart-safe store | Per-namespace file `skill-promotions/<hex>.json`; namespace validated as `project/<64hex>`; `flock` + load/modify/temp-write/`sync_all`/rename; oversized/malformed files fail closed | pass | `malformed_persisted_proposal_fails_safely`, concurrency inherited from M001 lock pattern; 16/16 promotion tests pass |
| 9. Preview with content, validation, scope, provenance, collision | `/skill-proposals` list, `/skill-proposal <id>` full preview (name/description/body, scope, habit fingerprint/rev, digest/rev, stored + live collision diagnostics, not-installed notice) | pass | TUI code in `src/tui/app/mod.rs`; `collision_diagnostic_is_advisory_without_overwrite` |
| 10. Reject/revise explicit, habit untouched | `reject_proposal` Validated→Rejected monotonic; re-promote via new `/skill-promote` request; habit stays `Ready` (never `Promoted` in M002) | pass | `rejection_lifecycle_is_monotonic_and_habit_stays_ready` |
| 11. No skill write or refresh/effective change | No publisher module; preview/reject paths carry no refresh call; effective registry length unchanged in test | pass | `proposal_restrictions_reject_allowed_tools_without_skill_root_changes` asserts no `.codegg/skills` dir and equal effective len |
| 12. Discovery/activation/memory compatible | Parser refactor preserves `skills_registry` behavior (24/24 pass); habit store additive `revision` with serde default; memory tests 23/23 pass | pass | `skills_registry`, `codegg-core memory`, `tui::command` suites |
| 13. Focused tests + quick verification pass | See §4 | pass | 16/16 promotion, 5/5 habit, 24/24 registry, `verify.sh quick` green |

## 3. Production implementation evidence

Ownership and touch set (all landed in `583c2702`):

- `crates/codegg-core/src/memory/habit.rs` — additive monotonic `revision`
  (`#[serde(default)]`, starts at 1, bumped on observe/dismiss/promote/
  supersede) so promotion requests detect post-review candidate changes
  without altering fingerprint, threshold, or privacy semantics.
- `src/skills/parser.rs` — extracted `validate_portable_document(source,
  config)` as the single portable frontmatter/body seam; added
  `ValidatedSkillDocument`; `parse_candidate` portable branch delegates to
  it (same size/frontmatter/name/description rules, same normalization).
  Native-compat path untouched.
- `src/skills/promotion.rs` (new, ~800 lines) — `SkillPromotionStore` with
  `begin_request`/`submit`/`list_proposals`/`get_proposal`/
  `reject_proposal`/`append_diagnostics`, `compute_content_digest`,
  `build_draft_prompt`, `collision_diagnostics`, generated-restriction
  diagnostics, bounded context/provenance, `flock`-locked atomic JSON
  persistence under `~/.config/codegg/memory/skill-promotions/`. No
  publisher, no skill-root write, no refresh invocation.
- `src/skills/mod.rs` — `pub mod promotion`, re-export of
  `validate_portable_document`/`ValidatedSkillDocument`.
- `src/tool/skill_proposal.rs` (new) — `skill_proposal` tool,
  `action=submit` only, `SafeMutating` + `DirectOnly` + `NonIdempotent` +
  no retry, output schema requires `status`/`proposal_id`/
  `content_digest`. `execute_structured` enforces session context, parses
  IDs, calls `SkillPromotionStore::submit`, then appends advisory
  collision diagnostics for `Validated` proposals.
- `src/tool/mod.rs` — `pub mod skill_proposal`; registered in
  `ToolRegistry::with_options` beside `skill` without altering it.
- `src/tui/command.rs` — `/skill-promote`, `/skill-proposals`,
  `/skill-proposal` registrations; built-in count 108→113.
- `src/tui/app/mod.rs` — `/skill-promote` (Ready-gated request + bounded
  drafting prompt injected via `set_text`/`send_prompt`),
  `/skill-proposals` (bounded list + re-draft hint),
  `/skill-proposal` (full preview with stored + live collision diagnostics,
  reject path). Long output uses the existing memory info dialog.
- `tests/habit_skill_promotion.rs` (new, 16 tests) — initiation,
  parser/restriction, store, privacy/provenance, non-effect, collision
  coverage per plan §15.
- `architecture/memory.md`, `architecture/skills.md`, `architecture/tool.md`
  — proposal-boundary documentation (initiation, validation seam,
  tool contract, commands, no-refresh invariant).

Deliberately absent (per non-goals): no `.codegg/skills` writer, no
`AssetRefreshCoordinator` call, no `Published` transition in M002 paths
(the `Published`/`Superseded` enum variants are reserved for M003), no
habit `Promoted` marking, no `Global` target selection (Project-only hint,
stricter than allowed), no subagent delegation token, no config-schema or
protocol migration.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
git diff --check
cargo test -p codegg-core habit --locked
cargo test -p codegg-core memory --locked
cargo test --test skills_registry --locked
cargo test --test habit_skill_promotion --locked
cargo test --locked --lib tui::command
cargo test --test tool_contract_guards --locked
bash scripts/check-core-boundary.sh
python3 scripts/check_sandbox_contract.py
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_git_forbidden_patterns.py
scripts/verify.sh quick
```

### Results

- `cargo fmt --all -- --check`: pass after formatting (3 files reformatted
  during implementation).
- `git diff --check`: pass.
- `cargo test -p codegg-core habit --locked`: 5 passed.
- `cargo test -p codegg-core memory --locked`: 23 passed.
- `cargo test --test skills_registry --locked`: 24 passed.
- `cargo test --test habit_skill_promotion --locked`: 16 passed
  (5 pre-existing + 11 new covering wrong-session/project/habit, expiry,
  non-ready gate, malformed/oversized/invalid-name drift, sidecar prose
  discrimination, digest determinism/CRLF, rejection monotonicity +
  habit-stays-Ready, host-enum scope, corrupt-file fail-closed, structural
  prompt privacy, advisory collision).
- `cargo test --locked --lib tui::command`: 115 passed (count test updated
  111→113 after recount of built-ins).
- `cargo test --test tool_contract_guards --locked`: 11 passed.
- `bash scripts/check-core-boundary.sh`: passed.
- `check_sandbox_contract.py`: passed.
- `check_daemon_cwd_usage.py`: passed (no `current_dir` in protected
  modules; `skill.rs` fallback untouched).
- `check_scheduler_bypass.py`: ok.
- `check_execution_ownership.py`: ok (no new process-spawn site; tool
  executes through the broker store path).
- `check_git_forbidden_patterns.py`: PASS.
- `scripts/verify.sh quick`: passed (fmt, generated-agent check, core
  boundary, sandbox, execution-ownership, capped workspace
  `check --all-targets` in ~3m21s).

Not run / environment-noted (matching M001 precedent, not acceptance
blockers):

- Full workspace `cargo test` sweep: not run per the test-resource budget;
  narrowest covering suites were run instead.
- `lsp-test-support` / `--all-features` paths: untouched by this change,
  not run.
- Live provider drafting: not required by the plan; integration uses
  store/tool-level submission with no model call.
- `check_tool_broker_boundary.py` reports a pre-existing violation in
  `src/tool/review.rs:216` (direct `execute_structured`, untouched by
  M002); M002 adds no direct tool-execution call.
- `check_project_catalog_invariants.py` reports `STORAGE_LAYOUT_VERSION is
  48` (storage file untouched by M002); `verify.sh quick` does not gate on
  it and it is recorded here as pre-existing.

## 5. Invariant review

From the roadmap §3 and plan §2/§11:

- Automatic observation never writes a skill: preserved — promotion modules
  contain no skill-root write; test asserts no `.codegg/skills` dir.
- Habit detection stays deterministic/host-owned, no background LLM: no new
  provider caller; `begin_request` is user-command-only.
- Multi-session readiness unchanged: `DEFAULT_READY_OCCURRENCES`/`MIN_READY_SESSIONS`
  untouched; `revision` is additive metadata only.
- No raw tool output/commands/reasoning in habit or proposal evidence:
  context carries skeleton/counts/fingerprint plus explicitly passed bounded
  memory summaries (currently empty from TUI); prompt hides session IDs.
- Allowlisted structural vocabulary only: unchanged from M001; proposal
  layer adds no new observation source.
- Generated skill is one `SKILL.md`, no scripts/resources/plugins/MCP/
  `allowed-tools` authority: enforced by the restriction layer and drift
  tests; runtime tool permissions remain authoritative (documented, not
  re-proven per sentence).
- `AssetRegistry` parsing/precedence authoritative: single parser seam,
  drift tests prove identical base results; precedence untouched.
- Active turns pinned; no refresh in M002: no refresh call exists, so
  pinning cannot regress.
- No implicit overwrite: no writer exists, so overwrite is impossible;
  collision is advisory-only.

## 6. Failure and recovery review

- Duplicate delivery: request `consumed` flag + `prune_expired` in the same
  locked transaction make double-submit fail (second call errors whether
  the request is found-consumed or already pruned). Tested.
- Cancellation races: no async persistence path; all store mutations are
  synchronous locked read-modify-write.
- Restart: file-backed JSON reloads on each operation; corrupt/oversized
  files fail closed with `InvalidData` rather than partial state. Tested
  (`malformed_persisted_proposal_fails_safely`).
- Partial persistence: temp-file + `sync_all` + atomic rename; crash leaves
  old file or complete new file. File-size bound (`MAX_PROMOTION_FILE_BYTES`
  ≈ 16.25 MiB for 64×256 KiB + overhead) prevents truncation masquerading
  as valid state; fixed during implementation after review found the
  initial 384 KiB bound too tight for large-but-legal stores.
- Stale generation: fingerprint + candidate `revision` equality checked at
  submit; post-request observations bump `revision` and fail closed with
  “start a new promotion request”. Tested.
- Contention: per-namespace `flock` serializes concurrent writers (same
  pattern as M001); capacity caps (`MAX_REQUESTS`/`MAX_PROPOSALS` 64) fail
  closed rather than evicting durable proposals.
- Malformed/unauthorized input: ID parsers reject empty/control/oversized;
  session/project/habit scope triple-checked; oversized markdown rejected
  before parsing; unsupported frontmatter fields rejected. Tested.
- Bounded behavior: diagnostics truncated to 32×512 chars; memory refs to
  16×512 chars; existing names to 32; actions to 32; proposals to 64.

## 7. Migration and compatibility review

- No SQLite migration; file-backed store beside M001 habits.
- Habit record migration: additive `revision` with `#[serde(default = 1)]`
  — old files without the field decode as revision 1 and continue to work.
- No config-schema, protocol, or `AssetRegistry` wire change; TUI additions
  are new commands only; existing `/habits`/`/habit-dismiss` behavior
  unchanged.
- Existing skills and memories require no migration; precedence unchanged.
- Rollback: deleting `skill-promotions/*.json` discards only proposals and
  pending requests; habits, memories, and skills are unaffected.

## 8. Security review

- Authorization: proposal submission requires a live, unexpired, unconsumed
  user request scoped to the exact session + project namespace + habit ID +
  fingerprint/revision. Wrong-scope, replayed, expired, and stale requests
  are denied and tested.
- Caller boundary: `DirectOnly` denies Tool Program (`Program`), subagent
  (`Subagent`), and API callers at the broker layer; only the agent loop
  (`Agent`) and internal paths may submit. Stricter than the plan’s
  “default denied unless explicitly delegated” — no delegation token exists
  in M002.
- Path validation: promotion file namespace must match
  `project/<64hex>`; no caller-supplied path reaches the filesystem;
  proposal `target_scope` is a closed enum (always `Project` in M002), never
  a path. No foreign-harness (`.agents`/`.opencode`/`.claude`) path appears
  in the promotion service.
- Secrets: habit fingerprints are digests; proposal evidence excludes raw
  commands/args/outputs/prompts/env/secrets; host provenance adds no
  session/user data (provenance is habit fingerprint + digest/revision
  only). Memory summaries are bounded and currently empty from the TUI.
- DoS bounds: per-file/per-record caps, request TTL + pruning, single
  proposal per request, no background work, no model-callable approval or
  publish path.
- Audit: proposal records retain promotion request ID, habit ID,
  fingerprint, candidate revision, content digest, status, and bounded
  diagnostics for M003 stale-approval checks.

## 9. Documentation and operations

- `architecture/memory.md`: new “Skill promotion proposals (M002)” section
  + 4 new TUI command rows.
- `architecture/skills.md`: promotion module row + “Proposal boundary
  (M002, pre-publication)” section.
- `architecture/tool.md`: `skill_proposal` row with contract and
  no-write guarantee.
- Operator surfaces: `/skill-promote <habit-id>`, `/skill-proposals`,
  `/skill-proposal <id>`, `/skill-proposal reject <id>`; revision via
  re-`/skill-promote`; rejection never dismisses the habit.
- Static guards: `check-core-boundary.sh`, sandbox, daemon-cwd, scheduler
  bypass, execution ownership, and git forbidden-patterns all pass; the two
  unrelated pre-existing guard findings above are recorded, not introduced.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `check_tool_broker_boundary.py` fails on pre-existing `src/tool/review.rs:216` direct execution | None for M002; guard remains red for unrelated reasons | None in M002; separate owner should route review through the broker |
| low | `check_project_catalog_invariants.py` reports `STORAGE_LAYOUT_VERSION is 48` | None for M002; storage untouched | None in M002; catalog owner to reconcile expectation |
| low | `Draft`/`Superseded` proposal states and `Global` target scope are reserved but unused | No user impact; M003 defines publish/supersede and global-root policy | M003 to own transitions and global refresh semantics |

No critical/high/medium finding remains.

## 11. Roadmap disposition

M002 is strictly closed. The subsystem roadmap moves M002 from `ready` to
`closed`; M003 (`003-approved-skill-publication-and-refresh.md`) is now
unblocked because its sole hard dependency (strict M002 closure) is
satisfied. No corrective pass is required.

## 12. Registry updates

In the same closure commit:

- `plans/implementation/memory-skill-promotion/002-...md`: `ready for
  handoff` → `implemented`.
- `plans/implementation/memory-skill-promotion/003-...md`: `blocked on
  M002` → `ready for handoff`.
- `plans/subsystems/memory-skill-promotion-roadmap.md`: M002 `ready` →
  `closed`; M003 `blocked on M002` → `ready`.
- `plans/registry.md`: subsystem row current milestone M002 ready → M002
  closed / M003 ready; dependency-ready table M002 → M003; newly
  registered order and blocked-work entries updated; M002 added to recently
  closed control points.

Downstream audit: no registered plan other than memory-skill-promotion M003
names M002 as a hard/interface dependency, so only M003 moves to `ready`.
