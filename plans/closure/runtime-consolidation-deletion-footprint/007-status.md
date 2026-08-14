# Runtime Consolidation, Deletion, and Footprint M007 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/runtime-consolidation-deletion-footprint/007-integration-verification-closure.md`
Source subsystem roadmap: `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`

Historical provisional baseline: `0dae4d8ce9a7988aef3b11db5ffa8b5993722712`

Accepted final candidate: `c8c31d909310131ca4b1cc38c725e0163f86a47d`

Implementation commit: `c8c31d90` — final corrective compatibility and
ownership pass

Hosted evidence: [CI run 31724978736](https://github.com/dbowm91/codegg/actions/runs/31724978736), [verify job 94530985774](https://github.com/dbowm91/codegg/actions/runs/31724978736/job/94530985774), green on the exact accepted candidate.

## 1. Executive finding

M007 is strictly closed. M001–M006 are accepted, the audited TUI compatibility
and provider-turn ownership gaps are corrected by M009, the final default and
production-feature measurements are complete, the capped local workspace
verification is green, and the ordinary hosted CI contract passed on the exact
final production candidate.

The earlier provisional record remains represented by its historical baseline
and incomplete-evidence notes in Git history; this record does not erase that
M006 was previously blocked or that the prior hosted/local attempts were not
yet conclusive.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| One durable scheduling owner and no UUID/u64 bridge | M001 closure; scheduler guards; source review | pass |
| Active TUI schedule/list/delete behavior | durable `ScheduleCreate`/`ScheduleList`/`ScheduleDelete` handlers and protocol test | pass |
| Structured status/effect facts remain authoritative | M002 closure; recovery tests | pass |
| AgentLoop ownership physically decomposed | `context_runtime.rs`, `tool_batch.rs`, and real provider body in `provider_turn.rs` | pass |
| Prompt/runtime/history authority remains canonical | M004 closure and architecture review | pass |
| Verification ratchets/docs remain current | M005 closure; final guards and architecture updates | pass |
| M006 final dependency/feature/size review | `006-status.md` on `c8c31d90` | pass |
| Focused behavior and harness verification | TUI, durable-schedule protocol, loop/recovery, and 40 harness tests | pass |
| Broad local verification | `CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1` | pass |
| Hosted CI on exact final candidate | run `31724978736`, job `94530985774` | pass |

## 3. Production implementation evidence

The active TUI task commands now require the session's canonical workspace and
send durable schedule requests. Schedule creation uses an existing durable
subagent `JobTemplate`, interval recurrence, `skip_if_running`, and
`run_once_now`; list/delete use opaque `ScheduleId` values and project durable
summaries for the existing view. The old `Task*` request rejection remains an
explicit compatibility boundary for external legacy callers.

`ProviderTurnAdapter` now contains the provider retry, timeout/stall timeout,
streaming, normalized event publication, usage accounting, and error body.
`AgentLoop` no longer contains a parallel `stream_with_retry_impl` or
`stream_once` implementation.

## 4. Verification and measurements

Passed locally on the accepted candidate:

```text
cargo fmt --all -- --check
cargo check -p codegg --lib --locked
cargo test -p codegg --lib tui::commands::tasks::tests -- --nocapture
cargo test -p codegg --lib core::daemon::tests::durable_schedule_protocol_supports_create_list_delete -- --nocapture
cargo test -p codegg-core jobs::schedule -- --nocapture
cargo test -p codegg --lib agent::progress_recovery -- --nocapture
cargo test -p codegg --lib agent::r#loop::tests -- --nocapture
cargo test --test agent_loop_harness -- --test-threads=1
scripts/verify.sh quick
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked -- -D warnings
cargo check -p codegg --locked --features server,plugins,lsp-test-support
CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1
git diff --check
```

M006 isolated release measurements are 54,347,840 bytes by default and
63,566,624 bytes with `server,plugins,lsp-test-support`; the full measurement
record is in `006-status.md`.

## 5. Invariant, failure, recovery, migration, compatibility, and security review

No storage migration or schema change was required. Existing durable schedule
stores and daemon services remain authoritative. Workspace/session authority,
scheduler admission, permission and Tool Broker enforcement, cancellation,
retry/idempotency, private-reasoning projection, provider credential handling,
path policy, and execution ownership are unchanged. Legacy task requests fail
deterministically and cannot reach removed scheduler state. The provider move
does not change provider wire formats or retry semantics.

## 6. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| critical/high/medium | None in scope | closed |
| low | None requiring follow-up | closed |
| deferred | Independent Provider M007, Tool Programs M019, and DVR M006 workstreams | retained in their own registries; not unblocked by this roadmap |

## 7. Roadmap and registry disposition

The runtime-consolidation roadmap is closed by this exact-candidate record and
M009. No unrelated registered future plan was newly unblocked. The Development
Verification and Release M006 plan remains blocked on Provider M007 and Tool
Programs M019; Tool Programs M019 remains ready for its own strict review.
