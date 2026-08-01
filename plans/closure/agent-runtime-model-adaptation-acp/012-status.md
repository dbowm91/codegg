# Agent Runtime, Model Adaptation, and ACP Milestone 012 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-runtime-model-adaptation-acp/012-acp-turn-lifecycle-and-correlation-correctness.md`
Source subsystem roadmap: `plans/subsystems/agent-runtime-model-adaptation-acp-corrective-closure-addendum.md#milestone-012--acp-turn-lifecycle-and-correlation-correctness`
Repository baseline reviewed: `7d8657e60aad85f677144b1bd0e7fb5d2929faa3`
Implementation commit: `da0f5b8668f523ab099b66fad632b99d36a59528` — fix ACP turn lifecycle correlation

## 1. Executive finding

M012 is strictly closed. The ACP adapter now binds a prompt only to a
post-submission, exact-session native turn, retains cancellation and close
intent until that turn is identified, sends native cancellation once, filters
stale/neighbour events, preserves public replay roles, rejects malformed
subscription success, and performs bounded close/EOF cleanup. ACP remains a
thin stdio adapter over daemon-owned sessions, turns, and projections.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| One prompt/one turn and no pre-submission binding | `ActivePrompt.submission_event_floor`; exact `TurnStarted` binding; lifecycle unit tests |
| Pending cancel/close delivery and idempotency | Shared `cancel_if_ready`; `cancel_requested`, `close_requested`, `cancel_sent`; cancellation/close unit test |
| Stale, neighboring, and wrong-turn isolation | `can_accept`, exact session/turn checks, terminal matching; stale/neighbor unit tests |
| Exactly one truthful terminal response | Active state is taken before writing the terminal response; matching terminal events only |
| Role-correct bounded replay | `ProjectionSnapshotBundle::One` replay maps public user/assistant messages and omits tool/system/reasoning/private entries |
| Typed subscription failure and teardown | Unexpected subscription responses return `AppError`; close and EOF unsubscribe transient bindings |
| Protocol purity and bounds | Existing 1 MiB frame/prompt checks retained; real stdio test passed |

## 3. Production implementation evidence

- `src/acp.rs` contains the bounded lifecycle state and the only ACP prompt
  slot. No second runtime or durable ACP store was introduced.
- The submission boundary is the event high-water mark drained immediately
  before `TurnSubmit`; native `TurnStarted` is the only turn binder.
- Projection updates require a non-empty exact envelope session and turn ID.
  Terminal core and projection events require the same bound turn.
- `session/cancel` and `$/cancel_request` use the same path. `session/close`
  marks close, cancels, unsubscribes, suppresses updates, and retains the
  prompt slot until its terminal response. EOF performs cancellation and
  unsubscription cleanup.
- Replay consumes the canonical role-bearing projection snapshot instead of
  relabeling legacy text parts as assistant output.
- `architecture/acp.md` documents lifecycle, correlation, cancellation,
  replay, and capability limits.

## 4. Verification executed

All commands were run locally against the reviewed worktree.

- `cargo fmt --all -- --check` — passed (formatting was applied first).
- `cargo check -p codegg --all-targets` — passed; existing unrelated warnings
  remain in `codegg-core` build/model-profile code.
- `cargo test -p codegg acp:: -- --nocapture` — passed; 6 ACP unit tests.
- `cargo test --test acp_stdio -- --nocapture` — passed; 1 real-process test.
- `cargo test --features server --test projection_transport_real -- --nocapture`
  — passed; 58 tests.
- `python3 scripts/check_projection_transport_isolation.py` — passed.
- `python3 scripts/check_projection_transport_lifecycle.py` — passed.
- `bash scripts/check_projection_disclosure.sh` — passed.
- `bash scripts/verify.sh quick` — passed; generated-agent, Tokio-flavor,
  core-boundary, workspace all-target check, and quick verification gates are
  green (with pre-existing unrelated warnings).
- The plan's named `session_projection_transport` target does not exist in this
  repository; `projection_transport_real` is the current server-feature
  transport equivalent and was run successfully.

## 5. Invariant review

ACP stdout remains newline-delimited JSON-RPC only. The daemon remains the
authority for sessions, turns, cancellation, replay, and publication. Events
cannot bind from before the submission floor or from another session/turn.
Private reasoning is not replayed or streamed. Bounds remain explicit and no
unbounded ACP buffer or durable restart promise was added.

## 6. Failure and recovery review

Invalid or unexpected subscription responses fail the binding. Repeated cancel
and close requests do not duplicate native cancellation. Native cancellation
failure is not converted into a false successful cancellation response; the
adapter continues to await the native terminal event. EOF releases transient
subscriptions and makes a best-effort bounded cancellation request.

## 7. Migration and compatibility review

No native protocol or storage schema change was required. Existing ACP method
names, v1 negotiation, `Ack` turn submission response, and one-active-prompt
compatibility surface remain unchanged. Replay now prefers canonical projection
role data and safely omits entries the ACP representation cannot faithfully
express.

## 8. Security review

Only public projection visibility is mapped to ACP. Reasoning, private content,
and unsupported tool/system entries are omitted. Frame and prompt bounds remain
enforced, and no shell, credential, cwd mutation, or new authority path was
introduced.

## 9. Documentation and operations

`architecture/acp.md`, the implementation plan status, the corrective addendum,
and this closure record describe the same lifecycle contract. The current
repository emits compiler warnings in unrelated existing code; no warning was
introduced by the ACP change.

## 10. Unresolved findings

No critical, high, or medium finding remains in M012 scope. Low-severity
limitations are intentional: one active prompt per ACP connection remains the
advertised adapter contract; unsupported tool-history representations are
omitted; native cancellation durability across daemon restart remains outside
this transient adapter milestone.

## 11. Roadmap disposition

M012 is closed. M013 is dependency-ready because its sole hard dependency is
M012 strict closure. M014–M017 remain blocked by their named predecessor
closures and must not be unblocked by this record.

## 12. Registry updates

- M012 moved from dependency-ready to recently closed.
- M013 moved from blocked to dependency-ready in the same registry change.
- M014–M017 remain blocked with their existing predecessor requirements.
- No new corrective pass is required for M012, and the corrective addendum
  remains active until M017 completes the broader closure sequence.
