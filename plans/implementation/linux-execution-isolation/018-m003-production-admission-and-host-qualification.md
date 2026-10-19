# Linux Execution Isolation C010 — M003 Production Admission and Host Qualification

Status: implemented
Repository baseline: merge commit `188edb371759f40b291052f25598bdb8ab57702d` (includes `origin/main` at `de49884f3fdc6d528620f2a2645694b9b19fc087`)

Source roadmap: `plans/subsystems/linux-execution-isolation-roadmap.md`
Long-term requirements: `plans/000-long-term-specification.md` §§4, 17, 27, 29; `plans/001-terminology-and-domain-model.md` §8.
Applicable ADRs: accepted ADR-0043 and ADR-0042; ADR-0001, ADR-0002, ADR-0004, ADR-0016.
Primary class: capability / security invariant

## 1. Objective

Make an evidence-backed production-admission decision for the opt-in Bubblewrap protected-mount candidate on explicitly described Linux host configurations. Reconcile the feature branch with current main, measure the C009 private Git object view on representative packed and fragmented repositories, qualify supported developer-tool paths, and keep production routing disabled unless every admission gate is met.

This corrective is bounded to the M003 admission decision. It does not claim M003 capability closure when the correct result is no admission.

## 2. Why this milestone is ready

- ADR-0043 is accepted, ADR IDs are unique, and C009 completed bounded candidate qualification.
- The merge with current `origin/main` is committed at `188edb371759f40b291052f25598bdb8ab57702d`; C010 evidence and the admission disposition are recorded at `plans/closure/linux-execution-isolation/018-status.md`.
- C009 names the remaining blockers: object-view performance, packed-store evidence, rustup's `/proc/self/exe` requirement, and the absence of a production backend route.

## 3. Current implementation evidence

The candidate remains opt-in and unregistered for production. Landlock remains canonical. Exact post-merge hosted Linux run `38749191367` on `f6befe51` passed 11/11 protected-mount and 18/18 trusted-Git tests. Its ephemeral Ubuntu 24.04.5 runner used kernel `6.17.0-1022-azure`, Bubblewrap `0.9.0`, AppArmor `unconfined`, UID/GID 1001, and relaxed `kernel.apparmor_restrict_unprivileged_userns` from 1 to 0. C009's earlier local fragmented-store result was 2.95 seconds for a 105 MiB / 3,945-file object store on ext4 without reflink support. The current workstation is Ubuntu x86_64, kernel 6.8.0-147-generic, ext4, Bubblewrap 0.9.0, UID/GID 1000, AppArmor enabled with that userns restriction left at 1; local native tests passed 11/11. Temporary exact-wrapper, cache-warm measurements recorded 325 ms for a mixed 105 MiB object store and 47 ms for a 29.07 MiB single-pack clone. These are exploratory single samples, not a production latency qualification.

The protected mount view intentionally omits host `/proc`; the C009 record documents that rustup needs `/proc/self/exe`. C009's Cargo check/test, direct rustc, Python, shell, PTY first-hop, and selected Git routes passed, but this does not establish general toolchain/LSP compatibility.

## 4. Invariants

- Preserve accepted ADR-0043 and unique canonical ADR identifiers.
- No protected-mount production backend registration, default change, or automatic fallback is permitted from this qualification alone.
- Required enhanced launch fails before target execution when Bubblewrap, user/mount namespaces, or the necessary host policy are unavailable.
- Requested and obtained filesystem scope remain distinct from process, IPC, seccomp, network, and resource guarantees.
- Workspace data and repository metadata remain adversarial; performance improvements may not use hard links or trust mutable object files.
- Hosted evidence must match the exact implementation commit and must not require an undocumented host-policy mutation to be described as generally supported.

## 5. Scope

### In scope

- Reconcile the branch to current main and run ADR, ownership, sandbox and relevant regression guards.
- Measure cold and warm private-object-view preparation for packed and fragmented repositories on ext4 and a reflink-capable filesystem where available.
- Exercise rustup/rustc/Cargo, Python, PTY, test children, and one available LSP path through the actual candidate.
- Define a host capability matrix by observed kernel, Bubblewrap, user-namespace policy, LSM and filesystem behavior; do not infer support from distribution names alone.
- Record an explicit admit / conditional / do-not-admit decision for each tested configuration.

### Out of scope

- Changing the Landlock default, registering the Bubblewrap candidate for normal production routing, changing public or persisted policy, or changing an accepted ADR.
- Solving protected object-view cost by hard links or a mutable cross-command cache.
- Implementing Git object-writing routes (C002), process/IPC/seccomp containment (M004), offline networking (M005), mediated egress (M006), or cgroup resource limits (M007).

## 6. Ordered work packages

1. Reconcile current main and run integrity/contract guards. Preserve the accepted Linux ADR and canonical IDs.
2. Run the exact-head native protected-mount and trusted-Git suite on a hosted Linux runner. Record kernel, Bubblewrap, userns and LSM facts, including any runner policy changes.
3. Measure the exact private-view preparation path across packed and fragmented object stores, with warm repeated samples and an honest cold-start limitation if cache eviction is unavailable. Establish a documented latency budget before recommending admission.
4. Run the compatibility probes through the actual mount view. Report rustup `/proc` behavior, LSP/PTY and ordinary toolchain outcomes separately.
5. Decide admission per tested host configuration. If any security, compatibility, performance or host-policy gate remains unqualified, keep production admission disabled and record the exact next gate.
6. Audit M004/M005 dependencies and register M004 as the next implementation target only if the M002 contract and M003 interface are stable; preserve M005's dependency on M004.

### M003 performance acceptance budget

Use paired baseline/candidate samples on the same host and filesystem, with a
recorded commit, kernel, storage type, repository object counts, loose/packed
bytes, and cache state. Measure the private object-view preparation separately
from Git's own command time, then also report end-to-end `git status` latency.
Use at least 100 warm samples and 30 cold samples per repository shape; report
median, p95, and max. If cold-cache control is unavailable, label that result
unqualified rather than infer it from a warm run.

Admission requires both a fragmented store (at least 4,000 loose objects and
100 MiB total object data) and a packed store of equivalent object content to
meet all of these budgets:

- private-view preparation: warm p95 <= 250 ms and cold p95 <= 1,000 ms;
- end-to-end `git status` overhead attributable to the protected view versus
  the paired unwrapped baseline: warm p95 <= 500 ms and cold p95 <= 1,500 ms;
- no individual view preparation may exceed 3 seconds, and the candidate must
  not increase the paired baseline's warm p95 by more than 2x.

These are production-admission limits, not claims that current samples pass.
If either store shape or filesystem exceeds a limit, that host profile remains
unadmitted. Optimization may not weaken object immutability, path identity,
race handling, or trusted-Git isolation to meet the budget.

### Tool compatibility admission threshold

The compatibility gate is a finite all-pass matrix for every host profile
proposed for admission. Each workflow must complete with its expected exit or
protocol result, within the stated bound, and through the selected contained
launch route; an unavailable feature, fallback launch, or unexplained
permission failure is a matrix failure:

| Workflow | Required observable result | Maximum time |
|---|---|---:|
| Rustup toolchain resolution | Selects the pinned toolchain and reports its compiler | 60 s |
| Rust compiler | Compiles and executes a small workspace-local program | 60 s |
| Cargo | `cargo check --locked --workspace` succeeds | 15 min |
| Test runner | The managed-process regression filter succeeds | 15 min |
| Python and shell | A workspace-local script/command reads and writes only its granted workspace and exits successfully | 60 s each |
| Trusted Git | A representative read command succeeds and returns the expected result | 60 s |
| Production PTY | Input/echo, resize, output read, and termination complete through the contained route | 30 s |
| Production LSP | Initialize, open one workspace file, receive a response, and shutdown through the contained route | 60 s |

Admission requires every row to pass on the same exact host profile, with no
skipped rows. M004 process isolation is part of the route for PTY and LSP; a
standalone helper smoke does not count. The restricted-user-namespace host
must instead demonstrate refusal before target execution. These are binary
compatibility gates, separate from the object-view latency budgets above.

## 7. Failure and recovery semantics

Missing namespaces, rejected Bubblewrap setup, status-channel failure, unsupported mount topology, identity change, or an incomplete required capability must fail before target execution. Benchmark failure or unavailable host/filesystem evidence is an unqualified result, never a pass. No admission decision may silently fall back to Landlock while retaining the stronger requested label.

## 8. Compatibility and migration

No schema, protocol, or default changes are in scope. Existing Landlock behavior stays supported. Any future production preference requires separate versioned policy and migration review.

## 9. Required verification

```bash
python3 scripts/check_adr_integrity.py
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_policy_wiring.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_git_forbidden_patterns.py
cargo test -p egggit --locked -- --test-threads=1
CODEGG_RUN_C006_BWRAP_NATIVE=1 CODEGG_RUN_C009_BWRAP_NATIVE=1 cargo test --locked --lib protected_mount_ -- --nocapture --test-threads=1
cargo test --test sandbox_containment -- --test-threads=1
cargo test --test sandbox_landlock -- --test-threads=1
scripts/verify.sh quick
```

The hosted native job must run the exact same protected-mount/trusted-Git test filters after the implementation commit. Record cross-platform compile evidence separately from Linux native enforcement.

## 10. Acceptance criteria

- Current main is merged without losing the accepted ADR or reusing a canonical ADR ID.
- Exact-head native hosted evidence passes for the protected-mount and trusted-Git suites.
- Packed and fragmented object-view costs use the paired 100-warm/30-cold protocol and meet the explicit latency budgets above, or produce a no-admission outcome.
- Toolchain and procfs limitations are explicit and tested.
- Each tested host configuration is identified by kernel/Landlock ABI, Bubblewrap version/trust result, AppArmor/SELinux state, unprivileged-userns result, and filesystem. Every configuration has a clear admission status; no configuration is called production-supported based only on compile or fixture evidence.
- Registry and roadmap identify M004/M005 sequencing and remaining blockers.

## 11. Stop conditions

Do not admit a host if user namespace policy had to be changed without an operator-owned supported configuration, the candidate cannot prove the required filesystem view, trusted Git cost exceeds the accepted budget, a common developer tool silently escapes or fails without an actionable result, or exact-head hosted native evidence is unavailable.

## 12. Closure evidence required

Create `plans/closure/linux-execution-isolation/018-status.md` with exact commit and hosted run, host matrix, benchmark samples and caveats, compatibility results, explicit admission disposition, unresolved findings, and the registry dependency audit. A negative admission result may close this bounded decision plan, but does not close M003.
