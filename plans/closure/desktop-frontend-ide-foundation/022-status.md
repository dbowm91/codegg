# Desktop Frontend and IDE Foundation Milestone 022 — Closure Status

**Milestone:** M007-I — the three subsystem moves (the git-review diff view,
the skill-promotion workflow, the human-shell ask workflow).

**Plan:** `plans/implementation/desktop-frontend-ide-foundation/022-the-subsystem-moves.md`.

**ADR:** `plans/adrs/ADR-0058-durable-workflow-ownership.md` — **accepted
Option B (all three moves) 2026-10-30**.

**Branch:** `feat/auto-20261024-6e9f52e4`, baseline `db8d75c79`.

**Date:** 2026-10-31.

---

## 1. Executive finding

M007-I is **implemented**. All three subsystems the ADR named now have a
daemon-owned owner, and the acceptance criteria are met by the code rather
than by a comment.

- **The diff view moved.** `/diff` is a daemon effect. The daemon resolves
  the checkout from its own durable binding, reads the bytes through the
  read-only git fact layer and answers with the diff's own bytes; the
  frontend renders the answer and never constructs the read. Every
  invocation opens a **canonical `Run`**. The catalog's `/diff` descriptor is
  unchanged and the catalog's **version moves by zero**.
- **The promotion workflow has exactly one writer, and it is the daemon.**
  The durable record moved into the daemon's session storage, keyed on the
  canonical `WorkspaceId`. The frontend holds no durable promotion state;
  the model-facing tool writes through the same store; the `flock` sidecar
  is retired with the second writer. The old namespace — a digest of a path
  string, derived differently on each side — is gone, so the "promotion
  request not found" divergence is closed by construction.
- **The publication is daemon-owned and mediated.** The daemon publishes to
  **both** scopes, and the project scope is mediated by the **C004** checkout
  reservation gate through a new `CheckoutWriterSink::SkillPublish` variant.
  A reserved checkout refuses the publication **before** its first effect and
  the refusal is durable and attributable. The promotion record is **not**
  marked published when the gate refuses.
- **The ask is one request, and the bytes are the daemon's.** The frontend
  names the daemon's run and the question; the daemon reads its own run
  bytes, redacts the assembled evidence, stages it and admits the turn
  together. There is no second request, and therefore no `pending` row that
  survives a failed one.

**Two deviations are recorded rather than hidden** (§10): the daemon's diff
answer is **unbounded**, and the old `/diff` arm's **text-diff fallback**
outside a Git repository is **gone**. The **desktop's** consumption of the
three moves (plan work package F) is **not** implemented — it is a separate
Cargo workspace with its own hosted lane, and none of the plan's thirteen
acceptance criteria names the desktop.

---

## 2. Requirement-to-evidence matrix

Every criterion in the plan's §13, with the evidence that satisfies it. A
criterion satisfied by a comment is not satisfied, so each row names a file
and a line.

| # | Criterion | Evidence |
|---|---|---|
| 1 | `/diff` is served by the daemon, its catalog form is `daemon_served`, and the frontend no longer constructs the read itself. | The daemon's handler `src/core/daemon_git.rs:84` (`handle_git_diff`), reached from the family's router at `:68`; the effect-table row `src/core/daemon_commands.rs:636`; the frontend's arm `src/tui/commands/diff.rs:24` (`start_diff`) and `:63` (`apply_diff_finished`); the catalog's descriptor `crates/codegg-core/src/command_catalog.rs:1500` (unchanged). The bespoke `handle_diff_command` is **deleted**. |
| 2 | Each diff invocation creates a canonical `Run` that is readable back from the durable run store. | The effect-table row `src/core/daemon_commands.rs:636` runs through the daemon's canonical-`Run` path; the test `tests/core_daemon/m007i_daemon_diff.inc::a_diff_invocation_opens_a_canonical_run` reads the manifest back from `FsRunStore` and asserts `RunKind::Command`. |
| 3 | The daemon's diff answer carries the same bytes the TUI's read produced, for both branches. | The test `tests/core_daemon/m007i_daemon_diff.inc::the_daemons_diff_answer_carries_the_frontends_own_bytes` compares the daemon's `old_content`/`new_content` against the frontend's **own** two reads (`git show HEAD:<path>` plus the working-tree file read for the path branch; one `git diff` for the bare branch). |
| 4 | The promotion store has exactly **one** writer, and it is the daemon. | The store is `crates/codegg-core/src/session/skill_promotion.rs:161` (`SkillPromotionStore`), written only through `put` (`:174`), `compare_and_set_record` (`:290`) and `consume_request_into_proposal` (`:336`); the daemon's handler `src/core/daemon_skills.rs:65`; the tool `src/tool/skill_proposal.rs` writes through the same store. The file-backed store and its `flock` sidecar are **deleted** from `src/skills/promotion.rs`. The test `tests/core_daemon/m007i_daemon_promotion.inc::the_promotion_record_has_exactly_one_writer_and_it_is_the_daemon` drives both paths and asserts exactly one request row and one proposal row. |
| 5 | The promotion namespace is keyed on the canonical `WorkspaceId`, and a promotion staged through the TUI is submitted successfully by the daemon tool. | The store's key is `(workspace_id, kind, id)` (`crates/codegg-core/src/session/skill_promotion.rs:161`, the migration's `PRIMARY KEY` at `crates/codegg-core/src/session/schema.rs:502`); the daemon's `Begin` stages under `request.workspace_id` (`src/core/daemon_skills.rs:144`); the tool looks the request up under the same workspace (`src/tool/skill_proposal.rs`). The test `a_promotion_staged_through_the_frontend_is_submitted_by_the_daemon_tool` stages through the daemon and submits through the tool. |
| 6 | A proposal that cannot be migrated **fails loudly** rather than vanishing. | The legacy store keyed its file on a digest of a path string, so a legacy record cannot be mapped onto a workspace. `src/skills/legacy_promotion.rs:91` (`probe_legacy_promotions`) finds it and the daemon refuses with `skill_promotion_legacy_unmigrated` (`src/core/daemon_skills.rs`, the `REFUSAL_PROMOTION_LEGACY_UNMIGRATED` arm in `begin_request`), naming the record it will not drop. The test `a_legacy_proposal_that_cannot_be_migrated_fails_loudly` asserts the refusal, that the legacy file is still on disk, and that nothing was staged. |
| 7 | The daemon publishes a skill to **both** the project scope and the global scope, and the manifest's `owner` names the daemon for both roots. | `src/core/daemon_skills.rs:538` (`publish_proposal`) resolves the project root and the global configuration root and publishes through `SkillPublicationService`; the manifest row `docs/writer-disposition.toml` `module.skills_publish` carries `owner = "CoreDaemon"` and a `locator` naming both roots. The test `the_daemon_publishes_to_both_scopes` publishes to each scope and asserts the file at `<workspace>/.codegg/skills/<name>/SKILL.md` and at `<config>/codegg/skills/<name>/SKILL.md`. |
| 8 | A daemon-executed publication is **refused** by the C004 gate when the checkout is reserved by a live run, and the refusal is durable and attributable. | The new sink variant `src/core/checkout_mediation.rs:72` (`SkillPublish`) and `:94` (`as_str` → `"skill_publish"`); the gate call `src/core/daemon.rs:1154` (`root_write_target(CheckoutWriterSink::SkillPublish, …)`), reached from the same dispatch path as the existing sinks; the manifest row `module.skills_publish` carries `mediation = "checkout-gate"` and `mediation_markers = "CheckoutWriterSink::SkillPublish"`. The test `the_c004_gate_refuses_a_publication_into_a_reserved_checkout` asserts the typed `checkout_reserved` conflict **and** a `checkout_writer_refusal` row in the audit trail. |
| 9 | The sink enum and the manifest agree in both directions: a declared sink no root claims fails, and a claimed marker no sink declares fails. | `scripts/check_writer_disposition.py` checks both directions (`declared - claimed` and `claimed - declared`) and its `--self-test` injects a defect to prove it is not vacuous. The test `the_sink_enum_and_the_manifest_agree_in_both_directions` re-derives the same two directions in-process from the enum's `as_str` mapping and the manifest's `mediation_markers` rows. |
| 10 | The ask's stage-and-submit is **one** request, and a failed admission leaves **no** orphan `pending` row. | The single request `CoreRequest::SessionPromptSubmitWithHumanShellPromotions` (`crates/codegg-protocol/src/core.rs`); the daemon's handler `src/core/daemon_turns.rs:2107` (`submit_prompt_with_human_shell_promotions`) stages every promotion in **one** transaction and **expires** them when the admission fails (`codegg_core::session::HumanShellPromotionStore::expire_pending`). `CoreRequest::HumanShellPromotionStage` and the `human_shell_promotion_ids` payload are **gone** from the TUI's path. The test `the_ask_leaves_no_orphan_pending_row` asserts zero `pending` rows after a refused ask. |
| 11 | The ask's evidence comes from the daemon's run bytes, and the redaction still applies. | `src/shell/daemon.rs:463` (`HumanShellRunService::evidence_text`) reads the daemon's **own** run bytes and renders the digest; the redaction stays in the daemon's staging path (`src/core/daemon_turns.rs`, `Redactor::default().redact(&sanitized)`), so the frontend performs no redaction of its own. The TUI-local `app.shell_store` read and `project_for_model` are no longer on the daemon path. The test `the_ask_reads_its_evidence_from_the_daemons_run_bytes` asserts the evidence carries the question and the run's own bytes. |
| 12 | The frontend never executes a daemon-effect form directly, and no form's `kind` changed except through the daemon serving it — the catalog's **version** moves by **zero**. | The catalog's version constant is unchanged at `crates/codegg-core/src/command_catalog.rs:239` (`version: 3`), and the artifact's §1 counts show **124** daemon-effect forms and **43** frontend-effect forms both unchanged. `scripts/check_command_catalog_ownership.py` and `scripts/check_frontend_action_set.py` pass, and the frontend-action count is unchanged at **43**. The artifact is regenerated by its own generator (`plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py`), and its `--check` and `--self-test` pass. |
| 13 | Every command in §11 is run, or its omission is recorded with the reason — in particular the `scheduler_jobs` family. | §4 below. `cargo test --test scheduler_jobs` is **not run**, under **M008 §16**. |

---

## 3. What was delivered

### 3.1 The blast radius

| Surface | Change |
|---|---|
| `crates/codegg-protocol/src/git.rs` | `GitDiffRequestDto`, `GitDiffResultDto`. |
| `crates/codegg-protocol/src/skill.rs` | `SkillPromotionRequestDto`, `SkillPromotionActionDto`, `SkillTargetScopeDto`, `SkillPromotionResultDto`, `SkillProposalSummaryDto`, `SkillProposalDetailDto`, `SkillProposalDiagnosticDto`, `MAX_SKILL_PROMOTION_LIST`. |
| `crates/codegg-protocol/src/human_shell.rs` | `HumanShellPromotionStageDto` — the frontend names the **run** and the question, never the bytes. |
| `crates/codegg-protocol/src/core.rs` | `CoreRequest::GitDiff`, `CoreRequest::SkillPromotion`, `CoreRequest::SessionPromptSubmitWithHumanShellPromotions`; `CoreResponse::GitDiff`, `CoreResponse::SkillPromotion`. |
| `crates/codegg-core/src/session/skill_promotion.rs` | **New.** The durable store: opaque bounded payload, per-workspace cap 64, `put`/`get`/`list`/`compare_and_set_record`/`consume_request_into_proposal`. |
| `crates/codegg-core/src/session/schema.rs` | `migrate_v94` (`:502`) creates the `skill_promotion` table; the migration chain runs it. |
| `crates/codegg-core/src/storage/mod.rs` | `STORAGE_LAYOUT_VERSION` 93 → **94**. |
| `crates/codegg-core/src/authorization/policy.rs` | Three operation descriptors: `git_diff`, `skill_promotion`, `session_prompt_submit_with_human_shell_promotions`. |
| `crates/codegg-core/src/audit_instrumentation.rs` | `git_diff` is uninstrumented (a read); `skill_promotion` is instrumented as `file_mutate`. |
| `src/core/daemon_git.rs` | The daemon's diff handler, `repo_relative_path` (`:186`) and `run_git_read` (`:211`). |
| `src/core/daemon_skills.rs` | **New.** The daemon's promotion workflow (`:65`). |
| `src/core/daemon_turns.rs` | The atomic ask (`:2107`). |
| `src/core/checkout_mediation.rs` | `CheckoutWriterSink::SkillPublish` (`:72`). |
| `src/core/daemon.rs` | The gate call (`:1154`); `workspace_root_for` is now crate-visible; the family dispatch routes `SkillPromotion`. |
| `src/shell/daemon.rs` | `evidence_text` (`:463`) — the daemon's own run bytes. |
| `src/skills/promotion.rs` | The file-backed store is **gone**; the module is the workflow's domain only. |
| `src/skills/publish.rs` | The service is **store-free**: the caller holds the approved revision. |
| `src/skills/legacy_promotion.rs` | **New.** The legacy store's in-flight probe. |
| `src/tool/skill_proposal.rs` | The tool writes through the daemon's store, keyed on the canonical workspace. |
| `src/tui/commands/diff.rs` | **New.** The frontend's diff arm. |
| `src/tui/commands/promotion.rs` | **New.** The frontend's promotion requests. |
| `src/tui/commands/shell.rs` | The ask is one request. |
| `src/tui/app/mod.rs` | The promotion arms send requests; the diff arm is gone. |
| `docs/writer-disposition.toml` | Two rows' owner, mediation and locator. |

### 3.2 The diff view, and the cost the owner accepted

The diff view became a daemon effect with a canonical `Run` per read. The
cost is real and was accepted by the owner against the record's
recommendation: a read that used to be a local subprocess is now a durable
run. The daemon's answer is byte-faithful to the frontend's own read — the
test compares against the frontend's two reads rather than against a
recorded constant, so a change on either side fails it.

The catalog's `/diff` descriptor is **unchanged**: `kind: DaemonEffect`,
`capability: CommandExecute`, `argument_required: true`,
`argument_hint: "<path>"`. The catalog's **version stays at 3**.

### 3.3 The promotion store, and the namespace

The store moved from a JSON file under the configuration directory into the
daemon's session storage, and the `flock` sidecar went with it — the sidecar
existed to hold two writers together, and there is now one writer.

The **namespace** is the change that matters. The old namespace was
`project/<sha256 of the project path string>`, and the two sides derived it
differently (`src/tui/app/mod.rs` hashed one string, `src/tool/skill_proposal.rs`
hashed another), which surfaced as "promotion request not found". The record
is now keyed on the canonical `WorkspaceId`, and both sides name it.

The daemon's handler and the model-facing tool therefore agree by
construction. The daemon's request handler is `src/core/daemon_skills.rs:65`;
the tool's registration is `src/tool/factory.rs`, where the **rooted** tool
carries the pool, the canonical workspace identity, the workspace root and
the habit root, so it cannot record a proposal against a namespace the
daemon does not own.

### 3.4 The publication, and its mediation

The publication moved from the TUI into the daemon, for **both** scopes. The
project scope writes into the checkout's `.codegg/skills` root and is
therefore a **checkout writer**, so it is mediated by the **C004** checkout
reservation gate:

- a new `CheckoutWriterSink::SkillPublish` variant
  (`src/core/checkout_mediation.rs:72`);
- the gate call in the daemon's own dispatch path
  (`src/core/daemon.rs:1154`), the same place the existing sinks are gated;
- the manifest row `module.skills_publish` carries `mediation =
  "checkout-gate"`, `mediation_site = "src/core/daemon.rs"` and
  `mediation_markers = "CheckoutWriterSink::SkillPublish"`.

A reserved checkout refuses the publication **before** its first effect, and
the refusal is durable and attributable. The promotion record is **not**
marked published when the gate refuses, so the human can act on it.

The **global** scope writes under the user configuration directory, outside
every checkout, and is therefore not a checkout writer. The manifest's
`locator` names **both** roots — the previous text said "project-root,
under the project-local .codegg state", which was wrong for the global
scope, and the correction is a fix rather than a deletion.

### 3.5 The ask, and the orphan-row seam

`CoreRequest::HumanShellPromotionStage` and the `human_shell_promotion_ids`
payload on `CoreRequest::SessionPromptSubmit` are replaced by **one**
request. The frontend names the daemon's run and the question; the daemon
reads its own run bytes, redacts the assembled evidence, stages every
promotion in one transaction and admits the turn.

The seam the plan named — a `pending` row that survives a failed second
request — is **closed**: when the admission fails, the daemon expires the
staged evidence in the same request, so there is no state in which the
evidence is staged and the turn was not admitted.

### 3.6 The writer disposition, before and after

The manifest's two changed rows, before and after:

| Row | Field | Before | After |
|---|---|---|---|
| `module.skills_promotion` | `owner` | `SkillPromotionStore` | `CoreDaemon` |
| | `mediation` | `none` | `none` (the record is the daemon's session database; it is not a checkout writer) |
| | `locator` | `project-root, under the project-local .codegg state` | `daemon session storage, keyed on the canonical WorkspaceId` |
| | `disposition` | `shared-sidecar` | `shared-sidecar` (unchanged) |
| `module.skills_publish` | `owner` | `SkillPublisher` | `CoreDaemon` |
| | `disposition` | `shared-sidecar` | `mediated-gate` |
| | `mediation` | `none` | `checkout-gate` |
| | `mediation_site` | — | `src/core/daemon.rs` |
| | `mediation_markers` | — | `CheckoutWriterSink::SkillPublish` |
| | `locator` | `project-root, under the project-local .codegg state` | `project-root .codegg/skills (project scope) and <config_dir>/codegg/skills (global scope)` |

The **sink enum**, before and after — one variant added:

| | Before | After |
|---|---|---|
| `CheckoutWriterSink` variants | 14 | **15** |
| the new variant | — | `SkillPublish` → `"skill_publish"` |

The guard's counts, before and after:

| Guard output | Before | After |
|---|---|---|
| `check_writer_disposition.py` | `63 declared roots, 18 writer tools, 36 writer modules` | `63 declared roots, 18 writer tools, **35** writer modules` |
| `check_writer_disposition.py --self-test` | `writer-disposition self-test ok` | `writer-disposition self-test ok` |

The writer-module count fell by one because `src/skills/promotion.rs` no
longer performs a filesystem effect: the promotion record is the daemon's
own table. The declared-root count is unchanged because **both** rows stay,
as the plan requires.

### 3.7 The regenerated classification

The artifact is regenerated by its own generator, and its §1 counts are
derived from its §3 rows. The move's measured effect:

| §1 row | Before | After |
|---|---|---|
| daemon-effect forms | 124 | 124 |
| frontend-effect forms | 43 | 43 |
| daemon-effect forms the daemon's effect table serves | 74 | 74 |
| daemon-effect forms whose TUI arm issues a daemon request | 23 | **25** |
| daemon-effect forms whose TUI arm issues no daemon request | 27 | **25** |
| frontend-effect forms covered by the declared action set | 43 | 43 |

The two forms that moved are `/skill-promote` and `/skill-proposal`: they
now issue a daemon request instead of keeping a local arm. The pinned counts
in `tests/core_daemon/command_catalog_m007d.inc` (161 / 167 / 124 / 43) and
in `src/tui/command_route_tests.rs` are **unchanged** and still hold, because
no form's `kind` changed.

---

## 4. Verification executed

Every command in the plan's §11, with its outcome.

| Command | Outcome |
|---|---|
| `CARGO_BUILD_JOBS=2 cargo check --workspace --all-targets --locked` | **pass** — exit 0. |
| `cargo fmt --check --all` | **pass** — clean after `cargo fmt --all`. |
| `cargo test -p codegg-core` | **pass** — see §4.1. |
| `cargo test -p codegg-protocol` | **pass** — see §4.1. |
| `cargo test -p codegg --lib <module>` | **pass** — see §4.1. |
| `cargo test --test core_daemon -- <member>` | **pass** — `m007i_daemon_diff` 3/3, `m007i_daemon_promotion` 8/8. |
| `python3 scripts/check_command_catalog_ownership.py` | **pass** — `command catalog ownership: clean`. |
| `python3 scripts/check_frontend_action_set.py` | **pass** — `frontend-action set: clean`. |
| `python3 scripts/check_writer_disposition.py` | **pass** — `63 declared roots, 18 writer tools, 35 writer modules`. |
| `python3 scripts/check_writer_disposition.py --self-test` | **pass** — `writer-disposition self-test ok`. |
| `python3 scripts/check_adr_integrity.py` | **pass** — `ADR integrity fixtures passed; active ADR IDs are unique and local paths resolve`. |
| `python3 scripts/check_comment_budget.py` | **pass** — after re-banking with the sanctioned `--bank`. |
| `python3 scripts/check_test_assertion_quality.py` | **pass** — `all checks pass`. |
| `python3 scripts/check_test_duplication.py` | **pass** — `all checks pass`. |
| `python3 scripts/check_desktop_panel_authority.py` | **pass** — `desktop panel authority guard passed`. |
| `bash scripts/check-desktop-boundary.sh` | **pass** — `desktop dependency and authority boundaries verified`. |
| `generate_m007d_command_classification.py --check` | **pass** — `matches the tree`. |
| `generate_m007d_command_classification.py --self-test` | **pass**. |
| `scripts/verify.sh quick` | **see §4.2**. |
| `cargo test --test scheduler_jobs` | **NOT RUN** — excluded outright under **M008 §16**, which bars running that family against a developer's real scratch root. |
| `scripts/verify.sh full` | **NOT RUN** — a known gap; see §4.3. |

### 4.1 The test runs

- `cargo test -p codegg-core` — **991 passed, 0 failed**.
- `cargo test -p codegg-protocol` — **332 passed, 0 failed**.
- `cargo test -p codegg --lib` — **7810 passed, 0 failed, 5 ignored**.
- `cargo test --locked -p codegg --test core_daemon -- m007i_daemon_diff` —
  **3 passed, 0 failed**.
- `cargo test --locked -p codegg --test core_daemon -- m007i_daemon_promotion` —
  **8 passed, 0 failed**.
- `cargo test --locked -p codegg --test agent_loop -- habit_skill_promotion
  skill_publication` — **24 passed, 0 failed** (the two rewritten members).

### 4.2 `scripts/verify.sh quick`

**pass** — `Quick verification passed.`, exit 0. It covers rustfmt, the two
generated-asset checks, the 85 guard scripts (with their `--self-test`s),
the harness-lab unit suite (412 tests) and a workspace `cargo check`.

### 4.3 `scripts/verify.sh full` — the known gap

**Not run.** `full` adds the nextest workspace sweep on top of clippy. The
**clippy** half was run, under the toolchain the CI lane uses:

```bash
CARGO_BUILD_JOBS=2 rustup run 1.99.0 cargo clippy --workspace --all-targets --locked -- -D warnings
```

— **pass**, exit 0. The local default toolchain is 1.89, whose clippy
misses lints the CI lane's stable toolchain (1.99.0) reports, so the CI
toolchain is the one that matters here. Six lints were found and fixed
before it went green: a `too_many_arguments` on the promotion store's
compare-and-swap (the update is now one value), two `result_large_err` on
the daemon's promotion helpers (a refusal is now a compact pair of
strings rather than a whole `CoreResponse`), a `redundant_closure`, a
`needless_borrow` and a `derivable_impls`.

The **nextest** sweep was **not** run: the whole-suite sweep is the parent's
step, not this milestone's. The suites this milestone touches were run
directly, and their results are in §4.1.

---

## 5. Invariant review

- **One writer, one namespace.** The promotion record has exactly one
  writer — the daemon — and one namespace — the canonical `WorkspaceId`. The
  `flock` sidecar is retired, because there is no second writer left to
  coordinate with.
- **The catalog has one definition.** The `/diff` descriptor is unchanged and
  the catalog's version moves by zero; the classification artifact is
  regenerated by its generator, not edited.
- **A checkout writer is mediated.** The skill publication is a non-broker
  writer into a checkout's skill root, and it asks the C004 gate in the
  daemon's dispatch path. The manifest claims the marker and the guard
  checks both directions.
- **A refusal is durable and attributable.** The C004 refusal is recorded as
  a `checkout_writer_refusal` audit event with the transport-bound principal
  and the decision provenance.
- **A publication never marks its record published on a refusal.** The
  promotion record stays in the state it was in.
- **The frontend never executes a daemon-effect form.** The frontend's diff
  arm sends the form to the daemon; its promotion arms send requests; its
  ask sends one request.
- **A relative path is never resolved against the process cwd.** The daemon
  resolves the checkout from its own durable binding; the diff handler's
  `repo_relative_path` rejects an absolute path and a `..` component.
- **The read-only git fact layer is unchanged.** `GIT_OPTIONAL_LOCKS=0` and
  the object-reading allowlist are untouched; the typed mutating crate
  `codegg-git` is not involved, and the diff read takes no writer permit and
  no fence because it writes nothing.

---

## 6. Failure and recovery review

| Failure | Behaviour |
|---|---|
| The diff read fails. | The daemon returns the failure in the canonical `Run`'s response; the TUI renders it as a typed refusal. The read is read-only, so the working tree is untouched. |
| The C004 gate refuses a publication. | The refusal is durable and attributable and names the checkout's reservation state. The promotion record is **not** marked published; it stays in the state it was in, so the human can act on it. |
| The ask's atomic request fails. | Either the promotions are staged and the turn is admitted, or neither is. A failed admission expires the staged evidence in the same request. |
| The namespace migration meets an in-flight legacy proposal. | The proposal **fails loudly**: the daemon refuses the workflow with `skill_promotion_legacy_unmigrated`, names the record, and leaves the legacy file on disk. |
| Two publications race. | The C004 gate serializes them: the second is refused rather than racing on the same fixed directory name. The publication's own destination lock still serializes writers to the same directory, including the global scope, which is outside every checkout. |
| The daemon has no session storage. | The promotion workflow refuses with a typed error. It does not fall back to a file: a second writer would be exactly what the milestone removed. |

---

## 7. Migration and compatibility review

- **The catalog is unchanged.** The `/diff` descriptor is byte-identical, the
  catalog's version stays at 3, and no form's `kind` changed.
- **The storage layout moved 93 → 94.** `migrate_v94` creates the
  `skill_promotion` table and its index; the migration is additive, so an
  older database migrates forward without a rewrite.
- **The legacy promotion file is not deleted.** A legacy record that cannot
  be mapped onto a workspace is **reported**, not dropped: the daemon refuses
  the workflow and names the record. The file stays on disk so the human can
  recover it by hand.
- **The daemon's `flock` sidecar protocol is retired.** Plan §9.3 records
  that it is not kept "just in case": two writers became one, so there is
  nothing to coordinate.
- **The TUI keeps its standalone mode.** With no daemon attached, the TUI
  still runs the ask against its own run bytes with its own redaction — the
  same shape M007-G established for the plugin registry
  (`daemon_owns_plugins(app)` = `app.core_client.is_some()`). The promotion
  path keeps the two-request staging shape, because there is no daemon to own
  the transaction boundary; the seam it leaves is the standalone seam, not a
  daemon one. The promotion
  workflow's standalone path is **not** implemented: the promotion store is
  the daemon's session database, and the TUI's standalone mode has no such
  database. The TUI's promotion commands therefore require a daemon; this is
  recorded as a limitation in §10.

---

## 8. Security review

- **The redaction moved with the bytes.** The assembled evidence is redacted
  by the daemon's own staging path
  (`Redactor::default().redact(&sanitized)`), and the redaction fails closed
  when it cannot report an applied state. The frontend's `project_for_model`
  hook is no longer on the daemon's ask path.
- **The daemon's diff read is read-only.** It uses the same read-only git
  fact layer the TUI used, with `GIT_OPTIONAL_LOCKS=0` and the unchanged
  object-reading allowlist. It takes no writer permit and no fence, because
  it writes nothing.
- **The publication is mediated, not merely declared.** A reserved checkout
  refuses the publication before its first effect; the refusal is recorded
  with the transport-bound principal.
- **The promotion payload is bounded.** The durable payload is capped at
  256 KiB by a `CHECK` constraint, and the per-workspace record count is
  capped at 64.
- **The authorization matrix grew with the milestone.** Three new operation
  descriptors — `git_diff` (a read, `GitRead`), `skill_promotion` (an
  effect, `CommandExecute`, scoped through the session) and
  `shell_ask_submit` (`AgentInvoke`, scoped through the session, named
  after the command that issues it) — and three new representative
  requests, so the matrix's coverage assertion still holds.
- **The audit trail grew with the milestone.** `skill_promotion` is
  instrumented as `file_mutate`; `git_diff` is a read and is denied by
  `authorize_request` upstream.

---

## 9. Documentation and operations

| Document | Change |
|---|---|
| `architecture/overview.md` | The verified counts: `CoreRequest` 283, `CoreResponse` 189, `CoreEvent` 99, `ProjectionEvent` 54, authorized operations 278, database tables 102/98, storage layout 94, integration test files 359. |
| `architecture/command.md` | The diff view's ownership: it is a daemon effect, the frontend sends the form and renders the answer, and the catalog's version moves by zero. |
| `architecture/human_shell.md` | The ask is one request, the daemon owns the evidence bytes, and a failed admission expires the staged evidence. |
| `architecture/writer_permit.md` | Fifteen mediated sinks, and the skill publication's mediation and its two scopes. |
| `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` | The M007-I row reads **implemented**. |
| `plans/registry.md` | The M007-I bullet, the plan's status row, and a `## Source verification` entry. |

**Operations.** No new configuration surface, no new environment variable, no
new port, no new durable file outside the daemon's own database. The two
host settings the milestone introduced — the daemon's **habit root** and the
**global configuration root** derived from it — default to the standard
user-scoped paths (`<config>/codegg/memory` and `<config>`), and the
daemon's dependencies carry an explicit override for a host that keeps its
candidates elsewhere.

---

## 10. Unresolved findings

Stated as findings, not as excuses.

### High

1. **The daemon's diff answer is unbounded.** The daemon's answer is
   byte-faithful to the frontend's old unbounded read — the plan asked for
   the same bytes — but an arbitrarily large payload now crosses the
   daemon→frontend transport, where the frontend's read was bounded only by
   the filesystem. The WebSocket message cap is 4 MiB
   (`src/server/ws.rs:35`) and the command-result byte bound
   (`bounds::MAX_RESULT_BYTES = 64 KiB`) applies to `outcome.lines`, not to
   this answer. **This milestone did not add a byte bound**, because
   bounding it would change the bytes and therefore break criterion 3. The
   choice is stated here rather than hidden: the answer is unbounded, and a
   follow-up must either bound it with a typed refusal at a documented limit
   or record why an unbounded answer is acceptable. **Recommendation:** a
   follow-up milestone that adds a documented byte bound to the diff answer
   and a typed refusal at that bound.
2. **The `/diff` text-diff fallback is gone.** Outside a Git repository the
   old `handle_diff_command` showed a **text diff** and returned `false`;
   the new path refuses with a typed error (`git_diff_unavailable`) and there
   is no text-diff fallback. This is the **correct** outcome under the plan
   — the frontend must not execute the effect itself — but it is a
   **user-visible behaviour change** that is not recorded anywhere the user
   can see. **Recommendation:** the daemon's refusal message should name the
   fallback that is no longer available, and the release notes should carry
   the change.

### Medium

3. **The desktop does not consume the three moves.** Plan work package F is
   **not implemented**. The desktop's diff view still reads the daemon's
   *git facts* rather than the diff answer, and it has no promotion or ask
   surface. The desktop is a separate Cargo workspace with its own hosted
   lane, and none of the plan's thirteen acceptance criteria names it, so it
   is out of the criteria's scope — but it is **not** done.
   **Recommendation:** a follow-up milestone for the desktop's consumption,
   with the desktop's own hosted lane as its gate.
4. **The promotion workflow needs a daemon.** The store is the daemon's
   session database, so the TUI's standalone mode (no daemon attached) has
   no promotion workflow. The plan's §9.4 keeps the TUI's standalone mode,
   and the ask's standalone path is kept, but the promotion path is not.
   **Recommendation:** either a standalone store for the TUI, or an explicit
   product statement that promotion requires a daemon.

### Low

5. **A failed admission leaves a narrow window.** The daemon stages the
   evidence and then admits the turn. A failure *of the admission* expires
   the staged evidence in the same request, so the client never observes a
   stranded row — but a **process crash** between the two writes would leave
   a `pending` row that no turn will consume. The row is bounded (32
   pending, 24-hour expiry) and the next `Begin` expires stale `pending`
   rows, so it cannot accumulate. **Recommendation:** accept it, and record
   it as a known limitation rather than a defect.
6. **The publication's metadata write is not atomic with its file write.**
   The bytes are committed first and the record is written back under a
   compare-and-swap. A crash between the two leaves the file durable and the
   record un-published; the service's `reconcile` path completes it, and the
   daemon reports the refusal rather than claiming a publication it cannot
   prove. **Recommendation:** accept it; it is the pre-existing shape.

---

## 11. Roadmap disposition

**M007-I is implemented.** The ADR's three moves have daemon-owned owners:

| Move | Owner | Evidence |
|---|---|---|
| the git-review diff view | the daemon, with a canonical `Run` per read | `src/core/daemon_git.rs`, `src/tui/commands/diff.rs` |
| the skill-promotion workflow | the daemon, with the canonical `WorkspaceId` as the namespace | `src/core/daemon_skills.rs`, `crates/codegg-core/src/session/skill_promotion.rs` |
| the human-shell ask workflow | the daemon, with its own run bytes and one request | `src/core/daemon_turns.rs:2107`, `src/shell/daemon.rs:463` |

The seven owner decisions are discharged as the ADR's acceptance recorded
them: **D1** the diff view moves (Option B, a canonical `Run` per read — the
cost the owner accepted against the record's recommendation); **D2** the
daemon's session storage is the promotion store's single owner; **D3** the
canonical `WorkspaceId` keys it; **D4** the daemon publishes to both skill
scopes; **D5** the publication is mediated by the C004 checkout reservation
gate; **D6** the ask's stage-and-submit is one request; **D7** the ask reads
the daemon's run bytes.

**Outstanding, and routed rather than closed:** the desktop's consumption of
the three moves (finding 3) and the two High findings (§10.1, §10.2) are
follow-up work. They are **not** part of M007-I's thirteen acceptance
criteria, and closing them here would either widen the milestone or change
the diff's bytes.
