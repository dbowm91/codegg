# Sandbox Containment Post-Closure Corrective C002 — Closure

Status: closed

Source plan: `plans/implementation/sandbox-containment-post-closure-corrective/002-landlock-abi-and-mutation-rights.md`

Implementation commit: `f4560665cbfe20ee7d478a56a114bdb3f164f8dc`

## 1. Executive finding

Landlock now hard-requires ABI v3 rights, including `TRUNCATE`, and requests newer crate-supported rights best-effort. `PartiallyEnforced` is accepted only when the effective ABI is at least v3 and `no_new_privs` is confirmed; an unavailable or lower-ABI helper setup fails before target execution. Real-helper evidence on Linux kernel `6.8.0-146-generic` reported effective Landlock ABI 4.

## 2. Rights and enforcement evidence

| Probe | Result |
|---|---|
| Helper status | Enforced, backend `landlock`, ABI 4, `no_new_privs` true; exec inheritance reported |
| ReadOnly truncation of an allowed inside file | Denied; original bytes retained |
| WorkspaceWrite truncation inside workspace | Allowed |
| Outside workspace shell redirection, `truncate -s 0`, Python `open(..., "w")` | Denied; outside bytes retained |
| Nested child shell truncation | Denied; inherited confinement retained |
| Cross-boundary rename and hardlink into outside roots | Denied |
| ABI below v3 | Not available on this host; implementation uses a v3 `HardRequirement` and rejects effective ABI below v3 before helper success |

The real-helper suite also retains ordinary write/read-only checks and validates ABI plus `no_new_privs` in status. Reported guarantees distinguish the ABI 3 truncate baseline, ABI 2 refer handling, best-effort newer supported rights, and crate-supported-ABI limit.

## 3. Verification executed

- `rtk proxy cargo test -p codegg --lib security::sandbox::landlock` — 6 passed.
- `rtk proxy cargo test -p codegg --test sandbox_containment -- --test-threads=1` — 10 passed.
- `rtk proxy cargo test -p codegg --test sandbox_containment helper_reports_enforcement_with_a_real_backend_identity -- --nocapture` — passed; actual status recorded above.
- `rtk python3 scripts/check_sandbox_contract.py` — passed.
- `rtk python3 scripts/check_sandbox_policy_wiring.py` — passed.
- `rtk cargo fmt --all -- --check`, `rtk git diff --check`, and `rtk scripts/verify.sh quick` — passed. Quick includes workspace all-target `cargo check`.

## 4. Invariant, failure, compatibility, and security review

The helper, private status channel, process supervisor, and `no_new_privs` ownership remain unchanged. Unsupported required rights fail closed. There is no older-kernel compatibility tier and no claim that rights newer than the crate's effective supported ABI are handled. No schema, configuration, or protocol migration was added. Network isolation and inherited pre-restriction file descriptors remain explicit limits.

## 5. Unresolved findings

- **Low — older-kernel runtime evidence absent.** No ABI <3 kernel was available for a real negative launch test. The hard-required v3 ruleset construction and effective-ABI guard provide the safe disposition in code; an injected older-kernel model is optional per the plan.
- **Low — hosted CI not observed.** Local quick and real-helper evidence passed.

## 6. Roadmap and registry disposition

C002 is closed. Registry dependency audit: C003's hard C002 dependency is satisfied, and C001's shared launch-selection interface is stable, so C003 is moved from blocked to active in this same closure update. C004 remains blocked on C003 and native macOS qualification. The predecessor runtime-safety conditional closure remains separate and untouched.
