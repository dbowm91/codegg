# Sandbox Containment Post-Closure Corrective C003 — Effective Denials and Scoped Tool-State Writes

Status: implemented (closed; Linux ABI 4 and native Apple Silicon qualification complete)

Repository baseline: `main` `8e9d8b01e5c229715c8e4dea929e050b391e252c` (2026-10-10).

Source roadmap: `plans/subsystems/sandbox-containment-post-closure-corrective-roadmap.md#7-milestones-and-useroperator-exits`

Long-term authority: `plans/000-long-term-specification.md`, `001-terminology-and-domain-model.md`, `002-long-term-roadmap.md`, `003-planning-process.md`; ADR-0004.

Historical predecessors: `plans/implementation/runtime-safety-resource-footprint/001-landlock-and-sandbox-contract-correction.md`; `plans/closure/runtime-safety-resource-footprint/{001,009,010}-status.md`. Current immediate predecessors: new C002 (hard) and new C001 shared-launch interface (interface).

Primary class: invariant (credential confidentiality, integrity, and write-authority boundary).

## 1. Objective

Make the same **effective** credential exclusion and persistent-write ceiling apply on Linux Landlock and macOS Seatbelt. Eliminate overbroad global writable cache/toolchain roots, correct overlap precedence, and confine agent-produced temporary artifacts to lifecycle-owned locations while keeping common shell, compiler, test and Git workflows functional.

## 2. Current evidence

`src/security/sandbox/policy.rs` declares `BackendPolicy {read_roots,write_roots,denied_roots}`, but:
- `tool_read_allowances()` includes all of `~/.config`, `~/.cargo`, `~/.rustup`, and other shared tool state. `sensitive_deny_paths()` names `~/.config/gh/hosts.yml`, `~/.config/codegg`, `~/.ssh`, `~/.aws`, `~/.cargo`-adjacent credentials but cannot retract Landlock access underneath an allowed ancestor.
- `src/security/sandbox/landlock.rs` installs grants for read and write roots and ignores `denied_roots` (the allowlist must be constructed without covering denied descendants).
- `src/security/sandbox/seatbelt.rs::sbpl_profile` emits sensitive write denials before blanket write denial and writable subpath re-allows. The final re-allow can supersede a sensitive write-deny underneath the writable ancestor.
- `tool_write_allowances()` includes global `~/.cargo`, `~/.rustup`, `~/.npm`, `~/.cache`, `~/.local/state`, `~/Library/Caches`, and shared `/tmp`; the profile describes workspace confinement more strongly than these persistent and cross-job grants permit.
- `BackendPolicy::normalized` drops nonexistent roots. For **deny** paths this may omit a future sensitive filename or directory; policy construction must not silently turn absence at setup into future access.
- `src/security/sandbox.rs::SandboxConfig::launch_spec` and `src/python_script/executor.rs::build_landlock_paths` have potentially divergent roots; code and tests should ensure one authorization ceiling for both consumers.
- The current test for a sensitive path places its fixture outside all allowed roots, which does not exercise nested exclusions.

## 3. Security invariants

- An explicit sensitive deny is never defeated by a broader allowance. If a backend cannot express a requested allow/deny intersection, reject the requested policy or narrow its grants; never pretend it is enforced.
- Credential data readable by a trusted host process is not thereby readable by an untrusted child. No `~/.config`-wide or private-key-containing ancestor allowlist is permitted while advertising protection of sensitive descendants.
- Workspaces remain the only generally writable persistent project tree; no global package-manager configuration, credential, executable toolchain, or unrelated session scratch tree is writable under `WorkspaceWrite`.
- `ReadOnly` cannot mutate user data, including through retained device allowances or shared writable roots; explicitly safe character-device I/O is narrowly permitted where supported. C002 ensures Landlock `TRUNCATE` is controlled.
- A sandboxed child cannot receive capabilities via path symlink, canonicalization races, relative paths, environment-supplied scratch roots, inherited FDs, cross-workspace aliases, or missing deny paths.
- No network/IPC isolation claims are introduced.

## 4. Scope and non-goals

**In:** normalized effective grants, effective deny path precedence, secure system/toolchain read roots, scoped scratch/cache creation and lifecycle, Python/Bash consistency, macOS device write correctness, status wording, tests and operator docs.

**Out:** full macOS deny-first read framework and IPC restrictions (C004), new container/mount namespaces or layered filesystems, arbitrary per-tool privilege broker, external package managers, global home-directory encryption, a new general credential manager.

## 5. Target policy

Define and validate a policy whose effective read set, write set and exclusions can be translated by *every registered backend* without broadening grants. The renderer must reject an unrepresentable policy rather than silently ignoring it.

For Linux, Landlock is an allowlist: do not grant an ancestor when a sensitive descendant must be denied. Grant safe directories/files separately, even if this requires narrowing `~/.config` and cache roots into a curated target-specific set. If a utility requires a credentialed read, it should be an explicitly authorized trusted action with its own authority, not a default permission of general agent Bash/Python.

For macOS, verify SBPL evaluation order with real helper tests and emit explicit credential read/write denials *after* the overlapping allow rules (or an equivalently proven non-overridable rule strategy). Include path aliases/resolved roots and future-create cases without trusting string containment.

Write roots: workspace, narrow per-run scratch, and *only* experimentally demonstrated safe cache paths. Prefer environment-configured per-run `CARGO_HOME`/target/cache, `RUSTUP_HOME` read-only, `TMPDIR` scoped to a fresh 0700 directory, and analogous supported tool settings; copying/seeding large toolchains on every process is not acceptable. Distinguish read-only shared toolchain/config files from writable per-run build products. Avoid granting shared `/tmp` as globally mutable scratch where a contained alternative works. Preserve `/dev/null` and necessary terminal-device I/O narrowly, not a broad `/dev` write grant.

A path named in `denied_roots` that does not exist needs an explicit policy: protect it by canonicalizing the nearest existing parent and using safe lexical remainder for Seatbelt, or reject a policy that cannot reliably protect creation. Do not silently drop the exclusion. Policy preparation failures are errors before target exec.

## 6. Ordered work packages

A. Inventory actual Linux/macOS filesystem access for `sh`, `git status`, `git diff`, `cargo check`, `cargo test`, `rustup`, Python, and representative `gh` usage. Record required *read* and *write* paths and behavior under a clean HOME/profile. Do not use permissive HOME reads as the fallback answer.

B. Build adversarial fixtures FIRST: secret fixture under `~/.config` with read-granted ancestor; credential under writable workspace; `~/.cargo/credentials.toml` and `~/.cargo/config.toml`; outside persistent cache; shared temporary directory belonging to another session; newly created previously absent deny path; symlink and hardlink alias behavior. Assert both read and write denials where required, including `O_TRUNC`, and approved inside-workspace writes.

C. Refactor `BackendPolicy` construction/preflight so backend renderers must prove exclusions and reject overlaps they cannot express. Narrow Landlock read paths; reorder/strengthen Seatbelt denial rules. Handle missing paths and symlink aliasing explicitly; do not turn `canonicalize` failures into `unwrap_or(raw)` if doing so broadens a grant.

D. Introduce lifecycle-owned scratch/cache roots through `ManagedProcessService` / launch preparation with secure permissions, explicit child environment, collision-resistant per-run names, bounded cleanup after exit/cancel and optional stale-cleanup sweep after abnormal crash. Avoid a new global service or database. Verify child process groups cannot use another session's scratch even when their uid matches via generic writable-ancestor access.

E. Align Python, Bash, typed/native and scheduler-owned execution *where the same sandbox profile is requested*. Inspect their actual authoritative calls; record intentional exemptions. Avoid routing an operation into a more privileged executor because the profile is inconvenient.

F. Update `architecture/security.md`, `architecture/python_scripting.md`, `docs/cli.md`, any operator status text and `scripts/check_sandbox_policy_wiring.py`. Ensure guarantee text says "workspace plus scoped scratch" only when that is actually the installed policy. Do not rewrite historical closure claims.

## 7. Failure, concurrency, cancellation, migration

A path alias, invalid root, conflicting deny, unsafe scratch permission, cache policy incompatibility, or unrepresentable Landlock exclusion fails the required launch before target code. Per-run scratch ownership is race-safe; cancellation waits for descendant termination before cleanup when practical. Distinguish restart-safe stale scratch cleanup from deleting directories that might belong to a live process. No durable state or database migration; environment variables and config defaults may need a documented compatibility migration and performance impact measured.

Compatibility safety tightening is acceptable only where explicit user-visible: a tool requiring global HOME writes should produce an actionable denial and a scoped approved remedy, not silently switch to `FullHost`.

## 8. Required verification

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib security::sandbox
cargo test -p codegg --test sandbox_containment -- --test-threads=1
cargo test -p codegg --test sandbox_landlock -- --test-threads=1  # Linux
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_policy_wiring.py
scripts/verify.sh quick
```

Reuse real-helper fixtures and add a focused utility smoke rather than large integration matrices. Run native Linux and macOS enforcement probes on qualified hosts; if one is missing, record conditional evidence instead of claiming security parity. Capture baseline vs corrected command compatibility and bounded overhead/cold-cache behavior where changes are meaningful.

## 9. Acceptance criteria and stop conditions

Accept only if:
1. existing and newly created sensitive fixtures are inaccessible for prohibited operations under overlapping allow roots on each backend;
2. Landlock enforces effective denials by removing ancestor grants instead of ignoring `denied_roots`;
3. Seatbelt sensitive write exclusions survive workspace/scratch re-allowances;
4. unrelated persistent user toolchain/config/cache state cannot be changed by `WorkspaceWrite`;
5. representative shell, cargo, Git and Python workflows function within approved scoped runtime/scratch roots;
6. no cross-session scratch write or symlink pivot is permitted by the approved rules; and
7. every actual filesystem grant has a truthful operator description.

Stop if a backend cannot express these invariants without a major mount namespace/container design, per-run cache isolation is infeasible for expected toolchains without an architectural choice, or the proposed effective policy would require exposing protected credentials by default. Record a narrow capability gap/ADR instead of widening global paths to pass tests.

## 10. Closure evidence

Create `plans/closure/sandbox-containment-post-closure-corrective/003-status.md`: SHA/PR, paths and policy diffs, real-host deny/allow matrix, utility compatibility, scratch isolation/lifetime/cancellation results, actual command outputs, risk and performance observations, guard/quick results, residual issues and C004 readiness. Historical runtime-safety M001/C001/C002 closure records remain untouched.
