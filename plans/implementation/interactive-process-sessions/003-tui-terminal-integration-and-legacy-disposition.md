# Interactive Process Sessions Milestone 003 — TUI Terminal Integration and Legacy Disposition

Status: blocked

Repository baseline: `9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6`

Source roadmap: `plans/subsystems/interactive-process-sessions-roadmap.md#M003--TUI-terminal-integration-and-legacy-disposition`

Long-term requirements: `plans/000-long-term-specification.md#25-tui-target-behavior`, `#29-system-invariants`.

Applicable ADRs: none. Primary class: capability.

## 1. Objective

Provide the reference TUI interactive-terminal experience over M002 and disposition the misleading deferred `terminal` model tool plus any remaining metadata-only shell-session compatibility so each execution surface has one truthful owner.

## 2. Why this milestone is ready

Blocked on M002. Bash remains canonical model shell and human shell commands/managed process paths are stable.

## 3. Current implementation evidence

The existing `terminal` tool describes itself as interactive but performs one `sh -c` managed-process invocation and returns captured output. It is deferred from ordinary model turns due overlap with Bash. The TUI has no real daemon PTY attach view at baseline. M001/M002 will establish the actual interactive owner.

## 4. Invariants that must not regress

TUI does not own PTY/process state; model-facing Bash remains unambiguous; keyboard input only goes to terminal when terminal focus is explicit; terminal escape/focus cannot accidentally submit prompts; client disconnect/close follows M002; no raw terminal becomes session observation.

## 5. Scope

In: TUI terminal view/controller, create/list/attach/detach/terminate actions, resize forwarding, bounded scrollback rendering, focus/escape/key handling, reconnection state, consumer census and remove/rename/delegate existing `terminal` tool, final obsolete shell-session docs/export disposition if not already removed by residual M001. Out: model-controlled interactive PTY, terminal sharing, tmux features, remote PTY.

## 6. Required production changes

Frontend: terminal state is projection of M002 handles/output; resize/focus/input commands use existing async TUI command architecture. Tool surface: census `terminal` external/config consumers; prefer removal because Bash owns model shell and M002 owns human interactive process; if retained, rename/description must truthfully say one-shot and delegate canonical process owner. Docs/skills: remove ambiguous shell-session/terminal claims.

## 7. Ordered work packages

A — implement TUI terminal reducer/view and bounded scrollback over M002.

B — input/focus/resize/create/attach/detach/terminate commands with robust key-state tests.

C — reconnect/process-exit/lag/resync UX and multi-project workspace routing.

D — compatibility census/disposition of `terminal` model tool and remaining shell-session surface.

E — user docs/help and end-to-end fixture.

## 8. Failure, cancellation, restart, and contention semantics

Process exit renders terminal state without crashing view. Disconnect permits reattach if process live; restart shows gone handles. Input after exit is rejected. TUI close detaches unless explicit terminate action/policy. Rapid resize/input uses bounded/coalesced behavior.

## 9. Compatibility and migration

No persisted TUI migration expected. Remove/rename `terminal` only after consumer census; historical stored tool names remain readable as history where needed but not executable aliases unless justified.

## 10. Required tests

Interactive command requiring input; resize; focus/key escape; project/workspace routing; detach/reattach; process exit; lag/resync; daemon disconnect/restart; model tool registry/disclosure after legacy disposition; no observer/projection raw terminal coupling.

## 11. Required verification commands

```bash
cargo test --workspace tui --no-fail-fast
cargo test --workspace interactive_process --no-fail-fast
cargo test --workspace tool_surface --no-fail-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

## 12. Documentation updates

TUI help/architecture, tool/shell/process architecture and skills; user-facing terminal semantics.

## 13. Acceptance criteria

A user can run a real interactive workspace process in TUI, resize/detach/reattach/terminate it, and recover cleanly from disconnect/exit; model tool descriptions contain no false interactive duplicate; one canonical PTY owner exists.

## 14. Stop conditions

M002 not closed; UI needs raw terminal to become session observation; removing legacy tool breaks demonstrated supported consumer without migration; model PTY authority would be widened.

## 15. Closure evidence required

M002 closure, end-to-end terminal fixture, focus/resize/reconnect tests, tool consumer census/disposition, model-visible surface evidence, docs and exact verification results.

## 16. Handoff notes

Coordinate deletion overlap with residual-runtime M001. Do not duplicate work already closed there; this milestone only performs remaining legacy disposition required for truthful terminal UX.
