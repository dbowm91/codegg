# Execution Reliability, Approval, and Autonomy M007 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/execution-reliability-approval-autonomy/007-yolo-auto-and-full-host-user-surfaces.md`

Source subsystem roadmap:

- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md#7-milestones`

Repository baseline reviewed: `36f0f48d`

Implementation commits or pull requests:

- `36f0f48d` — execution-reliability M007: yolo automatic and full host user surfaces

## 1. Executive finding

M007 is complete. The M003–M006 approval/sandbox architecture is now
exposed as a low-friction, frontend-neutral user feature: `Interactive`
/ `Automatic` / `Yolo` approval modes and `ReadOnly` / `WorkspaceWrite`
/ `FullHost` sandbox profiles are selectable through one daemon-owned
contract (`RuntimePolicySet` + `ApprovalModeSet`/`SandboxProfileSet`
with CAS, `ApprovalPreferenceGet`/`ExecutionPolicyGet` for reads),
through TUI `/approval` / `/sandbox` / `/policy` commands with
effective-state status rendering, and through CLI/headless flags
(`--approval-mode`, `--sandbox`, `--yolo`). `Yolo`+`WorkspaceWrite` is a
practical autonomous mode (prompts skipped, containment enforced);
`FullHost` is explicit and strongly warned (one confirmation, two for
`Yolo`+`FullHost`, revision-bound, never a permanent bypass token);
`Automatic` without a configured reviewer renders as visibly degraded
and defers to the human, never silent Yolo. Remembered approvals are
capability-scoped (`cmd:<argv0>`, `git:<subcommand>`) where
deterministic command data exists; legacy broad rows stay readable and
still match. No per-action re-prompting after selection. No storage
migration; Interactive behavior unchanged.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Frontend-neutral get/set/effective policy WP-A | `CoreRequest::RuntimePolicySet { approval_mode?, sandbox_profile?, expected_revision? }` (additive, either dimension optional, fail-closed validation, single-revision atomic write via `RuntimePreferenceStore::set_policy`); `ExecutionPolicySnapshotDto.reviewer_available/reviewer_detail` (daemon-resolved from `[approval_reviewer].model`); `RuntimePreferenceDto.last_provider_connection_id/last_model_id` (M004 identity now projected as daemon state); authorization matrix + representative request classified (`runtime_policy_set`, Global, none) | pass | Separate single-dimension sets retained; names follow protocol conventions per plan §6 allowance |
| Headless flags map to the same contract WP-A | Top-level `--approval-mode/--sandbox/--yolo` + `exec` equivalents resolved by `policy_surface::resolve_cli_policy` (`--yolo` is exactly `--approval-mode yolo`; conflicts rejected); TUI launch seeds the daemon preference once via `RuntimePolicySet` (FullHost strongly warned on stderr); `exec` applies the override to the loop snapshot; `--run` single-shot applies top-level flags; bare `exec` keeps legacy permissive behavior with a documented compatibility-alias note on `with_exec_mode` | pass | Explicit headless FullHost proceeds (the flag is the confirmation); interactive TUI FullHost still confirms |
| TUI selector/status/warnings WP-B | `/approval`, `/sandbox` (+ `-mode`/`-profile` aliases), `/policy` registry entries (139→142, count test + docs updated); `src/tui/commands/policy.rs` async start/apply flow (stale completions dropped, failed updates keep the cached policy); `warning_for` 3×3 matrix (Info/Caution/Strong, 0/1/2 confirmations); status-bar policy segment + `/status` line showing daemon-resolved effective state; two-step chained `ConfirmDialog` for `Yolo`+`FullHost`; cancel changes nothing; CAS conflict surfaces reload-and-retry | pass | Observer mode blocks the new commands fail-closed (not allowlisted); observers still read policy via `/status` |
| Automatic degraded transparency WP-B | `reviewer_available=false` renders `(reviewer unavailable: deferring to human)` in the one-line status and a `never silent Yolo` detail block in `/policy`; M006 defer/fallback behavior unchanged | pass | See §4 degraded tests |
| Preference restore UX WP-C | Startup `PolicySnapshotRequested(StartupRestore)` enqueued in `launch_tui`; one-time restore toast (`format_restore_summary`: restored mode/profile/revision + enforcement + ceiling-narrowing fallback + last-model daemon identity vs absent); TUI manifest hint stays display-only (`reconcile_tab_model_with_daemon`, pre-existing + tested) | pass | Per-session model-application outcomes are not yet projected (see §10) |
| Capability-scoped remembered approvals WP-D | `PersistentDecision.scope` (serde-defaulted, legacy JSON readable); `decision_scope_for_shell_command` (`cmd:<argv0>`, env/sudo-tolerant, bounded, no shell parser per stop conditions) and `decision_scope_for_git_subcommand` (`git:<sub>`); exact-scope-first then broad-fallback lookup in `check_with_args`; scope in HMAC material with legacy-signature fallback for unscoped rows only; `persist_always_choice` derives the scope from the tool call | pass | Same-argv0 coarseness documented (see §10); no silent broadening (see §8) |
| Docs/help WP | `architecture/permission.md` M007 item, `architecture/security.md` FullHost/scope note, `architecture/command.md` + `overview.md` 139→142, README approval/sandbox + exec-policy sections, clap help text, registry descriptions (palette discovery), `scripts/check_policy_surface.py` | pass | Help dialog is keybinding help; slash docs come from the registry |

## 3. Production implementation evidence

Landed ownership and behavior:

```text
Presentation (src/policy_surface.rs, NEW, frontend-neutral)
  WarningLevel Info/Caution/Strong
  warning_for(mode, profile): 3x3 matrix, confirmations 0/1/2
  EffectivePolicyView { requested vs effective, enforcement summary,
    reviewer_available/detail, revision } + is_degraded
  format_policy_line / format_policy_detail / format_restore_summary
  CliPolicyOverride + resolve_cli_policy + parse_* (fail-closed)

Core (crates/codegg-core/src/approval.rs)
  RuntimePreferenceStore::set_policy(mode?, profile?, expected_revision?)
    single-revision atomic write, CAS conflict, empty-update Validation

Protocol (crates/codegg-protocol/src/core.rs)
  CoreRequest::RuntimePolicySet { approval_mode?, sandbox_profile?,
    expected_revision? }
  ExecutionPolicySnapshotDto += reviewer_available (default false)
    + reviewer_detail (default "")
  RuntimePreferenceDto += last_provider_connection_id/last_model_id
    (default None)

Daemon (src/core/daemon_ops.rs + daemon_family.rs + authorization/policy.rs)
  RuntimePolicySet arm: validate-all-first, atomic set_policy,
    preference_conflict / invalid_* / empty_policy_update codes
  ExecutionPolicyGet projects reviewer availability (config-owned) and
    a stable reviewer_config_id when configured
  Ops family + authorization matrix classification (Global, none)

Permission recall (src/permission/mod.rs + src/agent/tool_batch.rs)
  PersistentDecision.scope (additive); scoped sign/verify with legacy
    fallback for unscoped rows only
  get_decision_scoped / add_decision_scoped / always_*_scoped
  check_with_args derives cmd:/git: scope; persist seam derives it
    from the tool call

CLI (src/main.rs + src/exec.rs)
  --approval-mode/--sandbox/--yolo (top level + exec); with_policy on
    ExecMode; run_single_shot + launch_tui application

TUI (src/tui/commands/policy.rs NEW + registry + status bar + dispatch)
  PolicyUiState cache (daemon DTOs only) + PolicySnapshotReason
    {StartupRestore, SelectorRefresh, AfterUpdate}
  PendingPolicyConfirm bound to expected_revision; chained confirms
  PolicySnapshotLoaded / PolicyUpdateFinished completions (stale dropped)
```

Approval × sandbox behavior matrix (router + harness):

```text
Allow x {Interactive,Automatic,Yolo} → Allow (mode-independent, M003 unregressed)
Deny x {Interactive,Automatic,Yolo} → Deny (new Yolo-deny harness test)
Escalate x Interactive → human wait (unchanged)
Escalate x Yolo → Allow(yolo) within ceiling (new harness test; snapshot keeps WorkspaceWrite)
Escalate x Automatic + reviewer verdict → Allow/Deny/Defer (M006 green)
Escalate x Automatic, no reviewer → human fallback (M006 migration test green)
Pending x concurrent mode toggle → captured snapshot wins (new router contention test)
Yolo + FullHost selection → two explicit confirmations, then no per-action prompts
```

## 4. Verification executed

### Commands run (local, this environment)

```bash
cargo test --test permission
cargo test --test policy_surface_m007
cargo test --test approval_router
cargo test --test agent_loop_harness
cargo test --test agent_loop_harness -- approval
cargo test --test session_selection
cargo test --test tui_project_picker --test tui_project_routing --test tui_project_tabs --test tui_render
cargo test --test sandbox_policy_wiring --test approval_reviewer --test sandbox_landlock
cargo test -p codegg --lib permission::
cargo test -p codegg --lib policy_surface
cargo test -p codegg-core --lib
cargo test -p codegg-protocol
python3 scripts/check_sandbox_contract.py
python3 scripts/check_policy_surface.py
python3 scripts/check_approval_router.py
python3 scripts/check_approval_reviewer.py
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo check -p codegg --features server,plugins,lsp-test-support --all-targets --locked
./scripts/verify.sh quick
./scripts/verify.sh full
```

Plan §11 names `python3 scripts/check_core_boundary.py` and
`cargo test --test tui_project_sessions`. The canonical guard in this
repo is `bash scripts/check-core-boundary.sh` (same invariant,
executed; same deviation as M006 §4), and no `tui_project_sessions`
target exists: the four existing TUI project/render suites above were
run instead (all green).

### Results

- `cargo test --test permission`: 44/44 pass (store/checker intact; new struct literals carry `scope: None`).
- `cargo test --test policy_surface_m007` (new): 14/14 pass — legacy snapshot/preference/policy-set JSON evolution, 9-combo warning bounds, Automatic-never-implies-FullHost, requested-vs-effective rendering, degraded-Automatic rendering, restore fallbacks, CLI alias/conflict mapping, file-backed scoped round-trip, legacy permission-file readability, cargo-scope precision, git-scope precision.
- `cargo test --test approval_router`: 17/17 pass (16 M003 + new pending-captured-snapshot contention test; DTO literal extended with M007 fields).
- `cargo test --test agent_loop_harness`: 52/52 pass — includes 2 new M007 loop tests (Yolo+WorkspaceWrite executes with no human `PermissionPending` while the snapshot keeps `WorkspaceWrite`; deterministic Deny stays denied even as Yolo+FullHost).
- `cargo test --test agent_loop_harness -- approval`: 4/4 pass (2 new Yolo + 2 M006 reviewer).
- `cargo test --test session_selection`: 21/21 pass (M004 restore unregressed).
- TUI suites: picker 22/22, routing 27/27, tabs 20/20, render 99/99 pass (registry 139→142, status-bar policy segment render test).
- `sandbox_policy_wiring` / `approval_reviewer` / `sandbox_landlock`: pass (M005/M006 contracts intact; reviewer still defers without config).
- `cargo test -p codegg --lib permission::`: 57/57 pass (scope normalization/bounds, narrow-without-widen, broad-legacy fallback, cross-family deny isolation, legacy-signature verification incl. anti-tamper, legacy JSON readability).
- `cargo test -p codegg --lib policy_surface`: 7/7 pass (matrix levels/confirmations, Yolo+WW copy, FullHost copy, strongest second confirmation, rendering, restore, CLI).
- `cargo test -p codegg-core --lib`: 667/667 pass (incl. new `set_policy` atomic/CAS/validation test and authorization-matrix coverage of `RuntimePolicySet`).
- `cargo test -p codegg-protocol`: 177/177 pass.
- Guards: `check_sandbox_contract.py` pass; `check_policy_surface.py` (new) pass; `check_approval_router.py` pass; `check_approval_reviewer.py` pass; `check-core-boundary.sh` pass.
- `cargo fmt --all -- --check`: pass; `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass; feature build (`server,plugins,lsp-test-support`) check: pass.
- `scripts/verify.sh quick`: pass. `scripts/verify.sh full`: pass (423 `test result: ok` lines, zero failures; workspace + feature suites).

Scoped-capability proof: allowing `cargo test` (`cmd:cargo`) leaves
`rm -rf /` escalating (`Ask`) at both unit and integration level;
`git:commit` allow does not authorize `git:push`; `git:push` deny does
not bleed into `commit`; legacy broad files still authorize every
scope; stripping a scope invalidates the signature while pre-M007
signed unscoped rows still verify.

Warning/confirmation evidence: the 3×3 matrix is asserted in unit
(`warning_matrix_levels_and_confirmations`) and integration
(`every_combination_has_a_warning_with_bounded_confirmations`) tests;
Yolo+WW copy asserts prompts-skipped + sandbox-enforced with no
containment-loss language; FullHost copy asserts OS-user authority +
unrestricted network for all three modes; Yolo+FullHost asserts the
strongest title plus `confirmations_required == 2`. Deterministic
render fixtures stand in for screenshots (status-bar render test,
policy line/detail/restore unit tests); no interactive screenshot was
captured headless.

## 5. Invariant review

| Plan invariant (§4) | Evidence |
|---|---|
| UI displays daemon-resolved effective state, never the request | `PolicyUiState` builds only from `ApprovalPreference`/`ExecutionPolicy` DTOs; failed updates keep the old cache; guard bans `ExecutionPolicySnapshot::capture`/`RuntimePreferenceStore` in TUI |
| Automatic/Yolo never imply FullHost | Orthogonal dimensions end to end (store, protocol, CLI, selector); matrix test asserts Automatic+constrained is never Strong and never mentions containment loss; harness asserts Yolo snapshot keeps `WorkspaceWrite` |
| FullHost never activates by silent fallback | Unavailable constrained enforcement stays `Unavailable` (M005 intact); `set_policy`/CLI parse fail closed on unknown names; CLI FullHost warns explicitly; TUI FullHost confirms |
| Deny/project/admin/parent ceilings enforceable and visible | Deny precedes routing in all modes (Yolo-deny test); ceilings narrow via `resolve_effective_mode`/`narrow_for_child` (M003 intact); narrowing renders `(narrowed)` + ceiling note |
| Clear warnings without per-command confirmation fatigue | Confirmations collected only at selection (`confirmations_required` 0/1/2); nothing re-prompts per action (structural: warnings live in the selection path only) |
| Preference persistence is daemon-owned and frontend-neutral | `set_policy` CAS store; `RuntimePolicySet` principal-from-transport; TUI/CLI carry no identity; manifest hint display-only |
| Mode changes do not retroactively alter pending/in-flight authorization | Batch snapshot (M003) + new router contention test (pending Interactive wait answers human despite concurrent Yolo snapshot) |
| No frontend stores credentials or becomes security truth | Preference DTOs carry mode/profile/model identity only (secret-free test intact); guard bans authority imports in `policy_surface.rs` |

## 6. Failure and recovery review

- Failed preference update leaves previous effective policy and reports error: `RuntimePolicySet` validates all names before writing; `apply_policy_updated` keeps the old cache on error and toasts the code (conflict gets reload-and-retry hint).
- Two frontends update via revision/CAS and stale update is rejected/reloaded: `set_policy` single-check CAS + `preference_conflict`; pending confirms bind `expected_revision`; core CAS test + conflict-hint test.
- Restart applies stored preference but recomputes sandbox/reviewer availability and ceilings: pool-backed store (M003 restart test green); enforcement and reviewer availability are computed per `ExecutionPolicyGet`, never persisted; startup toast reports narrowing.
- FullHost confirmation is tied to the requested revision/change: `PendingPolicyConfirm.expected_revision`; no permanent bypass token exists anywhere (guard-pinned: no new bypass path in TUI).
- Cancelling a mode-change dialog changes nothing: cancel branch clears pending + toasts `unchanged` (both confirm and dialog-close paths).
- Pending permission uses its captured old snapshot: batch snapshot (M003) + new contention test.

## 7. Migration and compatibility review

- Existing config/default remains Interactive unless documented otherwise: no config default changed; `Interactive`/`WorkspaceWrite` defaults untouched in every `unwrap_or_default` path.
- `with_exec_mode()`/legacy CLI flags map with deprecation/help text: `with_exec_mode` documented as the legacy permissive compatibility alias; bare `exec` preserves it; explicit flags select the new contract; no legacy CLI flag existed to alias beyond exec behavior.
- TUI manifest remains display-only: `reconcile_tab_model_with_daemon` overwrites the hint (pre-existing tests green); restore toast labels the model as daemon state.
- Legacy permission decisions load conservatively: serde-defaulted `scope: None`; legacy JSON readability test; legacy-signature verification test; broad rows still match scoped requests (no silent broadening, no silent dropping).
- Existing sessions keep explicit model selection: M004 precedence untouched (`session_selection` 21/21); preference is a default for unselected sessions only.
- Protocol evolution is additive: new DTO fields are `#[serde(default)]`; legacy snapshot/preference/policy-set JSON decode tests; authorization matrix extended, not reshaped.

## 8. Security review

- Yolo/Automatic cannot disable containment: mode/profile orthogonality asserted from store to status bar; enforcement reported separately with network always `Unrestricted` for shell.
- Scoped-grant tamper resistance: scope is HMAC-signed; dropping it invalidates the signature (unit-tested); scoped rows never accept legacy-shaped signatures; legacy fallback is unscoped-rows-only.
- No recursive approval or second human-wait path: router guard still passes; policy flow issues only `RuntimePolicySet`/`Get` requests.
- No secrets in new types: preference/policy DTOs carry identifiers + mode/profile + revision only; reviewer model id is not projected (availability boolean + bounded reason instead).
- FullHost explicitness: CLI flag warns on stderr; TUI confirms once (twice for Yolo+FullHost); status line marks `(no containment)`; audit distinguishes requested/effective via revision.
- Child authority unchanged: `narrow_for_child`/`resolve_child_sandbox` unregressed (core + sandbox suites green).

## 9. Documentation and operations

Updated:

- `architecture/permission.md` — M007 item (contract, TUI/CLI surfaces, scoped recall, guard).
- `architecture/security.md` — FullHost warning semantics + scope-signing note.
- `architecture/command.md` + `architecture/overview.md` — 139→142 with new-command rows.
- `README.md` — approval/sandbox workflow section + exec-policy note.
- CLI help: `--approval-mode`/`--sandbox`/`--yolo` help text (top level + `exec`); TUI discovery via registry descriptions.
- Guard: `scripts/check_policy_surface.py` (focused ownership lint; not a new CI lane, same standing as the M003/M006 guards).

Operator notes: configure `[approval_reviewer] model` to make
`Automatic` autonomous (bare id for the primary provider, or
`provider/model`); otherwise `Automatic` visibly defers to the human.
Watch `Runtime policy updated:` (info with effective line + detail)
vs `preference_conflict` (warn with reload-and-retry) vs
`reviewer unavailable` markers. `headless_deny = true` remains for
genuinely noninteractive operation only.

No new CI lane: the guard is a focused ownership lint run locally
(plan §6 allowance allows UX/fallback configuration surface; §11
commands plus the guard constitute the verification).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Low | Per-session model-application outcomes (`UnavailableConnection`/`UnknownModel`/`StaleCatalog`) are daemon-internal; the restore toast shows the stored last-model identity (or its absence), not per-session application diagnostics | A stale remembered model surfaces as "session left unselected" in-session rather than in the restore toast | M008 qualification may project the application outcome code; no M007 scope change |
| Low | No project/admin ceiling source is currently configured, so requested == effective in production | The narrowing path is implemented, rendered, and tested but unexercised live | M008 fault matrix may inject a synthetic ceiling; no correctness impact for M007 |
| Low | Scoped decisions share an argv0 family (`cargo test` allow covers `cargo publish`) | Deliberate conservative tradeoff; strictly narrower than legacy broad grants | A finer command grammar is out of scope per the stop conditions; no action |
| Low | No interactive TUI screenshot captured (headless environment) | Visual copy reviewed as text, not pixels | Deterministic render fixtures (status-bar + line/detail/restore tests) stand in; M008 may attach captures from a terminal run |
| — | No other open items | — | — |

No stop condition triggered (no daemon-policy bypass, no
Automatic→Yolo silent mapping, no weakened FullHost confirmation, no
shell-language parser was attempted).

## 11. Roadmap disposition

Milestone closed with one downstream unblock:

- M007 (Yolo/Automatic/FullHost user surfaces): hard dependencies were M003+M004+M005+M006. All closed and consumed as designed. **Close.**
- M008 (fault-injection and reliability qualification): hard dependency was M007 (M001+M002+M003+M004+M005+M006 closed). **Unblock to `ready`.**
- No corrective pass required; no deferred product work registered.

## 12. Registry updates

- `plans/registry.md`: subsystem row `M001+M002+M003+M004+M005+M006 closed; M007 ready` → `M001+M002+M003+M004+M005+M006+M007 closed; M008 ready`; dependency-ready table M007 row `ready` → `closed` with closure link and implementation `36f0f48d`, M008 row added as `ready`; execution-order item 2 rewritten (M007 closed, M008 ready); M007 appended to recently-closed work; blocked-work M008 row removed (no longer blocked).
- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md`: M007 section `ready` → `closed` with closure link; M008 `blocked` → `ready`; status table updated.
- `plans/implementation/execution-reliability-approval-autonomy/007-yolo-auto-and-full-host-user-surfaces.md`: `Status: ready for handoff` → `Status: implemented`.
- `plans/implementation/execution-reliability-approval-autonomy/008-fault-injection-and-reliability-qualification.md`: `Status: blocked` → `Status: ready for handoff`.
