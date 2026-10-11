# Sandbox Containment Post-Closure Corrective C004 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/sandbox-containment-post-closure-corrective/004-macos-confidentiality-and-cross-platform-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/sandbox-containment-post-closure-corrective-roadmap.md#7-milestones-and-useroperator-exits`

Repository baseline reviewed: `main` at `8e9d8b01e5c229715c8e4dea929e050b391e252c` (2026-10-10).

Implementation commits:

- `f4560665` — C001/C002 shared launch and Landlock ABI corrections.
- `be7537cc` — C003 effective deny precedence and scoped tool state.
- `875e28b8`, `98f96a99` — native macOS scratch lifecycle qualification and canonical temp-root assertion.
- Related follow-ups through `52bc62fc` — Seatbelt host probes, loopback bounds, and native tool compatibility.

## 1. Executive finding

The C001/C003 native qualification gate is discharged on a hosted Apple Silicon macOS runner. Real Seatbelt helper tests confirm required launch enforcement, credential path denials, scoped writes, fail-closed rejected profiles, and Python mode behavior. A direct managed-process test confirms private `TMPDIR` placement, `0700` permissions, and cleanup after process reap.

Seatbelt is not equivalent to Landlock. The qualified profile allows broad reads for developer-tool compatibility, does not isolate network access, retains inherited pre-opened descriptors, has no `no_new_privs` equivalent, and leaves Apple Events/Launch Services channels documented as available. XPC/Mach and setuid privilege behavior were not exercised against safe fixture services. The effective contract and these evidence gaps are explicit, so C004 is conditionally closed pending those bounded residual probes; no parity or complete IPC isolation claim is made.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Native Seatbelt helper applies before the target and reports actual backend | Hosted `sandbox_containment`, 15 passed; status `Enforced`, backend `seatbelt`, ABI `None` | Pass | Includes malformed/rejected profile no-marker checks |
| Credential exclusions resist nested and future-created paths | Hosted `sandbox_containment` | Pass | Nested workspace credential read/write, symlink alias, and future-deny cases |
| Write grants remain scoped and sibling session scratch is denied | Hosted `sandbox_containment`; managed scratch lifecycle test | Pass | Workspace and explicit scratch allowed; outside and sibling scratch denied; managed temp is private and reaped |
| Python Analyze, Verify, and Transform use Seatbelt with intended mutation limits | Hosted Python executor suite, 22 passed | Pass | Analyze/Verify persistence denied; Transform writes only within workspace |
| Toolchain compatibility under current broad-read profile | Six Seatbelt `--version` probes and shell/Cargo metadata probes | Pass | `git`, `cargo`, `rustc`, `python3`, `brew`, `gh`; no external service contacted |
| Network isolation | Loopback probe succeeds and status reports unrestricted network | Pass as a limitation | No network denial is claimed |
| Inherited descriptor behavior | Pre-opened denied-file descriptor remains usable and is reported | Pass as a limitation | No close-on-sandbox guarantee is claimed |
| Apple Events/Launch Services, XPC/Mach, and privilege transitions | Capability limits reviewed; no safe fixture service/privileged installation was available | Not qualified | Remains a named operational evidence condition |
| Cross-platform security parity | Linux Landlock ABI 4 evidence plus macOS Seatbelt evidence | Partial by design | Landlock read allowlisting and Seatbelt broad reads remain different contracts |

## 3. Production implementation evidence

C001 corrected Python backend selection and required-launch failure semantics. C002 requires Landlock ABI 3 or newer for the truncate guarantee and reports effective ABI. C003 removes overbroad persistent write roots, gives each managed launch a private `0700` scratch directory, applies deny precedence, and refuses policies the backend cannot represent safely. C004 adds bounded, native helper-backed probes and keeps actual Seatbelt limits visible in architecture and user-facing documentation.

No stricter Seatbelt read allowlist or IPC rule was enabled: the qualified baseline remains write-confined with explicit secret-path denials, while reads are broadly allowed so normal developer tooling can operate. No new network policy, entitlement, helper, database schema, public profile, or protocol was introduced.

## 4. Verification executed

### Commands run

Hosted workflow run `38166386985`, head `98f96a99b9f9703b7cc3d585e0401f3b44764f05`:

```bash
cargo test -p codegg --test sandbox_containment -- --test-threads=1 --nocapture
cargo test -p codegg --lib python_script::executor::tests -- --test-threads=1 --nocapture
cargo test -p codegg --lib managed_process::tests::constrained_launch_gets_private_tmpdir_removed_after_reap_on_macos -- --test-threads=1 --nocapture
cargo test -p codegg --lib managed_process::tests::timeout_and_cancellation_kill_the_process_group -- --test-threads=1 --nocapture
cargo test -p codegg --lib managed_process::tests::missing_private_status_fails_closed_with_test_only_helper_injection -- --test-threads=1 --nocapture
cargo test -p codegg --lib managed_process::tests::hardlinked_denied_path_fails_before_target_launch -- --test-threads=1 --nocapture
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_policy_wiring.py
```

Local Linux verification recorded by the C001–C003 closures includes focused sandbox, Landlock, Python, managed-process, and policy-wiring tests plus `scripts/verify.sh quick`.

### Results

- Hosted runner image on 2026-10-11: `macos-15-arm64`; macOS `15.7.9`, build `24G830`; Darwin `24.6.0`; `RELEASE_ARM64_VMAPPLE arm64`. `/usr/bin/sandbox-exec` was executable.
- Real Seatbelt helper suite: 15 passed, 0 failed. Reported `Enforced { backend: seatbelt, abi: None }`.
- Python executor modes: 22 passed, 0 failed.
- Managed scratch lifecycle: 1 passed; exact test verified canonical system temp root, mode `0700`, and removal after reap.
- Process-tree cancellation/timeout, fail-closed private status, and hardlink deny preflight: 1 passed each.
- `check_sandbox_contract.py` and `check_sandbox_policy_wiring.py`: passed.
- One earlier M1 attempt failed because the macOS `/var` alias did not string-prefix-match `/private/var`; the assertion now compares canonical paths. The corrected run above passed the exact test.
- This is a one-shot qualification run, not a permanent CI matrix. The temporary workflow is removed from the repository after recording the evidence.

## 5. Invariant review

- Required helper or profile rejection remains terminal before target execution; malformed and rejected Seatbelt profiles produced no target marker.
- C001 Python Analyze/Verify cannot persist writes; Transform persistence stays within the workspace.
- C003 credential denials survive workspace write re-allowances; explicit scratch is writable without opening sibling scratch or shared caches.
- C002 Linux strict Landlock requires ABI 3+ for truncation rights; the qualified Linux host reported ABI 4 and `no_new_privs`.
- Status names requested/applied backend and does not invent an ABI for Seatbelt.
- No network isolation, inherited-FD revocation, full read confidentiality, or complete IPC/privilege boundary is claimed.

## 6. Failure and recovery review

Malformed/rejected profiles and missing private status fail closed; marker absence is tested. Cancellation and timeout terminate the supervised process group. Per-launch scratch is retained until process reap and removed afterward. The tests do not exercise daemon restart cleanup or persistent scratch recovery because scratch is per-process ephemeral and no durable state was added.

## 7. Migration and compatibility review

No schema, protocol, configuration, or public sandbox-profile migration was added. Seatbelt retains broad read compatibility for system and developer tools while limiting writes and denying selected credentials. Python's three execution modes and six native tool probes pass. Workflows needing global persistent cache writes must use explicitly scoped approved state or receive a denial.

## 8. Security review

Path denial, write boundaries, private scratch permissions, fail-closed setup, private status, hardlink preflight, and process-tree cancellation are covered by real or focused tests. No secrets, generated SBPL, or external service credentials are logged or contacted. Remaining macOS risks are broad reads, unrestricted network, inherited file descriptors, Apple Events/Launch Services, unqualified XPC/Mach behavior, and no `no_new_privs` equivalent.

## 9. Documentation and operations

`architecture/security.md`, `architecture/python_scripting.md`, `docs/cli.md`, and the permission skill describe Seatbelt's broad-read, unrestricted-network, inherited-FD, IPC, and privilege limits. `scripts/check_sandbox_contract.py` and `scripts/check_sandbox_policy_wiring.py` passed on the hosted runner. The temporary workflow is removed after this qualification; no permanent macOS CI lane is retained. Any future native qualification must run the documented targets on a supported Apple Silicon host.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Medium | Apple Events/Launch Services reachability is documented, but no benign fixture service was used to measure actual invocation reachability | Contained tools may communicate with user-session services | If stronger IPC containment is required, add a safe fixture probe and evaluate a separately authorized profile change |
| Medium | XPC/Mach reachability and setuid privilege transition were not qualified on the hosted headless runner | Those channels must not be described as blocked | Add bounded fixture/privilege probes on a suitable native host before making stronger claims |
| Medium | Seatbelt broadly permits reads outside the workspace for tool compatibility | Secret paths are specifically denied, but this is not read allowlisting or Landlock parity | Keep this limitation visible; any narrower profile requires measured compatibility and a separate authority decision |
| Low | No native Windows, older-Landlock ABI, or cold-cache performance benchmark was run | These are outside the supported two-host qualification and bounded plan evidence | Retain as separate future evidence; no Windows support or performance claim is made |

No high-severity finding remains hidden by the closure. The remaining gaps prevent unconditional security-equivalence claims, not the documented write-containment contract.

## 11. Roadmap disposition

C004 is conditionally closed: native C001/C003 qualification and all supported plan acceptance checks are complete, while the named macOS IPC and privilege probes remain explicit. The sandbox corrective roadmap is conditionally closed. Historical runtime-safety conditions remain separate and unchanged.

## 12. Registry updates

- Mark C001 and C003 closed with hosted Apple Silicon evidence; retain C002 closed.
- Mark C004 conditionally closed with the residual evidence conditions above; remove its blocked entry.
- Mark the subsystem roadmap conditionally closed and retain the backend-specific contract and limitations.
