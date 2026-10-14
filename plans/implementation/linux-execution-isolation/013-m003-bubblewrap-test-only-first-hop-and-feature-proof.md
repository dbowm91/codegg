# Linux Execution Isolation C005 — M003 Test-Only Bubblewrap First-Hop and Native Feature Proof

Status: **implemented, bounded test-only**; full production adapter remains blocked on ADR-0031 disposition and later enforcement evidence
Class: infrastructure / feasibility
Closure target: `plans/closure/linux-execution-isolation/013-status.md`.
Baseline: after C004 closure @ `5ff248d` (2026-10-14 UTC). Long-term: `plans/000-long-term-specification.md` §§4/17/27/29, `plans/001-terminology-and-domain-model.md` §8, and `plans/003-planning-process.md`. Execution authority: ADR-0001/0004/0016; command classification: currently accepted `plans/adrs/ADR-0032-command-syntax-and-execution-authority.md` (canonical ID to be reconciled under C004). Existing Landlock ABI >=3 filesystem-only support must remain intact. This is a handoff plan, not a claim of implementation or test execution.

## 1. Purpose and factual baseline

M002 is **conditionally closed for internal typed launch contracts/probes only** at `plans/closure/linux-execution-isolation/002-status.md`. `src/security/sandbox/linux_launch.rs::ProductionLinuxLaunchAdapter::plan` returns `EnhancedUnavailable` for enhanced dimensions; `src/managed_process.rs` still launches only the trusted legacy private helper and records Landlock filesystem enforcement with `network=unrestricted`. On the reviewed Linux host, `/usr/bin/bwrap` was present but actual unprivileged user/mount namespace creation was not proven. There is no production Bubblewrap launch, no private process/network namespace, and no cgroup limit enforcement.

The immediate work is a **bounded native real-child experiment** that answers launcher feasibility, privilege and status-pipe questions without registering/enabling a new production backend.

## 2. Test-only authority contract

- Use disposable fixture workspaces, owned private scratch, synthetic secrets and harmless native binaries (Rust/static probe or `/bin/sh`), under an intentionally **test-only** launch adapter. Never add the namespace adapter to a public backend registry, production defaults, or a user-selectable sandbox profile.
- Resolve Bubblewrap helper from a trusted path (not workspace/PATH), verify owner/mode and reject setuid elevation or mutable helper identity; run unprivileged. No installer privilege grant, package manager bootstrap, host mount mutation, or ambient privileged broker.
- Set `no_new_privs`; create user/mount namespace and private mount propagation, narrow read-only runtime binds and scoped workspace/scratch mounts. Do **not** use `--ro-bind / /` as a shortcut and then claim narrow host read confidentiality.
- The target process must run **only after** setup, mount/namespace creation and private status verification have succeeded; failing setup produces **no target marker**. Distinguish host feature probes from *obtained enforcement*.
- Retain canonical `ManagedProcessService` lifecycle, output/timeouts, cancellation, process-group semantics and authenticated private status/spec channels. A helper that forks or becomes PID 1 needs explicit wait/reap/PTY/signal handling tests; do not infer child cleanup from parent exit.

## 3. Ordered test packages

A. Probe supported distro/kernel/AppArmor/SELinux seccomp environment with actual **unprivileged namespace creation** and an inherited-fd/port inventory; record exact error/errno and kernel, user.max_user_namespaces, bubblewrap version/identity and host LSM, not only readable `/proc` handles.

B. Build a disposable first-hop launcher fixture, wiring `LinuxLaunchRequest` to a test-only launch plan that explicitly returns `unsupported` for all unimplemented dimensions. Prove private status pipe is not writable/spoofable by target, malformed/duplicate/oversized receipt rejects, and executed child has the expected namespace identities and mount snapshot. The production `LegacyPrivateHelper` remains the only selected path.

C. Test negative no-userns/AppArmor denial, missing/untrusted helper, symlink/identity substitution, non-root execution, invalid mount inputs, leaked environment/FDs, helper killed mid-setup, timeout/cancel and concurrent attempts. Differentiate supported/no-capability host rather than weakening required launch.

D. Measure plain Bash, cached `cargo metadata --offline`, `rustc --version`, Python stdlib, `git status` and representative stdout/stderr/PTY path. Document observed overhead and incompatibilities before designing hard mounts.

E. Produce explicit *test-only* closure: a capability table for user/mount/pid/net/ipc namespace availability, seccomp and host LSM, helper provenance, no-target-on-setup-failure proof, process ownership, and a recommended M003 backend interface shape. No simulated success may stand in for real kernel proof.

## 4. Constraints, tests and acceptance

`cargo fmt --all -- --check`; `cargo test -p codegg --lib security::sandbox::linux_launch`; `cargo test -p codegg --lib managed_process`; `cargo test -p codegg --test sandbox_containment -- --test-threads=1`; `cargo test -p codegg --test managed_process_descendants -- --test-threads=1`; `python3 scripts/check_sandbox_contract.py`; `python3 scripts/check_execution_ownership.py`; `scripts/verify.sh quick`. Add one targeted, Linux-only **opt-in** real namespace test fixture, not a global privileged CI matrix. Record fixture invocation and exact host details.

Success here is **real evidence for or against feasibility**, plus zero production feature admission and full legacy regressions green. Native failure may still close the bounded experiment as **negative/unavailable**, but may NOT unblock actual production M003. Test-only fixture compilation or feature probing alone is insufficient.

## 5. Operational stop conditions

Stop if an unprivileged helper cannot create the required namespace on the documented supported host; if making it work requires setuid/root/host-wide AppArmor relaxation; if setup status can be forged by child; if a target marker appears after failed required setup; or if parent/namespace descendants cannot be cancelled/reaped correctly. Record a minimal negative reproducer and architecture issue, not a permission workaround.

No persisted schema or public DTO changes. ADR-0031 was proposed when this plan was written and was accepted separately on 2026-10-14; C004 identity cleanup is closed. Acceptance does not qualify or register a production backend.
