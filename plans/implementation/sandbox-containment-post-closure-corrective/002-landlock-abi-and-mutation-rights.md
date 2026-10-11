# Sandbox Containment Post-Closure Corrective C002 — Landlock ABI and Complete File-Mutation Rights

Status: implemented

Repository baseline: `main` `8e9d8b01e5c229715c8e4dea929e050b391e252c` (2026-10-10).

Source roadmap: `plans/subsystems/sandbox-containment-post-closure-corrective-roadmap.md#7-milestones-and-useroperator-exits`

Long-term authority: `plans/000-long-term-specification.md`, `001-terminology-and-domain-model.md`, `002-long-term-roadmap.md`, `003-planning-process.md`; ADR-0004.

Predecessor evidence: runtime-safety M001/C001 implementation and `plans/closure/runtime-safety-resource-footprint/{001,009,010}-status.md`. This is a new defect found after conditional closure; do not rewrite earlier dispositions.

Primary class: invariant (actual read-only/WorkspaceWrite filesystem mutation limits).

## 1. Objective

Correct Landlock's hardcoded `ABI::V1` rights so Codegg does not claim an enforced read-only/workspace-write boundary while file truncation (and later relevant mutation operations) remain outside the handled access set. Implement capability/rights negotiation and an explicit unsupported-kernel disposition, then prove *actual* file mutation prevention on a supported Linux host.

## 2. Readiness and source evidence

`src/security/sandbox/landlock.rs::apply` constructs:
```rust
let abi = ABI::V1;
let handled = AccessFs::from_all(abi);
```
Then `restrict_self()` can report `FullyEnforced`, which proves the **requested** ABI v1 rules were installed, not that modern destructive rights were handled. `LANDLOCK_ACCESS_FS_TRUNCATE` was introduced with Landlock ABI v3 (Linux 6.2). `LANDLOCK_ACCESS_FS_REFER` was introduced with ABI v2, and should be represented in the chosen handled-set where available; documented Landlock restrictions have additional ABI-version differences. `src/security/sandbox/landlock.rs::probe` only asks for ABI v1 read rights. Existing fixtures test regular writes and read-only openings, not content truncation on already accessible files.

References: https://docs.kernel.org/userspace-api/landlock.html and https://man7.org/linux/man-pages/man7/landlock.7.html . Validate supported `landlock` crate API at implementation time, not hardcode guessed APIs.

## 3. Invariants

- A process reported `ReadOnly` cannot truncate, append, overwrite, rename into, link into, or otherwise mutate a non-writable ordinary file under capabilities Codegg claims to enforce.
- A process reported `WorkspaceWrite` may mutate only specifically granted persistent workspace roots; outside files retain bytes and metadata when tested.
- `FullyEnforced` is not sufficient proof of protection for an access class omitted from `handled_access`; backend capabilities and effective rights are reported correctly.
- No untrusted code executes under a required policy unless the active host supports its necessary rights. A lower ABI must not be silently promoted.
- `no_new_privs`, helper-only restriction, status-channel integrity and process-supervision ownership remain unchanged.
- Device access required for shell redirection must continue to work without allowing mutations to arbitrary devices.

## 4. Scope and technical decision

**In:** supported ABI query, minimum ABI requirements per claimed policy, modern handled-rights ruleset, rights mapping to directories vs special files, version-aware diagnostics and fail-closed unsupported-host behavior, real-helper regression tests.

**Out:** new sandbox backend, raw syscalls, seccomp, network isolation, namespace/container support, rewriting generic policy/credential excludes (C003), universal compatibility with Linux kernels unable to enforce essential rights.

Policy must distinguish what the kernel can enforce from what it cannot. Default recommendation: require at least ABI v3 for the strong filesystem-mutation guarantee; use all relevant implemented rights for the effective supported ABI, with carefully audited compatibility behavior. If a future older-ABI compatibility tier is retained, it **must** be explicitly authorized as weaker, and must never be described as protecting against truncation.

## 5. Required production changes

**Capability probe:** `landlock::probe` must check the *minimum effective rights* needed by Codegg's claimed sandbox contract, rather than merely creating a v1 read-only ruleset. Expose a readable kernel/ABI incompatibility reason, avoiding a generic "Landlock available" false assurance. Verify with the maintained `landlock` crate's own compatibility/ABI helpers.

**Ruleset builder:** `landlock::apply` shall select and handle modern filesystem access rights, including `TRUNCATE` and relevant `REFER` behavior. Explicitly enumerate which rights are enforced by each supported kernel ABI. Avoid treating unhandled unknown future ABI rights as if available; do not accidentally request unsupported rights under `HardRequirement`. Preserve ruleset validation, file-vs-directory access mask filtering, `FullyEnforced`, and `status.no_new_privs`. If process request promises protection the kernel cannot enforce, abort before target exec.

**Status/diagnostics:** carry *actual selected policy ABI*, *host effective ABI* if distinguishable, and capabilities/limits without misleadingly reporting v1-only protection as complete. Keep the current wire ABI type backwards compatible if possible. A backend-level `guarantees` list should name the protections actually installed and an old tier's omitted rights explicitly.

**Caller behavior:** C001 separately standardizes fail-closed invocation; this milestone itself must ensure the helper rejects unsupported ABI under any `SandboxRequest::Required`. Keep Linux Bash/Python paths consistent and avoid platform-dependent policy widening.

## 6. Ordered work packages

A. Confirm effective Landlock ABI and crate support on a Linux 6.2+ kernel; document at least ABI v1/v2/v3 behavior and which classes of mutation remain vulnerable when `TRUNCATE` is not handled.

B. Add an adversarial regression: create an existing outside-workspace file with known contents, attempt `truncate -s 0`, `open(..., O_TRUNC)`, Rust `OpenOptions::truncate(true)`, and a shell truncating redirection under required confinement; confirm that the file *retains bytes*. Use separate distinct inside-workspace fixture to confirm approved transforms work; run ReadOnly against inside-workspace truncation.

C. Update `probe`, rights negotiation and `apply`. Reconcile device-node rights (`/dev/null` etc.) and special file filtering. Ensure `status` and rule enforcement agree.

D. Cover rename/link/hardlink exchange operations crossing boundary where versioned `REFER` semantics matter; characterize limitations and avoid claiming full kernel-mediated inode confinement if unsupported. Include a subprocess-inherited confinement probe and no-new-privs outcome.

E. Reconcile `tests/sandbox_landlock.rs`, `tests/sandbox_containment.rs`, `architecture/security.md`, and policy wiring/contract guards. No new CI lane; emit useful ABI and skip information from existing focused fixture.

## 7. Failure/restart/concurrency/compatibility

Probe/application race: capability may change; final helper application is authoritative and must return typed setup failure, never run command uncontained. A sandbox restriction is process-local and inherited by spawned targets; no daemon state and no database migration. Simultaneous helpers have isolated kernel rulesets, no shared mutable policy. Timeouts/cancellation terminate child process group. An old-kernel explicit weak mode requires approval semantics consistent with C001; do not introduce it merely to preserve green CI. Tests on unsupported kernels skip *positive enforcement qualification* with exact reason, while negative no-exec behavior remains tested.

## 8. Required verification

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib security::sandbox::landlock
cargo test -p codegg --test sandbox_landlock -- --test-threads=1
cargo test -p codegg --test sandbox_containment -- --test-threads=1
python3 scripts/check_sandbox_contract.py
scripts/verify.sh quick
```

Use actual test names and supported host gates; run one focused supported-Linux host (kernel >=6.2, ABI >=3) and record kernel release, actual and requested ABI, each mutation probe result, exit status and target marker. Optionally exercise older kernel via fixture/injected capability model without fabricating real-kernel evidence. No extra hosted matrix required.

## 9. Acceptance and stop

**Accept only when:** Linux helper installs required filesystem rights including `TRUNCATE`, read-only and outside-workspace truncation are denied on a supported kernel, approved workspace mutation succeeds, `no_new_privs` stays on, and unsupported old kernels do not silently run under strong profiles. `FullyEnforced` must be tested against the policy's required rights, not accepted by itself.

**Stop** if the maintained Landlock crate cannot express required rights without an unsafe/handwritten implementation, supported kernel evidence contradicts expected enforcement, a global config/API migration is required, or the Linux minimum support tier must be changed without authorization. Record the narrow blocker and propose a reviewed approach instead of reporting false completion.

## 10. Closure evidence

Create `plans/closure/sandbox-containment-post-closure-corrective/002-status.md`. Include accepted SHA/PR, tested kernel and Landlock ABI, rights matrix, all inside/outside/write/truncate/refer probes, helper and `no_new_privs` status, actual commands/outcomes, negative old-kernel disposition, static guards/quick result, unresolved issues and C003 readiness. Preserve predecessor conditional evidence separately.
