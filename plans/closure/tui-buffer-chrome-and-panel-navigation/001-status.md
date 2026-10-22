# TUI Buffer Chrome, Exit Protocol, and Files Panel Navigation M001 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-buffer-chrome-and-panel-navigation-roadmap.md` (M001)

Repository baseline reviewed: `a588a264` (branch
`feat/auto-20261020-513a8ce6`)

Research artifact: `research/tui-files-panel-depth-and-input-chrome-2026-10-21.md`

Applicable ADRs (unchanged by this milestone): none. This milestone adds no
protocol, storage, migration, authorization, or dependency change.

## 1. What landed

**One motion for "open this row".** `l` and `Enter` are the same verb in the
`/files` box, and the Ctrl+T tree pane already agreed. `Enter` on a directory
emits `TuiMsg::FilesDescend` instead of calling the retired
`FileTreeState::toggle_selected`; `l` on a **file** asks the command layer to
open it, because a file has nothing to enter and a dead key is a dead key.

The state layer grew one variant to say so: `PeekStep::Open { path }`. The
command layer routes it to `set_pending_editor_handoff` + `open_editor` — the
same path `Enter` took for a file, so there is one open path and not two.

**The scope slide lands on the grandparent.** The listing window is three
levels (`MAX_LISTING_DEPTH = 3`), so a directory at the window's edge is a
leaf in the listing and cannot show its children. The slide now moves the
scope to the target's **grandparent**:

```rust
scope: parent_path(&parent_path(&row.path)),
```

The grandparent is the *shallowest* scope in which the target's children fit
back inside the window, so the user keeps the widest context: the target's
parent stays visible beside the target's siblings. The user's own example is
the arithmetic: the workspace root is `~/projects/codegg`, the target is
`~/projects/codegg/crates/codegg-client/src`, and the new visual working
directory is `~/projects/codegg/crates`.

The workspace root never changes. The scope travels as the listing request's
`prefix`, so the daemon re-resolves the canonical root on every request and
the panel can never escape the workspace.

**The slide opens the chain down to the directory.** A slide replaces the
listing, and in the new scope the target's *ancestors* are the rows — they
arrive collapsed. Expanding only the target therefore leaves the directory
the user pressed `l` on off screen: the pane has changed scope and shown
nothing new, which is the same "empty directory" symptom the slide was built
to remove. `FileTreeState::expand_ancestors_of` opens the chain between the
new scope and the directory, and the directory is expanded with it.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| `Enter` on a directory enters it instead of toggling it | met | `src/tui/components/dialogs/files.rs` — `activate` emits `TuiMsg::FilesDescend` |
| `l` on a file opens the document | met | `PeekStep::Open` → `descend_to` → `open_editor` |
| the slide lands on the grandparent | met | `src/tui/app/state/file_tree.rs` — `peek_into` |
| the slid-to directory is on screen | met | `FileTreeState::expand_ancestors_of` |
| the scope moves one level **inward**, and never out of the workspace | met | the target's grandparent is always a child of the current scope, and the scope travels as the listing request's prefix |

## 3. What was verified

On the reconciled head (`45ffe2acb`, merged with `origin/main` at `2c1953347`):

- **Hosted CI run `39669710537` — the `verify` job passed** (21m4s): `cargo fmt
  --check --all`, `cargo clippy --workspace --all-targets --locked -- -D
  warnings`, and the workspace test suite. The Linux jobs
  (`btrfs-snapshot`, `native-probe-contract`, `protected-mount`,
  `restricted-userns-fails-closed`) passed too.
- `scripts/verify.sh quick` — **"Quick verification passed."**, all twelve TUI
  guards included.
- `cargo fmt --all -- --check` — clean.
- `cargo nextest run -E 'binary(codegg) and (test(file_tree) or test(panels::tests))'`
  — 139/139 green, including the reworked worked example and the new
  ancestor-expansion cases.
- `cargo nextest run --workspace --locked --profile ci --no-fail-fast` —
  **15,769 tests run, 15,763 passed, 5 failed, 1 timed out, 8 skipped**, every
  test this milestone touched green. The six non-passing are not this
  milestone's, and each is in a file this branch does not touch: the two
  `core::daemon_refresh` projection tests fail with `InvalidRootPath("/tmp")`
  on the untouched base commit too, the 300 s
  `agent::asset_refresh` coalescing wait becomes a timeout under load, the
  `managed_process` OfflineRequired refusal is macOS-only, and the two
  `scratch_cache_root_contract` tests are main's new ones resolving the
  scratch root under the tempdir's `HOME`.
- The twelve TUI guards (`check_tui_editor_*`,
  `check_tui_composer_key_authority`, `check_tui_transport_authority`,
  `check_daemon_listing_authority`) — green.

On this macOS host the merged tree reports eight dead-code errors under
`cargo clippy --workspace --all-targets --locked -- -D warnings`, all in
main's `#[cfg(target_os = "linux")]` code (`src/lsp/m004.rs`,
`src/managed_process.rs`, `src/security/sandbox.rs`,
`src/interactive_process.rs`): it compiles on macOS but nothing calls it.
Linux CI compiles the same files with their call sites present and is green.
Those files are byte-identical between this head and `origin/main`, so the
warnings are main's, inherited through the reconcile.

## 4. Recorded deviations

- The implementation's slide target was the target's **parent**; the requested
  rule is the **grandparent** (roadmap §2, D1). The change is recorded rather
  than absorbed: the parent's rule also fits the children inside the window,
  but it discards a level of context for no navigational gain.
