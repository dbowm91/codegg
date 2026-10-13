# TUI Editor Post-Closure Corrective C005 — Closure Status

Status: blocked

Source implementation plan:

- `plans/implementation/tui-editor-correctness-usability/007-daemon-scoped-directory-listing-corrective.md`

Source subsystem roadmap:

- `plans/subsystems/tui-editor-correctness-usability-post-closure-corrective-addendum.md#C005`

Repository baseline reviewed: `b9400c98d013030ebb01414d3fd63a5b078c299d`
(C004 single-flight admission head; branch
`plans/tui-editor-correctness-usability-20261013`).

Implementation commits or pull requests:

- none — blocked before any production change per the plan's stop
  conditions. No native protocol, daemon handler, TUI rewiring, or fallback
  was implemented.

## 1. Executive finding

C005 is not implemented and remains blocked on its hard gate: proposed
`plans/adrs/ADR-0022-daemon-authorized-workspace-directory-listing.md` is
still **proposed** (recommended Option B, "no implementation authority until
explicitly accepted"). The plan's §10 stop conditions forbid implementing a
new `WorkspaceDirectoryList` native contract, authorization mapping, or
client-local fallback before that acceptance. This record exists only to
formalize the blocked disposition, preserve the gate, and audit unblocking
after C004's closure. It authorizes no merge and changes no behavior.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| ADR-0022 accepted with frozen DTO/auth/ownership/failure/compat semantics (AC1, plan §4A) | `plans/adrs/ADR-0022-daemon-authorized-workspace-directory-listing.md:3` reads `Status: proposed — recommended Option B; no implementation authority until explicitly accepted` | blocked | Hard gate unsatisfied; no maintainer acceptance observed at `b9400c98` |
| Same-path/different-filesystem decoy shows only daemon entries; no client-host read (AC2) | Not run — blocked before code | blocked | Requires the daemon read that does not exist |
| `ViaSession + FileRead` scoping; traversal/observer denial before I/O (AC3) | Not run — blocked before code | blocked | No new authorization matrix entry was added |
| Old-daemon/denial fails closed with no local fallback (AC4) | Not run — blocked before code | blocked | Current `local_walk_root` refusal (C002, preserved by C004) remains the fail-closed behavior; no fallback was added or removed |
| Bounded depth/node/examined/name/bytes + daemon-side worker admission measured (AC5) | Not run — blocked before code | blocked | C004's local single-flight admission is closed and reusable, but no daemon-side worker exists to qualify |
| `/files` + Ctrl+T parity with authorized open path (AC6) | Not run — blocked before code | blocked | Current navigator still local-only; `DocumentOpen` still independently reauthorizes each file |
| Local/simulated-remote transports, reconnect, cancel, concurrent refresh + docs/guards + hosted CI (AC7) | Not run — blocked before code | blocked | No new transport fixture was created |

## 3. Production implementation evidence

None. Deliberately zero diff for C005:

- No `CoreRequest::WorkspaceDirectoryList` variant, DTO, or `codegg-client`
  accessor was added.
- No daemon read handler, authorization matrix entry, or canonical-root
  containment was added.
- `src/tui/commands/file_tree.rs` still calls the local
  `build_tree_with_cancel` through the C004 single-flight coordinator; no
  daemon-origin branch exists.
- No static guard for "no direct TUI `read_dir`" beyond the existing
  locality refusal was added (that guard belongs to the implementation pass,
  not to a blocked record).
- `local_walk_root` fail-closed semantics and the C004 `Waiting`/admission
  behavior are unchanged by this record.

The C004 implementation (`b9400c98`) is the only production delta on this
branch since `f465536bc`, and it is separately closed at
`plans/closure/tui-editor-correctness-usability-post-closure-corrective/004-status.md`.

## 4. Verification executed

### Commands run

```bash
# Gate check only; no implementation verification was run because no
# implementation exists. These reads establish the blocker:
grep -n "^Status:" plans/adrs/ADR-0022-daemon-authorized-workspace-directory-listing.md
git rev-parse HEAD
```

### Results

- ADR-0022 status line: `proposed` (not `accepted`). Gate unsatisfied.
- Baseline `b9400c98` confirmed as the C004-closed head.
- No focused, integration, security, compatibility, guard, fmt, clippy, or
  hosted-CI verification is claimed for C005. Listing the plan's §8 command
  block here as "passed" would be fabrication; it was not executed because
  the stop condition fired first.

## 5. Invariant review

- No second text authority: unchanged (no code).
- No new protocol/authority invented to finish a checklist: upheld by
  refusing to implement.
- Local-only remote limitation stays fail-closed: `local_walk_root` refusal
  plus C004 admission remain the enforced behavior; no silent fallback was
  added.
- C004 admission invariant (one active + one pending, `Waiting`, generation
  fencing) is closed and available for future daemon-side reuse, but no
  daemon-side worker exists yet.

## 6. Failure and recovery review

Not applicable beyond the gate: there is no new read to duplicate, cancel,
restart, or contend. The existing local failure semantics (C002/C004:
cancelled/superseded truncation, stale discard, shutdown not-joined policy,
explicit error vs empty) remain in force and are not re-qualified here.

## 7. Migration and compatibility review

No migration, no additive DTO, no old-daemon `Unsupported` path, no durable
state. Nothing to roll back. If ADR-0022 is later accepted, C005 activates
with a fresh baseline per the addendum's handoff order; if rejected, the
explicit local-only fail-closed alternative must be recorded and C005
retained blocked/superseded per governance — neither decision is made here.

## 8. Security review

No new network-visible metadata surface was created, so no new
authorization, containment, symlink/TOCTOU, non-enumerability, or redaction
evidence is owed. The same-path/different-host confusion identified in the
plan remains an accepted limitation of the local-only tree until the ADR is
decided; it is prominently refused (not silently listed) by
`local_walk_root`, and selected files still reauthorize via `DocumentOpen`.

## 9. Documentation and operations

No architecture or user docs were changed for C005. `architecture/tui.md`
and `docs/tui.md` as updated by C004 continue to state that remote directory
listing is unsupported and only a readable local workspace is listed. No new
operator diagnostics, version-skew notes, or file-watching disclaimers were
added because the feature does not exist.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | ADR-0022 still proposed; C005 hard gate unsatisfied | Daemon-authorized listing cannot begin; remote TUI file-tree authority stays local-only fail-closed | Maintainer decision on ADR-0022 (accept B with frozen contract, accept another fully specified mechanism, or explicitly defer/reject with a recorded local-only alternative) |
| low | C004 soft integration now closed but unexercised daemon-side | Future daemon worker can reuse the single-flight pattern, but no daemon worker exists to prove it | On ADR acceptance, port or retire the coordinator cleanly during C005 per plan §5E; no temporary framework was built |

No critical finding (nothing unsafe was merged). The high finding is the
registered blocker itself, not a defect in landed code.

## 11. Roadmap disposition

Milestone C005 blocked and reason retained: proposed ADR-0022 acceptance
required. C004's closure satisfies the soft interface-integration dependency
(a correctly bounded local worker now exists for future daemon-side reuse),
but the hard ADR gate is unchanged, so C005 does not move to ready. C003
remains ready independent of both. Corrective plan required: none beyond the
pending ADR decision. Subsystem roadmap must not mark daemon listing complete
on the strength of C004.

## 12. Registry updates

- `plans/implementation/tui-editor-correctness-usability/007-daemon-scoped-directory-listing-corrective.md`:
  stays `blocked`; no implementation SHA.
- `plans/subsystems/tui-editor-correctness-usability-post-closure-corrective-addendum.md`
  C005 row: stays `blocked` on ADR-0022; notes C004 soft dependency now
  closed at `b9400c98`.
- `plans/registry.md`: C005 stays `blocked` with the same hard gate; unblock
  audit recorded (C004 closed, but C005 hard gate is ADR acceptance, not
  C004; C003 already ready; nothing unblocked).
