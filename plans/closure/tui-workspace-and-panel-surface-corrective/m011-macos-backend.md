# M011 macOS containment backend — decision and evidence

Date: 2026-11-27. Branch `feat/auto-20261126-cae46b00`. Owner decision: qualify a
macOS containment backend for managed checkout creation rather than ship M011 as
Linux-only (answering the platform decision in
`remaining-work-review.md`).

This record covers the backend qualification and the live path. It is strict for
a remote-backed repository; a local-only repository still cannot launch, for the
separate identity reason recorded below.

## What changed

- `WorktreeLaunchTiers::for_host()` selects the Linux production tiers on Linux
  and the filesystem grant elsewhere; the request follows the host instead of
  being hardcoded to `PRODUCTION`.
- `build_worktree_grant` requests the protected mount view only on Linux. On
  macOS the helper selects Seatbelt, which denies writes and re-allows the same
  three roots.
- `assert_grant` refuses a Linux-only flag in a filesystem-only launch, so the
  non-Linux shape cannot smuggle an unenforceable request.
- The post-launch network assertion is gated on the requested tier. It used to
  demand the M005 boundary unconditionally, so no macOS launch could pass.
- Seatbelt reports the **obtained** writable roots in the same structured form
  as the mount view (`enforcement_for`), so `obtained_write_root_is_present` is
  not skipped on macOS. Device nodes are excluded from the report.
- ADR-0048 gained a 2026-11-27 amendment naming what macOS does **not** obtain.
- `architecture/git.md` records the per-host enforcement; the stale macOS
  exclusion in `architecture/testing.md` is removed.

Frontend ordering fixes landed while driving the live path (M007 conformance,
not part of the backend):

- `open_workspace_dashboard` selects the chat panel but blurs it, so the view
  owns the keyboard on entry.
- The Workspace route is consulted before the focused side panel, as M007
  states; `Shift+L`/`Shift+H` move between the tree and the pane.

## Evidence

Run on this host (macOS 26.6.2, arm64):

- `cargo test -p codegg --test contained_worktree_c008 -- a_managed_checkout_is_created_under_seatbelt`
  — **1 passed**. The production creator (daemon-pinned Git, the same grant, the
  private helper, Seatbelt) creates a real linked worktree; `.git` is a pointer
  file, `git worktree list --porcelain` names the checkout and
  `branch refs/heads/codegg/worktree/macos-c008`, and the outcome names
  `seatbelt` as the backend.
- `cargo test -p codegg --test security_permission -- seatbelt_confines_the_worktree_grant`
  — **1 passed**. A child under the worktree grant writes inside the checkout and
  is refused outside it; the frame names Seatbelt and reports exactly the three
  obtained roots.
- `the_grant_names_only_the_shared_administration_root` and
  `the_grant_writes_exactly_one_path_under_the_git_directory` now run on macOS —
  **2 passed**.
- `cargo test -p codegg-tui --lib` — **3,329 passed, 0 failed, 1 ignored**.
  Includes the new `n_opens_the_launch_sheet_for_the_selected_workspace`
  regression.
- Live, on this head: `/workspace` → `n` → `j` → `n` opens the launch sheet at
  `base:main`; Enter through the three steps creates
  `data/codegg/worktrees/00b21697.../wt-5598dd39eaea` on
  `codegg/worktree/5598dd39eaea` and opens the session bound to it
  (`m011-launch-sheet-macos.svg`, `m011-checkout-macos.svg`).
- `scripts/verify.sh quick` — **passed**;
  `check_contained_worktree_grant.py`, `check_contained_worktree_hooks_path.py`
  and the comment budget pass.

## Limits this qualification reports (not claims)

From ADR-0048's amendment:

- No process/IPC scope. The child can signal and observe other same-uid
  processes.
- No network isolation. The backend's existing evidence records loopback as
  reachable; a hostile smudge filter is write-confined but not offline.
- No read confinement. Seatbelt allows reads by default.
- No `no_new_privs` equivalent.
- No hiding of a nested `.git` inside a writable root; the pointer post-check
  remains the gate.

The Linux production tier's claim is unchanged and remains the stricter one.

## The live path, and what blocked it

The sheet was reachable once the keyboard reached the route. Two things hid it.
The focused side panel was consulted before the route and consumed `n`, `d` and
`Space`; the route now runs first and the view blurs the chat panel on entry.
The sheet's fields are rendered in the **footer**, so a screen dump that stops
at the tree shows no change — the first attempts were read from the wrong rows.
With both corrected, the whole flow is driven in the evidence above.

**Local-only repository identity remains.** A project registered from a bare
`git init` has no repository identity and `SessionLaunch` refuses with
`project_context_required`. The live receipt uses a remote-backed fixture; that
decision is unchanged by this work.

Not verified: `scripts/verify.sh full`.

## Files

- `plans/adrs/ADR-0048-contained-git-execution-for-managed-worktree-creation.md`
- `src/git_contained_worktree.rs`
- `crates/codegg-sandbox/src/seatbelt.rs`
- `crates/codegg-sandbox-helper/src/main.rs`
- `crates/codegg-tui/src/app/mod.rs`
- `crates/codegg-tui/src/commands/workspace_dashboard.rs`
- `crates/codegg-tui/src/commands/workspace_view.rs`
- `crates/codegg-tui/src/commands/interactive_terminal.rs`
- `tests/contained_worktree_c008.rs`
- `tests/security_permission/sandbox_containment.inc`
- `architecture/git.md`, `architecture/testing.md`, `docs/comment-budget.toml`
