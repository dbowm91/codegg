# Sandbox Containment Audit Corrective C006 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/sandbox-containment-audit-corrective/006-descriptor-hygiene-and-grant-path-pinning.md`

Source subsystem roadmap:

- `plans/subsystems/sandbox-containment-audit-corrective-roadmap.md#milestone-c006--descriptor-hygiene-and-grant-path-pinning`

Repository baseline reviewed: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8` (`feat-auto-20261109-f8abad0d` HEAD) on a **macOS 26.6.2** host (Darwin 25.6.0, arm64, `sandbox-exec` present, backend `Seatbelt`; soft `RLIMIT_NOFILE` 1,048,575, `getdtablesize()` 92,160), with a sibling lane editing `src/managed_process.rs` during the review (last mtime observed `18:00:01`).

Implementation commits or pull requests:

- **None. The implementation is uncommitted working-tree work**, shared with sibling agents
  (C001–C005) on `feat-auto-20261109-f8abad0d`. No commit, stage, or push identifies this
  milestone. Line numbers are for the tree as read at `18:04` and the tree is moving.

## 1. Executive finding

Descriptor closure is now a property of every contained launch, and grant pinning is one
primitive rather than two.

**Closure is uniform.** `configure_status_fds` is called with `close_unapproved_fds = true` at
both production call sites — `run_streaming` (the mount-view route,
`src/managed_process.rs:827`) and `run_inner` (the legacy route, `:1147`) — plus one unit test
that exercises the hook directly (`:4982`). Before C006 the parameter was `cfg`-split:

```text
#[cfg(not(target_os = "linux"))] let close_unapproved_fds = false;
#[cfg(target_os = "linux")]      let close_unapproved_fds =
    matches!(&sandbox, SandboxRequest::Required(spec) if spec.linux_process_isolation);
```

so the default Landlock tier closed nothing, the mount-view tier closed nothing, and macOS closed
nothing **ever**. The `true` at both sites is now unconditional, which is the correct reading:
`prepare_launch_argv` creates the status pipe only for a `SandboxRequest::Required` launch, so
reaching either call site is the contained path.

**The hook keeps the channel by construction.** `configure_status_fds`
(`src/managed_process.rs:1986-2045`) duplicates `SANDBOX_STATUS_FD` and `SANDBOX_ACK_FD` to
temporary numbers with `F_DUPFD_CLOEXEC` (min 10) *first*, so assigning the fixed numbers cannot
clobber a source; it then `dup2`s the temporaries onto 3 and 198, clears close-on-exec on both,
and only then performs the closure. The order is the invariant: the closure skips those two numbers, and
clearing close-on-exec first is what lets the helper receive them across the final `exec`.

**The hook stays async-signal-safe.** It calls `fcntl`, `dup2`, `close`, `getdtablesize` and one
`close_range` syscall. Nothing allocates, nothing locks, nothing logs; failure returns an
`io::Error` and aborts the spawn. The macOS scan and the Linux range close are separate
`cfg`-selected functions (`:2055-2063` and `:2076-2092`) with the same contract.

**Grant pinning is a single shared primitive.** `linux_mount_view::open_pinned(path, identity)`
(`src/security/sandbox/linux_mount_view.rs:441`) is `openat2` with
`RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS`, then a `dev`/`ino` verification against the
snapshot captured when the policy was built. Both consumers use it and neither keeps a second
implementation: the helper's `open_mount_source` now delegates
(`src/bin/codegg-sandbox-helper.rs:604-618`) and the Landlock rule compiler binds every rule to
the descriptor it returns (`src/security/sandbox/landlock.rs:287`, inside `add_landlock_path`),
with the identity mandatory there — a policy root with no captured identity is refused rather
than opened by name. The `PathFd::new` + `exists()` check-then-open pair is gone from the
production path; the only surviving `exists()` in that file is inside a test.

**One wording for one channel.** `backend::INHERITED_DESCRIPTOR_LIMIT`
(`src/security/sandbox/backend.rs:116-117`) is *"open file descriptors inherited before the
profile apply remain usable"*. Landlock imports it, the mount view emits it from
`protected_mount_view_limits` (`managed_process.rs:2657`), and Seatbelt carries the same string
in `SEATBELT_LIMITS` (`seatbelt.rs:46`).
`every_tier_names_the_inherited_descriptor_channel_with_the_same_words`
(`backend.rs:274`) asserts all three, on both mount-view tiers, and asserts the existing network
limit is still present. No tier claims the channel is closed; the constant's own docs explain why
it cannot be (the helper's status, acknowledgment, mount-source and seccomp descriptors cross the
final `exec`).

**What is not observed.** No Linux runtime evidence exists for any of this: the fixture that
proves the channel is closed, the `close_range` path, and the whole `open_pinned` open/refuse
behaviour are `#[cfg]`-gated or live in Linux-only modules. The macOS half is observed and
green (§4). The lane that would observe the Linux half is `ci.yml`'s `ubuntu-latest` job —
helper built at `:659-663`, workspace sweep at `:665-680` — so it is one committed-and-pushed
run away rather than host-blocked. That job does not set `CODEGG_SANDBOX_QUALIFICATION`, so read
a green sandbox result there together with `002-status.md` §10.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| §13.1 A non-`CLOEXEC` descriptor opened in the parent is unreadable by a contained child, **on macOS**, with a positive control | `a_contained_child_cannot_read_an_inherited_descriptor` (`tests/security_permission/sandbox_containment.inc:1231`) passed on macOS in the 22/22 run. It duplicates a real open file to a number ≥ 200, clears `CLOEXEC`, drives the **production** owner (`ManagedProcessService::run` with a `Required` spec), and asserts `FD-DENIED`, no sentinel on stdout, and an `Enforced` outcome. The control launches the real helper directly, bypassing the daemon's closure, and asserts the same descriptor **is** readable | Pass (macOS) | The helper is called with `launch_helper_with_inherited_fd`, which still places the descriptor on fixed fd 42 inside its own `pre_exec`; the fixture's own holding number is ≥ 200 |
| §13.1 (Linux half) the same on the default Linux tier | Not run | **Not run** | The closure call site is compiled for Linux, but nothing here executed it. The lane is `ci.yml`'s `ubuntu-latest` job, and the blocker is that nothing is committed and pushed, not a missing host |
| §13.2 The status and ack descriptors still reach the helper on every tier | `descriptor_closure_preserves_the_channel_and_closes_the_rest` (`managed_process.rs:4982`) passed: fd 200 is closed, fd 3 and fd 198 are open in the child | Pass (unit) | Asserts the child's view of `/dev/fd/{3,198,200}`, so the fixed numbers are observed after the closure, not assumed from the code order |
| §13.3 The process-isolation tier's behaviour is unchanged | The Linux tier already had `close_unapproved_fds = true`; C006 replaces the predicate with the literal `true`, so that tier's inputs are identical | Partial | Reasoned from the diff, not run: the tier is Linux-only |
| §13.4 A grant root replaced between policy construction and apply refuses the launch, with a positive control for the unchanged root | `a_grant_root_replaced_after_identity_capture_is_refused` (`src/security/sandbox/policy.rs:1266`) passed on macOS — it asserts the unchanged root is accepted, the swapped root is refused with "changed after its identity was captured", and a symlinked root is refused at capture with "symlink and cannot be granted" | **Partial** | The **identity check** is evidenced here. The `open_pinned` half — `openat2` flags, the ELOOP refusal, the missing-path refusal — is `#[cfg(target_os = "linux")]` and unobserved: `a_grant_root_replaced_between_policy_construction_and_apply_is_refused` (`landlock.rs:386`) and `open_pinned_refuses_a_grant_path_that_vanished` (`:416`) both require Linux |
| §13.5 The mount-view limits disclose the inherited-descriptor channel and the three tiers agree | `protected_mount_view_limits` (`managed_process.rs:2655-2675`) emits `INHERITED_DESCRIPTOR_LIMIT` first on both mount-view shapes; `every_tier_names_the_inherited_descriptor_channel_with_the_same_words` passed | Pass | The test also asserts the mount view keeps its network limit, so the disclosure is additive rather than a replacement |
| §13.6 The existing containment suite still passes on every tier | macOS: 22/22, twice in the parallel harness and once serially | Partial | The Linux half runs in `ci.yml`'s `ubuntu-latest` job (`:665-680`); the blocker is the commit, plus the marker gap that would let the member skip (`002-status.md` §10) |
| §10 focused: `configure_status_fds` closes unapproved descriptors and preserves the channel | `descriptor_closure_preserves_the_channel_and_closes_the_rest` | Pass | Same test as §13.2 |
| §10 focused: the identity check rejects a changed root and accepts an unchanged one | `a_grant_root_replaced_after_identity_capture_is_refused` | Pass (macOS) | See §13.4 |
| §10 contention: concurrent contained launches do not affect one another | Not exercised by this milestone's fixtures | Not run | The closure is per-child `pre_exec` state and the identity capture is per-spec; no shared state was added. Not converted into a test here |
| §10 negative: "a test fails if the mount-view limits omit the descriptor channel" | The consistency assertion is explicit per tier | Pass | Removing the entry from `protected_mount_view_limits` fails it by construction; not demonstrated by mutation |

## 3. Production implementation evidence

**Closure.** `close_unapproved_fds_in_child` has two bodies. Linux uses
`close_range(4, 197)` and `close_range(199, u32::MAX)` — a syscall per range, skipping 3 and 198.
macOS has neither `close_range(2)` nor `closefrom(3)`, so it scans `4..getdtablesize()` and skips
3 and 198 explicitly. Both are `#[allow(unsafe_code)]` with a doc comment stating why the
`pre_exec` contract holds.

**Pinning.** `open_pinned` takes an `Option<&SandboxPathIdentity>`: `None` for mounts that have no
captured snapshot (device nodes, launch scratch), `Some` for every policy root. `/dev/null` is
opened `O_RDONLY | O_CLOEXEC` rather than `O_PATH` because the rule compiler needs a readable
descriptor for it; everything else is `O_PATH`. Errors distinguish "required sandbox path does not
exist" from "cannot open sandbox path … without following aliases", so an operator can tell a
missing grant from a swapped one.

**Disclosure.** The mount-view limits list moved into one function used by both mount-view shapes,
so a channel cannot be disclosed on the process-isolation tier and omitted on the filesystem-only
one. That is a small refactor with a real property behind it.

**Nothing else changed.** No change to which descriptors are *intentionally* inherited (status,
ack, PTY, seccomp filter, pinned mount sources); no change to the Bubblewrap mount construction,
the grant set, the helper status protocol, or Windows. `SANDBOX_STATUS_VERSION` is unchanged.

## 4. Verification executed

### Commands run

```bash
cargo build --locked --bin codegg-sandbox-helper
cargo test --locked -p codegg --lib managed_process
cargo test --locked -p codegg --lib security::sandbox
cargo test --locked --test security_permission -- sandbox_containment
cargo test --locked --test security_permission -- sandbox_containment --test-threads=1
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_contract.py --self-test
python3 scripts/check_sandbox_policy_wiring.py
python3 scripts/check_sandbox_policy_wiring.py --self-test
python3 scripts/check_comment_budget.py
git status --porcelain
```

The Linux-hosted commands the plan names (`sandbox_containment` and `sandbox_landlock` on Linux)
were **not run here**: this macOS host cannot compile or execute them. That is a fact about this
review, not about the repository. The lane they belong to is `ci.yml`'s `ubuntu-latest` job —
helper built at `:659-663`, workspace sweep at `:665-680` — and the blocker is that nothing in
this tree is committed and pushed, not that no host exists.

### Results

- `cargo build --locked --bin codegg-sandbox-helper` — succeeded (`3m 01s`). The fixtures launch
  `target/debug/codegg-sandbox-helper`, so a stale helper would have made every result doubtful.
- `cargo test -p codegg --lib managed_process` — **26 passed**, 0 failed, including
  `descriptor_closure_preserves_the_channel_and_closes_the_rest`.
- `cargo test -p codegg --lib security::sandbox` — **66 passed**, 0 failed, including
  `every_tier_names_the_inherited_descriptor_channel_with_the_same_words` and
  `a_grant_root_replaced_after_identity_capture_is_refused`. (The figure was 65 before a sibling's
  C003 follow-up landed; the extra test belongs to that change.)
- `cargo test --test security_permission -- sandbox_containment` — **22 passed, 0 failed**,
  `324 filtered out`, `finished in 0.91s`; repeated after the sibling edit at `0.52s`, and again on
  the settled tree at `0.61s`, all three green.
- `--test-threads=1` — **22 passed**, `finished in 1.25s`.
- `check_sandbox_contract.py` — passed (`0.035 s`); `--self-test` — passed (`0.025 s`).
- `check_sandbox_policy_wiring.py` — passed (`0.221 s`); `--self-test` — passed (`0.718 s`), nine
  injected defects rejected.
- `check_comment_budget.py` — **failed at this review, 20 violations**, none under the sandbox
  surface or `src/managed_process.rs`. The failing files were sibling lanes' TUI, config,
  protocol and `src/main.rs` work (full list in `003-status.md` §4). All C006 files are inside
  their recorded ceilings; `src/managed_process.rs` sits at `317` comment lines against a `317`
  ceiling. **Discharged 2026-11-13** by `scripts/verify.sh full`: the guard passes, the ratchet
  is banked at 139,245 lines, fmt and clippy are clean, and the sweep ran 17,377 workspace +
  8,441 feature-pass tests with 0 failures.
- **`git status --porcelain` reports 169 modified/untracked paths at `18:20`; it was 165 at
  `17:44`, so the figure grows as sibling lanes work. `git log -1` is `7642b9e38`.** Nothing is
  committed.
- **Not observed:** any Linux execution of the closure or of `open_pinned` on this host. The
  `ubuntu-latest` job would execute both once the tree is committed.

**A brief tree-wide compile blocker, not this milestone, and resolved during the review.** A
re-run at `18:14` could not build the lib test target:

```text
error[E0004]: non-exhaustive patterns: `tui::editor::HelixVerb::ToggleComments { .. }` not covered
   --> src/tui/editor.rs:3222:11
error: could not compile `codegg` (lib test) due to 1 previous error
```

A sibling TUI lane's in-flight edit — the only error in the build, and not a sandbox file. The lane
fixed it at `18:24:32`, and the re-runs at `18:25-18:31` on the settled tree reproduce every sandbox
figure: `managed_process` **26**, `security::sandbox` **66**, `sandbox_containment` **22**,
`host_admission_manifest` **7**.

### Non-vacuity controls — reported, not re-executed

The implementing lane reports that both controls were observed and then restored: disabling the
closure at the call site made the inherited-descriptor fixture fail, and making
`verify_path_identity` unconditionally `Ok(())` made the swapped-root refusal fail. **This record
did not perform either mutation** — both require editing `src/**`, outside its write scope. The
second claim is mechanically plausible: `a_grant_root_replaced_after_identity_capture_is_refused`
asserts the refusal text, so a stub that always returns `Ok(())` fails it on this host. The first
is consistent with the fixture asserting `FD-DENIED` from the production path. Recorded as
reported.

### Measured cost — reported versus observed on this host

The macOS closure scans `4..getdtablesize()` because Darwin has neither `close_range` nor
`closefrom`; exactness was chosen over a heuristic high-water mark, so the scan's cost is a
disclosed cost of the design, not a defect.

| Source | Table size | Cost per spawn |
|---|---|---|
| Reported by the implementing lane | 1,048,575 (`RLIMIT_NOFILE`) | ≈ 85 ms |
| Reported by the implementing lane | 256 – 10,240 | ≈ 0.02 – 1 ms |
| **Measured here** (isolated loop of the same `close` scan) | 92,160 (`getdtablesize()`) | **7.7 ms** |
| **Measured here** | 10,240 | 0.83 ms |
| **Measured here** | 256 | 0.02 ms |

On this host `getdtablesize()` reports **92,160** and does not follow `RLIMIT_NOFILE`: setting
`rlim_cur` to 1,048,575 (its default here) leaves the table at 92,160. So the top of the reported
range is not reproducible, and the elastic closure is cheaper here than the reported figure. The
end-to-end test body that performs one such spawn completes in `0.01–0.04 s`. See §10.

## 5. Invariant review

- **A constrained request never degrades to host authority.** Preserved. A closure failure returns
  `Err` from `pre_exec`, which fails the spawn; it does not continue with an unclosed table.
- **The status and ack descriptors survive the closure.** Asserted in the child, not inferred:
  `FD3-OPEN`, `FD198-OPEN`, `FD200-CLOSED`.
- **The process-isolation tier keeps its behaviour exactly.** The predicate became the literal
  `true`; that tier's resolved value was already `true`.
- **A grant root that changed after the policy was built refuses rather than granting the
  replacement.** Evidenced on this host for the identity check; the `open_pinned` refusal is
  Linux-only and unobserved.
- **No descriptor is closed that the target is documented to receive.** The intentional set
  (status, ack, PTY, seccomp, mount sources) is untouched, and the mount sources are opened by the
  helper *after* `exec` rather than inherited from the daemon, so the Linux range close cannot
  remove them.
- **No new descriptor is inherited by the target.** The closure only removes; the two descriptors
  it preserves are the two the helper requires.
- **The descriptor text never claims a channel is closed when it is not.** Asserted by the
  three-tier consistency test, and the constant's docs state the reason it cannot be closed.

One reading of the last invariant deserves to be written down rather than left implicit: the
closure makes the *daemon's* descriptors unavailable, and the channel that remains is the
*helper's* own. Those are different claims and the text keeps them apart.

## 6. Failure and recovery review

A failed `close` or `close_range` propagates out of the `pre_exec` hook, so the spawn fails
fail-closed and no target runs with a table the launch did not approve — unchanged from the
pre-existing hook, now with a wider audience. A failed `dup2` or `F_DUPFD_CLOEXEC` takes the same
path and cleans up the temporaries it already opened. There is no durable state, no restart
semantics and no cross-launch contention: the closure is forked-child state and the identity
capture is per-spec. A grant identity mismatch refuses before `restrict_self`, so no partial
ruleset exists to unwind.

## 7. Migration and compatibility review

No persisted configuration, durable payload, protocol frame or DTO change; the status descriptor
numbers are unchanged and `SANDBOX_STATUS_VERSION` stays 1. The one behaviour change is the
intended one: a launch that relied on reading a descriptor the daemon held now cannot read it.
That is the fix, and it is a compatibility change only for a tool that was relying on leaked
inheritance. A grant root replaced between policy construction and apply now refuses instead of
granting the replacement; that is also the fix. No migration, and no rollback hazard: reverting the
call sites to the old predicate restores the previous posture without touching the pinning code.

## 8. Security review

- **Obtained:** the descriptor table is part of the containment boundary on every contained
  launch, in code on both platforms and observed on macOS. An inherited descriptor can no longer
  read a host file the policy excluded.
- **Conditional, not a live exploit.** `std` sets `CLOEXEC` on what it opens, and most
  tokio/SQLite handles do too, so few launches were leaking anything. The plan's handoff note asks
  for that framing and `architecture/security.md:729-731` carries it: "the channel is closed
  because the invariant, not the incident, is what has to hold."
- **Obtained on this host:** the identity check refuses a swapped grant root and a symlinked root
  at capture.
- **Not obtained, and disclosed:** the `openat2` no-follow open itself, and any Linux kernel
  evidence for `close_range`.
- **Residual channel, disclosed on all three tiers:** the helper's own descriptors cross the final
  `exec`. No tier claims otherwise.
- **Residual bound, not previously named:** the macOS scan's reach is `getdtablesize()`, not
  `RLIMIT_NOFILE` (see §10). A descriptor above that number would survive.
- No secret is logged, and the closure itself emits nothing.

## 9. Documentation and operations

- `architecture/security.md:714-757` — uniform descriptor hygiene, the `pre_exec` ordering, the
  macOS scan and its reason, the mount-view disclosure and the correction that its scope text was
  true of the mount tree and false of the descriptor table, the grant-pinning rule and both
  fixtures, and the positive controls.
- `src/managed_process.rs:2047-2092` — the two closure bodies, each with its platform reason.
- `src/security/sandbox/linux_mount_view.rs:425-499` — `open_pinned`'s contract, including that it
  is the single open the containment path uses for both the mount view and the ruleset.
- `src/bin/codegg-sandbox-helper.rs:604-618` — the delegation note, so the next reader does not
  add a second variant with different flags.
- `tests/security_permission/sandbox_containment.inc:1183-1280` — the fixture section, the
  positive control's rationale and the production-path call.
- No new guard. `check_sandbox_contract.py` and `check_sandbox_policy_wiring.py` pass with their
  self-tests; `docs/comment-budget.toml` carries the raised ceilings for every sandbox file
  (`managed_process.rs` `285 → 317`, `linux_mount_view.rs` `54 → 75`, `sandbox_containment.inc`
  `105 → 135`, `landlock.rs` `32 → 70`, `backend.rs` `88 → 104`,
  `codegg-sandbox-helper.rs` `46 → 62`).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Medium | No Linux runtime evidence for any part of the milestone | §13.1's Linux half, §13.3, §13.4's `open_pinned` half and §13.6's Linux half are all unobserved. A defect in `close_range`'s ranges or in the `openat2` flags would not be caught here — and the range path is the one that runs on the production default tier | In `ci.yml`'s `ubuntu-latest` job (`:659-663` builds the helper, `:665-680` runs the sweep), after WP 0 of `plans/implementation/five-line-closure-corrective/001-closure-evidence-and-residue.md` commits the tree, with `CODEGG_SANDBOX_QUALIFICATION=required` set: `cargo test --locked --test security_permission -- sandbox_containment --test-threads=1` (expect 22/22 including `a_contained_child_cannot_read_an_inherited_descriptor`) and `cargo test --locked -p codegg --lib security::sandbox::landlock` (expect `a_grant_root_replaced_between_policy_construction_and_apply_is_refused` and `open_pinned_refuses_a_grant_path_that_vanished` to pass). Without the marker the family would skip rather than run (`002-status.md` §10) |
| Low | The macOS closure's reach is `getdtablesize()`, but the code comment says it is "bounded by the process's own `RLIMIT_NOFILE`" | On this host the two differ by an order of magnitude (92,160 vs 1,048,575), so a descriptor above 92,160 would survive the closure while the comment implies otherwise. Reaching that number needs ~92 k open descriptors first, which bounds the exposure | Either say "bounded by `getdtablesize()`, which Darwin does not derive from `RLIMIT_NOFILE`" or close the remainder above the table; then re-measure |
| Low | `SANDBOX_ACK_FD` (198) is skipped by the closure unconditionally, even when the launch has no ack channel (`source_ack_fd: None`) | An unrelated descriptor already sitting on 198 in the daemon would survive into the target. It requires a coincidental fd number, which bounds the exposure | Skip 198 only when an ack descriptor was established, or record why the fixed-number reservation is deliberate |
| Low | Seatbelt's limits list spells the shared sentence as a literal instead of referencing `INHERITED_DESCRIPTOR_LIMIT` | This is the "three tiers, three spellings" failure the milestone exists to prevent, held together only by the equality assertion in the consistency test | Import the constant in `seatbelt.rs` and delete the duplicate literal |
| Low | The reported ≈85 ms closure cost at a 1,048,575 table is not reproducible on this host | The disclosed cost is ~7.7 ms at the table this host actually reports; the report overstates the cost by an order of magnitude, which is the safe direction but still inaccurate | Re-measure on the host the figure came from, or correct it to a `getdtablesize()`-relative statement |
| Low | One containment run in the implementing lane showed 21/22 among ~10 green runs and never reproduced | An intermittent member failure would make a local run untrustworthy. Today's three runs here were 22/22, and the `001`/`004` records' diagnosed suspect (a fixed fd-42 collision in `launch_helper_with_inherited_fd`) is unchanged | Keep watching it in the central sweep; if it recurs, capture the failing member name and its status-frame error rather than assuming the old diagnosis |

No high-severity finding. Every acceptance criterion that this host can produce evidence for is
evidenced, and the two that it cannot — Linux kernel behaviour and the mutation demonstrations —
are named with the exact commands that would produce them.

## 11. Roadmap disposition

**Milestone conditionally closed.** Production implementation is complete and the macOS evidence
is observed; the named Linux evidence is outstanding. This host cannot produce it and
`ci.yml`'s `ubuntu-latest` job can, so it is one committed-and-pushed run away rather than
blocked on a machine.

Conditions, with their exact future evidence:

1. **Linux runtime evidence for closure and pinning.** In `ci.yml`'s `ubuntu-latest` job — helper
   built by the lane at `:659-663`, sweep at `:665-680` — with
   `CODEGG_SANDBOX_QUALIFICATION=required` set:
   ```bash
   cargo build --locked --bin codegg-sandbox-helper
   cargo test --locked --test security_permission -- sandbox_containment --test-threads=1
   cargo test --locked -p codegg --lib security::sandbox::landlock
   ```
   Expect 22/22 in the containment family — including
   `a_contained_child_cannot_read_an_inherited_descriptor` — and the two `open_pinned` fixtures
   green. The blocker is WP 0 of
   `plans/implementation/five-line-closure-corrective/001-closure-evidence-and-residue.md` (the
   commit), not a host.
2. **The mutation demonstrations.** Disable the closure at one call site and observe
   `a_contained_child_cannot_read_an_inherited_descriptor` fail on macOS; make
   `verify_path_identity` return `Ok(())` unconditionally and observe
   `a_grant_root_replaced_after_identity_capture_is_refused` fail; restore both. Reported as
   observed by the implementing lane, not re-executed here.
3. **Two bound questions, left open for the owner** — not decided here: (a) should the macOS
   closure's comment say it is bounded by `getdtablesize()` rather than `RLIMIT_NOFILE`, and
   (b) should fd 198 be skipped when the launch has no ack channel (`source_ack_fd: None`)?
   Neither blocks the milestone; both are cheap to settle while the file is open, and the first
   is a comment that currently overstates the coverage.

The roadmap's exit conditions are met in substance: descriptor closure is compiled on macOS and
on the default Linux tier, the Landlock grant path opens with no-follow resolution and
re-verifies by identity, and the mount-view limits disclose the channel. What remains is
evidence on one platform, not implementation — and that evidence is one committed run of the
Linux job away.

## 12. Registry updates

- Roadmap §12 status table: C006 → `conditionally closed`, closure record
  `plans/closure/sandbox-containment-audit-corrective/006-status.md`, with condition 1 (Linux
  evidence) and conditions 2-3. Its "operational after C002" ordering is satisfied: C002's
  descriptor work is in the same tree.
- `plans/registry.md`: the subsystem row's "proposed; six plans authored 2026-11-10, no work
  started" is inaccurate; the C006 plan row moves from `ready` to `conditionally closed` and must
  carry the pending Linux evidence, or the row will read as though the closure was proved on both
  platforms.
- Unblock audit: C006 had no hard dependency, and it unblocks C003 ("operational after C006"),
  which is now in the same tree. Nothing waits on C006's conditional state.
- **Not done here, by scope:** the plan's own `Status:` line and `plans/registry.md` are the
  parent's to change; this record touches neither, nor the roadmap status table.

## Correction log (2026-11-13)

Status unchanged: **`conditionally closed`**. The two §10 bound questions stay **questions**, and
nothing here decides them.

- **§1, "What is not observed."** Appended the lane (`ci.yml`'s `ubuntu-latest` job, `:659-663`
  and `:665-680`), the commit as the blocker, and the marker caveat that a green sandbox result
  there is not self-certifying.
- **§2, §13.1 (Linux half) note "No Linux host."** Corrected: the lane exists and the blocker is
  that nothing is committed.
- **§2, §13.6 note "Linux half not run."** Corrected to name the lane, the commit, and the
  marker gap.
- **§4, "were not run: there is no Linux host in this environment."** Corrected to "not run
  here" — a fact about this review — with the CI lane and the commit named as the real path.
- **§4, the `check_comment_budget.py` "failed, 20 violations" claim.** Corrected in place:
  **`scripts/verify.sh full` on 2026-11-13** passes it with the ratchet banked at 139,245 lines,
  fmt and clippy clean, and 17,377 workspace + 8,441 feature-pass tests green.
- **§4, "Not observed: any Linux execution of the closure or of `open_pinned`."** Corrected to
  "on this host", with the `ubuntu-latest` job named as the place it would execute.
- **§10, Medium Linux-runtime required action, "On a Linux host with the helper built there."**
  Corrected to the `ubuntu-latest` job with the marker set, after WP 0 (the commit).
- **§11 lead, "cannot be obtained on this host."** Corrected: the host cannot, CI can, and the
  evidence is one committed run away.
- **§11 condition 1, "On a Linux host with the helper built there."** Corrected to the
  `ubuntu-latest` job with the marker set and the commit as the blocker.
- **§11 condition 3.** Rewritten as two explicitly **open owner questions** rather than a
  "decide" instruction; the substance — `getdtablesize()` wording and fd 198 without an ack
  channel — is unchanged.
- **§11 closing, "What remains is evidence on one platform, not implementation."** Appended
  that the evidence is one committed run of the Linux job away.
