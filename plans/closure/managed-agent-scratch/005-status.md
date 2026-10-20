# Managed Agent Scratch Milestone M005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/managed-agent-scratch/005-temp-root-redirect-offer-before-human-approval.md`

Source subsystem roadmap:

- `plans/subsystems/managed-agent-scratch-roadmap.md#7-milestones`

Corrective predecessor:

- `plans/implementation/managed-agent-scratch/003-permission-guidance-and-lifecycle-qualification.md`
- `plans/closure/managed-agent-scratch/003-status.md`

Repository baseline reviewed: `fe2c1306` on `main`

Implementation commits:

- `39e1445b` — `feat(permission): answer the model with a temp-root redirect
  before the human`
- `d806d6d0` — `test(agent-loop): cover the temp-root redirect offer end to end`
- `141f7114` — `docs(permission,plans): record the temp-root redirect offer
  contract and close M005`
- `d861e1f6` — `docs(plans): record the M005 hosted CI evidence and the
  milestone status`
- `e52695b9` — merge commit for pull request #134

## 1. Executive finding

The production change is complete and the merged-tree behavior is verified.
M003 shipped an advisory recovery hint for a structured write of a recognized
host temporary root, but the hint lives *inside* the write tool, so it is only
reachable once the permission layer has already said Allow. Under the default
policy a write outside every authorized root is the implicit `Ask`, so the
human was asked first and the model never received the suggestion at all.

M005 answers the **model** first in exactly that case. When the pending
decision is the implicit default `Ask` and the target is a structured `write`
of a recognized host temp root outside every authorized root, the agent loop
refuses the call with the same actionable message — naming the caller's own
private scratch root — and the turn continues. A model that retries at the
suggested path succeeds with no human involved. A model that repeats the same
restricted path is not offered again and reaches the unchanged human approval
path.

No authority moved. The offer grants nothing and rewrites nothing: the
restricted path is still refused, and the human is still the only authority
for it. The offer replaces the *implicit default* `Ask` only; an operator tool
rule, an operator path rule, a stored decision, and a hard deny all keep their
authority.

**The one condition on this closure — a live observation with a real model —
has since been obtained**, and the milestone is closed on that evidence. See
§4 for the run and §13 for the correction that discharges the condition.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| The model is answered with the actionable guidance before any human is asked | `agent_loop_harness::temp_root_write_offers_a_redirect_before_asking_the_human` | pass — the first attempt's tool result carries the guidance and **no** `PermissionPending` is published for it |
| A model that follows the offer succeeds with no human involved | `agent_loop_harness::temp_root_write_following_the_offer_needs_no_human` | pass — the write lands in the caller's own private scratch root, zero permission prompts are published, and the restricted path is never created |
| A model that repeats the same restricted path reaches the human | `agent_loop_harness::temp_root_write_offers_a_redirect_before_asking_the_human` | pass — the second, identical attempt publishes `PermissionPending` and is answered through the unchanged approval path |
| The offer is issued once per (session, tool, target) | `permission::tests::temp_root_redirect_offer_answers_the_model_before_the_human` | pass — the same path in the same session is not offered twice; another session is offered again |
| The offer replaces the implicit default only | `permission::tests::temp_root_redirect_offer_requires_the_implicit_default` | pass — an operator tool rule, an operator path rule, and a stored decision each suppress the offer |
| The offer covers only an unauthorized temp root | `permission::tests::temp_root_redirect_offer_only_covers_an_unauthorized_temp_root` | pass — a target inside an authorized root, a target outside every root that is not a known temp root, a missing private scratch root, and the edit family are all out of scope |
| The restricted path is never written | both `agent_loop_harness` tests | pass — the `/tmp` target does not exist after either test |
| The message the model sees is identical in both paths | `tool::write::delegated_scratch_tests::unauthorized_temp_write_suggests_private_scratch_without_rewriting`; `tool::util::tests::temp_guidance_uses_path_components_and_rejects_traversal` | pass — both still green with the shared wording |
| A live observation with a real model | the durable `projection_event` stream of sessions `4cb43218-4435-4b0e-beeb-a3cfa39777b0`, `4ef5f8cb-cae6-4654-bbb9-e37418f9500c`, and `4112e79b-cce5-480b-9bed-62e6b7e089fe` | pass — the first attempt was answered with the guidance and no `PermissionPending` was published; the follow-through wrote at the suggested scratch path, on two models and two wire surfaces; the repeated attempt published `PermissionPending` |

## 3. Production implementation evidence

| File | Change |
|---|---|
| `src/tool/util.rs` | `temp_root_redirect_message` renders the redirect offer; `canonicalize_with_missing_tail` canonicalizes a target that does not exist yet, using the same walk `validate_target_path` performs |
| `src/tool/write.rs` | `temp_path_recovery_error` now uses the shared wording. Behavior unchanged: the same message, the same refusal |
| `src/permission/mod.rs` | `PermissionChecker::temp_root_redirect_offer` decides the offer; `temp_root_offers` holds the per-session state; `operator_spoke_about` keeps operator decisions authoritative; `tool_rule_name_matches` and `canonical_target_key` are local helpers |
| `src/agent/tool_batch.rs` | the trusted roots are resolved once, before the permission check, and the general-`Ask` arm consults the offer before `resolve_general_ask_via_human` |

The offer is consulted in the general-`Ask` arm only, which is reached after
the hard-deny, security, and sensitive-path branches. A denied, a security
finding, and a sensitive path therefore reach the human exactly as before.

## 4. Verification executed

Local (macOS, rustc 1.89.0, load-averaged host):

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `scripts/verify.sh quick` | pass — every guard green, and `cargo check --workspace --all-targets --locked` clean |
| Focused suite — scratch, delegation, scheduler, sandbox, cleanup, permission, subagent, workspace-root authority, approval, temp-root | pass — **719 passed, 0 failed** |
| `cargo nextest run -p codegg --lib -E 'test(/temp_root_redirect/)'` | 3/3 pass |
| `cargo nextest run -p codegg --test agent_loop_harness -E 'test(/temp_root_write/)'` | 2/2 pass |

Hosted CI on the pull-request head: run **`39374727100`** — the `verify` job
passed on head `141f7114` (all 27 guards, workspace Clippy, and the workspace
tests). The pull request is [#134](https://github.com/dbowm91/codegg/pull/134).
The planning-record-only follow-up head `d861e1f6` was green on run
**`39377117365`**.

Post-merge verification on `main`: the milestone merged as **`e52695b9`**
(merge commit, pull request #134), and the `main` push run **`39378897301`**
passed the same `verify` job on the merged head.

No other workflow applies to this change: the Windows launcher and native
probe contracts are path-gated on `src/managed_process/**`, `src/scratch.rs`,
`src/tool/mod.rs`, and `src/tool/factory.rs`, and this milestone touches
none of them.

### Live observation with a real model (2026-10-20)

The condition this record originally named has been discharged. The
observation was taken on an **isolated** daemon (its own `HOME` and
`CODEGG_DAEMON_HOME`, a separate socket, and a separate workspace), so the
developer's own daemon was not driven. The provider was `opencode_go` on
model `muse-spark-1.3-contributor`, and the isolated configuration was set to
the **default** permission policy — `permission: null`, with no `default` or
`write` rule — which is the policy the milestone is about. The two earlier
sessions cited in §13 are *not* the evidence for this: they ran under
`permission: {default: "allow", write: "allow"}`.

The credential was supplied by the operator for this run only and was passed
**through the process environment** as `OPENCODE_GO_API_KEY`. It was never
written to the configuration file, never written to the credential store, never
committed, and never appears in any artifact: a recursive search of the
isolated home and the harness directory for the key's prefix returned nothing,
the daemon log contains no occurrence of it, and the `permission` table is
empty (0 rows) — so the run also demonstrates that the offer creates no
stored decision.

Three sessions were driven: two proving the offer and its follow-through, and
one proving that the escalation path is intact.

**Session `4cb43218-4435-4b0e-beeb-a3cfa39777b0` — the offer, then the
follow-through, with no human involved.** The prompt asked for a file at
`/tmp/codegg-m005-live-probe.txt` and told the model to follow the tool's
guidance if the path was refused; it did not name any scratch location, so the
model could only learn the redirect from the tool's own message. The durable
`projection_event` stream records:

| seq | event | content |
|---|---|---|
| 3 | `tool_started` | `write` with `{"content":"live probe","path":"/tmp/codegg-m005-live-probe.txt"}` |
| 4 | `tool_completed` | `temporary path is outside the authorized workspace; retry at '[HOME]/Library/Caches/codegg/scratch/sessions/69291aca…/codegg-m005-live-probe.txt' or request access to the original location through the normal permission flow` |
| 5 | `message_appended` | the model's own text: *"Tool refused /tmp path — retrying at the suggested scratch location."* |
| 6 | `tool_started` | `write` with the **suggested scratch path** |
| 7 | `tool_completed` | `Created [HOME]/Library/Caches/…/codegg-m005-live-probe.txt …` — success |

For that session the projection contains **zero** `permission_pending` events,
`core_event_log` holds **zero** rows, `session_events` holds only the two
events of the successful scratch write, and the `permission` table holds zero
rows. The restricted path was never created: `/tmp/codegg-m005-live-probe.txt`
and `/private/tmp/codegg-m005-live-probe.txt` both do not exist, while the
scratch file exists with exactly the requested content.

**Session `4112e79b-cce5-480b-9bed-62e6b7e089fe` — the escalation path is
intact.** The prompt asked for `/tmp/codegg-m005-insist-probe.txt` and told the
model to retry the *same* path if refused. The projection records the first
attempt answered with the same guidance (seq 4, no human asked), then the
model repeating the identical path (seq 6), then — the decisive event — seq 7:

```json
{"kind": "permission_pending",
 "permission": {"tool": "write",
                "path": "/tmp/codegg-m005-insist-probe.txt",
                "status": "pending",
                "resolved_at": null}}
```

So the offer is issued once for a given target, and the model's insistence
reaches the human exactly as before. The turn then parks on the unanswered
human decision, which is the unchanged behaviour.

**Session `4ef5f8cb-cae6-4654-bbb9-e37418f9500c` — the same behaviour on a
second model and a second wire surface.** The provider and the first session
used `muse-spark-1.3-contributor`, whose reviewed wire hint is the
`openai_responses` surface. This session used `longcat-2.5-preview-free`,
whose hint is `openai_chat_completions`, and the same prompt shape. The
projection records the same sequence: `tool_started` for
`/tmp/codegg-m005-longcat-probe.txt` (seq 2), `tool_completed` carrying the
same guidance (seq 3), `tool_started` for the suggested scratch path (seq 4),
`tool_completed` reporting the file created (seq 5) — and **zero**
`permission_pending` events for the session. The scratch file exists with
exactly the requested content, and the restricted path does not exist.

The offer is therefore not an artifact of one model or of one wire surface: it
is the permission layer's behaviour, and it is reproduced on both surfaces
CodeGG resolves for this provider.

One observation from the run that is **not** an M005 regression: the daemon
logged `checkpoint path normalization failed, batch non-restorable: unsafe
path: <scratch path>`. `normalize_single_path`
(`crates/codegg-core/src/snapshot/affected_paths.rs:206`) rejects *any*
absolute path that is not under the workspace root, and a private scratch root
never is, so every scratch write has always been non-restorable for
checkpointing. The write itself succeeded; the warning reports that the
mutation batch cannot be restored, which is the correct outcome for a path
outside the workspace.

**Not run, and why.** Nothing in §2 is outstanding. The earlier statement that
the live observation could not be obtained is superseded by the run above; the
reason it was unavailable then — unusable provider credentials in the isolated
harness — no longer applies, because the operator supplied a working
`opencode_go` credential for this run.

## 5. Invariant review

| Invariant | Result |
|---|---|
| The restricted path is never written | held — the offer is a refusal with guidance, not an approval |
| No path is rewritten | held — the offer names a suggestion; the requested path is unchanged |
| No grant is created | held — the offer adds no decision to the permission store |
| The human is the only authority for the original location | held — the offer changes *when* the human is asked, never whether the original path is allowed |
| Operator decisions keep their authority | held — tool rules, path rules, stored decisions, and hard denials suppress the offer |
| Scratch access does not apply to another session, the human shell, plugins, or arbitrary absolute paths | held — unchanged; the offer names only the caller's own private scratch root, and a shared root is never disclosed |
| The offer never pre-empts a security or sensitive-path escalation | held — it sits after those branches, not before |
| The edit family is not offered a scratch path it cannot use | held — the offer is scoped to `write` |

## 6. Failure and recovery review

The offer state is in-memory and per session, so a daemon restart clears it
and the next attempt is offered again — the conservative direction. A
cancelled or crashed turn leaves nothing behind. The decision is re-derived
from the checker's own rules on every call, so a policy change between the
offer and a later attempt is honored: the offer is not a cached permission.

The offer state lives on the `PermissionChecker`, which is constructed per
agent loop. The key carries the session id, so two sessions sharing a checker
do not suppress each other's offer.

## 7. Migration and compatibility review

No schema, DTO, configuration key, or protocol change. The operator-visible
change is narrow: a temp-root write under the default policy now reaches the
model before it reaches a human. An operator who wants the human asked first
can say so explicitly (`permission.tools.write = "ask"`), and the offer
honors it.

## 8. Security review

The offer **reduces** human involvement for one class of call and adds no
capability. The refused path is still refused; the only path that succeeds is
the scratch path, which was already authorized. The offer never discloses a
shared scratch root, a path outside the caller's own roots, or a credential.

One property worth recording, found while implementing: the guidance the
model receives goes through the same `redact_local_paths` pass as any tool
output, so a scratch root under `/var` or `/tmp` is shown redacted. The model
already knows its own scratch root — the capability contract in its prompt
names the private session scratch, and a delegated run's prompt block names
its private run scratch — so the offered file name stays actionable without
disclosing a path the model was not given.

A second property: the security classifier treats a write under
`^/(etc|usr|var|sys|proc|dev)/` as a system-directory write and escalates it
to the human *before* the general-`Ask` arm. The offer is therefore never
consulted for a system-path write. That is correct: the recognized host temp
roots this milestone covers are `/tmp`-style paths, and the product's own
scratch roots live under the user cache directory.

## 9. Documentation and operations

Updated together, as the module contract requires: `architecture/permission.md`
(the Check Flow contract and the new "The temp-root redirect offer" section),
`.opencode/skills/permission/SKILL.md`, and `docs/tools.md` (the
user-facing scratch section). No operator CLI, configuration key, or
migration is needed.

## 10. Unresolved findings

| Severity | Finding | Condition and exact future evidence |
|---|---|---|
| — | none open | The one finding this record originally carried — that the live observation with a real model had not been obtained — is discharged by the run in §4 and §13. |

No high, medium, or low finding is open.

## 11. Roadmap disposition

M003's advisory temp-write recovery now reaches the model under the default
policy. M003's own closure record is unchanged; this milestone is a
corrective pass on it, not a rewrite of it. The Windows Required half of
M003 remains outstanding and keeps M003 conditionally closed — that block is
unrelated to this change and is owned by
`plans/subsystems/windows-native-sandbox-roadmap.md`.

M005 is **closed**: the production change is merged and verified on the merged
tree, the hosted CI is green on the pull-request head and on the merged head,
and the live observation with a real model has been obtained under the default
policy. No successor milestone in this line is currently registered, and no
registered plan lists M005 as a dependency.

## 12. Registry updates

- `plans/registry.md`: the M005 row moves to `closed`, with the hosted CI run
  IDs and the live-observation evidence recorded.
- `plans/subsystems/managed-agent-scratch-roadmap.md`: the M005 row moves to
  `closed` with the same evidence.
- The M003 rows are unchanged.
- No downstream plan is unblocked by this milestone: the Windows Required
  container remains blocked on the independently owned backend, and no
  registered plan lists M005 as a dependency.

## 13. Factual correction — live observation obtained (2026-10-20)

This record was merged as **conditionally closed** with exactly one named
condition: *a live observation with a real model*. That condition is now
discharged, and the milestone is closed on that evidence. The correction is
factual; the implementation is unchanged since `e52695b9`.

What this correction does **not** change:

- **The production change is not touched.** The implementation, the shared
  message wording, the implicit-default-only gating, the once-per-target
  offer, and the "the offer sits after the security escalations" ordering are
  all as merged.
- **The M003 closure record is not rewritten.** M003 remains conditionally
  closed on its own Windows Required-sandbox condition, which is unrelated to
  this change and stays owned by
  `plans/subsystems/windows-native-sandbox-roadmap.md`.
- **The two earlier sessions are not the evidence for this.** The observations
  in sessions `281f028f` and `3bfab29a` — the model following the suggestion —
  were taken while the isolated configuration carried
  `permission: {default: "allow", write: "allow"}`. Under that permissive
  policy the write was allowed outright and the tool's own hint produced the
  message; the *human-prompt* observation in session `2164790b` was taken
  under the default policy. Pairing the two, as the earlier draft of this
  record did, compared runs with different permission policies. The evidence
  that closes this milestone is the run in §4, where both halves were observed
  under the **same** default policy in the **same** session family.
- **The credential is not part of the record.** The operator-supplied
  `opencode_go` key used for the run was passed only through the process
  environment and is not reproduced here. Nothing in the repository, the
  isolated configuration, the credential store, or the daemon log contains it.

The durable evidence lives in the isolated daemon's `projection_event` stream
for sessions `4cb43218-4435-4b0e-beeb-a3cfa39777b0`,
`4ef5f8cb-cae6-4654-bbb9-e37418f9500c`, and
`4112e79b-cce5-480b-9bed-62e6b7e089fe`, and the file-system state confirms
that the restricted path was never created. The observation is reproduced in
§4.
