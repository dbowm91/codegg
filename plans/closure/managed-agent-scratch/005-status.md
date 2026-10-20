# Managed Agent Scratch Milestone M005 — Closure Status

Status: conditionally closed

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

**The one condition on this closure is a live observation with a real model,
which could not be obtained in this environment** — see §10.

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
| A live observation with a real model | — | **not obtained** — see §10 |

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

**Not run, and why.** The live observation in §2 was not obtained: the
isolated harness's provider credentials are no longer usable. The
`generalcompute` key is rejected with HTTP 401, the `minimax` key fails
decryption, and the stored `openai` and `opencode_go` credentials have no
bound provider connection. Binding a credential is an operator decision, and
the developer's own daemon was not driven, so no live run was possible.

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
| low | The live observation with a real model was not obtained in this environment | The isolated harness's provider credentials are unusable: `generalcompute` returns HTTP 401, `minimax` fails decryption, and the stored `openai` and `opencode_go` credentials have no bound provider connection. The future evidence is one daemon run in which a model writes to a known host temp root under the default policy, and the durable `core_event_log` shows a `tool_started` with **no** `permission_pending` for that attempt, followed by a write at the suggested scratch path. The product path is already covered by the two `agent_loop_harness` tests above, and the model-follows-the-guidance behavior was observed live in session `3bfab29a` for the tool-level variant of the same message. |

No high or medium finding is open.

## 11. Roadmap disposition

M003's advisory temp-write recovery now reaches the model under the default
policy. M003's own closure record is unchanged; this milestone is a
corrective pass on it, not a rewrite of it. The Windows Required half of
M003 remains outstanding and keeps M003 conditionally closed — that block is
unrelated to this change and is owned by
`plans/subsystems/windows-native-sandbox-roadmap.md`.

M005 is **conditionally closed**: the production change and the merged-tree
verification are complete, and the single named condition is the live
observation above.

## 12. Registry updates

- `plans/registry.md`: the M005 row moves to `conditionally closed`, with the
  live-observation condition named and the hosted CI run IDs recorded.
- `plans/subsystems/managed-agent-scratch-roadmap.md`: the M005 row moves to
  `conditionally closed` with the same condition and the same run IDs.
- The M003 rows are unchanged.
- No downstream plan is unblocked by this milestone: the Windows Required
  container remains blocked on the independently owned backend, and no
  registered plan lists M005 as a dependency.
