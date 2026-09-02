# Agent Runtime Correctness Milestone 013 — Goal Evidence Provenance and Criterion Corrective Pass

Status: ready

Repository baseline: `4dd1220cf0f297d1e3d6206a1e2b39d2152fd8ce`

Source corrective roadmap:

- `plans/subsystems/agent-runtime-goal-verification-corrective-addendum.md`

Original milestone and closure corrected by this pass:

- M012: `plans/implementation/agent-runtime-correctness-autonomy-simplification/012-host-owned-goal-completion-verification.md`
- M012 closure: `plans/closure/agent-runtime-correctness-autonomy-simplification/012-status.md`

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

Applicable architecture:

- `architecture/goal.md`
- `architecture/jobs.md`
- `architecture/agent.md`

Primary class: corrective invariant / provenance / goal-state correctness

## 1. Objective

Tighten the M012 host-owned goal verifier so deterministic evidence is correlated to the exact active goal rather than merely to post-goal session activity, remove permissive substring-based natural-language criterion inference, and make model test/file claims incapable of borrowing unrelated host evidence.

Preserve the M012 authority boundary: the working model submits a proposal, the host verifier decides whether the goal may transition to `Complete`, and existing continuation/budget/pause/cancel/user-steering semantics remain authoritative. Do not introduce a second scheduler, workflow engine, or mutating verifier.

## 2. Discovered defects and limitations

### 2.1 Evidence correlation is session/time based, not goal based

`src/goal_verification.rs::assemble()` currently queries durable `Test` and `Subagent` jobs by `session_id`, then accepts every matching job whose `created_at >= goal.created_at`.

This is host-owned data, but the relation to the goal is inferred from session identity and creation time rather than explicit goal provenance. A later or unrelated test/subagent job in the same long-lived session can therefore become evidence for the active goal.

The original M012 plan explicitly required host-owned goal correlation and recommended attaching the active goal identifier to job/run metadata when created. The implementation did not complete that part of the contract.

### 2.2 Claimed tests are not matched to specific host evidence

`GoalCompletionProposal.tests_run` is bounded model input. The verifier currently checks whether at least one host-recorded test exists/passed after goal creation, not whether the claimed completion evidence corresponds to a test owned by this goal.

Thus a model can claim test X while unrelated host test Y provides the only passing record. The model still cannot override a failed host test, which is good, but the positive evidence relation is too broad.

### 2.3 Natural-language completion criteria use permissive substring heuristics

The deterministic verifier currently classifies any criterion containing substrings such as `test`, `pass`, or `green` as test-verifiable and criteria containing `todo`, `task`, or `remaining` as todo-verifiable.

This can misclassify semantically unrelated criteria. For example, `Pass security review` can be treated as a test criterion and potentially satisfied by an ordinary passing test job.

A deterministic verifier must only claim to prove criteria whose semantics it actually owns. Unsupported natural-language criteria should remain `AwaitingUser` or otherwise unresolved rather than being guessed from loose keyword matches.

### 2.4 Model file claims are bounded but not host-derived

`files_changed` remains proposal data and is not independently correlated to Git/edit-checkpoint state. The current verifier also does not use it as authoritative positive proof, which avoids a direct safety failure. The corrective pass should make this contract explicit and, where a criterion requires changed-file evidence, derive it from an existing host-owned source rather than model prose.

## 3. Why original verification missed these defects

- M012 tests proved that failed host tests override model prose and that absence of host evidence blocks completion, but fixtures used one obvious goal/test relation and did not include unrelated post-goal jobs in the same session.
- restart evidence reconstruction was tested around durable stores, but not around ambiguous goal ownership inside one long-lived session.
- criterion tests covered clearly unsupported text (`Product owner signs off`) and obvious test-oriented text, not adversarial phrases such as `Pass security review` that collide with the substring heuristic.
- the closure record correctly documented model file/test fields as claims but overstated the completeness of host correlation relative to the original plan.

## 4. Invariants that must not regress

- the working model cannot directly transition an active goal to `Complete`.
- only a host-owned `Met` verdict may call the revision-checked terminal transition.
- failed/in-flight relevant host evidence always overrides positive model claims.
- host evidence must be related to the exact goal through host-written provenance, not prompt text, display names, timestamps alone, or subagent names.
- unsupported natural-language completion criteria are never declared satisfied by heuristic keyword matching.
- verification remains read-only with respect to tools/workspace execution.
- no plugin is required for the core safe completion path.
- pause, cancel, replacement, budget limits, and user steering remain authoritative.
- `NotMet` still returns to the existing bounded continuation controller; no synthetic retry loop is added.
- old jobs lacking explicit goal provenance fail conservatively for goal-specific positive evidence.

## 5. Required production changes

### 5.1 Host-written goal provenance on jobs

At every production creation path for supervised work that may count as goal evidence, attach the active goal identity from host state.

At minimum cover:

- supervised `JobKind::Test` jobs created while a goal is active;
- delegated/subagent jobs intended to count as completion evidence.

Prefer the existing durable bounded `JobRecord.labels: HashMap<String, String>` for this corrective pass if it provides a stable unambiguous relation without schema churn. Use one constant key owned by core/application code, for example:

```text
goal_id = <canonical goal id>
```

Do not let the model provide/override this value through tool arguments. The job creation/submission path must read the currently active goal from host state and write the label itself.

If current job submission architecture makes a host-owned reserved label impossible to protect from user/model-supplied labels, stop and add a typed `goal_id` field with an additive migration rather than creating an ambiguous convention.

Document whether child jobs inherit goal provenance from a parent job or receive it directly from active goal context. There must be one deterministic rule.

### 5.2 Exact-goal evidence assembly

Change `goal_verification::assemble()` to accept the active goal ID and include only evidence explicitly carrying that goal provenance.

Session identity remains a defense-in-depth scope filter, not the relation itself. Creation time may remain a sanity/restart bound but must not substitute for `goal_id`.

Required behavior:

- matching session + matching `goal_id`: eligible evidence;
- matching session but missing/different `goal_id`: not positive evidence for this goal;
- legacy jobs without goal provenance: unavailable/ignored for positive proof, never synthesized into a match;
- a failed matching-goal job blocks completion according to existing semantics;
- unrelated failed jobs from another goal do not poison the active goal.

### 5.3 Test evidence identity

Do not treat arbitrary passed test jobs as satisfying arbitrary model test claims.

At minimum, completion should require a passing test job associated with the active goal whenever the proposal claims tests were run.

Where the supervised test job already persists a normalized command, test selector, invocation key, or equivalent bounded host metadata, expose that as evidence and correlate claims conservatively. Do not parse free-form job display text merely to manufacture a match.

If no stable test-identity metadata exists, use the safer rule:

- host can prove `the active goal has a passing supervised test`;
- host cannot prove `the claimed test name X ran`;
- the model's exact test names remain explanatory only.

Do not block this milestone on a broad job-payload redesign unless a positive criterion explicitly requires test-name identity.

### 5.4 Remove loose natural-language criterion inference

Remove substring rules such as `contains("pass")`, `contains("green")`, `contains("test")`, `contains("todo")`, etc. as evidence of semantic meaning.

The deterministic verifier may only automatically satisfy criteria represented by an explicit typed/canonical deterministic contract.

Acceptable approaches, in order:

1. existing typed criterion metadata, if one already exists and can be consumed without redesign;
2. exact CodeGG-generated canonical criterion forms with explicit prefixes/tags whose semantics are documented and tested;
3. otherwise treat non-empty natural-language `completion_criteria` as semantically unavailable and return `AwaitingUser` after deterministic global gates pass.

Do not add a large parser or regex taxonomy of English phrases. Do not add an LLM verifier in this corrective pass.

The verifier may still apply *global deterministic gates* independent of natural-language criteria, such as:

- relevant goal-owned test/subagent jobs are not failed/in-flight;
- required supervised test evidence exists when the proposal claims tests;
- unfinished todos block completion where current goal policy treats them as global outstanding work.

Be explicit in code/docs about the distinction between global gates and criterion-specific proof.

### 5.5 Host-owned changed-file evidence

Keep `proposal.files_changed` non-authoritative.

If the current goal completion path needs changed-file verification, prefer one existing bounded source:

- edit checkpoints scoped to the same workspace/session/goal/turn lineage where provenance is available after runtime-safety correction; or
- Git/workspace diff state owned by the existing Git service.

Do not create a second file-history store solely for goal verification.

If no exact goal-level changed-file relation can be established cheaply, leave changed-file claims explanatory and document that they cannot contribute positive proof. They must never elevate a verdict from unresolved/failed to `Met`.

## 6. Storage, migration, and compatibility

Preferred implementation uses existing durable job labels and requires no database migration.

If a typed `goal_id` job field becomes necessary, use the smallest additive nullable migration and preserve old rows. Legacy null/missing provenance means unavailable evidence, not inferred ownership.

No public goal protocol change is expected. Existing `GoalCompleted` remains compatible; its meaning continues to be host-accepted completion.

Existing active goals/jobs created before this corrective version may lack provenance. Do not retroactively guess relations after restart.

## 7. Ordered work packages

### WP A — Reserved host goal provenance

Add one canonical helper/constant for writing and reading goal provenance. Wire supervised test and delegated job creation paths.

Acceptance evidence:

- active goal A creates test/subagent jobs labeled for A;
- after replacing/starting goal B in the same session, new jobs are labeled B;
- model/tool arguments cannot spoof the reserved relation;
- jobs outside a goal do not accidentally inherit stale goal identity.

### WP B — Exact-goal evidence assembly

Filter evidence by explicit goal provenance.

Acceptance evidence:

- unrelated passing post-goal session job cannot make the goal `Met`;
- unrelated failed job from another goal cannot block the active goal;
- matching failed/in-progress job still blocks;
- restart reconstruction uses persisted goal provenance and no in-memory cache.

### WP C — Conservative deterministic criteria

Delete permissive substring semantic inference and implement the chosen explicit deterministic criterion contract.

Acceptance evidence:

- `Pass security review` is not satisfied by a passing test job;
- `Product owner signs off` remains `AwaitingUser`;
- exact supported structured/canonical deterministic criteria behave as documented;
- unsupported natural-language criteria never become `Met` automatically.

### WP D — Proposal/evidence contract and closure

Clarify test/file claim semantics and add adversarial integration fixtures.

Acceptance evidence:

- claimed test names cannot borrow unrelated host tests;
- `files_changed` cannot provide positive proof without host-derived evidence;
- `NotMet`/`AwaitingUser` continue using the existing bounded continuation/state paths;
- revision-CAS race tests remain green.

## 8. Failure, cancellation, restart, and contention semantics

- if evidence assembly cannot read the durable provenance store, verification fails closed and does not complete;
- pause/cancel/replacement during evidence assembly still wins through the existing goal revision/status check;
- restart reads goal IDs from durable job provenance; no in-memory map is authoritative;
- missing legacy provenance yields unavailable evidence rather than a guessed relation;
- repeated identical unresolved criteria do not create another autonomous loop;
- job creation races around goal replacement must resolve to the active goal snapshot used by the host submission path, not whichever goal is active later during verification.

## 9. Security and authorization

Goal provenance is metadata, not authority. It must not grant a job additional tools, filesystem access, scheduler priority, or agent authority.

Reserved goal metadata must be written by the host after/beside validation of model-supplied job labels so an untrusted caller cannot claim association with another goal.

Do not log proposal prose, file bodies, or secret-bearing command output merely to establish verification evidence.

## 10. Required tests

Focused tests must include:

- active-goal host label insertion for supervised Test jobs;
- active-goal host label insertion/inheritance for Subagent jobs;
- no active goal -> no stale provenance;
- same session, goal A passing test, goal B active -> A test cannot satisfy B;
- same session, unrelated goal failure cannot poison current goal;
- matching-goal failed/in-progress evidence remains fail-closed;
- restart reconstruction from durable labels;
- model cannot spoof reserved goal label;
- `Pass security review` does not classify as a test criterion;
- unsupported natural-language criteria -> `AwaitingUser`;
- model `files_changed` and exact `tests_run` strings do not independently create positive evidence;
- existing stale revision/pause/cancel completion tests.

## 11. Required verification

```bash
cargo fmt --check --all
cargo test -p codegg-core goal
cargo test goal_request_completion --lib
cargo test goal
cargo clippy -p codegg-core --all-targets -- -D warnings
cargo clippy -p codegg --lib -- -D warnings
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_tool_broker_boundary.py
scripts/verify.sh quick
git diff --check
```

Use exact existing selectors where appropriate. No new CI lane or semantic-model evaluation suite is required.

## 12. Documentation updates

Update as needed:

- `architecture/goal.md` — exact-goal provenance and supported deterministic criterion semantics;
- `architecture/jobs.md` — reserved host goal provenance if labels are used;
- goal tool/prompt wording only if it currently implies exact test/file claims are authoritative;
- corrective roadmap and registry;
- new closure record `plans/closure/agent-runtime-correctness-autonomy-simplification/013-status.md`.

Do not rewrite the M012 closure record. Preserve it as the historical point where direct model-owned completion was removed; M013 owns the later-discovered evidence-quality defects.

## 13. Acceptance criteria

- positive test/subagent evidence is correlated to the exact goal through durable host-written provenance;
- same-session jobs from another goal cannot satisfy or poison the active goal;
- missing legacy goal provenance is treated conservatively;
- permissive substring-based natural-language criterion inference is removed;
- unsupported criteria never become `Met` through guessed semantics;
- model test/file claims remain bounded explanatory data unless backed by exact host-owned evidence;
- direct model completion remains structurally impossible;
- existing continuation, budget, pause, cancel, user-steering, and revision-CAS semantics remain intact;
- focused tests, Clippy, and `scripts/verify.sh quick` pass;
- no LLM verifier, second scheduler, or broad workflow engine is introduced.

## 14. Stop conditions

Stop and report when:

- reserved job labels cannot be made host-owned without ambiguity or spoofing; use a typed field/migration instead;
- exact goal provenance requires changing unrelated scheduler authority or job lifecycle semantics;
- criterion correctness appears to require a general natural-language parser or LLM verifier;
- changed-file proof requires a new history subsystem rather than existing Git/checkpoint evidence;
- implementation starts redesigning goal budgets, todos, worktrees, or run groups outside the discovered defects.

## 15. Closure evidence required

The M013 closure record must include:

- implementation commit(s);
- explicit link to M012 and `plans/closure/agent-runtime-correctness-autonomy-simplification/012-status.md`;
- explanation of the prior session/time correlation and substring-classification gaps;
- evidence provenance schema/key and host-write ownership proof;
- adversarial same-session multi-goal test results;
- supported versus unsupported deterministic criterion matrix;
- restart and stale-goal race outcomes;
- exact verification commands/outcomes;
- unresolved findings by severity and final disposition.
