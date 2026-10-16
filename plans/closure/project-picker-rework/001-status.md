# Project Picker Rework Milestone 001 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/project-picker-rework/001-remove-ctrl-backslash-opener.md`
Source subsystem roadmap: `plans/subsystems/project-picker-rework-roadmap.md#7-milestones`
Repository baseline reviewed: `292e5f5b`
Implementation commits: `292e5f5b` — removed the Ctrl+\ project-picker binding and updated key help and documentation

## 1. Executive finding

M001 is strictly closed. The default and vim keymaps no longer bind
Ctrl+\ to the project picker. Shift+F remains the Normal-mode opener,
the slash command remains available mid-prompt, and vim Normal-mode
backslash remains unchanged. Focused input/command/modifier suites and
the repository quick verification passed locally.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Finding |
|---|---|---|
| Ctrl+\ does not open the picker in Insert or Normal mode | `tui::input::` test suite: 44 passed; explicit negative assertions for both modes | Satisfied |
| Shift+F opens in Normal mode and types in Insert mode | Same input suite, including both terminal encodings | Satisfied |
| vim Normal backslash opener remains available | Existing input behavior retained; `modifier_chords` suite: 16 passed | Satisfied |
| `/projects` remains registered and described without the retired chord | `tui::command::` suite: 25 passed | Satisfied |
| No user-facing help or documentation advertises Ctrl+\ | Search across `src/tui`, `docs`, and `architecture` found only negative test messages | Satisfied |
| Repository sanity gate | `scripts/verify.sh quick`: passed | Satisfied |

## 3. Production implementation evidence

- Removed the default-map and vim-map Ctrl+\ entries.
- Removed the Insert and Normal help rows for that chord.
- Updated Shift+F and `/projects` descriptions, `docs/tui.md`, and
  `architecture/command.md`.
- Retained a negative regression assertion in both input modes.

## 4. Verification executed

All commands below ran locally on the implementation revision. The
environment’s default pkg-config search selected arm64 MacPorts native
libraries while the active Rust target was x86_64; focused test commands
were rerun successfully with `PKG_CONFIG_LIBDIR=/usr/local/lib/pkgconfig`
and `CARGO_BUILD_JOBS=2`.

- `cargo fmt --check` — passed.
- `cargo test -p codegg --lib tui::input::` — 44 passed.
- `cargo test -p codegg --lib tui::command::` — 25 passed.
- `cargo test -p codegg --test modifier_chords` — 16 passed.
- `scripts/verify.sh quick` — passed, including workspace check.
- Repository search for Ctrl+\ in `src/tui`, `docs`, and
  `architecture` — no user-facing references remain.
- Hosted CI was not run.
- Interactive reinstall/daemon-restart verification was not run. The
  key-dispatch and command-registration behaviors were verified by the
  focused local suites.

## 5. Invariant review

Shift+F remains Normal-only and text in Insert mode. Custom binding
precedence and the shared Insert-mode handling were not changed.
The vim backslash and slash-command routes remain intact.

## 6. Failure and recovery review

The retired chord is inert with respect to picker dispatch in both
modes. This milestone adds no asynchronous or durable state and has no
recovery path.

## 7. Migration and compatibility review

No configuration migration is needed. User-defined bindings are
unaffected; only the built-in binding was removed.

## 8. Security review

No authority, filesystem, protocol, or storage behavior changed.

## 9. Documentation and operations

`docs/tui.md` and `architecture/command.md` now describe Shift+F and
`/projects` as the picker routes. The slash route remains available
while composing.

## 10. Unresolved findings

None. Hosted CI and an interactive reinstall/restart check were not
available in this run; local acceptance suites and quick verification
passed.

## 11. Roadmap disposition

M001 is closed. The project-picker roadmap remains active. M002 is
eligible for handoff now that its hard dependency is closed; M003 and
M004 still require M002 interface stability, and M005 still requires
M2–M4 closure.

## 12. Registry updates

- M001 moved to closed and is recorded in recently closed work.
- M002 moved from blocked to ready in this commit.
- M003, M004, and M005 remain blocked because their remaining
  dependencies are not satisfied.
