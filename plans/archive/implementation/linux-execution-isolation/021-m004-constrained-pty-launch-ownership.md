# Linux Execution Isolation M004 — Constrained PTY Launch Ownership

Status: closed
Archived after C013 closure; current disposition is recorded in
`plans/closure/linux-execution-isolation/021-status.md`.
Repository baseline: `c997586a9aba4df4f7acb8e30239a5e6bc8dc87b`

Source roadmap: `plans/subsystems/linux-execution-isolation-roadmap.md`
Source milestone: `plans/implementation/linux-execution-isolation/004-process-ipc-seccomp.md`
Long-term requirements: `plans/000-long-term-specification.md` §§4,17,27,29; `plans/001-terminology-and-domain-model.md` §8
Applicable ADRs: ADR-0001, ADR-0002, ADR-0016, ADR-0034, ADR-0035
Primary class: capability

## 1. Objective

Route daemon-authorized constrained PTY launches through a supervised M004
owner that applies the qualified Linux process/IPC boundary for the complete
child lifetime, while preserving the existing FullHost behavior and scheduler
ownership. This bounded PTY-owner slice is complete with exact-source hosted
positive and negative qualification. M004 remains active for contained LSP
ownership and broader supported-host qualification.

## 2. Why this milestone is ready

M004's Bubblewrap process/IPC mechanism has exact-head hosted denial,
cancellation, concurrency, ordinary-tool, and restricted-user-namespace
refusal evidence. The daemon resolves PTY profile from the transport-bound
principal preference, and constrained PTY routing now reaches the supervised
owner. C012 rejected a no-writer lease and retained private Git snapshots;
this plan does not alter M003 or enable trusted Git.

## 3. Current implementation evidence

- `src/core/daemon.rs` resolves the profile and passes it to
  `InteractiveProcessProtocol`; preference lookup failure refuses creation.
- `src/interactive_process_attach.rs` transfers the resolved constrained
  profile into the internal spawn specification and launches through the
  supervised M004 owner; FullHost continues through the existing route.
- `src/interactive_process.rs::spawn_unix_child` owns direct `tokio::process`
  spawn, PTY setup, cancellation, reader/waiter tasks, and session lifetime.
- `src/managed_process.rs` owns the helper status/ack handshake only inside a
  finite capture-and-wait operation. Its current API cannot hand a live child,
  PTY descriptors, and status receipt to the interactive session owner.
- The helper's protected Bubblewrap path places the target behind a setup
  acknowledgment and composes PID/IPC namespaces, private procfs, Landlock
  scopes, and seccomp. The current protected mount view still uses private Git
  snapshots; M003 remains unadmitted.

## 4. Invariants that must not regress

- The daemon-resolved profile is authoritative; no request DTO may choose or
  widen it.
- Required containment is established before target user code starts. Any
  helper, namespace, status, identity, or acknowledgment failure leaves no
  target running and no successful handle/audit receipt.
- The PTY master/slave, helper status and acknowledgment descriptors, and
  other inherited FDs are explicitly owned; arbitrary host FDs do not cross
  the boundary.
- The parent retains ownership of the complete descendant lifetime, terminal
  input/output, resize, cancellation, escalation, wait/reap, and scheduler
  permit. A Bubblewrap PID or status line alone is not proof of descendant
  cleanup.
- FullHost routing remains compatible. Constrained routing never falls back
  to direct spawn.
- PTY containment does not admit M003, authorize local/remote Git execution,
  or claim M005 network isolation.

## 5. Scope

In scope: a long-lived helper launch receipt/lifecycle for PTY; trusted daemon
policy transfer; PTY descriptor and controlling-terminal setup across the
helper boundary; process-tree termination and reaping; negative and positive
hosted qualification; route guard and ownership manifest updates.

Out of scope: contained LSP integration (successor work after this plan),
offline network production routing (M005), Cgroup limits, Git snapshot
optimization/admission, persisted public sandbox preferences, and changing
FullHost semantics.

## 6. Required production changes

- Extend or introduce the daemon-owned supervised launch owner so a long-lived
  child can be returned only after the private helper handshake proves the
  requested M004 dimensions were installed.
- Carry a bounded, daemon-constructed `SandboxLaunchSpec` to that owner. Its
  roots come from trusted workspace and profile policy, never the PTY request.
- Make the launch receipt part of the live PTY session owner and retain the
  helper/status resources required to detect setup or terminal failure.
- Ensure `openpty`, `setsid`, controlling-terminal assignment, signal delivery,
  and Bubblewrap's namespace transition produce a usable terminal without
  exposing the host's `/dev/pts` or unrelated descriptors.
- Preserve existing admission ordering: validate trusted policy and establish
  that the selected backend can be requested before scheduler admission;
  release the permit on every setup failure.
- Route only daemon-authorized constrained creates to this owner after
  qualification. Unsupported host setup still refuses before target execution,
  handle creation, or success audit, with no direct-spawn fallback.

## 7. Ordered work packages

1. Define and test the long-lived launch receipt state machine: requested
   dimensions, setup-ready acknowledgment, target start, terminal event, and
   failure before target execution.
2. Implement the helper-backed PTY launch while preserving descriptor
   ownership and status/ack framing. Prove the requested child, not merely the
   helper, is the process supervised by the PTY session.
3. Add daemon routing from the principal-resolved profile. Do not add a
   client-controlled switch.
4. Add exact-host native tests for protected proc/IPC, denied host process and
   socket access, allowed PTY I/O and resize, normal descendant execution,
   cancellation of descendants, and refusal before target marker when user
   namespaces or handshake setup are unavailable.
5. Run ownership/static guards and relevant native tests; obtain exact-SHA
   hosted Linux M004 runs and supported-host policy review. Update M004 and
   roadmap evidence without claiming M005 or M003 admission.

## 8. Failure, cancellation, restart, contention semantics

Any setup or acknowledgment error terminates the helper and all owned
descendants, closes the PTY, releases the scheduler permit, and returns a
typed unavailable result before creating a handle. Cancellation, terminate,
service shutdown, and daemon shutdown must reach the contained process tree
without signaling unrelated host processes. Concurrent sessions receive
independent namespaces and status channels. Restart does not reuse a receipt
or process from another session.

## 9. Compatibility and migration

No persisted config, protocol DTO, or public default changes. FullHost remains
on the existing route. Unsupported constrained hosts keep the current
pre-spawn refusal. No automatic direct-spawn fallback is permitted.

## 10. Required tests

- PTY unit/integration tests for TTY identity, stdin/stdout/stderr, resize,
  exit, cancellation, descendant cleanup, permit release, and failure before
  handle/audit creation.
- Linux helper-backed probes for namespace/process/IPC denial and ordinary
  PTY child operation, including process inspection from the host and child.
- Negative tests for unavailable user namespaces, invalidated path identity,
  malformed/missing/duplicate status frames, missing acknowledgment, helper
  death, target exec failure, and parent cancellation.
- Concurrency test proving two sessions have distinct PID/IPC namespaces and
  cancellation of one cannot affect the other.
- FullHost compatibility regression for launch, resize, terminate, and
  shutdown.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib interactive_process -- --test-threads=1
cargo test -p codegg --test interactive_process -- --test-threads=1
python3 scripts/check_execution_ownership.py
python3 scripts/check_sandbox_contract.py
python3 scripts/check_adr_integrity.py
```

Run the new opt-in native PTY M004 target on the exact source SHA locally when
supported and on hosted Linux. Hosted evidence is mandatory; skipped or
compile-only jobs do not qualify the owner. Run `scripts/verify.sh quick`
after source or manifest changes.

## 12. Documentation updates

Update `docs/execution-ownership.toml`, `architecture/jobs.md`,
`architecture/security.md`, and this implementation plan to describe the
actual long-lived owner and its obtained guarantees. User-facing docs describe
the constrained PTY route as available only when the daemon resolves a
constrained profile and the host supports the required setup; unsupported hosts
fail closed. Record host/kernel/Bubblewrap/AppArmor/user-namespace facts with
hosted evidence.

## 13. Acceptance criteria

- A constrained request can only reach the helper-backed owner from the
  daemon-resolved principal policy.
- The PTY target cannot start before a valid helper receipt and acknowledgment.
- Hosted negative probes demonstrate the target cannot access host proc,
  same-UID host processes, or withheld host IPC; ordinary PTY I/O, resize,
  subprocess use, and owned cleanup pass.
- Unsupported setup fails before target marker, handle creation, and success
  audit; no direct-spawn fallback occurs.
- FullHost behavior and scheduler accounting pass their regressions.
- Exact-source hosted Linux qualification, execution-ownership guard, sandbox
  contract guard, ADR guard, and quick verification pass.
- M004 remains open for LSP owner integration and broader host qualification;
  this plan does not close M004 alone.

## 14. Stop conditions

Stop and preserve constrained refusal if Bubblewrap cannot retain a useful
PTY/controlling-terminal contract while keeping host proc/devpts inaccessible,
if the parent cannot reliably own descendants after helper exit or cancellation,
or if the helper receipt cannot be bound to the actual target and full live
session. Do not weaken M004, bypass the private handshake, use a host-side PTY
spawn followed by attempted adoption, or permit uncontained fallback.

## 15. Closure evidence required

Closure record `plans/closure/linux-execution-isolation/021-status.md` with
the requirement-to-evidence matrix, exact source/host identities, local and
hosted results, negative setup behavior, lifecycle/cancellation evidence,
FullHost compatibility, ownership and docs changes, unresolved findings, and
an explicit statement that LSP/M005/M003 remain independently gated.

## 16. Handoff notes

The current helper handshake is embedded in `ManagedProcessService::run_inner`
and is oriented around finite output capture. Do not copy it into a second
owner without consolidating status parsing and one-shot ACK rules. The PTY
owner needs an explicit long-lived lifecycle contract. Once this plan closes,
the next bounded M004 slice should integrate egglsp's existing
`LspProcessLauncher` seam with a distinct daemon-owned constrained service;
M005 production-owner work follows both M004 routes.
