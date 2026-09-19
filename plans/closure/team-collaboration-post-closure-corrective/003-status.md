# Team Collaboration Post-Closure Corrective M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/team-collaboration-post-closure-corrective/003-verification-guard-convergence.md`

Source subsystem roadmap:

- `plans/subsystems/team-collaboration-post-closure-corrective-addendum.md#m003--verification-guard-convergence-and-canonical-gating`

Repository baseline reviewed: `23ad6937`

Implementation commits or pull requests:

- `23ad6937` — feat(verify): verification guard convergence (team-collaboration post-closure M003)

## 1. Executive finding

M003 is complete. The three stale authority/scheduler/audit static
guards now encode the corrected M001/M002 production semantics, carry
regressions pinning their exact failure modes, and run in canonical
quick verification and routine CI before Cargo-heavy work:

- `check_audit_coverage.py` parses the canonical
  `authorization/policy.rs` descriptor table (not the historical
  `authorization.rs` location), fails closed on an empty inventory,
  and passes with a 216-operation non-empty inventory. 42 drifted
  operations are classified truthfully: 5 instrumented
  (`provider_connection_create` → `provider_select`;
  `work_order_trigger_create`/`revoke` → `work_order_lifecycle`;
  `chat_project_policy_set`/`chat_channel_policy_set` →
  `membership_change`) and 37 explicitly uninstrumented with
  category evidence (chat domain, presence, interactive execution
  surface, principal preferences, listings). No shared-state mutation
  was marked uninstrumented merely to pass.
- `check_scheduler_bypass.py` passes the deliberate
  `snapshot_capture` standalone fallback via an annotation moved
  immediately above the direct send, with the checker tightened to
  same-line-or-one-above plus a `--self-test` fixture proving
  adjacent/same-line pass and missing/distant fail. No file-wide
  waiver was introduced.
- `check_http_route_disposition.py` passes with
  `POST /api/project` as `LocalOwnerOnly` (M001) and
  `every_shared_authz_row_names_a_capability` green unchanged.
- `scripts/verify.sh quick` and the routine CI `verify` job run all
  three guards. M001 registration (10/10), M002 cancellation (4/4),
  and M006 trajectory (13/13) regressions remain green. No
  high/medium audit/authorization/scheduler finding remains. The
  post-closure corrective workstream (M001+M002+M003) is complete.

## 2. Requirement-to-evidence matrix

| Plan requirement (§6) | Evidence | Result | Notes |
|---|---|---|---|
| A. Audit guard reads canonical descriptor module or stable list fn; truthfully classify unclassified ops; regression for move/empty set | `scripts/check_audit_coverage.py`: `AUTHZ_POLICY_MODULE` → `authorization/policy.rs`, `operation_descriptor` presence check, `check_descriptor_source_is_canonical` (known ops + ≥130 breadth), `check_every_operation_is_classified` fails closed with "no daemon operations discovered" | pass | Historical `AUTHZ_MODULE` (`authorization.rs`) removed |
| A. Do not mark mutations uninstrumented merely to pass | 5 privileged mutations instrumented (see §3); `UNINSTRUMENTED_OPERATIONS` docstring enumerates allowed categories and forbids hiding shared-state mutations | pass | Chat policy admin now emits `membership_change`; trigger mint/revoke already emitted post-mutation, table makes guard truthful |
| A. Regression: checker discovers known ops from policy.rs | New `check_descriptor_source_is_canonical` + Rust `operation_matrix_covers_canonical_policy_descriptor_set` (matrix ≥130, spot-checks 8 ops, every matrix op classified) | pass | Fails if table moves or inventory empties |
| B. Standalone `snapshot_capture` exception passes for declared reason; annotation adjacent/structural; fixture for adjacent vs absent vs distant | `src/agent/snapshot_capture.rs`: `// scheduler-audit: standalone-compat` moved inside `tokio::spawn` immediately above `pool.spawner().send`; checker `has_audit_annotation` tightened 24→2 lines (same or one above) | pass | Preferred correction from plan (move annotation, not enlarge window) |
| B. Do not widen to file-wide waiver | Window tightened, not enlarged; `--self-test` proves distant annotation does not bless | pass | Only one `scheduler-audit` site in `src/`; no other call site relied on 24-line window |
| C. After M001: `POST /api/project` not `SharedAuthz + none`; every `SharedAuthz` names capability; exactly one disposition per mounted authenticated route; payload authority green; do not weaken unit invariant | `src/server/authz.rs`: `POST /api/project` → `LocalOwnerOnly`/`none` (M001, untouched here); `check_http_route_disposition.py` 4/4 pass; `server::authz::tests` 5/5 including `every_shared_authz_row_names_a_capability` unchanged | pass | No production route change in M003; reconciliation is confirmation + canonical gating |
| D. Three guards in `verify.sh quick` + routine CI `verify` job, failing early before Cargo-heavy work; resource policy unchanged | `scripts/verify.sh`: three guards after TUI authority, before `cargo check`; `.github/workflows/ci.yml`: three guard steps after TUI authority, before formatting; header comment updated | pass | Commands identical locally and in CI |
| D. Update `AGENTS.md` quick-start + change-triggered section; update `architecture/testing.md` canonical guard set | `AGENTS.md`: quick-start lists http-route-disposition, audit-coverage, scheduler-bypass; routine CI list includes all three. `architecture/testing.md`: quick list 10 steps, CI structure 11 steps, "audit remains local" wording corrected | pass | — |
| Work package A: repair each guard independently; record before/after | Before (baseline `2c34696e`): audit FAIL (`every daemon operation is classified`), scheduler FAIL (`snapshot_capture.rs:181`), route PASS. After (`23ad6937`): all PASS (see §4) | pass | No product semantics touched except truthful audit classification (new mappings emit via existing seams) |
| Work package B: regression fixtures for each failure mode | Audit: canonical-source check + Rust matrix test. Scheduler: `--self-test` (adjacent/same-line pass, missing/distant fail). Route: existing `every_shared_authz_row_names_a_capability` unchanged (rejects `SharedAuthz`/`none`) | pass | — |
| Work package C: wire canonical verification, fail early, identical locally/CI | Guards placed before `cargo check` / formatting in both `verify.sh` and `ci.yml` | pass | `set -euo pipefail` preserves stop-on-first-failure |
| Work package D: rerun M001/M002 regressions, M006 trajectory, authz matrix, canonical quick; new high/medium → separate corrective | M001 10/10, M002 4/4, M006 13/13, authz matrix 5/5, `verify.sh quick` pass (see §4) | pass | Two out-of-scope pre-existing findings recorded in §10; neither is an audit/authorization/scheduler high/medium |

## 3. Production implementation evidence

- `scripts/check_audit_coverage.py`: canonical module constant
  (`authorization/policy.rs`) with `operation_descriptor` presence
  check; `_authz_operations` fails closed with "canonical descriptor
  module missing" / "operation_descriptor not found" / "no
  operations discovered"; new
  `check_descriptor_source_is_canonical` (known M001 ops +
  breadth ≥130); empty-inventory fail message in
  `check_every_operation_is_classified`; check count 5→6. Test-only
  `crate::authorization` references stripped before the
  core-boundary production check so the new Rust regression does not
  trip the guard.
- `crates/codegg-core/src/audit_instrumentation.rs`:
  `INSTRUMENTED_OPERATIONS` +5 (`provider_connection_create` →
  `provider_select`; `work_order_trigger_create`/`revoke` →
  `work_order_lifecycle` with post-mutation skip comment;
  `chat_project_policy_set`/`chat_channel_policy_set` →
  `membership_change` with generic-seam comment);
  `UNINSTRUMENTED_OPERATIONS` +37 grouped with evidence comments
  (trigger reads; M003 principal preferences; setup catalog;
  dashboard enumeration; presence trio; chat-domain 14 ops with
  "only structured actions emit" rationale; interactive-process 10
  ops with deferred-execution rationale); docstring rewritten to
  enumerate allowed categories and forbid hiding shared-state
  mutations. Tests: `operation_mapping_covers_representative_privileged_operations`
  extended with the 5 new mappings;
  `operation_matrix_covers_canonical_policy_descriptor_set` pins
  matrix breadth, 8 spot-checks, and every-matrix-op-classified.
- `crates/codegg-core/src/authorization/policy.rs`:
  `representative_requests` drift repaired
  (`ProviderConnectionCreate` + `ProviderSetupList` with new
  `dummy_provider_connection_create()`); `operation_capability_matrix`
  is complete again (216 descriptor variants = 216 matrix rows).
- `scripts/check-core-boundary.sh`: `auth` alternative narrowed to
  `auth([^a-z]|$)` so core's own `crate::authorization` no longer
  trips the boundary while forbidden root `crate::auth::` is still
  caught (verified both directions). Required by the new Rust
  regression referencing the canonical matrix from the test module.
- `scripts/check_scheduler_bypass.py`: `has_audit_annotation`
  window 24→2 lines with tightened docstring; new `--self-test`
  fixture (`run_self_test`) proving adjacent-above pass, same-line
  pass, missing fail, distant fail.
- `src/agent/snapshot_capture.rs`: explanatory comment kept at the
  fallback head; `// scheduler-audit: standalone-compat` moved
  inside the spawn block immediately above
  `pool.spawner().send(request).await` (comment-only move, no
  behavior change).
- `scripts/verify.sh` + `.github/workflows/ci.yml`: three guards
  wired before Cargo-heavy work with identical commands; CI header
  comment corrected (cheap authority guards are routine; only
  optional feature/plugin/LSP/cross-platform remain local).
- Docs: `AGENTS.md`, `architecture/testing.md` (canonical guard
  set); `architecture/scheduler.md` + `.opencode/skills/scheduler/SKILL.md`
  (annotation adjacency: same line or immediately above; distant
  does not bless); `architecture/audit.md` (canonical descriptor
  source + fail-closed note). `architecture/authorization.md`
  needed no change (`POST /api/project` `LocalOwnerOnly` wording
  from M001 remains accurate). `docs/execution-ownership.toml`
  needed no change (`snapshot_capture.rs` site/owner/entrypoint
  unchanged; `check_execution_ownership.py` passes).
- Deliberately not built: no audit architecture rewrite (no new
  `AuditAction`; chat-domain/interactive gaps documented as
  explicit deferred gaps like `command_execute`/`git_operation`);
  no genuine unaudited mutation hidden as uninstrumented (the 5
  privileged mutations are instrumented); no broad scheduler
  refactor; no new CI jobs/matrices; no `lsp-real-server-tests` or
  `--all-features`; no other change-triggered guard promoted.

## 4. Verification executed

All local truth (no CI run in this environment). Baseline for
before/after is `2c34696e` (M001+M002 closed, M003 ready).

### Direct guard outputs

```bash
python3 scripts/check_http_route_disposition.py --verbose
python3 scripts/check_audit_coverage.py --verbose
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_scheduler_bypass.py --self-test
python3 scripts/check_authorization_matrix.py --verbose
```

Before (baseline `2c34696e`):

- route disposition: 4/4 PASS (M001 already truthful).
- audit coverage: 4/5 PASS, `every daemon operation is classified`
  FAIL (stale `authorization.rs` source inventoried an empty set).
- scheduler bypass: FAIL —
  `src/agent/snapshot_capture.rs:181: forbidden direct call to
  '.spawner().send('` (annotation 24 lines away, outside even the
  old window by one).

After (`23ad6937`):

- `check_http_route_disposition.py --verbose`: 4/4 PASS —
  "All HTTP route disposition invariants verified."
- `check_audit_coverage.py --verbose`: 6/6 PASS (including new
  "descriptor source is canonical policy module") — "All audit
  coverage invariants verified."
- `check_scheduler_bypass.py`: "scheduler-bypass guard ok".
- `check_scheduler_bypass.py --self-test`: "scheduler-bypass
  self-test ok (adjacent/same-line pass, missing/distant fail)".
- `check_authorization_matrix.py --verbose`: 5/5 PASS — "All
  authorization matrix invariants verified."

### Commands run (after, `23ad6937`)

```bash
python3 scripts/check_http_route_disposition.py --verbose
python3 scripts/check_audit_coverage.py --verbose
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_scheduler_bypass.py --self-test
python3 scripts/check_authorization_matrix.py --verbose
cargo test -p codegg --features server --lib server::authz::tests -- --test-threads=1
cargo test --features server --test team_collaboration_postclosure_m001_registration_auth -- --test-threads=1
cargo test --test workspace_postclosure_m002_task_cancellation -- --test-threads=1
cargo test --features server --test team_collaboration_m006_trajectory -- --test-threads=1
cargo test -p codegg-core --lib audit_instrumentation -- --test-threads=1
cargo test -p codegg-core --lib authorization -- --test-threads=1
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked -- --test-threads=1
cargo test -p codegg --locked --features server,plugins,lsp-test-support --lib -- --test-threads=1
git diff --check
```

### Results

- `server::authz::tests`: 5/5 pass, including
  `every_shared_authz_row_names_a_capability` unchanged.
- M001 `team_collaboration_postclosure_m001_registration_auth`:
  10/10 pass.
- M002 `workspace_postclosure_m002_task_cancellation`: 4/4 pass.
- M006 `team_collaboration_m006_trajectory`: 13/13 pass.
- `codegg-core audit_instrumentation`: 9/9 pass (including new
  canonical-matrix test).
- `codegg-core authorization`: 21/21 pass.
- `scripts/verify.sh quick`: pass (fmt, agent schema,
  core-boundary, sandbox, execution-ownership, TUI authority,
  route-disposition, audit-coverage, scheduler-bypass,
  `cargo check` workspace).
- `cargo clippy --workspace --all-targets --locked -- -D warnings`:
  pass.
- `cargo test --workspace --locked -- --test-threads=1`: 112 test
  targets pass; 1 target fails with 2 pre-existing tests (see
  §10 — fails identically on the `2c34696e` baseline without M003
  changes; out of M003 scope).
- `cargo test -p codegg --locked --features
  server,plugins,lsp-test-support --lib -- --test-threads=1`:
  4831 passed, 0 failed. The all-targets variant of this command
  could not link in this environment (disk-full linker failure,
  `errno=28`; see §10). Lib coverage plus the targeted
  `--features server` suites above supply the feature-gated
  evidence.
- `git diff --check`: pass.
- `bash scripts/check-core-boundary.sh`: pass (plus directional
  check: `crate::auth::Foo` still caught, core's own
  `crate::authorization::` allowed).

CI evidence: no CI run in this environment; CI wiring is the
identical three guard invocations added to the routine `verify`
job in `.github/workflows/ci.yml` (see §3). The next CI run on
`main` will execute them.

## 5. Invariant review

- Static guards parse the canonical source of truth: audit guard
  reads `authorization/policy.rs` (`operation_descriptor`); authz
  matrix guard already did; both fail closed with named-module
  messages on move/empty.
- No guard weakened to pass: scheduler window tightened 24→2;
  audit uninstrumented list grew only with categorized evidence and
  a docstring forbidding mutation-hiding; route unit invariant
  untouched.
- Every `SharedAuthz` route names a semantic capability:
  `POST /api/project` is `LocalOwnerOnly`, not `SharedAuthz`;
  unit test unchanged and green.
- Scheduler exceptions explicit and adjacent: single
  `standalone-compat` annotation on the line above the send;
  distant annotations do not bless (fixture-pinned).
- Every canonical daemon operation classified: 216/216 in
  `INSTRUMENTED` + `UNINSTRUMENTED` (plus synthetic
  `provider_connection_use` which is not a `CoreRequest` variant).
- Canonical quick/CI fail on authority-invariant failure: three
  guards run before Cargo-heavy work under `set -euo pipefail` in
  both `verify.sh` and the CI `verify` job.
- Verification stays bounded: no optional external-tool,
  real-server, or `--all-features` test added to routine CI.

## 6. Failure and recovery review

- Guard parser errors fail closed: audit guard returns 1 with
  "canonical descriptor module missing" / "operation_descriptor
  not found" / "no operations discovered" instead of an empty-set
  success; scheduler guard fails closed on unannotated sends;
  route guard fails closed on missing/orphan dispositions.
- `verify.sh`/CI stop on first failing guard via existing
  `set -euo pipefail` semantics; guards are deterministic,
  offline, side-effect free (read-only source scans).
- New audit mappings use existing seams: trigger mint/revoke skip
  pre-emit via `is_work_order_mutation` and emit post-mutation
  with durable ids (no double-emit); `provider_connection_create`
  and chat policy sets emit through the generic pre-side-effect
  seam with structural locators only (no secrets; secret-bearing
  keys rejected by the builder guard).
- New `representative_requests` entries are inert placeholders
  (only the variant matters); matrix/spot-check tests pin
  lockstep.

## 7. Migration and compatibility review

- No runtime migration: no storage change (`STORAGE_LAYOUT_VERSION`
  untouched), no wire/protocol change, no new capability or role.
- Behavioral compatibility changes (intentional, additive audit
  only): `provider_connection_create` and chat policy-set
  mutations now emit structural audit events through existing
  actions (`provider_select`, `membership_change`); trigger
  mint/revoke were already audited post-mutation and only gained
  the truthful table entry. No denial shape, authorization
  decision, or API contract changed.
- Developer/CI compatibility change (intentional per plan §9):
  revisions carrying authority-invariant violations now fail
  quick/CI at the guard step with messages naming the exact guard
  and source (`FAIL: ...`, file:line for scheduler bypass).
  Diagnostics point to the guard script and source location.
- Rollback: reverting `23ad6937` restores the stale guards
  (audit empty-set fail, scheduler false positive) and drops the
  three guards from quick/CI; no data to unwind.

## 8. Security review

- Authority narrowing only: one route disposition already
  `LocalOwnerOnly` (M001, preserved); three `Global/none`-era
  descriptor semantics unchanged; two chat policy mutations
  (`member.manage`) gained audit; no widening anywhere.
- Denial shapes unchanged; no secret material in new audit
  metadata (connection/policy/trigger builders carry ids,
  revisions, and outcome enums only; secret-bearing keys/values
  rejected by `AuditEventBuilder`).
- `provider_connection_create` audit carries session/connection/
  model ids only (same shape as existing connection lifecycle
  mappings); no credential, secret, or endpoint secret enters
  audit.
- Chat policy audit carries member/role/revision via the generic
  membership seam; message bodies, prompts, and decisions never
  enter audit (only structured-action locators, as before).
- Trigger audit unchanged (post-mutation `WorkOrderLifecycle`
  with trigger/work-order ids and state; secret never enters).
- Static guards remain offline/deterministic; bounded-emission
  invariant preserved (new uninstrumented entries are reads,
  caller-scoped infrastructure, chat-domain content,
  high-volume presence, or deferred execution surface — never
  shared-state mutations).

## 9. Documentation and operations

- `AGENTS.md`: quick-start and routine CI guard lists now name
  http-route-disposition, audit-coverage, scheduler-bypass.
- `architecture/testing.md`: quick list 10 steps, CI structure 11
  steps, routine-vs-local wording corrected.
- `architecture/scheduler.md` + `.opencode/skills/scheduler/SKILL.md`:
  annotation adjacency rule (same line or immediately above;
  distant does not bless).
- `architecture/audit.md`: canonical descriptor source
  (`authorization/policy.rs`) + fail-closed + classification
  pinning note.
- `architecture/authorization.md`: no change needed (M001
  `POST /api/project` `LocalOwnerOnly` wording remains accurate).
- `docs/execution-ownership.toml`: no change needed
  (`snapshot_capture.rs` owner/entrypoint unchanged;
  `check_execution_ownership.py` passes).
- Operator diagnostics: new quick/CI guard failures name the
  script and check; scheduler failures name file:line plus the
  required `// scheduler-audit: <reason>` annotation; audit
  failures name the unclassified operation or moved module.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `tests/security_review_receipt.rs`: `security_review_show_command_is_registered` + `security_review_show_without_receipt_warns` fail (2 tests; `/security-review-show` dialog `None`) | TUI security-review-show command does not open its dialog in the default workspace suite | Out of M003 scope (verification tooling only; M003 touches no TUI command registration). Fails identically on the `2c34696e` baseline without M003 changes (verified via stash). Separate corrective/plan required; not an audit/authorization/scheduler finding. |
| low (environment) | `cargo test -p codegg --locked --features server,plugins,lsp-test-support` (all targets) could not link in this environment: `ld: write() failed, errno=28 (No space left on device)` | No code failure; feature all-targets envelope not executed here | Environment limitation (`target/debug` 181G, 2G free). Substituted with `--lib` envelope (4831 passed) plus targeted `--features server` suites (M001 10/10, M006 13/13, authz lib 5/5). Re-run the all-targets envelope in CI or a clean host; no code action required unless CI reproduces. |

No high/medium audit/authorization/scheduler finding remains. No
stop condition triggered: the repaired audit guard revealed only
classifiable drift (instrumented above with evidence), the
scheduler guard exposed only the known annotation-location false
positive (fixed by moving the annotation, no daemon-mode bypass),
route disposition needed no new capability/ADR, and routine CI
remains free of external services.

## 11. Roadmap disposition

Milestone M003 closed. The team-collaboration post-closure
corrective workstream (M001 registration authority boundary, M002
Workspace task-cancellation ownership, M003 verification-guard
convergence) is complete: all three milestones closed with
accepted closure evidence. No new ADR is required (no team
project-creation capability or workspace-ownership model was
invented). No corrective pass is required by this closure; the two
§10 findings are out-of-scope pre-existing/environment items with
their own required actions, not M003 defects.

## 12. Registry updates

- `plans/registry.md` Active subsystem roadmaps: post-closure row
  now reads `M001+M002+M003 closed` with M003 closure record
  `plans/closure/team-collaboration-post-closure-corrective/003-status.md`
  and implementation `23ad6937`.
- `plans/registry.md` execution-gate paragraph: M003 marked closed;
  post-closure follow-up complete.
- `plans/registry.md` Recently closed: M003 row → `closed`,
  closure record
  `plans/closure/team-collaboration-post-closure-corrective/003-status.md`,
  implementation `23ad6937`.
- `plans/subsystems/team-collaboration-post-closure-corrective-addendum.md`:
  Status `active` → `closed`; milestone table M003 → `closed`.
- `plans/implementation/team-collaboration-post-closure-corrective/003-verification-guard-convergence.md`:
  status → implemented with closure pointer.
- Unblock audit: no registered plan becomes dependency-ready on
  this closure. The post-closure M003 hard-dependents are none
  (M003 is terminal in its workstream). Registry Blocked work
  (dependency-security M005 external updater; architecture M009
  strict evidence; runtime-safety C002 Linux evidence) lists no
  post-closure M003 hard/interface dependency and is unaffected.
  No new corrective plan was registered (none required).
