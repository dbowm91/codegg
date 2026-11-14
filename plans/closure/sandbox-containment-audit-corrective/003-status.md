# Sandbox Containment Audit Corrective C003 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/sandbox-containment-audit-corrective/003-enforcement-claims-match-the-mechanism.md`

Source subsystem roadmap:

- `plans/subsystems/sandbox-containment-audit-corrective-roadmap.md#milestone-c003--enforcement-claims-match-the-mechanism`

Repository baseline reviewed: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8` (`feat-auto-20261109-f8abad0d` HEAD) on a macOS 26.6.2 host (Darwin 25.6.0, arm64, `sandbox-exec` present, backend `Seatbelt`).

Implementation commits or pull requests:

- **None. The implementation is uncommitted working-tree work**, shared with sibling agents
  (C001, C002, C004, C005, C006) on `feat-auto-20261109-f8abad0d`. No commit, stage, or push
  identifies this milestone.

**Review-window note.** `src/managed_process.rs` was being edited by a sibling lane while this
record was written (mtime `17:52:06`, then `18:00:01`). The macOS half of work package A landed
at `17:52:06` — *after* the handoff state this review began from — and is recorded in §1 as a
post-review observation with its own line references. Line numbers in this record are for the
tree as read at `18:04`; the tree is uncommitted and moving.


## Corrective implementation, 2026-11-12 — AC4's ABI clause is met

The corrective the research addendum below specified has landed in this tree. This section records
it; the addendum is retained unchanged as the diagnosis that produced the work, and its
present-tense statements about the discarded ABI ("the value exists and is discarded one process
earlier") are historical as of this section.

### What landed

**Producer.** `src/bin/codegg-sandbox-helper.rs:433-447` keeps the value
`apply_process_scopes()` returns, only when `spec.linux_process_isolation` is set — the only tier
that applies a scope. `:527-540` writes it on the private status descriptor **after** the
`pre_exec` hook is installed and **before** `command.exec()` (`:541`), which is the last moment a
helper-authored line exists: `exec` replaces the process with Bubblewrap, whose records say
nothing about scope. `report_process_scope_abi` (`:552-564`) encodes
`encode_process_scope_abi_record(abi)` (`src/security/sandbox.rs:893-900`) and does not close the
descriptor (`mem::forget`), because the same fd is also Bubblewrap's `--json-status-fd`.

**Consumer.** `parse_helper_status_record` (`src/managed_process.rs:2144-2161`) reads the
namespaced key and treats an unusable value as an error, not a line to skip;
`split_helper_status` (`:2163-2184`) strips the prefix and refuses a duplicate record;
`interpret_bubblewrap_status` (`:2668-2713`) threads the number into `mount_view_enforcement`
(`:2728-2773`), which on the process-isolation tier pushes
`landlock::process_scope_guarantee(abi)` — **the returned number**, never the literal 6 — and
otherwise pushes `PROCESS_SCOPE_ABI_UNREPORTED_LIMIT` (`:2715-2721`), so an absent record is now
a stated claim rather than a silent `None`.

**Filesystem-only tier.** It applies no scope, so it reports none, and
`protected_mount_view_limits` (`:2784-2810`) says why: "PID/IPC isolation and seccomp are not
claimed by this launch, so no process-scope ABI is reported".

**The `abi` field still stays `None`** (`:2764-2769`), and a test pins it. The durable audit label
must not read `enforced:bubblewrap:abi=9`, attributing Landlock's version to Bubblewrap; the
obtained ABI rides the guarantee list, where it is named with the scope it belongs to.

**Documentation.** `architecture/security.md:258-286` documents the record
(`{"codegg-landlock-process-scope-abi": n}`) on the private status descriptor, the prefix
ordering, the absence limit, the filesystem-only tier, and why `abi` stays `None`.

### The deadlock hazard is addressed — verified in the code

- The helper writes the record **before** `exec`, so it is a prefix of the status stream rather
  than a line interleaved with Bubblewrap's.
- Both readers now consume a helper line and continue instead of refusing the first line:
  `read_bubblewrap_status_with_ack` (`src/managed_process.rs:2186-2256`) advances `line_start` on
  `MountViewStatusLine::Helper` (`:2213`) and writes the acknowledgment only after a
  `ChildReceipt` line (`:2225-2231`); `await_bubblewrap_ready` (`:2258-2310`) does the same
  (`:2287`) and hands only Bubblewrap's own records to the terminal decode (`:2296-2298`).
- Two real-pipe tests prove it rather than infer it from ordering:
  `the_streaming_reader_acknowledges_a_receipt_behind_a_helper_record` (`:3017`) and
  `the_live_child_reader_returns_only_bubblewraps_own_records` (`:3060`). Each writes the helper
  record first, then waits for the `0xA5` acknowledgment before releasing the exit record — the
  behaviour Bubblewrap's `--block-fd` has. Both compile on this host under `test`
  (`#[cfg(any(target_os = "linux", test))]`).
- What the addendum warned about is still refused: a **foreign** first line is not scanned past.
  `classify_mount_view_status_line` (`:2118-2137`) returns `Foreign`, the streaming reader breaks
  without acknowledging, and `await_bubblewrap_ready` errors.

### Evidence: what this host produced, and what it could not

"Host-verified" means compiled and executed on this macOS host at `HEAD 7642b9e38` plus this
working tree. "Linux-only" means the item sits behind `#[cfg(target_os = "linux")]` and **was not
compiled here**, let alone run.

Host-verified:

- `encode_process_scope_abi_record` — platform-independent, exercised by every test below.
- `split_helper_status` — `a_helper_record_is_split_from_the_bubblewrap_stream` (`:3102`): record
  9 splits off, Bubblewrap's stream is untouched, the helper's framed `SetupError` reason
  survives the split, a zero ABI is a refusal, and two records are a refusal.
- both status readers — the two pipe tests named above.
- `mount_view_enforcement` and `process_scope_guarantee`:
  `the_mount_view_descriptor_names_the_abi_the_helper_reported` (`:3151`, which asserts
  `Landlock ABI 11` **and** asserts no `ABI 6` survives),
  `a_process_isolation_launch_without_an_abi_record_states_the_gap` (`:3189`), and
  `the_filesystem_only_tier_reports_no_scope_abi_and_says_why` (`:3214`). **Measured by this
  review: 3 passed, 0 failed, 8197 filtered out.**
- `landlock_enforcement` / `process_scope_from` —
  `descriptor_claims_a_process_scope_only_when_one_was_obtained` (`landlock.rs:489`) and
  `a_failed_process_scope_is_a_refusal_not_an_absent_scope` (`:471`).

Linux-only, **not compiled and not run here**:

- the helper's capture and pre-exec write: `exec_linux_protected_mount_view` and
  `report_process_scope_abi` are `#[cfg(target_os = "linux")]`
  (`codegg-sandbox-helper.rs:358-359`, `:555-556`), so the producer is not even type-checked on
  this host;
- `landlock::apply_process_scopes()`'s real kernel call (`src/security/sandbox/landlock.rs:194-222`);
- **the glue that joins the two halves** — `interpret_bubblewrap_status` is
  `#[cfg(target_os = "linux")]` (`src/managed_process.rs:2668`), so the call from
  `split_helper_status` into `mount_view_enforcement` is not type-checked here. Its two halves
  are host-tested; the line connecting them is not.

**The end-to-end producer/consumer path has therefore not run anywhere**: no launch on this host
had a helper apply a scope, write the record and have the descriptor name the returned ABI. That
is the named evidence gap. The lane that can produce it is `ci.yml`'s `ubuntu-latest` job —
helper built at `:659-663`, sweep at `:665-680` — and the blocker is that nothing is committed
(nothing is observable until the tree is committed and pushed), not a missing host:

```bash
cargo build --locked --bin codegg-sandbox-helper
cargo test --locked --test security_permission -- sandbox_containment --test-threads=1
cargo test --locked -p codegg --lib security::sandbox
```

**Non-vacuity.** The ABI-11 test keys on the value it is handed, so a hardcoded 6, a dropped
record, or a record that falls into the limit arm all fail it; that is structural rather than a
mutation demonstration. **No mutation was performed by this review**, and none can be — the write
scope here is `plans/closure/**`.

**Regression.** `sandbox_containment` 22, `managed_process` 32 and `landlock` 7 are supplied by
the assignment, not re-run by this review; the three ABI tests above are the ones this review
executed.

### The Low finding this record raised is still open

`src/python_script/` was not touched (`git status --porcelain src/python_script/` is empty).
`executor.rs:543-552` still copies `SandboxExecutionOutcome::Enforced { abi }` into
`SandboxOutcome::Enforced { abi }`, and `projection.rs:120-121` still renders it with
`{outcome:?}`. On that route "the backend has no ABI" and "this route did not carry one it
obtained" remain indistinguishable. The mount-view correction does not reach it: Bubblewrap's
`abi` is genuinely `None`, and the obtained Landlock ABI rides the guarantee list, which that
projection does not print.

### Disposition

**`conditionally closed`**, and the reason is platform evidence, not implementation.

AC4's second clause — the scope ABI is no longer discarded — is implemented end to end in the
tree, with the ABI it names taken from the producer's return value and the absence of a record
promoted to a stated limit. It is not `closed` for the same reason the rest of this milestone is
not: the Linux path cannot be compiled on this host, so the producer, the kernel call and the
glue between them have no executed evidence, and §11's earlier conditions (runtime refusal
evidence, mutation demonstrations, Linux comparison) are untouched. It is not
`corrective pass required` because the only defect that made it so is corrected: no unclosed
finding now names a claim the tree still gets wrong.

Remaining conditions, which replace but do not cancel §11's:

1. **Linux end-to-end ABI evidence** — build the helper there and run the three commands above.
   The lib run covers the descriptor composition, and the containment family must stay 22/22; that
   family is also where the producer/consumer path would first *execute*, so a malformed record or
   a refusal would surface there. **No fixture asserts the mount-view descriptor's ABI text at
   launch level** — the positive end-to-end assertion needs a fixture that prints the descriptor,
   and none exists. That fixture is part of the evidence, not a run.
2. **§11's conditions 1–3** — runtime refusal evidence, the mutation demonstrations, and the
   Linux comparison — unchanged.
3. **Two documentation/decision items** — the `python_script` ambiguity above, and the
   request-derived labelling of the non-ABI guarantee entries that §10 still records.

## Research addendum, 2026-11-12 — AC4 is unmet, and §7E does not excuse it

A read-only re-investigation of this milestone's own acceptance criterion,
prompted by the strict-reading paragraph above, changed the answer. It found
three things.

**1. AC4 does not ask for an `abi` on the mount-view route.** The plan's §13
criterion 4 reads:

> The mount-view guarantee list is either corroborated by a receipt or labelled
> inferred, and the scope ABI is no longer discarded.

and work package E reads "Stop discarding the effective scope ABI … If C002 is
landing the ABI capture on the default route, do not duplicate it here —
coordinate." The criterion's subject is **the obtained ABI being thrown away**,
not the `SandboxExecutionOutcome::Enforced.abi` field. This record and the
owner's paraphrase both conflate the two, and the field would be the wrong
destination anyway: its documented meaning is *the backend's* version
(`src/managed_process.rs:558-560`), and the backend on this route is Bubblewrap,
which has none.

**2. The value exists and is discarded one process earlier.** The mount-view
helper applies the scope and drops the number it gets back
(`src/bin/codegg-sandbox-helper.rs:433-443`):

```rust
if let Err(reason) = codegg::security::sandbox::landlock::apply_process_scopes() {
```

`apply_process_scopes()` returns `Result<u32, String>` and yields the effective
kernel ABI on success (`src/security/sandbox/landlock.rs:184`, `:204-211`). The
descriptor then substitutes a hardcoded baseline: `"Landlock ABI 6 scopes …"`
(`src/managed_process.rs:2632`). On a host whose effective ABI is 9, the 9 is
dropped and the text says 6. `abi: None` is a literal at
`src/managed_process.rs:2637-2643`, present at `HEAD` too, so this is
pre-existing and not a C003 regression.

**3. §7E is a coordination clause, not an exemption.** Its "do not duplicate the
capture" means *consume the capture this route already performs*. C002's record
confirms the mount-view route kept its own call. Threading the returned `u32`
duplicates nothing; it stops discarding. This record's sentences at §10 and §13
that treat §7E as forbidding the change are therefore **not supported by the
plan text** and should not be relied on again.

### Consequences for the fix

- **Do not** put the Landlock ABI into `Enforced.abi`. The durable audit label
  is `enforced:{backend}:abi={n}` (`src/tool/bash.rs:399-421`, persisted at
  `src/live_execution_audit.rs:179`), so `enforced:bubblewrap:abi=9` would
  attribute Landlock's ABI to Bubblewrap — a false enforcement claim of exactly
  the class this milestone exists to remove. The sanctioned carrier is the
  guarantee list, which is where the default route already puts it
  (`src/security/sandbox/landlock.rs:77-85`); the structured
  `process_scope_abi` is deliberately dropped by `SandboxLaunchOutcome::enforced`
  (`src/security/sandbox.rs:834-841`).
- **A naive pre-exec line deadlocks one path.**
  `read_bubblewrap_status_with_ack` (`src/managed_process.rs:2108-2165`) writes
  its ack byte only when the first line decodes *and* contains `"child-pid"`
  (`:2131-2151`), while `await_bubblewrap_ready` (`:2174-2218`) rejects a
  foreign first line outright (`:2206-2212`). A helper-authored line after
  `exec` would leave Bubblewrap blocked on `--block-fd` — a hang, not a refusal.
  Both readers must be generalised, preserving the bound and the
  exit-before-child ordering rule.
- **The ABI cannot ride stderr.** `scripts/check_sandbox_contract.py:35-36`
  forbids a stderr-marker channel; it must ride the status fd.
- **No guard asserts the ABI claim at all.** `check_sandbox_contract.py` and
  `check_sandbox_policy_wiring.py` do not mention `abi`, so this class of
  regression is currently silent.
- **Not a live correctness bug.** No caller reads `None` as "scope absent":
  `sandbox_outcome_label` renders it honestly as `abi=none`
  (`src/tool/bash.rs:399-421`), `git_contained_worktree` copies it with no
  production reader (`src/git_contained_worktree.rs:365-378`). The one ambiguity
  is `python_script`'s `{:?}` rendering
  (`src/python_script/executor.rs:543-552`,
  `src/python_script/projection.rs:118-121`), where "the backend has no ABI" and
  "this route did not carry one it obtained" look identical.
- **The fix is conditional on the tier.** With `linux_process_isolation` false,
  no Landlock scope is applied and `None` is correct, so the descriptor change
  applies only to the process-isolation tier.

### Evidence required to close AC4

1. **Producer (Linux):** a mount-view launch's status stream carries the ABI
   `apply_process_scopes()` returned, and the parent's descriptor names **that
   value** — not the literal 6. Assert against the returned number, or a
   synthetic frame carrying a value ≠ 6; on a kernel whose ABI is 6 the
   assertion is vacuous.
2. **Absence is a claim:** with `linux_process_isolation: true` and no ABI
   record, the descriptor emits a limit naming the absence rather than silently
   reporting `None`; the filesystem-only tier keeps `None` and documents it.
3. **Non-vacuity:** removing the threading makes (1) fail.
4. **Regression:** `sandbox_containment` stays green and every real contained
   launch on Linux is still `Enforced` with the selected backend
   (`verify_reported_backend`, `src/security/sandbox.rs:857`).
5. **Docs:** `architecture/security.md:258-269` no longer says the ABI "is not
   carried into the mount-view descriptor".

**Status consequence.** Acceptance criterion 4's second clause is unmet, and the
record's escape rests on a misreading of §7E, so this milestone moves from
`conditionally closed` to **`corrective pass required`**, bounded to that one
item. The remaining §11 conditions — the runtime mismatch fixture, the mutation
demonstrations and the Linux comparison evidence — are unchanged by this
addendum and remain evidence debt. No code was changed by this investigation.
## 1. Executive finding


Every enforcement claim the launch path makes now has a producer or a recorded disposition, and
the parent no longer trusts the helper's self-report.

**The backend comparison exists and is a shared helper.** `verify_reported_backend(selected,
outcome)` is at `src/security/sandbox.rs:852-867` (doc block `:852-856`, `fn` at `:857`). It is a
new function rather than a call into `guarantee::verify_helper_backend`, and that is the
authorised fallback rather than a shortcut: `verify_helper_backend` takes a `SatisfiedContract`,
which only the deliberately-unwired `negotiate()` produces, so reusing it would have required
wiring the one surface the plan puts out of scope. Plan §7A says so explicitly ("otherwise
compare directly and leave `guarantee.rs` alone"). The helper compares the frame's own backend
identity against the caller's selection and returns a refusal naming both; a non-setup frame
carries no backend and is accepted.

**Wiring — Linux.** `linux_launch::observe_legacy_outcome` calls it as the first thing it does
with an `Enforced` frame (`src/security/sandbox/linux_launch.rs:365-367`), passing
`BackendId::LANDLOCK` because the legacy route selects Landlock and nothing else. The error
propagates through `managed_process`'s `?` at
`src/managed_process.rs:2536-2540`, so a mismatch is `ManagedProcessError::SandboxFailed` before
any target executes — the same fail-closed window as a malformed frame.

**Wiring — macOS (landed mid-review).** The macOS decode site had no comparison when this
review began: `interpret_sandbox_status_for_termination` decoded and matched, and the only
backend check in the tree sat behind `#[cfg(target_os = "linux")]` inside `linux_launch`, a
module that is not compiled on Darwin (`src/security/sandbox.rs:30-35`). While this record was
being drafted the in-flight follow-up landed the same check at
`src/managed_process.rs:2543-2550`, under `#[cfg(target_os = "macos")]`, against
`LEGACY_HELPER_BACKEND` (`src/security/sandbox.rs:873`), with two tests
(`a_frame_naming_another_backend_fails_closed_on_macos`,
`a_frame_naming_the_selected_backend_is_recorded_as_containment`). I re-ran the evidence after
it landed (§4); the three tests pass and the containment family still passes 22/22, which is
what proves the new refusal does not reject the frame a real macOS launch produces. **This
record does not treat that as reviewed-before-handoff work**: it is recorded because it is in
the tree, and the parent should decide whether the macOS arm belongs to C003's delivery or to
the follow-up that produced it.

**`BubblewrapReady` is gone.** The variant, its decoder arm and both consumer arms are removed;
`SandboxLaunchOutcome` now carries `Enforced`, `Unavailable`, `SetupError`, `ExecError`
(`src/security/sandbox.rs:799-830`), and the enum's own docs say why no mount-view receipt
variant exists. `status_decoder_rejects_a_setup_shape_this_tree_never_produces`
(`src/security/sandbox.rs:1597`) encodes a produced shape, asserts it round-trips, then feeds a
literal `{"version":1,"outcome":"BubblewrapReady"}` frame and asserts the decoder fails with
`malformed`. The accepted setup set therefore equals the produced set. Plan §14's stop condition
("removing `BubblewrapReady` contradicts a closure record's claim") did **not** fire: no closure
record in `plans/closure/` claimed the variant. The two edits this forced outside the milestone's
own surface are recorded in §3.

**The Seatbelt probe now tests the facility, not the front end.** `seatbelt::probe()`
(`src/security/sandbox/seatbelt.rs:89-104`) first requires `sandbox_init` to resolve, then keeps
the `/usr/bin/sandbox-exec` check as a separate serviceability signal for the SBPL compiler.
Resolution is `dlsym(RTLD_DEFAULT, "sandbox_init")` behind one reviewed `#[allow(unsafe_code)]`
(`:68-76`) and is never an application.

**`guarantee.rs` is dispositioned, not deleted or wired.** Its module docs now state in one
place that it is "a specification, not a launch gate": `negotiate()` and
`verify_helper_backend()` have no production caller, `verify_helper_backend`'s
`SatisfiedContract` can only come from the unwired `negotiate()`, and what a launch *is* held to
is the reported-backend check (`src/security/sandbox/guarantee.rs:14-28`).
`architecture/security.md:585-600` carries the same disposition in the
"Deliberately test-only and unwired surfaces" table, alongside `host_admission.rs` and the
`windows-sandbox-test-support` feature.

**`host_admission.rs` was not touched and still passes.** `git diff --stat --
src/security/sandbox/host_admission.rs` is empty and `host_admission_manifest` is 7/7 (§4).

**The `landlock_effective` comment now matches the mechanism.** It states that the deny-first
carving is Landlock's alone and that `sbpl_profile` renders `write_roots` and `denied_roots` and
never reads `read_roots` or `read_dir_roots`, so the Landlock shape is not the macOS contract
(`src/security/sandbox/policy.rs:139-150`). `architecture/security.md:608-615` repeats the same
fact from the documentation side.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| §13.1 A helper reporting a backend other than the selected one is refused before target execution | `verify_reported_backend` (`sandbox.rs:857`); Linux call `linux_launch.rs:367` → `managed_process.rs:2536-2540`; macOS call `managed_process.rs:2543-2550`; tests `a_frame_naming_another_backend_is_refused` (host-independent), `a_frame_naming_another_backend_fails_closed_on_macos`, `a_frame_naming_the_selected_backend_is_recorded_as_containment` — all passed | Pass (unit) | No *runtime* mismatch was produced: that needs a helper manipulated to report a falsified backend, which no fixture does. The Linux call site has no host that can execute it here |
| §13.1 (second half) the test fails when the comparison is removed | Not demonstrated | **Not run** | A mutation over `managed_process.rs`/`linux_launch.rs`; outside this record's write scope. The tests assert the refusal text names both identities, so an unconditional `Ok(())` would fail them by construction — reasoned, not observed |
| §13.2 Every accepted setup shape has a producer, or the exception is recorded | `SandboxLaunchOutcome` has four variants and four writers; `status_decoder_rejects_a_setup_shape_this_tree_never_produces` passed | Pass | The removal's own non-vacuity (re-adding the variant and watching the test pass) was not exercised; the test's positive half proves it is not rejecting everything |
| §13.3 `seatbelt::probe()` verifies the symbol without consuming the one-shot | `seatbelt.rs:89-104`; `probe_resolves_the_applied_symbol_without_consuming_the_one_shot` (`:438`) passed: it asserts the real symbol resolves, a bogus symbol does not, and two `probe()` calls agree | Pass | Host experiment in §4 reproduces the underlying one-shot, with one correction to the handed-over figure — see §10 |
| §13.4 The mount-view guarantee list is corroborated by a receipt **or** labelled inferred, and the scope ABI is not discarded | Network claim: `obtained_network_enforcement(spec, observed.child_pid)` rests on the decoded `child-pid` receipt (`managed_process.rs:2705-2712`). The remaining list is labelled request-derived in `architecture/security.md:258-286`. The ABI is now produced by the helper (`codegg-sandbox-helper.rs:433-447`, reported at `:527-540`) and named in the descriptor from the returned value (`managed_process.rs:2754-2758`) | **Pass** — corrected 2026-11-12, see "Corrective implementation" above | The scope ABI is no longer discarded, and an absent record is a stated limit rather than a silent `None`. The *descriptor text* still does not label the non-network entries — the documentation does — so that Low finding remains. Executing the producer/consumer path on Linux is the outstanding evidence |
| §13.4 (test half) "a test fails if the descriptor claims a guarantee with neither a receipt nor an 'inferred' label" | No such test exists | **Not met** | The label exists only in prose, so there is nothing for a test to key on. Recorded in §10 |
| §13.5 The `landlock_effective` comment matches `sbpl_profile` | `policy.rs:139-150`; `security.md:608-615` | Pass | The comment names `write_roots`/`denied_roots`, says `read_roots` is never read, and confines the deny-first carving to Landlock |
| §13.6 `host_admission.rs` and `host_admission_manifest` unchanged and passing | Empty diff for `src/security/sandbox/host_admission.rs`; member 7 passed | Pass | The module is live test infrastructure, not dead code; its docs and imports are untouched |
| §10 focused: mismatch refused; decoder shapes; probe reason off macOS; probe does not consume | Four unit tests, all in the 66-test `security::sandbox` run | Pass | "Reason off macOS" is compile-time on this host: the non-macOS `probe()` body is the constant `Err("Seatbelt is a macOS sandbox and is not available on this platform")` (`seatbelt.rs:106`) |
| §10 integration: `sandbox_containment` on macOS and Linux | macOS 22/22, three runs; Linux **not run** | Partial | The macOS half is observed. The Linux half runs in `ci.yml`'s `ubuntu-latest` job — helper built at `:659-663`, sweep at `:665-680` — so the blocker is that nothing is committed, not a host. That job does not set `CODEGG_SANDBOX_QUALIFICATION`, so a green sandbox member there must be read together with `002-status.md` §10 |
| §10 security: an unproduced outcome is rejected | `status_decoder_rejects_a_setup_shape_this_tree_never_produces` | Pass | — |

## 3. Production implementation evidence

**One new shared helper.** `verify_reported_backend` is a `pub fn` in a `pub mod`
(`codegg::security::sandbox`), so it is part of the library's public surface rather than a
crate-private detail. It takes `(selected: BackendId, outcome: &SandboxLaunchOutcome)` and is
documented as the acceptance rule rather than a probe: "The helper's `Enforced` frame is its own
report; the parent's selection is the decision" (`sandbox.rs:852-856`). It is the only comparison;
there is no second one.

**The variant removal.** `SandboxLaunchOutcome::BubblewrapReady` is deleted. The decoder now has
no arm for it, so a frame carrying it fails serde deserialization and is reported as malformed
rather than decoding into an enforcement record nothing produced.

**Two out-of-scope edits in `src/managed_process.rs`, both recorded here.**

1. **Forced deletion of the removed variant's match arm.** Removing the variant made
   `interpret_bubblewrap_status`'s `SandboxLaunchOutcome::BubblewrapReady => Err(...)` arm
   uncompilable. The hunk deletes exactly three lines; the surviving `outcome =>` catch-all still
   refuses any unexpected setup outcome with `"Bubblewrap helper returned an unexpected setup
   outcome: {outcome:?}"`, so removing the named arm did not open a hole. This edit was
   unavoidable: the variant and its consumer cannot be removed in separate commits in one tree.
2. **A one-line comment reflow.** The decode-site comment "The legacy Landlock route has no
   network dimension: it scopes the filesystem and nothing else, so the obtained network boundary
   is truthfully unrestricted" was re-wrapped to two lines from three
   (`managed_process.rs:2558-2559`), a net one-comment-line reduction, to stay inside that file's
   ratcheted comment ceiling (`docs/comment-budget.toml`, `src/managed_process.rs`). It changes
   no word of substance. The ceiling itself moved `285 → 317` comment lines across the whole
   shared tree, which is the aggregate of C003, C006 and sibling work and cannot be attributed
   to this milestone alone.

**No behaviour change to a launch that succeeds today, except the intended one.** The Linux path
compares a Landlock frame against `LANDLOCK`, which is what `apply_backend` selected on that
route; the macOS path compares against the platform's own backend. The 22/22 containment run
after the macOS arm landed is the evidence that real launches are still accepted (§4).

**Nothing else changed.** No storage, migration or `STORAGE_LAYOUT_VERSION` bump; no protocol,
DTO or durable payload change; `SANDBOX_STATUS_VERSION` stays 1; `negotiate()` is still unwired;
`host_admission.rs` is untouched.

## 4. Verification executed

### Commands run

```bash
cargo build --locked --bin codegg-sandbox-helper          # 3m 01s, helper at target/debug/
cargo test --locked -p codegg --lib security::sandbox
cargo test --locked -p codegg --lib -- a_frame_naming
cargo test --locked -p codegg --lib managed_process
cargo test --locked --test security_permission -- sandbox_containment
cargo test --locked --test security_permission -- sandbox_containment --test-threads=1
cargo test --locked --test security_permission -- host_admission_manifest
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_contract.py --self-test
python3 scripts/check_sandbox_policy_wiring.py
python3 scripts/check_sandbox_policy_wiring.py --self-test
python3 scripts/check_comment_budget.py
git status --porcelain
```

### Results

- `cargo build --locked --bin codegg-sandbox-helper` — succeeded in `3m 01s`; the fixtures launch
  that binary, so it was rebuilt rather than reused. An intermediate build of this tree emitted
  `warning: constant LEGACY_HELPER_BACKEND is never used`, which is independent evidence that the
  macOS comparison did not exist when that build ran; the later test binary has no such warning.
- `cargo test -p codegg --lib security::sandbox` — **66 passed**, 0 failed. It was **65** before
  the macOS follow-up landed; the extra test is part of that change, not of the handoff state.
- `cargo test -p codegg --lib -- a_frame_naming` — **3 passed**: the host-independent mismatch
  test plus the two macOS tests.
- `cargo test -p codegg --lib managed_process` — **26 passed**, 0 failed. This module is where
  `interpret_sandbox_status`'s unit tests live, including the two macOS ones.
- `cargo test --test security_permission -- sandbox_containment` — **22 passed, 0 failed** in the
  default parallel harness, run three times across the review (before and after the macOS arm
  landed, and again once the tree settled) with the same result; `324 filtered out`.
- The serial run (`--test-threads=1`) — **22 passed**, 0 failed. Both runs are green, so the
  parallel-harness flake recorded in `001`/`004` for
  `seatbelt_preserves_and_reports_preopened_denied_file_descriptors` did **not** reproduce here.
- `cargo test --test security_permission -- host_admission_manifest` — **7 passed**, 0 failed.
- `check_sandbox_contract.py` — passed (`0.035 s`); `--self-test` — passed (`0.025 s`).
- `check_sandbox_policy_wiring.py` — passed (`0.221 s`), three `note:` lines naming the accepted
  unrestricted-default sites; `--self-test` — passed (`0.718 s`), nine injected defects each
  rejected.
- `check_comment_budget.py` — **failed at this review, 20 violations**, none under
  `src/security/sandbox/**`, `src/managed_process.rs` or `tests/security_permission/**`. The
  failing files were sibling lanes' in-flight work: the corpus ceilings, `src/main.rs`,
  `src/tui/app/render.rs`, `src/tui/commands/editor.rs`, `src/tui/components/editor.rs`,
  `src/tui/document_session.rs`, `src/tui/editor.rs`,
  `crates/codegg-config/{config_catalog,paths,provenance}.rs`,
  `crates/codegg-core/src/protocol_conversions.rs`, `crates/codegg-protocol/src/dto.rs` and
  `tests/common/session_replay.rs`. **Discharged 2026-11-13** by `scripts/verify.sh full`: the
  ratchet is banked at 139,245 lines and the guard passes on this tree. Every sandbox file named
  in the plan is inside its recorded ceiling.
- **`git status --porcelain` reports 169 modified/untracked paths at `18:20`; it was 165 at
  `17:44`, so the figure grows as sibling lanes work. `git log -1` is `7642b9e38`.** Nothing in
  this milestone is committed; the diff cannot be attributed file by file from the tree alone.
- **Host experiment (outside the repository), one-shot application.** A throwaway C program in a
  temporary directory resolved `sandbox_init` with `dlsym(RTLD_DEFAULT, …)` and then applied a
  profile twice.
  - With a **restricting** profile — `(version 1)(deny default)(allow file-read*)(allow
    process-exec*)` — the first call returned `0` and the second returned `-1` with
    `Operation not permitted`, so the one-shot is real on this host.
  - With `(version 1)(allow default)`, **both** calls returned `0`.
  - In both runs the symbol was resolved *before* the first successful application, which is the
    property the code depends on: resolution consumes nothing.

  The handed-over figure ("a second call returns -1") is therefore reproducible only when the
  first profile actually restricts. See §10.

**Gate class, 2026-11-13.** `scripts/verify.sh full` passed on this tree — 17,377 workspace and
8,441 feature-pass tests, 0 failures; `cargo fmt --check` and clippy clean, and
`check_comment_budget.py` green at 139,245 lines. That discharges the one gate-class sentence in
§4 (the 20 comment-budget violations) tree-wide. It produces none of this milestone's Linux
evidence.

**A brief tree-wide compile blocker, not this milestone, and resolved during the review.** A
re-run at `18:14` could not build the lib test target at all:

```text
error[E0004]: non-exhaustive patterns: `tui::editor::HelixVerb::ToggleComments { .. }` not covered
   --> src/tui/editor.rs:3222:11
error: could not compile `codegg` (lib test) due to 1 previous error
```

That was a sibling TUI lane's in-flight edit — the only error in the build, and not a sandbox
file. The lane fixed it at `18:24:32`, and the re-runs at `18:25-18:31` on the settled tree are
green: `security::sandbox` **66**, `managed_process` **26**, `sandbox_containment` **22**,
`host_admission_manifest` **7**. Every figure in this section is therefore reproduced at the
current tree state, not only at the `≈17:57` snapshot; the guard figures were taken at `≈18:05`.

## 5. Invariant review

- **A constrained request never degrades to host authority.** Preserved: the comparison narrows
  acceptance. A frame naming another backend is refused, never downgraded to "contained by
  something".
- **The status protocol stays bounded, versioned, and fail-closed.** `SANDBOX_STATUS_VERSION`
  is still 1, the frame size caps are untouched, and the removal of the variant makes one more
  input malformed rather than accepted. The duplicate/oversized/terminal-state tests still pass
  in the 66.
- **A decoded frame is never accepted as proof of a guarantee it does not carry.** This is
  exactly what the new helper enforces for the backend dimension. It does **not** verify
  guarantees or limits, and the record does not claim it does.
- **`host_admission_manifest` keeps passing; `host_admission.rs` is not removed.** 7/7, empty
  diff.
- **No launch that succeeds today starts refusing, except a backend mismatch.** The 22/22
  containment run after the macOS arm landed is the operative evidence on this host: every real
  contained launch in that family still decodes as `Enforced` with the selected backend. The
  Linux half of that statement is unobserved here.
- **No descriptor claims a dimension the mechanism did not obtain.** The helper's refusal text
  names both identities, and the scope ABI is now the number the helper reported rather than a
  baseline; the remaining mount-view residue is the labelling of the request-derived entries
  (§10), not an unobserved dimension presented as captured.

## 6. Failure and recovery review

The refusal is a value, not a state change: `verify_reported_backend` returns `Result<(), String>`
and the call sites turn `Err` into `ManagedProcessError::SandboxFailed` before the target runs, in
the same window as a malformed frame. There is no durable state, no retry, no partial ruleset and
no restart semantics — a failed decode aborts one spawn. The probe remains side-effect free, which
is what makes it safe to call repeatedly; `probe_resolves_the_applied_symbol_without_consuming_the_one_shot`
asserts two calls agree in one process. Cancellation and contention are untouched: the check runs
inside one launch's own decode and reads no shared state.

## 7. Migration and compatibility review

No schema, no migration, no durable payload, no public DTO change. **Removing
`SandboxLaunchOutcome::BubblewrapReady` does not change the encoding of any frame that is
produced**, so `SANDBOX_STATUS_VERSION` is not bumped and there is no dual-accept window; a frame
carrying the retired name is now malformed on both sides of the channel. No helper in this tree
emits it, and the helper and parent ship together, so there is no versioned peer to keep
compatible. Rollback is a single call-site deletion per platform. `guarantee.rs`'s types are
unchanged, so the Windows dossier that depends on them still compiles.

## 8. Security review

- **What is now enforced:** a helper that reports a backend other than the one the parent
  selected cannot be recorded as containment. This is the security-relevant half of the
  milestone — before it, the parent's decision and the helper's self-report were never compared.
- **What this does not establish:** the comparison is identity, not negotiation. It does not
  prove the mechanism obtained a dimension; a helper that reports the *right* backend but omits a
  guarantee is still accepted, and the descriptor's `guarantees`/`limits` are taken as reported.
  That is the deferred requested-versus-obtained negotiation, and the docs say so
  (`security.md:585-600`).
- **Residual descriptor honesty gap:** the mount-view guarantee list is still request-derived and
  unlabelled in the descriptor text (`security.md:258-286`). The route still reports `abi: None`
  by design, but the obtained scope ABI now rides the guarantee list (see "Corrective
  implementation"), so that half of this finding is closed. The labelling half is not.
- **Probe safety:** `dlsym` cannot apply a profile, so a probe that runs on every capability
  query cannot spend the helper's single application. The host experiment reproduces the
  application (and its one-shot) on the same host, which is what makes the "resolution consumes
  nothing" claim meaningful rather than circular.
- No secret is introduced or logged; the refusal names two static backend identifiers.

## 9. Documentation and operations

- `src/security/sandbox/guarantee.rs:14-28` — the module's disposition as a specification, why
  neither entry point is wired, and where the launch path's actual backend check lives.
- `architecture/security.md:585-600` — the "Deliberately test-only and unwired surfaces" table,
  covering `guarantee.rs`, `host_admission.rs` and the debug-only Windows harness, and the rule
  that "a surface a launch passes through is a gate; only the second may be described as
  enforcement".
- `architecture/security.md:573-578` — the probe's meaning: symbol resolution first, the front
  end as a serviceability signal, and `apply_profile` as the real gate.
- `architecture/security.md:608-615` — the Seatbelt read-policy facts the `landlock_effective`
  comment now agrees with.
- `architecture/security.md:258-286` — **added by the 2026-11-12 corrective**: the mount-view
  status record (`{"codegg-landlock-process-scope-abi": n}`), the prefix ordering, the
  absence-as-limit rule, the filesystem-only tier, and why `abi` stays `None`.
- `plans/registry.md` was **not** touched by this record, and the C003 plan's `Status:` line still
  reads `corrective pass required`. Both are the parent's to update (§12); with the corrective
  landed the plan header should read `conditionally closed`.
- No guard was added or changed for this milestone. `check_sandbox_contract.py` and
  `check_sandbox_policy_wiring.py` both pass with their self-tests and cover the surrounding
  surface, not this comparison.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Medium | The backend comparison has no *runtime* evidence on either platform | Every test for it feeds a synthetic frame to the decoder; no fixture makes a real helper report a falsified backend, and the Linux call site cannot execute on macOS at all | If a real end-to-end mismatch is wanted: a `lsp-test-support`-style helper binary that reports a fixed wrong backend, launched through `ManagedProcessService`, asserting the spawn fails before the target writes its marker |
| Medium | The "fail when the comparison is removed" demonstration was not performed | §13.1's acceptance evidence is stated as the mutation, and this record has only the reasoned argument (the tests assert both identities in the refusal text) | On a private tree: replace the helper body with `Ok(())`, run `cargo test -p codegg --lib -- a_frame_naming`, observe all three fail, revert |
| Low | The mount-view guarantee list is labelled "request-derived" in `architecture/security.md` but not in the descriptor text, and no test enforces either | A reader of the descriptor alone still sees unlabelled claims — the exact reading the milestone exists to prevent. Plan §10's "a test fails if the descriptor claims a guarantee with neither a receipt nor an 'inferred' label" has no counterpart in the tree | Either prefix the request-derived entries in `managed_process::interpret_bubblewrap_status` and test the prefix, or amend the plan's acceptance text to name documentation as the labelling surface |
| Medium | The ABI record's producer/consumer path has no executed evidence on the only platform it exists | The helper's capture and prefix write, `apply_process_scopes()`'s kernel call, and the `interpret_bubblewrap_status` glue are all `#[cfg(target_os = "linux")]`; only the composition and the readers are host-tested. A type error in the glue, or a helper that writes the record at the wrong point, would not be caught here | Run the three commands in "Corrective implementation → Evidence" in `ci.yml`'s `ubuntu-latest` job with `CODEGG_SANDBOX_QUALIFICATION=required` set, and assert the process-isolation descriptor names the ABI the helper reported. Blocked on WP 0 of `plans/implementation/five-line-closure-corrective/001-closure-evidence-and-residue.md` (the commit), not on a host |
| Low | The mount-view route still reports `abi: None` **by design**, and that is now the correct reading: the field is the backend's version, Bubblewrap has none, and the obtained Landlock ABI rides the guarantee list (`managed_process.rs:2754-2773`). Corrected 2026-11-12 — see "Corrective implementation" | The earlier wording implied the discard remained; it does not. The audit label stays `enforced:bubblewrap:abi=none`, intentionally | Closed. No action beyond keeping the two carriers apart: the guarantee list names the obtained scope, the field names the backend |
| Low | `src/python_script/` still renders `Enforced { abi: None }` with `{:?}` (`executor.rs:543-552`, `projection.rs:120-121`), so "the backend has no ABI" and "this route did not carry one it obtained" remain indistinguishable there | The mount-view correction does not reach this projection; a Python-script run's report cannot say whether a scope ABI existed | Print the guarantee list, or render the outcome structurally rather than with `{:?}`. Bounded and independent of the Linux evidence gap |
| Low | The one-shot figure handed to this review is only reproducible with a restricting profile | `sandbox_init` returned `0` on a *second* `(allow default)` application on this host and `-1` only after a `(deny default)` first profile (§4). The safety property the code relies on is unaffected — resolution precedes a successful application either way — but the blanket wording is wrong | Correct the claim wherever it is repeated; the accurate form is "a second application of a restricting profile is refused, so the application is one-shot in the case that matters" |
| Low | The two macOS tests were added while this record was being written | The record's §1 covers them, but a reader cannot reproduce the reviewed state from HEAD, and a sibling lane may still be editing the same file | Parent to decide whether they belong to C003 or to the follow-up, then commit the tree — WP 0 of `plans/implementation/five-line-closure-corrective/001-closure-evidence-and-residue.md` — so the state is identifiable. As of 2026-11-13 the tree is still entirely uncommitted |

No high-severity finding. The one security-relevant claim the plan asked for — the parent no longer
trusts the helper's self-report — is implemented on both routes and tested at unit level; what is
missing is a runtime mismatch fixture and the mutation demonstration, not the mechanism. Since the
2026-11-12 corrective, the scope ABI is also obtained and reported rather than discarded, and its
composition and readers are host-tested; what it lacks is Linux execution, not code.

## 11. Roadmap disposition

**Updated 2026-11-12:** the ABI half of work package E landed in the corrective recorded above.
The conditions below are the residue after it; condition 4 is narrowed to the descriptor-labelling
question, because the ABI is no longer discarded on the mount-view route.

**Milestone conditionally closed.** Work package A is implemented and wired on the Linux route;
the macOS route carries the same comparison as of the post-review state described in §1. Work
packages B (guarantee disposition), C (variant removal), D (probe depth), F (comment) are
complete and evidenced. Work package E's ABI clause is complete and host-tested in its halves,
with Linux execution outstanding; its labelling question remains open.

Conditions, each with the exact evidence that would discharge it:

1. **Runtime refusal evidence.** A helper stub that reports a backend other than the selected one,
   launched through `ManagedProcessService`, provably fails before target execution — on macOS
   and, in the same form, on Linux. Until it exists, §13.1 rests on unit tests over synthetic
   frames.
2. **Non-vacuity demonstrations.** The helper body replaced with `Ok(())` fails all three
   `a_frame_naming*` tests; re-adding a `BubblewrapReady` variant makes
   `status_decoder_rejects_a_setup_shape_this_tree_never_produces` fail. Both are mutations over
   `src/**`, outside this record's write scope.
3. **The Linux-side evidence for the comparison.** On `ci.yml`'s `ubuntu-latest` job — helper
   built at `:659-663`, workspace sweep at `:665-680` — run `cargo test --locked -p codegg --lib
   security::sandbox` and `cargo test --locked --test security_permission -- sandbox_containment`
   with `CODEGG_SANDBOX_QUALIFICATION=required` set. The Linux call site is inside a
   `#[cfg(target_os = "linux")]` module and has never executed in this environment. The blocker
   is WP 0 of
   `plans/implementation/five-line-closure-corrective/001-closure-evidence-and-residue.md` (the
   commit), not a missing host — and without the marker the member would skip rather than run,
   which is the gap `002-status.md` §10 records. The same run is where the ABI record's
   producer/consumer path would first execute; a fixture asserting the descriptor's ABI text at
   launch level does not exist yet (see "Corrective implementation").
4. **Decide the descriptor-labelling question.** Either label the request-derived entries in the
   descriptor and test it, or record in the roadmap that the labelling surface is documentation.

The question this section used to close on — whether §13.4's ABI clause was mandatory for the
mount-view route — is answered by implementation rather than by reading: the helper now reports
the ABI, and the descriptor names the number the helper returned.

## 12. Registry updates

- Roadmap §12 status table: C003 → `conditionally closed`, closure record
  `plans/closure/sandbox-containment-audit-corrective/003-status.md`, with the four conditions
  above. Its blockers text ("Runtime refusal evidence, non-vacuity demonstrations, Linux-side
  comparison evidence, and the work-package-E labelling decision") must become "Runtime refusal
  evidence, non-vacuity demonstrations, Linux-side comparison and ABI producer/consumer evidence,
  and the descriptor-labelling decision". The row's "none; operational after C006" dependency
  note is satisfied.
- `plans/registry.md:28` (the subsystem row): its summary still says "C003 corrective pass
  required"; with the corrective landed the entry is `conditionally closed`.
- `plans/registry.md:733` (the C003 milestone row): its "AC4 is unmet" text is superseded. The
  row must record that the helper reports the ABI before `exec`, the descriptor names the returned
  number, an absent record is a stated limit, the filesystem-only tier says why, and
  `architecture/security.md:258-286` documents the record — with the Linux producer/consumer run
  as the outstanding evidence. Do not restate the retired `BubblewrapReady` / `guarantee.rs`
  work, which the row already carries.
- The C003 plan's `Status:` line moves from `corrective pass required` to `conditionally closed`.
  **Not edited here:** this session's write scope is the closure record only.
- Unblock audit: C003 had no hard dependency and blocks nothing. Its "operational after C006"
  ordering is satisfied — C006's descriptor work is in the same tree.
- **Not done here, by scope:** the plan's own `Status:` line and `plans/registry.md` are the
  parent's to change; this record touches neither, nor the roadmap status table.

## Correction log (2026-11-13)

Status unchanged: **`conditionally closed`**. The research addendum above is left intact — it is
the diagnosis, and the "Corrective implementation" section is what supersedes its present-tense
claims about the discarded ABI. Nothing here touches it.

- **§4, the `check_comment_budget.py` "failed, 20 violations" claim.** Corrected in place:
  the failures were real at the review snapshot, and **`scripts/verify.sh full` on 2026-11-13
  discharged the gate class** — fmt and clippy clean, `check_comment_budget.py` green with the
  ratchet banked at 139,245 lines, 17,377 workspace + 8,441 feature-pass tests, 0 failures. A
  **Gate class, 2026-11-13** paragraph was added to §4 saying so and saying it produces no Linux
  evidence.
- **"Corrective implementation" §"Evidence", "it needs a Linux host with the helper built
  there."** Corrected to the `ubuntu-latest` job and the commit as the blocker. The addendum's
  diagnosis is untouched; only the lane that can discharge the gap is named.
- **§2, the `sandbox_containment` integration row's note "No Linux host in this environment."**
  Corrected: the Linux half runs in `ci.yml`'s `ubuntu-latest` job (`:659-663`, `:665-680`), so
  the blocker is the commit; the note also points at the marker gap recorded in
  `002-status.md` §10.
- **§10, Medium ABI producer/consumer required action, "On a Linux host with the helper built
  there."** Corrected to the `ubuntu-latest` job with `CODEGG_SANDBOX_QUALIFICATION=required`
  set, blocked on WP 0 of the five-line plan (the commit).
- **§10, Low "commit the tree so the state is identifiable."** Corrected to name WP 0 and that the
  tree is still entirely uncommitted as of 2026-11-13.
- **§11 condition 3, "on a Linux host with the helper built there."** Corrected to the
  `ubuntu-latest` job with the marker set, the commit as the blocker, and the skip caveat named.
