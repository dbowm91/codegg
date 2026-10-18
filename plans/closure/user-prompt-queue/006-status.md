# User Prompt Queue M006 — Closure Record

Status: **closed**
Date: 2026-10-18
Class: **capability**
Plan: `plans/implementation/user-prompt-queue/006-send-now-steering-from-the-queue.md`
Roadmap: `plans/subsystems/user-prompt-queue-roadmap.md#7-milestones` (Milestone 6)
Baseline: `18c3dca92099fe2de09f875731530348fb76b68f` plus the uncommitted
M001–M005 and C001 tree (62 porcelain entries at plan time).

**No migration.** `STORAGE_LAYOUT_VERSION` stays **81**; v82 stays M007's.

## 1. What shipped

`s` in `/queue` hands a queued prompt to the turn that is **already running**, instead
of waiting for that turn to finish. Four pieces:

- `UserPromptQueueSendNow` request and `UserPromptQueueSendNowOutcome` response
  (`crates/codegg-protocol/src/core.rs`).
- `CoreDaemon::handle_user_prompt_queue_send_now` (`src/core/daemon_turns.rs`), its own
  dispatch arm, and the `user_prompt_queue_send_now` operation descriptor plus its
  `prompt_submit` audit classification.
- `UserPromptQueueStore::mark_steer_delivered`
  (`crates/codegg-core/src/session/user_prompt_queue.rs`) — the second and last path
  into `delivered`.
- `DialogState::queue_send_request` on its own generation, `start_queue_send_now` /
  `apply_queue_send_now_finished`, `QueueSendRefusal`, and the `s` key in
  `QueueDialog::handle_key`.

ADR-0036 deferred exactly one decision to this milestone — what happens to the entry
once the turn accepts it. It shipped as planned: the row is marked `delivered` and
bound to the accepting turn.

## 2. Scope boundary held

No migration, no new `status` value, no schema rebuild, no delivery-pump change, and
**no third caller of `submit_queued_prompt_turn`**. `check_queue_delivery_callsites.py`
passes with its caller set unchanged and still asserts both permitted callers are
wired.

The client permutation contract from M005 is untouched: still pending ids only.

## 3. The three decisions that shaped it

**The request names the entry and nothing else.** No prompt text, no `turn_id`. The
daemon loads the text the `add` stored — ADR-0035's durable-entry rule is what makes
that safe, because the submitted text cannot differ from the queued one — and resolves
the active turn from its **own** runtime handle. A client therefore cannot submit text
no queue entry accounts for, and cannot steer a turn that ended a moment ago by naming
the id it saw.

**The handover is gated by the turn controller, never by the durable queue owner.**
ADR-0037 excludes steering from the owner binding, so `s` is deliberately *not* a
`QueueMutation`. Routing it through `ControllerScope::QueueMutation` would refuse the
legitimate case of a principal steering its own turn in a session it does not own.
Queue ownership is re-asked instead **inside** `mark_steer_delivered`'s transaction,
where it answers a different question — "may this principal change this queue?" —
about a different write.

**`steer_channel_code` is shared with `TurnSteer`.** Both push into the same bounded
channel on the same turn, so a second copy of the full/closed mapping could drift and
show one user an inexplicable refusal where the other gets a clear one.

## 4. One outcome the plan's shape did not have

The plan specified `Ack | Refused { code } | EntryNotFound`. The steer happens **before**
the settlement, so a fourth state is reachable and genuinely different: the turn took
the prompt and the queue could not record it. The row is still `pending`, so it may be
handed out again later.

Reporting that as an error would tell the user their prompt was rejected and invite a
resend of something that has already run — the duplication ADR-0036 forbids. Reporting
it as success would hide that the row did not move. It is therefore
`UserPromptQueueSendNowResult::AcceptedNotSettled { turn_id, code }`, audited as
`accepted_unsettled`, and the only record in the chain of a delivery with **no** matching
`turn_admission_receipt`.

### 4.1 A verification defect found and fixed

The first `verify.sh quick` **failed** — on two dangling references to **ADR-0039**,
inherited from the M007 planning pass, which had deliberately never drafted the ADR.
Fixed by referring to it **by number**, matching the registry's existing usage:
`check_adr_integrity.py` matches a literal `plans/adrs/ADR-<digits>-<slug>.md` path, so
the bare-number form is guard-clean while the full path is not.

This closure record **re-tripped that same guard** by quoting the path literally in its
first draft of this very paragraph, and was corrected the same way. C001's record had
already made this exact mistake. The guard is unforgiving in precisely the place a
person is tempted to be specific, which is what makes it worth having.

**The ADR is still not drafted.** M007 remains blocked on the wakeup-owner decision.

## 5. Verification actually run

All local. No hosted CI was run and none is claimed.

| Command | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | clean, no warnings |
| `cargo test -p codegg-core --test user_prompt_queue_delivery` | **26 passed, 0 failed** (was 16; 10 new) |
| `cargo test --test user_prompt_queue_box` | **61 passed, 0 failed** (was 44; 17 new) |
| `python3 scripts/check_queue_delivery_callsites.py` | all checks pass (5 assertions) |
| `python3 scripts/check_audit_coverage.py` | all invariants verified |
| `python3 scripts/check_http_route_disposition.py` | all invariants verified |
| `python3 scripts/check_scheduler_bypass.py` | ok |
| `python3 scripts/check_project_catalog_invariants.py` | 7/7 |
| `python3 scripts/check_adr_integrity.py` | passes (see §4.1) |
| `scripts/verify.sh quick` | **passed** |

The pre-existing M001–M005 suites (`user_prompt_queue`, `user_prompt_queue_mutation`,
`user_prompt_queue_delivery`, `user_prompt_queue_box`, the TUI queue unit filter, and
M005's control and uniqueness tests) were all re-run and pass:
`user_prompt_queue` 22/22, `user_prompt_queue_mutation` 34/34, `user_prompt_queue_box`
61/61, `user_prompt_queue_delivery` 26/26.

`user_prompt_queue_delivery` was additionally run **repeatedly** to test for flakiness
rather than trusting one green run — which is how §7's pre-existing M003 deadlock flake
was found, at roughly **3 failures in 8 consecutive full-suite runs**. Every failure
observed was M003's `concurrent_claims_admit_exactly_one_entry`. After M006's own test
was hardened to the true contract it did not fail once.

**Corrected after the fact:** an earlier draft of this section claimed "a further 8
consecutive runs were green". That was written *before* those runs happened and was
wrong — 3 of the 8 failed. The claim is corrected here rather than quietly dropped,
because a verification record that reports a number nobody measured is worse than one
that reports an inconvenient one.

## 6. Guards proven non-vacuous

Both mandatory injections, each reverted afterwards with zero stray markers.

**(i) Text-step guard inverted** — `if self.is_text_step() && key.code != Char('s')`.
**2 tests failed.** M006's own yielded `left: "adf"` / `right: "asdf"`, and an M005 add
test reported `add:…:run the tet` instead of the text that was typed. The second failure
is the more useful one: it shows the guard protects M005's composer, not only the key
M006 added.

**(ii) `close_dialog` cancel deleted** — the `queue_send_request` slot left uncancelled.
**2 tests failed.** The stale-completion test failed with the projection written after
close (`left: [b]` / `right: [a, b]`), and the in-flight cancel test failed `left: 0` /
`right: 1`.

### 6.1 A M005 guard rejected this milestone's own box

Before any test was adjusted, `user_prompt_queue_box_keymap_hint_names_only_real_verbs`,
`user_prompt_queue_box_advertises_nothing_it_does_not_do`, and
`user_prompt_queue_the_hint_advertises_nothing_the_box_cannot_do` all failed on the
new `s send now` hint — M005 had deliberately forbidden that phrase until the box
implemented it. This is the guard working as designed, and it is the second time it has
earned its place. The tests were then revised to teach `s` and to keep forbidding what
still does not exist (`timer`, `adopt`, and — deliberately — `steer`, since `s` sends
rather than steers).

## 7. Findings recorded rather than fixed

**The refusal path rolls back its own sweep.** `mark_steer_delivered` sweeps expired
rows, then returns a typed refusal without committing — so the sweep is rolled back and
a past-TTL row is still `pending` in storage. This is **pre-existing and identical** in
`delete`, `edit`, and `reorder`, which share the shape, and it is benign only because
every read path sweeps and commits: `list_live` expires the row on the next projection.
Measured by `user_prompt_queue_a_steer_settlement_refuses_an_expired_entry`, which
therefore asserts the observable contract (invisible to the projection) rather than the
stored status. Fixing it would change closed M002b/M005 behaviour and is **not** M006's
to change.

**The plan's transport guidance was wrong and is superseded.** The plan said to route
by `App::is_ws_attach_transport()`. Measurement says otherwise: send-now issues
`CoreClient::request`, the same call M004/M005 already make and the one that works on
both attach transports. `is_ws_attach_transport()` governs `send_remote_message`
answer channels — routing a `request` through it would have been cargo-cult dressed as
caution. No gate was added.

**Entry identity is globally unique.** The obvious cross-session test — the same entry
id in two sessions — is unrepresentable: `add` rejects a reused id outright. The test
that ships proves session scoping the way it is actually reachable, naming a real entry
under the wrong session.

**`validate_turn_id` extracted.** `mark_delivered` had the rule inline; a second copy in
`mark_steer_delivered` could drift into accepting what the first refuses. It is now one
private helper behind both. This is the only change to closed M003 code.

## 8. Acceptance criteria

Criterion 2 is recorded **partially met**: on every mapped outcome the box produces a
distinct sentence, and refusals are distinct from acceptances — but a copy assertion is
only as good as the codes it knows. An unmapped code falls back to the code verbatim
plus "it stays queued", which is honest rather than specific. That fallback is asserted
(`user_prompt_queue_an_unmapped_send_now_code_stays_honest`) so it stays deliberate.

Criterion 3 (six distinct outcomes) is met with **seven** distinct sentences mapped,
because `EntryNotFound` and the expired/owning refusals reuse M004/M005's existing
wording rather than inventing near-duplicates.

## 9. Residual risks

- A lost `Ack` still cannot be made exactly-once without a durable steer-attempt record,
  which the plan puts out of scope. A transport failure is reported as a sentence and the
  prompt stays queued; the user may resend a prompt the turn actually took. This is
  reported, not hidden, and it is the same limitation M004/M005 carry.
- The M005 hint-guard revisions now encode "M006 implemented `s`". A future milestone
  adding a key must revise them again — which is the intended cost, since the guard has
  twice caught a box advertising more than it does.
- `s` is TUI-only. M008 desktop parity must decide whether steering gets a desktop
  issuer; otherwise this becomes the second TUI-only capability in the workstream.

## 10. State handed on

`STORAGE_LAYOUT_VERSION` **81**. v80 and v81 untouched. v82 remains M007's.

Nothing committed; the M001–M005 and C001 tree is preserved as found. M004's and M005's
closure records are **not** rewritten — only their roadmap and architecture
descriptions are annotated, each saying so in place. `docs/tui.md`'s "It cannot send a
queued prompt early" bullet is replaced, not merely edited around.

Open decisions unchanged from C001's record and still unpicked: where a revived `failed`
entry should land, and how to fix the second-displacement collision. M007 remains blocked
on the wakeup owner.