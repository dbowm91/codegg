# Post-Audit Maintainability and Surface Milestone 002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/post-audit-maintainability-surface/002-model-visible-tool-surface-minimization.md`

Source subsystem roadmap:

- `plans/subsystems/post-audit-maintainability-surface-roadmap.md#7-milestones`

Repository baseline reviewed: `a11614a1` (M001 closed; canonical-name
inventory in `plans/closure/post-audit-maintainability-surface/001-status.md#3-production-implementation-evidence`)

Implementation commits:

- `3f1d63ff` — feat(tools): minimize model-visible tool surface (maintainability M002)

## 1. Executive finding

M002 is complete. One explicit classification source
(`src/tool/disclosure.rs`) now owns prompt disclosure as
Core/Deferred/ProfileSpecific/Hidden, derived from the existing
`defer_loading` / `expose_in_definitions` / profile-policy machinery —
no second registry, router, semantic retrieval, telemetry, or count gate
was introduced. Seventeen specialist, evidence, and overlapping helpers
are deferred from the ordinary immediate set while remaining registered
and discoverable via `tool_search`; the internal `invalid` catch-all is
hidden; plan-mode and curated/minimal palettes are reconciled to the
canonical source; `research` / `security-review` / `verifier` roles
receive small role-appropriate immediate overrides. Discovery is
monotonic and capped with selection metadata; broker/permission/contract
authority is unchanged. M002's hard dependency (M001 canonical names) was
consumed without reopening compatibility. M005's hard dependency on M002
is now satisfied; its interface dependency on M003 remains, so M005 stays
blocked on M003 only.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Single classification source, no second registry/router | `src/tool/disclosure.rs` (`ToolDisclosure`, `disclosure_for`, palettes, `immediate_for_agent`); per-tool `defer_loading()` delegates to `is_deferred_by_default`; registration still only in `ToolRegistry::with_options`; broker/contracts untouched | pass |
| Small canonical core palette for ordinary coding | `CORE_PALETTE` (~30 immediate: read/glob/grep/list/diff/edit/write/apply_patch/bash/test/git/task/question/skill/todos/plan/websearch/webfetch/repo_search/lsp/python? no — python deferred; plus deterministic 8 + context_read) + `CURATED/MINIMAL` subsets; ordinary immediate ~30 vs baseline ~48 | pass |
| Every model-exposed built-in classified core/deferred/profile-specific/hidden | Section 3 table; `disclosure_for` covers all built-ins with unknown-default-Core; unit tests pin representative members of each state | pass |
| Research/evidence/specialists deferred/profile-specific first | 17 deferred: research, research_search, repo_fetch, repo_map, security_search, batch_fetch, evidence_bundle, codesearch (alias), review, image, terminal, skill_proposal, security, replace, commit, python_script, tool_program; plus 5 eggsact deferred retained; `repo_search` canonical stays core | pass |
| Deferred remain searchable with useful metadata | `ToolCatalog::ToolMetadata` now carries `category` + `disclosure`; `tool_search` returns canonical_name/category/risk/disclosure, caps at 10, rejects empty queries, filters hidden even without allow-list | pass |
| Plan-mode and specialist palettes reconciled | `PLAN_ALLOWED` single source consumed by both `filter_tools_for_model` and `ResolvedToolSurface::plan_allowed` (now includes `repo_search` canonical + `tool_search`); `immediate_for_agent` for research/security-review/verifier consumed in deferral partition | pass |
| Contract tests for visibility/discoverability/invocability, not raw counts alone | `tests/tool_surface_minimization.rs` (13 tests) + `disclosure.rs` unit tests (7); no global count constant added | pass |
| Prompt/tool-surface docs updated | `architecture/agent-tool-surface.md` (disclosure states + plan list), `architecture/tool.md` (registration vs advertised vs discoverable vs callable + per-tool disclosure labels), `architecture/permission.md` (discovery never widens authority); agent prompts reviewed, no changes required | pass |
| No capability deleted to reduce count | All deferred tools still registered (`registry.get` + catalog present); `invalid` still registered, only hidden from definitions/discovery | pass |
| No new routing/telemetry/count-gate/Tool Program rework | No new crate, service, embeddings, chaining, telemetry, benchmark, or broker/manifest change; Tool Program contracts unchanged | pass |

## 3. Production implementation evidence

### 3.1 Classification table (ordinary coding; M001 names)

| Tool | Disclosure | Rationale |
|---|---|---|
| `bash`, `read`, `edit`, `write`, `glob`, `grep`, `list`, `diff`, `apply_patch` | Core | Direct edit-loop primitives; kept immediate |
| `task`, `test`, `git` | Core | Controlled delegation/test/Git; kept immediate |
| `question`, `skill`, `todoread`, `todowrite`, `plan_enter`, `plan_exit` | Core | Interaction/task/planning; kept immediate |
| `websearch`, `webfetch`, `repo_search` | Core | Primitive inspect (web + canonical repo); kept immediate |
| `lsp` | Core | Code intelligence; kept immediate (native when available) |
| `text_equal`, `text_diff_explain`, `text_replace_check`, `validate_json`, `validate_toml`, `command_preflight`, `path_normalize`, `text_security_inspect` | Core | Eggsact always-visible pre-edit validators; retained per eggsact design |
| `context_read` | Core (when registered) | Artifact expansion; needed to recover compressed output |
| `research` | Deferred (+research immediate) | Synthesis over primitive search; discoverable; immediate for `research` role |
| `research_search`, `repo_fetch`, `repo_map`, `batch_fetch`, `evidence_bundle` | Deferred (+research immediate; evidence_bundle also verifier immediate) | Evidence/fetch variants; discoverable |
| `security_search` | Deferred (+security-review immediate) | Advisory search; discoverable |
| `security` | Deferred (+security-review immediate) | Deterministic scanning; discoverable |
| `codesearch` | Deferred (+research immediate) | M001 retained alias; canonical `repo_search` advertised instead |
| `review`, `image`, `terminal`, `skill_proposal` | Deferred | Synthesis/media/interactive/specialist submission; discoverable |
| `replace` | Deferred | Regex variant overlaps `edit`/`apply_patch` core pair |
| `commit` | Deferred | Message helper overlaps `git` core |
| `python_script`, `tool_program` | Deferred | Specialist execution/submission next to `bash`/`task` core; contract callability unchanged |
| `text_inspect`, `config_preflight`, `identifier_inspect`, `structured_data_compare`, `text_fingerprint` | Deferred (pre-existing) | Eggsact deferred validators; unchanged |
| `invalid` | Hidden | Internal malformed-call handler; registered, never advertised/discoverable |
| Disabled `lsp`/`security` stubs, task without spawner | Unavailable (not deferred) | `expose_in_definitions=false` / `NonCallable` omission; never advertised as merely deferred |

Unresolved tools intentionally kept core and why: `diff` (read-only compare, no overlap with edit pair); `lsp` (common code intelligence, not specialist-only); deterministic 8 (eggsact always-visible contract with dedicated tests); `commit` deferred rather than removed because `git` covers ops but the helper remains callable; `python_script`/`tool_program` deferred rather than removed because scheduler/broker paths remain intact.

### 3.2 Before/after visible inventory

Baseline (post-M001, pre-M002; `with_defaults`, deferral-capable provider):

- registered (list): ~53 (31 always + 7 eggsearch + 8 eggsact visible + 5 eggsact deferred + todo + lsp/security natives + tool_search + invalid)
- model-facing definitions: all exposed (~53, invalid exposed)
- deferred (`defer_loading=true`): 5 (eggsact deferred only)
- ordinary immediate: ~48

After (`3f1d63ff`):

- registered (list): unchanged (~53; no capability deleted)
- model-facing definitions: ~52 (invalid hidden; all else exposed with flags)
- deferred (`defer_loading=true`): 22 (17 new + 5 eggsact)
- ordinary immediate: ~30
- reduction: ~48 → ~30 immediate (~37% fewer initial choices) with zero unregistrations

Evidence: `ordinary_immediate_set_is_materially_smaller_without_capability_loss`
asserts deferred ≥10, immediate+deferred == definitions, core primitives
immediate, hidden in neither set; `deferred_specialists_remain_registered_but_deferred`
pins each moved tool registered + deferred + catalog-retained.

### 3.3 Discovery-and-invocation proof

`deferred_tool_absent_initially_found_via_search_and_invocable`: `security`
absent from immediate, found via `tool_search` ("security" query returns
`security`/`security_search` with canonical/category/risk/disclosure),
then invoked via `registry.execute_capture("security", classify_command)`
successfully. Denied-tool negative (`discovery_does_not_widen_authority…`),
hidden negative (`hidden_internal_tools…`), secret-leak sweep
(`discovery_output_contains_no_secret…`), cap/empty-query
(`discovery_caps…`), and contract-independence
(`tool_program_callability_follows_contracts_not_disclosure`) all green.

### 3.4 Plan/profile behavior

`plan_mode_remains_read_only_with_discoverable_search`: plan allows
inspection + `tool_search` + both repo names, denies
edit/write/apply_patch/task/commit; resolved surface parity checked.
`specialist_roles_receive_role_appropriate_immediate_palette` pins
research/security-review/verifier overrides and ordinary negatives.
Curated/minimal remain core subsets advertising canonical `repo_search`
(not the alias); model `disabled_tools` filtering still applies on top.
`runtime_unavailable_tools_are_not_advertised_as_merely_deferred` pins
disabled-LSP hidden (not deferred) and task NonCallable omission.

## 4. Verification executed (commands + results; local unless noted)

- `cargo test -p codegg --lib -- tool::` — 534 passed, 0 failed (was 524
  in M001; +7 disclosure unit + catalog/tool_search coverage, no regressions).
- `cargo test -p codegg --lib -- permission:: agent::` — 348 passed
  (includes tool-surface/plan/exposure tests); focused
  `tool_surface/tool_search/catalog/model_profile/permission` selector — 113 passed.
- `cargo test --test tool_registry --test tool_structured_execution --test tool_execution` — 12 + 9 + remainder green (disabled-backend hidden, session-config preserved).
- `cargo test --test eggsact_deterministic_tools` — 25 passed (always-visible stay immediate, deferred stay deferred with flags).
- `cargo test --test search_backend_eggsearch native_wrappers` — green.
- `cargo test --test tool_surface_minimization` — 13 passed (new M002 contract suite).
- `cargo fmt --all -- --check` — clean (after `cargo fmt --all`).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — clean.
- `scripts/verify.sh quick` — passed (generated-agents check, core boundary, sandbox contract, execution ownership, locked workspace check).
- `python3 scripts/generate_builtin_agents.py --check` — passed (via verify quick; no agent TOML changes required).
- Static guards: core-boundary, sandbox-contract, execution-ownership green via verify quick; tool-broker boundary untouched (no broker/contract ownership change); no new guard added per roadmap (behavioral tests preferred).
- No hosted `CI / verify` run: no daemon, scheduler, protocol, or release change; local quick + all-feature Clippy is the proportionate posture per the roadmap (same basis as M001).

## 5. Invariant review

- Registration owned by `with_options`; no second registry/router: disclosure is a pure name→state function plus palette constants, consumed by existing `defer_loading`, exposure filters, and deferral partition.
- Invocation owned by broker/contracts/scheduler: untouched; deferred tools invoke through `registry.get`/`execute_capture`/broker exactly as before.
- Discovery monotonic: `tool_search` allow-list comes from the resolved surface (deny/plan/disable/backend/ceiling already applied); hidden filtered even without allow-list; denied stays undiscoverable (tested).
- Plan mode read-only except permitted planning/state ops: plan list adds only `repo_search` canonical + `tool_search` (both read-only/discovery); no mutating addition.
- Child-agent ceiling intact: surface parent-ceiling logic untouched; role overrides only affect deferral partition, never ceiling/capability computation.
- Tool Program contracts independent of disclosure: `read` stays DirectOrProgrammatic, `research`/`tool_program` stay DirectOnly (tested); manifest resolves via broker catalog, not definitions.
- Runtime-unavailable ≠ deferred: disabled stubs hidden via `expose_in_definitions`; task-without-spawner omitted as NonCallable (tested).
- Specialist palettes broader only where roles require: research gets evidence/synthesis immediate, security-review gets security immediate, verifier gets evidence_bundle immediate; ordinary keeps core.
- No capability deleted: every deferred tool still registered with identical backend/permission/contract behavior; only advertisement changed.

## 6. Failure and recovery review

Metadata-only disclosure change; no new tasks, locks, stores, or recovery paths. Failed search leaves the turn's allowed surface intact (search is read-only over the catalog; `no_results` returns empty tools, no surface mutation). A model that never discovers a deferred tool completes ordinary work with core primitives. No durable discovery state: fresh registries deterministically reproduce classifications (tested). Invocation cancellation owned by broker/scheduler exactly as before.

## 7. Migration and compatibility review

M001 owns literal-name migration; M002 preserves it. `codesearch` alias retained and still delegates to `dispatch_repo_search`; only its advertisement changed (core → deferred) with canonical `repo_search` advertised instead. Explicit profile configs requesting a canonical tool still resolve (tools remain registered; `disabled_tools` filtering and `always_loaded` overrides operate on top; role overrides promote explicitly-allowed specialists). No config syntax change, no stored-transcript rewrite (historical names untouched), no protocol/DTO change (definition payloads carry the same schema with more `defer_loading=true` flags). No permanent compatibility switch invented.

## 8. Security review

- Monotonic authority: discovery reveals only within the accepted policy set; prohibited tools non-callable via discovery (negative tests).
- Hidden/internal (`invalid`, `DisabledTool` stubs) absent from definitions and discovery (tested).
- Plan mode cannot discover/invoke mutating tools outside accepted exceptions (tested).
- Discovery output whitelisted to name/canonical/description/parameters/category/risk/disclosure; secret sweep over research/security/repo/batch/evidence queries finds no api_key/bearer/credential/endpoint leakage (tested).
- Permission/risk contracts unchanged: no tool changed category; `classify_tool_risk` and `tool_category_for_name` untouched.
- Auth/sandbox/destructive-command policy untouched.

## 9. Documentation and operations

- `architecture/agent-tool-surface.md`: disclosure states table (registered/advertised/discoverable/callable), canonical plan list with `repo_search` + `tool_search`, role-override note; ownership row for `disclosure.rs`.
- `architecture/tool.md`: registration vs definitions vs discovery vs invocation table; per-tool disclosure labels; operator-view note (registered capability, not just advertised slice); `tool_search` cap/metadata contract; `invalid` Hidden note.
- `architecture/permission.md`: discovery-never-widens-authority invariant.
- Built-in agent docs/prompts: reviewed `assets/agents/research.toml` / `security-review.toml` / `verifier.toml` + `assets/prompts/agents/research.md` / `security-review.md` / `verifier.md`; no edits required — prompts already describe the role tools our overrides make immediate (research synthesis vs websearch lookups; security tool + lsp securityContext; verifier evidence discipline).
- `AGENTS.md`: no count edit — registered count unchanged; advertised reduction is profile/mode-dependent and documented in `architecture/` rather than a fixed count.
- Operator surface: `/tool-backends` builds from resolved config (registered capability), not the prompt slice; deferred tools remain visible to operators by design (no code change needed).

## 10. Unresolved findings (severity: low)

1. (low) Non-deferral providers (`supports_defer_loading=false`) still receive all definitions immediately. Deferred flags are present but the provider cannot partition. Accepted: existing provider-capability architecture; changing it would require a protocol/transport decision out of scope.
2. (low) `PERMISSION_TYPES`, builtin mode allow-lists, and agent TOMLs still name `codesearch` in places. Intentional: M001 retained the alias; M002 defers (not removes) it. Full literal migration off the alias is future work with config/profile evidence, not alias disposal.
3. (low) Curated (16) / minimal (12) palettes remain model-profile subsets of core rather than the full core set. Intentional: fragile-model entropy reduction predates M002; M002 centralizes the lists without widening them.

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

M002 meets all exit conditions in
`plans/subsystems/post-audit-maintainability-surface-roadmap.md#7-milestones`:
documented core palette exists; overlapping research/evidence/specialist
tools are deferred/profile-specific or intentionally core with rationale;
`tool_search` discovers deferred tools with selection metadata; plan mode,
agent profiles, permission classes, and Tool Program callability remain
correct; no new routing framework added.

Recommendation: closed. M005 hard dependency on M002 is satisfied; M005
remains blocked only on the M003 interface dependency (agent construction
seams). M003/M004 are unaffected (ready, independent/soft).

## 12. Registry updates

- `plans/registry.md`: M002 moved to closed with this closure record;
  subsystem row updated to `M001 closed, M002 closed`; M005 blocked-work
  entry updated (hard dependency on M002 satisfied; interface on M003
  remains); dependency-ready table retains M003/M004.
- `plans/subsystems/post-audit-maintainability-surface-roadmap.md`:
  M002 → closed with closure link; M005 blocker updated to interface on
  M003 only.
- `plans/implementation/post-audit-maintainability-surface/002-model-visible-tool-surface-minimization.md`:
  status → closed.
- `plans/implementation/post-audit-maintainability-surface/005-runtime-service-context-global-state-cleanup.md`:
  status remains blocked; hard-dependency note updated (M002 closed).
