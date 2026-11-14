# Sandbox Containment Audit Corrective Milestone C003 — Enforcement Claims Match the Mechanism

Status: conditionally closed

Repository baseline: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`

Source roadmap:

- `plans/subsystems/sandbox-containment-audit-corrective-roadmap.md#milestone-c003--enforcement-claims-match-the-mechanism`

Long-term requirements:

- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Applicable ADRs:

- `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`
- `plans/adrs/ADR-0046-composable-linux-execution-isolation.md`

Primary class: infrastructure

## 1. Objective

Make every enforcement claim on the launch path traceable to a producer, and make
the parent verify the backend the helper reports instead of trusting it — without
changing the behaviour of any launch that succeeds today.

## 2. Why this milestone is ready

No hard dependency. Every item is local to the claim surface: the helper status
decoder, the backend probe, one doc comment, and the parent's acceptance of a
decoded frame. Nothing here needs a new mechanism, a new ADR, or a change to
`SandboxProfile` semantics.

The one item that would change refusal semantics — full requested-versus-obtained
negotiation at launch — is explicitly deferred in §5, so no unresolved
architectural decision is in play.

## 3. Current implementation evidence

**The parent does not verify the reported backend.**
`apply_backend` selects a backend from the registry, applies it, and returns
`SandboxLaunchOutcome::enforced(selected.id, enforcement)`. The parent decodes the
frame and accepts `Enforced { backend, .. }`. It never compares `backend` against
the backend it selected, although `guarantee::verify_helper_backend()` exists for
exactly that comparison and is unit-tested.

**`guarantee.rs` is orphaned; `host_admission.rs` is not.**
These look alike and are not. `guarantee::negotiate()`,
`verify_helper_backend()`, `RequestedGuarantees`, `AttestedCapabilities`,
`CapabilityFact`, `SatisfiedContract` and `Refusal` appear outside `guarantee.rs`
only in the `sandbox.rs` re-export — there is no production caller and no external
test consumer. `host_admission.rs`, by contrast, is consumed by the
`host_admission_manifest` family member (`tests/security_permission/
host_admission_manifest.inc`, declared at `tests/security_permission/mod.rs:34`)
and is live test infrastructure. Both were deliberate deliverables of recorded
closure work (`plans/closure/windows-native-sandbox/042-status.md` for the
guarantee contract, `plans/closure/linux-sandbox-admission-follow-up-corrective/
002-status.md` for the admission manifest), so neither is accidental dead code —
but only one has a consumer.

**A frame type exists that nothing produces.**
`SandboxLaunchOutcome::BubblewrapReady` has no producer in the tree. The status
decoder nevertheless accepts it as a legitimate setup outcome alongside
`Enforced`, and both consumers then treat it as a failure. A decoder that accepts
an unproduced "ready" claim is the class this milestone exists to close.

**The macOS probe checks a front end, not the facility.**
`seatbelt::probe()` returns `Ok` when `/usr/bin/sandbox-exec` exists. It does not
check that `sandbox_init` resolves in the helper's address space. `sandbox_init` is
one-shot per process, so the probe correctly refuses to call it — but presence of
the CLI is a weaker statement than presence of the symbol the helper actually
links against. Verified on macOS 26.6.2 during the audit: the symbol is still
callable and `sandbox_init` returns 0 for a valid profile, so the backend works;
the probe simply does not test that.

**The mount-view guarantee list is inferred, and the corroborating fact is
discarded.**
`interpret_bubblewrap_status` appends the PID-namespace, IPC, Landlock-ABI-6 and
seccomp guarantees from `spec.linux_process_isolation`, and reports `abi: None`.
The helper already receives a Bubblewrap `child-pid` record and rejects pid 0,
duplicates and out-of-order `exit-code` — so the corroboration exists; the
guarantee list does not use it and the ABI is thrown away.

**One doc comment describes behaviour the code does not have.**
`BackendPolicy::landlock_effective` states "Seatbelt uses the requested roots
directly and applies its explicit final deny rules." `sbpl_profile` uses
`write_roots` and `denied_roots`; it never reads `read_roots`. A future reader
takes the comment as the contract.

## 4. Invariants that must not regress

- A constrained request never degrades to host authority.
- The helper status protocol stays bounded, versioned, and fail-closed on
  malformed, duplicate, oversized or contradictory frames.
- A decoded frame is never accepted as proof of a guarantee it does not carry.
- `host_admission_manifest` keeps passing; `host_admission.rs` is not removed.
- No launch that succeeds today starts refusing, except where the helper reports a
  backend different from the one selected — which is the defect being fixed.
- No descriptor claims a dimension the mechanism did not obtain.

## 5. Scope

### In scope

- Parent acceptance of `SandboxLaunchOutcome::Enforced` — verify the reported
  backend.
- `guarantee.rs` — an explicit, documented disposition.
- `SandboxLaunchOutcome::BubblewrapReady` — producer or removal.
- `seatbelt::probe()` — symbol-level availability.
- `interpret_bubblewrap_status` — corroborate the inferred list or record it as
  inferred, and stop discarding the scope ABI.
- The `landlock_effective` doc comment.

### Explicitly out of scope

- Full requested-versus-obtained negotiation at launch. Wiring `negotiate()` would
  let a request be refused for a guarantee the mechanism never claimed to provide;
  that is a refusal-semantics change and needs its own ADR.
- Removing `host_admission.rs` or its test.
- Any change to the helper's trust channel, spec file handling, or fd plumbing.
- Any change to the grant set (C002) or to descriptor hygiene (C006).
- Any new backend.

## 6. Required production changes

### Security and authorization

The parent compares the decoded `Enforced { backend }` against the backend it
selected and refuses a mismatch. This is a narrowing: a helper that reports a
different mechanism than requested can no longer be read as contained.

### Core/domain

`SandboxLaunchOutcome::BubblewrapReady` is removed, or it gains a producer and its
consumers stop treating it as a failure. The plan's recommendation is **removal**:
nothing emits it, the wire format has no peer that would, and a decoder arm for an
unproduced claim is a liability. If removal would break a recorded closure claim,
keep it and record the reason instead — but do not leave it accepted-and-unverified.

### Storage and migrations

None.

### Protocol and DTOs

Removing a `SandboxLaunchOutcome` variant does not change the encoded shape of any
frame that is produced. If the decision is to keep the variant, and any frame can
carry it, `SANDBOX_STATUS_VERSION` is bumped and the parent accepts both shapes for
one release.

### Runtime and concurrency

`seatbelt::probe()` gains a symbol-resolution check. It must remain side-effect
free: resolve `sandbox_init` (for example via `dlsym(RTLD_DEFAULT, "sandbox_init")`)
without calling it, so the helper's single application is not consumed.

### Frontend or operator surface

None directly. The operator-visible effect is that an `Available` capability
report is backed by the symbol the helper links against.

### Documentation and static guards

`architecture/security.md` records which surfaces are deliberately test-only and
why. The `landlock_effective` comment is corrected.

## 7. Ordered work packages

### Work package A — Verify the reported backend

Intent: the parent stops trusting the helper's self-report.

Required changes: after decoding `Enforced`, compare the reported backend with the
selected one and fail closed on mismatch. Reuse `guarantee::verify_helper_backend`
if its signature fits the call site; otherwise compare directly and leave
`guarantee.rs` alone. Add a test that injects a mismatched frame and observes the
refusal.

Acceptance evidence: the mismatch test fails if the comparison is removed.

### Work package B — Disposition the orphaned guarantee contract

Intent: an audit should not re-report this surface every time.

Required changes: `guarantee.rs` is either consumed (work package A) or documented
as the specification the runtime gate is not. Its module docs must say which, and
`architecture/security.md` must agree. Do **not** delete it: a recorded closure
delivered it and a Windows dossier depends on its types. Do **not** wire
`negotiate()`; that is out of scope.

Acceptance evidence: the module doc and `architecture/security.md` state the same
disposition, and a reader can tell whether it gates a launch.

### Work package C — Remove or produce `BubblewrapReady`

Intent: no decoder arm may accept a claim nothing produces.

Required changes: remove the variant and its decoder arm and its two consumer
arms; or give it a producer and stop treating it as a failure. Recommendation:
remove. If a closure record claims it, record the conflict rather than silently
contradicting it.

Acceptance evidence: the status decoder's accepted setup shapes are exactly the
shapes that have producers, asserted by a test that enumerates them.

### Work package D — Symbol-level Seatbelt probe

Intent: `Available` should mean the helper can link what it needs.

Required changes: resolve `sandbox_init` without calling it. Keep the
`sandbox-exec` check if it adds signal, and keep the failure reason
operator-readable and free of paths that would not help.

Acceptance evidence: on macOS the probe passes and names the facility; off macOS
it still returns the platform reason; a test asserts the probe does not call
`sandbox_init` (for example by asserting the helper still applies a profile after
probing in the same process — the existing
`rejected_seatbelt_profile_does_not_execute_the_target` fixture is the place to
extend).

### Work package E — Corroborate or label the inferred guarantee list

Intent: an inferred guarantee should say so, and the corroborating fact should not
be discarded.

Required changes: use the already-decoded Bubblewrap `child-pid` receipt to
corroborate the namespace claims, or record the list as inferred in the descriptor
text. Stop discarding the effective scope ABI. If C002 is landing the ABI capture
on the default route, do not duplicate it here — coordinate.

Acceptance evidence: a test fails if the descriptor claims a guarantee with neither
a receipt nor an "inferred" label.

### Work package F — Correct the `landlock_effective` comment

Intent: the comment is the contract a future reader will trust.

Required changes: state that Seatbelt consumes `write_roots` and `denied_roots`
and does not consume `read_roots`, and that the deny-first carving is Landlock's
alone.

Acceptance evidence: the comment matches `sbpl_profile` and `landlock::apply`.

## 8. Failure, cancellation, restart, and contention semantics

The backend-mismatch refusal happens after the helper has applied containment and
before target execution, so it fails closed in the same window as a malformed
frame. No new durable state, no restart semantics, no contention. The probe remains
side-effect free, which is what keeps it safe to call more than once.

## 9. Compatibility and migration

Removing a variant nothing produces is source-compatible for every peer. No
persisted configuration, durable payload or public DTO changes. If the variant is
kept and a frame can carry it, the status version is bumped with a one-release
dual-accept window. No migration.

## 10. Required tests

### Focused unit tests

- Backend mismatch is refused.
- The status decoder's accepted setup shapes match the shapes with producers.
- `seatbelt::probe()` returns a readable reason off macOS and passes on macOS.
- The probe does not consume the one-shot application.

### Integration tests

- `sandbox_containment` still passes on macOS and Linux: a valid launch is still
  `Enforced` with the expected backend.
- `host_admission_manifest` still passes (it must not be affected).

### Restart and recovery tests

Not applicable.

### Contention and cancellation tests

Not applicable.

### Security and negative tests

- A helper frame naming a backend other than the selected one is refused before
  target execution.
- A frame carrying an unproduced outcome is rejected.

### Migration and compatibility tests

- If the status version is bumped: a frame in the old shape is still accepted.

## 11. Required verification commands

```bash
# focused
cargo test -p codegg --lib security::sandbox
cargo test -p codegg --lib security::sandbox::guarantee
cargo test -p codegg --lib security::sandbox::seatbelt

# real contained child
cargo test -p codegg --test security_permission -- sandbox_containment
cargo test -p codegg --test security_permission -- host_admission_manifest

# static guards
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_contract.py --self-test
python3 scripts/check_comment_budget.py

# formatting and lint
cargo fmt
cargo clippy -p codegg --all-targets --locked -- -D warnings
```

## 12. Documentation updates

- `architecture/security.md` — the test-only disposition of the guarantee
  contract, the backend verification, and the probe's meaning.
- `guarantee.rs` module docs — the disposition.
- The roadmap §12 status table and `plans/registry.md` on closure.

## 13. Acceptance criteria

1. A helper reporting a backend other than the selected one is refused before
   target execution, proved by a test that fails when the comparison is removed.
2. Every accepted `SandboxLaunchOutcome` setup shape has a producer, or the
   exception is recorded with its reason.
3. `seatbelt::probe()` verifies the symbol the helper links against without
   consuming the one-shot application.
4. The mount-view guarantee list is either corroborated by a receipt or labelled
   inferred, and the scope ABI is no longer discarded.
5. The `landlock_effective` comment matches `sbpl_profile`.
6. `host_admission.rs` and `host_admission_manifest` are unchanged and passing.

## 14. Stop conditions

Stop and report rather than improvise when:

- removing `BubblewrapReady` contradicts a closure record's claim — record the
  conflict instead of editing history;
- verifying the reported backend would refuse a launch the repository currently
  relies on, which would mean the helper and the parent disagree about selection
  in a way this milestone did not anticipate;
- `seatbelt_init` symbol resolution cannot be done without calling the one-shot;
- corroborating the namespace claims would require trusting a receipt the parent
  already rejects as unverifiable;
- the work would require wiring `negotiate()`, which is out of scope.

## 15. Closure evidence required

- The mismatch test with its non-vacuity demonstration.
- The enumeration of accepted setup shapes and their producers.
- The probe behaviour on macOS and off it, with the one-shot assertion.
- The descriptor evidence for the inferred-or-corroborated list.
- The doc diffs, and confirmation that `host_admission_manifest` is unaffected.
- The roadmap §12 and registry updates, including the unblock audit.

## 16. Handoff notes

- `host_admission.rs` is **not** dead. It backs the `host_admission_manifest`
  family member. Do not remove it, and do not "clean up" its imports.
- `guarantee.rs` is orphaned but deliberately delivered. The disposition is
  documentation, not deletion, unless the owner says otherwise.
- The helper's `sandbox_init` is one-shot per process. Any probe change must not
  call it, or the helper loses its only application and every launch fails.
- `apply_backend` returns the outcome the parent decodes; the selected backend is
  available at the call site in `managed_process`. Compare there rather than
  re-selecting, so a probe that changed between selection and launch cannot be
  papered over.
- This plan and **C006** both edit the enforcement-descriptor surface. C006 lands
  first; rebase rather than duplicating the ABI capture.
- If **C002** lands its ABI capture first, this plan's work package E must not add
  a second one.
