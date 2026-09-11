# Multi-Project TUI Frontend Convergence M008 — Closure Record

Status: closed

Implementation plan: `plans/implementation/tui-project-sessions/008-app-domain-decomposition-intent-effect-boundaries.md`

Roadmap: `plans/subsystems/tui-project-sessions-frontend-convergence-corrective-addendum.md`

## Result

M008 is complete. The TUI `App` remains the single composition root, while its
largest responsibility clusters now live in source modules with explicit
ownership boundaries:

- `commands.rs` owns `TuiCommand`, session mutation operations, and the typed
  command send adapter;
- `input.rs` owns synchronous component/user-intent processing;
- `render.rs` owns cached presentation and rendering helpers;
- `modal.rs` owns dialog opening and focus-facing modal lifecycle;
- `project_session.rs` owns active-project/session/projection lifecycle;
- `prompt_turn.rs` owns route-safe prompt and turn initiation;
- `plugin_ui.rs` owns validated plugin effect application.

The existing runtime command dispatcher remains the one exhaustive runtime
entry point. Its domain handlers and scheduler/effect boundaries were not
duplicated or replaced.

## Implementation evidence

- Implementation commit: `73a5a3e` (`refactor(tui): decompose app domain responsibilities`)
- `src/tui/app/mod.rs`: 17,656 lines at the plan census point reduced to 13,643;
  the moved bodies are now in seven responsibility-oriented modules plus the
  command module.
- `TuiMsg` remains the synchronous component/user-intent channel. It can cause
  the `App` to enqueue a typed `TuiCommand`, but it is not an async completion
  channel.
- `TuiCommand` remains the runtime request/completion boundary. Runtime
  completions are handled by the command path and do not re-enter
  `process_msg` as if they were component intent.
- Rendering performs no I/O, command dispatch, or async work; it consumes the
  current cached `App` state.
- No `TuiMsg` or `TuiCommand` variants were removed. The remaining similarly
  named values represent distinct synchronous intent versus runtime request or
  completion semantics, so retaining them is intentional rather than an
  accidental duplicate representation.
- `architecture/tui.md` and `.opencode/skills/tui/SKILL.md` document the module
  layout and message-direction contract.

## Verification

Passed:

- `cargo fmt --all`
- `cargo check --tests`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `scripts/verify.sh quick`
- `python3 scripts/check_tui_project_authority.py`
- `python3 scripts/check_daemon_cwd_usage.py`
- `python3 scripts/check_scheduler_bypass.py`
- `cargo check -p codegg --lib`

The focused TUI test targets compiled through Rust code generation but could
not link on this host. `cargo test --test tui --no-run`,
`cargo test --test tui_render --no-run`, `cargo test --test tui_project_tabs
--no-run`, `cargo test --test tui_project_routing --no-run`, and the focused
`tui::app` library test command all stop at the same host configuration issue:
the configured target is `x86_64-apple-darwin`, while `/opt/local/lib`
provides arm64 `liblzma` and `libiconv`, leaving the x86_64 linker without
`lzma_*` symbols. This is an environment limitation, not a source or test
failure; the repository's quick verification and all compile/clippy checks
pass.

## Regression and authority review

- M005 active-project/workspace authority is preserved; the extracted project
  lifecycle code continues to use the established execution context and
  active-tab authority.
- M006 route-generation, cancellation, and exactly-once session-submit
  behavior is preserved; prompt/session initiation was moved as a body-only
  source-layout change.
- M007 `FocusManager` ownership and modal behavior are preserved; modal code
  calls the existing owner rather than introducing a second dialog state.
- No daemon, protocol, scheduler, service-bus, state-management framework, or
  alternate routing authority was introduced.
- No new blocked plan was discovered. M009 and M010 were already dependency
  ready before M008 and remain `ready`; M008 does not add a hard dependency to
  either plan. The unrelated architecture M009 operational-evidence blocker
  and runtime-safety C002 supported-Linux evidence blocker remain unchanged.

## Residual follow-up

The host linker mismatch should be resolved in a compatible test environment
before relying on executable TUI integration-test runs. That operational issue
does not reopen M008 or alter the source-level closure decision. Future TUI
capability work proceeds through M009 and then M010 under their existing
plans.
