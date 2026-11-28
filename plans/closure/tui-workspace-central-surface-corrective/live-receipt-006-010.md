# Live receipt — M006–M010 on the delivered binary

Captured 2026-11-27 on `feat/auto-20261126-cae46b00` at `256b215ac` plus the uncommitted M006–M010
change set, driving `target/debug/codegg` built from this tree through `tui-test` 0.1.1 in a
150x45 terminal. Each frame below is a stored screenshot in `receipts/`; each quoted line is the
terminal's own cells from that frame.

## The isolated identity

No operator session store, provider credential, or running daemon is read or written:

```bash
ROOT=/tmp/codegg-receipt.LIVE
HOME=$ROOT/home CODEGG_DATA_HOME=$ROOT/data CODEGG_DAEMON_HOME=$ROOT/daemon \
  CODEGG_CORE_ENDPOINT=$ROOT/daemon/core.sock
./target/debug/codegg daemon start          # core.sock ready in ~0.2 s
# then one tui-test session, --cwd at the repository root, same env
```

`git status --porcelain` reported 41 entries immediately before the run and 41 after it: the
receipt mutates nothing in the repository.

## The two tui-test traps that hid this receipt for a while

Both are properties of the harness and of pre-existing interaction, not of the corrected code, and
both are recorded so the next receipt does not rediscover them.

1. **`tui-test run <binary>` never reaches the alternate screen here.** The session stays a
   canonical-mode tty (`ready: false`, `modes.alternate_screen: false`, `cwd: null`), typed keys
   merely echo, and `submit` writes the text plus a newline into a screen nobody is drawing. The
   shape that works is the shell route: `tui-test open --cwd <repo> --env …`, then
   `submit "exec <absolute binary>"`. That session reports `alternate_screen: true` and shows the
   app's own frame. The `--env KEY=VALUE` flags are the documented spelling; the `env VAR=… binary`
   prefix form is what fails.
2. **A submitted `/command` needs a second Return once its completion list is open**, and **no
   Return submits while the startup dialog is mounted**. The first Return accepts the completion;
   command text therefore sits in the composer until a second one arrives. Four capture attempts
   failed on this before it was understood.

## What each milestone looks like live

| Milestone | Frame | The line the terminal shows |
| --- | --- | --- |
| **M006** route vocabulary | `receipts/m006-route-surface.png` | header `WORKSPACE  NOR · tree  j/k move · Enter reveal · i filter · Shift+H input · Shift+L chat · Shift+X exit`; footer `a add · e edit · s archive/restore · x destroy · t team · n new session · i filter · Shift+H input · Shift+L chat · Shift+X exit`; tree row `>▸ feat-auto-20261126-cae46b00  idle`; dock `#general 0/0`, `no messages — press i to write`, `Shift+→ focus · ⇧S switch` |
| **M006** a capital is a tree claim | `receipts/m006-composer-focus.png` | after `Shift+H` the header reads `NOR · composer`, the badge becomes `[INS][CHAT]  ❯`, and the placeholder is `Message the project chat…`; typing gives `Hello` |
| **M006** a capital is text in the composer | (same session) | `Shift+X` typed into that composer yields `HelloX` and the route stays open — the letter lands in the buffer instead of leaving the route |
| **M006** the route exit | (same session) | `Shift+X` from the tree leaves the route; the session view returns with the notice `Workspace — j/k move, Enter reveal, n new session, i filter, Shift+X leaves` |
| **M007** the filter is its own mode | `receipts/m007-filter.png` | `i` gives `WORKSPACE  INS · filter  type to filter · Enter reveal · Esc back` |
| **M008** the route needs no session | `receipts/m006-route-surface.png` | the route paints its tree and dock with no session bound (`Session  no session` in the start frame), and **no** `run_store_unavailable` appears on this route |
| **M009** archive first, verbatim | `receipts/m009-archive-first-notice.png` | `x` on the active row: `press s to archive feat-auto-20261126-cae46b00 first; destroy it from the project picker (Tab shows archived)` |
| **M010** the selector refuses a miss | `receipts/m010-selector-unknown.png` | `/worktree no-such-target`: `No workspace or worktree matched /worktree no-such-target` |
| **M010** the selector resolves a hit | `receipts/m010-selector-resolved.png` | `/worktree feat-auto-20261126-cae46b00`: `Workspace: feat-auto-20261126-cae46b00 (77fe16a7-15e0-497e-bebd-4c9334084985)` over a `worktrees` section — the route, not a toast and not a dialog, which is the retired `WorktreeList` path staying retired |

## Three things that look like defects and are not

**The startup `ERROR` dialog.** The session view opens with `Runtime policy: Policy preference read
failed (run_store_unavailable): the command's workspace could not be resolved, so its run cannot be
recorded`. It is the session view's run store refusing in an isolated data root;
`REFUSAL_RUN_STORE_UNAVAILABLE` is `src/core/daemon_commands.rs:49`, and the identical message is
already recorded against this environment in
`plans/closure/tui-workspace-and-panel-surface-corrective/m009-m010-receipt.md:22`. M006–M010 touch
no part of that path, and the route itself paints clean, so it is neither a regression nor M008's
finding F3 (which was about the route's scope and is fixed).

**The `INFO` panel after leaving the route** is the leave-path toast, not a dialog that outlived the
route: `crate::commands::workspace_dashboard.rs:156` raises
`toasts.info("Workspace — j/k move, Enter reveal, n new session, i filter, Shift+X leaves")` under
`announce`. It is the designed way back.

**A route-composer draft surviving the exit** (`HelloX` visible in the session composer after
`Shift+X`) is the single shared composer being restored, which is what M001's
`workspace_focus_routes_composition_once_and_restores_the_session_mode` pins. Neither M006 nor M007
claims the draft is cleared on exit.

## What this receipt does not cover

- **M007's selection-following dock and its rebind** need two projects; the isolated catalog holds
  one. The filter mode, its vocabulary and its keys are covered here; the rebind rests on
  `moving_to_an_uncached_project_asks_for_its_history` and `filtering_to_another_project_rebinds_the_chat`.
- **M008's team path** (`t`, the member list) was not driven live; the frame above establishes only
  that the route paints with no session.
- **M009's launch identity** (`n`, `Shift+F`, refusal rotation) needs a provider to run a turn; the
  refusal notice is the part that is live here, the request-id contract is test-proven.
- **M010's launch** (`Shift+F` from a selected worktree) is likewise test-proven only.
- The unfiltered `tui_render` family and `verify.sh quick` remain the authority for everything above
  the pixel level; this receipt adds the surface a user actually presses keys against.
