# Human Execution Corrective Roadmap

Status: active (C003 closed; C001 corrective pass required; C002 blocked on C001)

Repository baseline: `578b62bd9580382e00fb19e096fc1794e1f5618b` (2026-10-09).

Predecessors: `architecture/human_shell.md`; `plans/shell_output_projection_rtk_roadmap.md`; closed `plans/subsystems/interactive-process-sessions-roadmap.md` M001–M003 and `plans/closure/interactive-process-sessions/003-status.md`; `plans/closure/residual-runtime-consolidation/001-status.md` (removal of obsolete metadata-only `shell_session`).

Long-term references:
- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#18-remote-projects-and-execution-targets`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Applicable ADRs: none for the corrective contracts; remote-node execution federation or durable PTY identity requires separate architectural authorization.

## 1. Purpose and ownership boundary

Correct human-initiated command execution without conflating it with the agent's `bash` tool. `!`/`!!` currently run genuine, finite, non-PTY shell processes through the frontend's `ShellRuntime`. `/terminal-*` uses a separate, real Unix PTY owned by the daemon and execution-node service. These are distinct capabilities, not model echoes. This roadmap covers actual context promotion, workspace locality/authorization for finite human commands, and faithful terminal keyboard/display handling. Prior PTY M001–M003 closure remains historical fact, not rewritten.

## 2. Work classification

Invariants: model involvement is never required for human execution; `!` and raw PTY output are not model context by default; only explicitly approved, bounded/redacted content can become session/model context; workspace execution is owned by the authorized execution node; finite shell processes belong to `ManagedProcessService` and PTYs to `InteractiveProcessService`.

Capabilities: functional `!!`, `/shell-include`, `/shell-ask` into the intended session/model turn; local and remote frontend commands execute in the selected workspace or fail explicitly; Esc and VT screen behavior permit meaningful interactive terminal use.

Infrastructure: typed provenance-bound, idempotent promotion/consumption; typed daemon shell-run/stream/cancel protocol where needed, reusing existing auth/scheduler/context components; bounded VT emulator state in the presentation layer.

Polish: accurate skill/manual/config claims, visible execution-locality/availability diagnostics, comprehensible focus and keybinding UX.

## 3. Non-goals

No agent tool rerouting through human shell, model-driven PTY, new process supervisor/scheduler, tmux or reboot-surviving terminals, generalized SSH/remote-node fabric, Windows ConPTY, generic context storage, or new CI lanes. Do not conflate `Session`, `Job`, shell-command run and PTY handle identities.

## 4. Current implementation and deficiencies

`src/tui/app/prompt_turn.rs::send_prompt` intercepts bangs before agent turn submission. `src/tui/commands/shell.rs` creates `ShellRuntime::new()` in the TUI and `src/shell/runtime.rs` runs `$SHELL -lc` via `ManagedProcessService::run_streaming`. The process runs on the frontend's host and cannot retain shell state across separate commands.

Shell-cell projection metadata comes from `config_command_projection(..., ModelContext)`, but `!!`/`/shell-include`/`/shell-ask` separately format `ShellOutputStore` bytes and insert `UIMessage` through `add_user_message`, without demonstrated daemon/session acceptance or model-facing dispatch. That can bypass projection redaction/bounds. Config knobs (`enabled`, `default_timeout_secs`, `auto_promote_bangbang`) require a real production-use audit.

`src/interactive_process.rs` supplies Linux/macOS PTYs and `tests/interactive_terminal_tui.rs` demonstrates input through `cat`. But `src/tui/interactive_terminal.rs::classify_key` always consumes Esc as focus exit, and `render_lines` simply newline-splits lossy text; cursor control, erase and alternate screen are not faithfully rendered. That is a terminal presentation gap, not absence of a PTY.

## 5. Target architecture

The TUI remains a view and human-intent adapter. Human finite execution receives explicit authenticated workspace/node context, bounded process lifecycle and output/provenance. Promoted output is redacted once by the canonical model-target projector and acknowledged/staged by the authoritative session/context owner, never just a local chat bubble. PTY sessions remain daemon-owned and separate; frontend renders a bounded VT screen and uses a dedicated exit-focus chord without suppressing literal Esc input.

## 6. Dependency graph

`C001` (promotion and redaction) is ready. `C002` (workspace-owned human shell dispatch) hard-depends on C001's accepted context/output ownership and is blocked until C001 closes. `C003` (PTY keyboard and VT presentation) is ready independently because original interactive-process M001–M003 are closed.

## 7. Milestones

### C001 — Promotion, redaction, and config truth
Class: invariant/capability. Plan: `plans/implementation/human-execution-corrective/001-human-shell-promotion-and-redaction.md`. Output from `!` stays private. Explicit promotion reaches exactly one correct next model-facing turn or, for `/shell-ask`, an explicitly submitted question turn; projection is bounded and redacted on every branch; reported success is backed by canonical acceptance. Verify advertised config.

### C002 — Workspace-owned finite shell dispatch
Class: infrastructure/capability. Plan: `plans/implementation/human-execution-corrective/002-workspace-owned-human-shell-dispatch.md`. Remove accidental frontend-local execution of workspace commands: daemon/execution owner authorizes, admits, streams and cancels finite human commands. Unsupported cross-node cases fail, not silently execute on client. Hard dependency: C001.

### C003 — PTY keyboard and VT fidelity
Class: capability/polish. Plan: `plans/implementation/human-execution-corrective/003-terminal-keyboard-and-vt-fidelity.md`. Deliver explicit focus exit and literal Esc forwarding, safe bounded VT screen interpretation, truthful resize/resync, and real PTY fixtures beyond `cat` echo. Independent of C001/C002.

## 8. Cross-cutting requirements

Storage: only authoritative session state owns model-visible promotion; raw PTY bytes not stored as session chat. Protocol: bounded typed requests, capabilities and backward compatibility, trusted server-side identity, unambiguous failure. Security: redaction and source provenance are mandatory for model-target output; command pattern warnings are not an OS sandbox; never send client environment/credentials as trusted execution authority. Lifecycle: stale tab/session, retry, reconnect, duplicate promotion, output truncation, cancellation and process restart must be handled explicitly. Tests: fake-provider context capture, multi-workspace daemon/client fixture, and real Unix PTY/VT fixtures. Reuse existing ownership guards and CI.

## 9. Verification strategy

Require focused tests and `scripts/verify.sh quick`; `python3 scripts/check_execution_ownership.py` for spawn changes; authorization and scheduler guards where relevant. No network-dependent live model test or permanent verification lane. Document platform limitations and actual executed results in separate closure records.

## 10. Risks and decisions

UI-state promotion is not evidence of model context. Raw shell-store reformatting bypasses model projection. A frontend may be on a different host from the workspace. Replayed PTY output may lack prior escape state after bounded-history gaps. If cross-node execution or context persistence requires a new architectural decision, stop and split the milestone, not fabricate local fallback.

## 11. Completion definition

C001–C003 each need an independent closure record including user-visible end-to-end acceptance. Prior interactive PTY M001–M003 closure stays intact. Update `architecture/human_shell.md`, `architecture/tui.md`, `architecture/process-tool-execution-ownership.md`, relevant skills, user docs and active registry with verified facts.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure |
|---|---|---|---|
| C001 | corrective pass required | `plans/implementation/human-execution-corrective/001-human-shell-promotion-and-redaction.md` | `plans/closure/human-execution-corrective/001-status.md`; follow-up C004 |
| C002 | blocked (C001) | `plans/implementation/human-execution-corrective/002-workspace-owned-human-shell-dispatch.md` | pending; C001 authoritative staging contract is incomplete |
| C003 | closed | `plans/implementation/human-execution-corrective/003-terminal-keyboard-and-vt-fidelity.md` | `plans/closure/human-execution-corrective/003-status.md` |
| C004 | ready | `plans/implementation/human-execution-corrective/004-authoritative-human-shell-promotion-staging.md` | Corrective follow-up for C001; C002 remains blocked until C004 closes and C001 is accepted |
