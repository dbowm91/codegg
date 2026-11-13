# Sandbox Containment Audit Corrective Milestone C006 — Descriptor Hygiene and Grant-Path Pinning

Status: conditionally closed

Repository baseline: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`

Source roadmap:

- `plans/subsystems/sandbox-containment-audit-corrective-roadmap.md#milestone-c006--descriptor-hygiene-and-grant-path-pinning`

Long-term requirements:

- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Applicable ADRs:

- `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`
- `plans/adrs/ADR-0046-composable-linux-execution-isolation.md`

Primary class: invariant

## 1. Objective

Apply descriptor closure and grant-path identity pinning to **every** contained
launch, not only the opt-in tier, and disclose the descriptor channel in the tiers
that still pass it.

## 2. Why this milestone is ready

No hard dependency. Both mechanisms already exist in the repository and are
already proved on one tier each:

- descriptor closure is implemented and exercised on the process-isolation tier;
- grant-path pinning is implemented for the mount view (`openat2` with
  `RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS`, re-verified by `dev`/`ino`).

This milestone generalises each to the tier that lacks it. It does not invent a
mechanism.

## 3. Current implementation evidence

**Descriptor closure is scoped to one tier.**
`managed_process` computes
`close_unapproved_fds = matches!(&sandbox, SandboxRequest::Required(spec) if spec.linux_process_isolation)`
and passes it to `configure_status_fds`, which closes the unapproved descriptors in
a `pre_exec` hook. The parameter is `cfg`-split: on non-Linux it does not exist and
the hook closes nothing. So:

- the protected-process-isolation tier closes unapproved descriptors;
- the protected-mount-view tier does not;
- the default Landlock tier does not;
- macOS does not, ever.

The helper then `exec`s the target, so any descriptor that survived the daemon →
helper hop is available to the target. `std` sets `CLOEXEC` on what it opens and
most tokio/sqlite handles are `CLOEXEC`, which is why this is a conditional gap
rather than a live exploit — but it is exactly the channel the mount view's own
claim excludes.

**The mount view's claim does not mention it.**
`linux_mount_view::enforcement_scope` states "unlisted host paths are absent from
the fresh mount root". That is true of the mount tree and false of the descriptor
table: an inherited descriptor reads and writes a host file the view excludes. The
mount-view limits list discloses the network and pathname-socket channels and does
**not** disclose this one. Landlock's limits do ("open file descriptors inherited
before restriction remain usable"), and Seatbelt's do ("open file descriptors
inherited before the profile apply remain usable"), so the omission is the
mount-view tier's alone.

**The Landlock grant path is re-opened by name after canonicalisation.**
`BackendPolicy::normalized` canonicalises every root, de-duplicates and sorts.
`landlock::apply` then, per root, checks `path.exists()` and calls
`PathFd::new(path)` — which opens **by path**, following symlinks. Between the
canonicalisation pass and that open, a process able to write the parent of a grant
root could replace it with a symlink, and the ruleset would grant the resolved
target. The window is narrow and requires local write access to a grant root's
parent, but the mount view closes exactly this class with `openat2` plus an identity
check, so the asymmetry is a choice rather than a limit.

## 4. Invariants that must not regress

- A constrained request never degrades to host authority.
- The status descriptor (`SANDBOX_STATUS_FD`) and the ack descriptor
  (`SANDBOX_ACK_FD`) survive the `pre_exec` closure; a closure that also closes them
  breaks every contained launch.
- The protected-process-isolation tier keeps its current behaviour exactly.
- A grant root that changed after the policy was built refuses the launch rather
  than granting the replacement.
- No descriptor is closed that the target is documented to receive.
- No new descriptor is inherited by the target.
- The descriptor text never claims a channel is closed when it is not.

## 5. Scope

### In scope

- `managed_process::configure_status_fds` — descriptor closure on every tier.
- `landlock::apply` / `landlock_access_for_path` — open grants with no-follow
  resolution and verify identity.
- The mount-view limits list — disclose the inherited-descriptor channel.
- A native fixture for each.

### Explicitly out of scope

- Changing which descriptors are *intentionally* inherited (status, ack, PTY,
  seccomp filter, pinned mount sources). Those are correct.
- Any change to the Bubblewrap mount construction.
- Any change to the grant set (C002).
- Any change to the helper status protocol.
- Windows.

## 6. Required production changes

### Security and authorization

Close unapproved descriptors on every contained launch, on both platforms. Pin
every Landlock grant by opening it with no-follow resolution and verifying that the
opened object is the one the policy canonicalised.

### Core/domain

None.

### Storage and migrations

None.

### Protocol and DTOs

None. The status descriptor numbers are unchanged.

### Runtime and concurrency

The `pre_exec` hook runs between `fork` and `exec` and must remain async-signal-safe:
no allocation, no locking, no logging. The existing implementation already meets
this; extending its scope must not break it. The identity check for a Landlock grant
happens in the helper before restriction, which is a normal process context, so it
may allocate.

### Frontend or operator surface

None.

### Documentation and static guards

`architecture/security.md` records that descriptor hygiene is uniform and that the
mount-view limits disclose the channel.

## 7. Ordered work packages

### Work package A — Close unapproved descriptors on every tier

Intent: the descriptor table is part of the containment boundary on every tier.

Required changes: make `close_unapproved_fds` true for every `SandboxRequest::
Required` launch, not only the process-isolation tier, and make the macOS arm
participate rather than being `cfg`-excluded. Keep the status and ack descriptors
open by construction — they are duplicated to temporary numbers before the closure
and restored after, which is what the existing hook already does.

Acceptance evidence: a native fixture opens a non-`CLOEXEC` descriptor in the
parent before launching a contained child, and the child cannot read it, on macOS
and on the default Linux tier. The same fixture passes on the
process-isolation tier unchanged.

### Work package B — Pin Landlock grants by identity

Intent: close the check-then-open window on the default route.

Required changes: replace the `exists()` + `PathFd::new(path)` pair with an open
that refuses symlinks and magic links, then verify the opened object's `dev`/`ino`
against the identity captured when the policy was built. Reuse the pattern already
proved in the helper's `open_mount_source` rather than writing a second one. On a
mismatch, refuse with `SetupError` before restriction.

Acceptance evidence: a fixture replaces a grant root with a symlink between policy
construction and apply, and the launch refuses; the positive control shows the
unchanged root is accepted.

### Work package C — Disclose the descriptor channel

Intent: the descriptor must not claim more than the mechanism obtains.

Required changes: add the inherited-descriptor channel to the mount-view limits
list, phrased like the Landlock and Seatbelt entries so the three tiers read the
same. Do not remove the existing network and pathname-socket limits.

Acceptance evidence: a test fails if the mount-view limits omit the channel while
the tier still passes descriptors.

### Work package D — Keep the tiers' text consistent

Intent: three tiers describing the same channel three ways is how a descriptor
stops meaning anything.

Required changes: make the Landlock, Seatbelt and mount-view limits name the
descriptor channel with the same words. Do not claim the channel is closed on a
tier where closure has not been proved.

Acceptance evidence: the three lists agree on the wording and each is backed by a
fixture.

## 8. Failure, cancellation, restart, and contention semantics

A failed descriptor closure in `pre_exec` returns an error to the child, which
fails the spawn — fail-closed, unchanged. A failed grant identity check refuses
before restriction, so no partial ruleset exists. There is no durable state and no
restart semantics. Concurrent launches are independent; the identity capture is
per-spec.

## 9. Compatibility and migration

No persisted configuration, durable payload or protocol change. A launch that
relied on an inherited descriptor now fails to read it — that is the fix, and it is
recorded as the expected behaviour change. A grant root replaced between policy
construction and apply now refuses instead of granting the replacement; that is also
the fix. No migration.

## 10. Required tests

### Focused unit tests

- `configure_status_fds` closes unapproved descriptors and preserves the status and
  ack descriptors.
- The grant identity check rejects a changed root and accepts an unchanged one.

### Integration tests

- Native fixture: a non-`CLOEXEC` descriptor opened in the parent is unreadable by a
  contained child, on macOS and on the default Linux tier.
- Native fixture: a grant root replaced by a symlink before apply refuses the
  launch.

### Restart and recovery tests

Not applicable.

### Contention and cancellation tests

- Concurrent contained launches each close their own descriptors and do not affect
  one another.

### Security and negative tests

- Both fixtures carry a positive control: the descriptor is readable when closure is
  disabled, and the unchanged root is accepted.
- A test fails if the mount-view limits omit the descriptor channel.

### Migration and compatibility tests

- The existing containment suite still passes on every tier after closure is
  widened, which is the compatibility test for tools that legitimately use
  inherited descriptors.

## 11. Required verification commands

```bash
# focused
cargo test -p codegg --lib security::sandbox
cargo test -p codegg --lib managed_process

# real contained child (macOS host)
cargo test -p codegg --test security_permission -- sandbox_containment

# real contained child (Linux host)
cargo test -p codegg --test security_permission -- sandbox_containment
cargo test -p codegg --test security_permission -- sandbox_landlock

# static guards
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_contract.py --self-test

# formatting and lint
cargo fmt
cargo clippy -p codegg --all-targets --locked -- -D warnings
```

The macOS host cannot produce the Linux evidence. Do not claim it.

## 12. Documentation updates

- `architecture/security.md` — uniform descriptor hygiene, the mount-view
  disclosure, and the grant-pinning rule.
- `architecture/tool.md` or `architecture/exec.md` if either describes inherited
  descriptors on the launch path.
- The roadmap §12 status table and `plans/registry.md` on closure.

## 13. Acceptance criteria

1. A non-`CLOEXEC` descriptor opened in the parent is unreadable by a contained
   child on macOS and on the default Linux tier, with a positive control.
2. The status and ack descriptors still reach the helper on every tier.
3. The process-isolation tier's behaviour is unchanged.
4. A grant root replaced between policy construction and apply refuses the launch,
   with a positive control for the unchanged root.
5. The mount-view limits disclose the inherited-descriptor channel, and the three
   tiers describe it consistently.
6. The existing containment suite still passes on every tier.

## 14. Stop conditions

Stop and report rather than improvise when:

- closing descriptors breaks a tier because a descriptor is legitimately inherited
  and not covered by the status/ack/PTY/seccomp set — report which one and why
  rather than exempting the tier;
- the identity check cannot be performed without changing `BackendPolicy`'s public
  shape;
- `pre_exec` would need to allocate or lock to perform the closure;
- pinning grants makes a supported toolchain fail because a legitimate grant root is
  reached through a symlink on that host — report the host and the path rather than
  weakening the check to path-following;
- the work would change the mount construction.

## 15. Closure evidence required

- The descriptor fixture on macOS and the default Linux tier, with its positive
  control.
- The unchanged process-isolation behaviour.
- The grant-pinning fixture with its positive control.
- The limits text for all three tiers, with the consistency assertion.
- The exact commands run, labelled by host.
- The roadmap §12 and registry updates, including the unblock audit.

## 16. Handoff notes

- The `pre_exec` closure runs in the forked child before `exec`. It must stay
  async-signal-safe: no allocation, no `println!`, no locking. The existing hook is
  careful about this; do not add a convenience call that allocates.
- `SANDBOX_STATUS_FD = 3` and `SANDBOX_ACK_FD = 198` are fixed numbers. The hook
  duplicates them to temporary descriptors before closing the rest and restores
  them after. Preserve that ordering exactly.
- `open_mount_source` in the helper is the reference implementation for no-follow
  open plus identity verification. Reuse it; do not write a second variant with
  different flags.
- This plan and **C002** both rewrite the Landlock limits disclosure and touch the
  helper. C002 lands first; rebase rather than duplicating the limits edit.
- This plan and **C003** both edit the enforcement-descriptor surface. C003 lands
  after this plan.
- The descriptor channel is conditional, not a live exploit: most handles are
  `CLOEXEC`. Say so in the closure record rather than overstating the finding.
