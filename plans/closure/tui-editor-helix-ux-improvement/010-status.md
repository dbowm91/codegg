# TUI Editor & Files Panel Helix UX Improvement M010 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md` (M010)

Repository baseline reviewed: `a588a264` (branch `feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none. This milestone adds no
protocol, storage, migration, authorization, or dependency change, and it
reopens no closed milestone of `tui-editor-helix-parity-roadmap.md`.

## 1. What landed

`CHROME_ROWS` is 1. The block's top border carries **no label** — it
used to carry `{path} · {mode} · {status}`, and all three moved: the path and
the status to the single bottom row, the mode to the badge above the input
box. The bottom row has a **left slot** (the file's path, or the modal `:`/`
/` line when one is open) and a **right slot** (the status cluster). The line
number and the column are gone: they restated the caret, which is already on
screen.

The cluster reads as one story, left to right: the **persistent signals**
(the modified state, the scope badge, writable or read-only, the LSP
semantic-coverage note, the "line too long" marker), then the **transient
disclosure** (the find note, the search count, the history position), then
the **notice**, then the **armed prefix**. The armed prefix is **last**, at
the far right: it is the one element that appears and disappears on its own,
so it gets a home nothing else competes for.

The two slots **never overlap**, and the cluster is **fitted by rank**
(`ClusterWeight`, `fit_cluster`): the least important group is dropped first,
and a group that is still too wide on its own is cut from the **right** so
the head of the sentence survives. A caveat outranks everything except the
notice and the prefix, because dropping a caveat is not a smaller truth but a
**misleading** one.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| the editor block reserves exactly one chrome row | met | `CHROME_ROWS = 1` in `src/tui/components/editor.rs` |
| the title is empty | met | `the_title_carries_nothing` |
| the path is on the left of the bottom row | met | `render_chrome`'s left slot |
| the cluster is on the right | met | `render_chrome`'s right slot |
| the line number and column are gone | met | the readouts were removed from the chrome |

## 3. Residual risk and open items

**The research's single-slot design was changed, and the change is
recorded** (`research/…` §0.0 item 4): a priority switch would make the
scope badge disappear every time a prefix is armed, and the scope badge is
the one thing on screen that says whether `:wq` reaches this project or
every project on the machine. A safety signal cannot flicker.

The armed prefix is a **styled span** (primary + bold), not a letter: the
scope badge (`[project]`) and the writable marker carry the same letters as
the `t`, `f`, `m`, and `[` indicators, so a test that searched the row's
*text* for the indicator would find the badge instead. The indicator is read
out of the frame's **styles**.
