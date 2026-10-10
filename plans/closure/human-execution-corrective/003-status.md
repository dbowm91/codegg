# Human Execution Corrective C003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/human-execution-corrective/003-terminal-keyboard-and-vt-fidelity.md`

Source subsystem roadmap:

- `plans/subsystems/human-execution-corrective-roadmap.md#c003--pty-keyboard-and-vt-fidelity`

Repository baseline reviewed: `2f026f05220f60c62bfc381af3d7479172dfdb50`

Implementation commits or pull requests:

- `7e1892a13755b6f6a04e9c7012cf5db1dcaddf4f` — terminal keyboard/VT implementation and closure evidence.

## 1. Executive finding

C003's TUI-side terminal interaction gap is closed. Focused PTY input now forwards literal Escape and uses Ctrl-] to leave focus. The frontend interprets output through a bounded incremental VT screen, handles resize and screen-history gaps explicitly, and prevents child escape sequences from reaching the host terminal. The daemon-owned PTY lifecycle remains unchanged.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Explicit focus exit and literal Escape | `src/tui/interactive_terminal.rs` unit tests; `tests/interactive_terminal_tui.rs` raw-mode PTY fixture | pass | Esc reaches the child; Ctrl-] exits focus. |
| Incremental VT interpretation | `cargo test -p codegg --lib interactive_terminal` | pass | 32 focused tests cover split control sequences, cursor/SGR, alternate screen, OSC filtering, bounds and history gaps. |
| Real PTY lifecycle/input | `cargo test --test interactive_terminal_tui` | pass | 6 Unix PTY integration tests, including Esc delivery and resize/lifecycle behavior. |
| Bounded screen and host-terminal isolation | VT parser unit tests and source review | pass | Parser dimensions and retained state are capped; OSC payload is discarded. |
| Process ownership unchanged | `python3 scripts/check_execution_ownership.py` | pass | Existing process ownership guard passes. |
| Repository quick verification | `scripts/verify.sh quick` | pass | Canonical quick verification completed successfully, including locked workspace/all-targets check. |

## 3. Production implementation evidence

`src/tui/interactive_terminal.rs` maintains the bounded VT screen state. `src/tui/commands/interactive_terminal.rs` sizes the PTY viewport from the dialog and sends resize updates. Help and dialog rendering describe focus behavior and render interpreted cells. `tests/interactive_terminal_tui.rs` uses a raw-mode child so the Escape test verifies byte forwarding rather than shell line discipline.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg --lib interactive_terminal
cargo test --test interactive_terminal_tui
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
```

### Results

- Focused VT/controller suite: 32 passed.
- Unix PTY integration suite: 6 passed.
- Execution ownership guard: passed.
- `scripts/verify.sh quick`: passed.

## 5. Invariant review

- PTY process and attachment ownership remain in the existing daemon services; only frontend presentation changed.
- Input is sent only while an authorized terminal is focused. Ctrl-] is the documented focus exit; Escape is forwarded.
- Child VT sequences are parsed into bounded screen state and are never written directly to the host terminal.
- Raw PTY output remains separate from session/model context.
- Sequence gaps reset the parser and surface degraded state rather than claiming a faithful screen.

## 6. Failure and recovery review

VT dimensions and parser state are bounded. A retained-output gap resets screen state with an explicit notice. Escape sequences such as OSC are not passed to the host. The existing PTY detach/reattach and daemon restart lifecycle was not changed; this milestone adds no persistent state or process identity.

## 7. Migration and compatibility review

No schema or protocol migration was added. Existing PTY byte input and resize operations are reused. The new `vt100` dependency is pinned to `0.16.2`; its declared MSRV and license were reviewed during implementation. Non-Unix remains unsupported as before.

## 8. Security review

The parser is bounded, terminal control sequences are interpreted in the presentation layer, and OSC payloads are discarded. No shell execution authority or model-context path was added.

## 9. Documentation and operations

Updated `architecture/tui.md`, `architecture/process-tool-execution-ownership.md`, `docs/tui.md`, `.opencode/skills/tui/SKILL.md`, `.opencode/skills/human-shell/SKILL.md`, and terminal help/shortcuts.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | VT fidelity is limited to the selected parser's supported behavior; no platform-specific macOS or Windows run was performed in this Linux environment. | Unqualified edge behavior may vary by terminal application/platform. | Keep documented support truthful; add platform evidence when available. |

## 11. Roadmap disposition

Milestone closed; C003 is independent of C001/C002 and may be considered complete. C001 remains active and C002 remains blocked on C001.

## 12. Registry updates

Mark C003 closed in `plans/registry.md` and the source subsystem roadmap. Preserve historical PTY M001–M003 closure unchanged.
