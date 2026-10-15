# Command Authorization M001 C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/command-authorization-and-execution-boundaries/004-m001-permission-precedence-and-scope-corrective.md`

Source subsystem roadmap:

- `plans/subsystems/command-authorization-and-execution-boundaries-roadmap.md#6-dependency-graph`

Repository baseline reviewed: `b01ee2f7` (fresh plan baseline); implementation reviewed at `fc4520df`

Implementation commits:

- `fc4520df` — correct current Deny precedence, Bash rule precedence, and reusable shell-command grant scopes.

## 1. Executive finding

C001 is complete. Matching invocation-specific rules now take priority over
fallback rules inside each existing agent > session > config tier. Within a
tier, Deny outranks Ask, which outranks Allow. Current effective Deny decisions
are evaluated before persisted Allow decisions, including the legacy
`with_exec_mode` path. Simple commands retain practical executable-family
grants; compound/ambiguous shell text and wrapper/interpreter executables use
an exact-invocation digest scope. Persisted formats and signing behavior are
unchanged. C002 is unblocked; no sandbox or shell-parser security guarantee is
claimed.

The original catch-all defect is reproducible from the baseline code: config
rules appended the unpatterned Bash fallback after specific patterns, while
`find_tool_rule_with_args` searched the vector in reverse and returned the
catch-all first. The original shell scope used the first whitespace token, so
`cargo test` and `cargo test; ./other-script` both mapped to `cmd:cargo`.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Specific deny/ask/allow patterns beat a generic Bash fallback | `tests/permission.rs::specific_bash_patterns_override_generic_permission_fallbacks`; `allow_all_bash_fallback_does_not_override_higher_priority_deny` | pass | Covers configured default Allow, explicit Bash Ask, and `allow_all_bash` with a higher-tier Deny. |
| Existing agent > session > config tier order remains intact | `tests/permission.rs` existing tier tests; `tests/approval_router.rs` child-ceiling and Yolo Deny cases | pass | Resolution remains tier-first; Deny precedence is applied within the effective tier. |
| Current configured Deny cannot be masked by saved broad/family Allow | `tests/permission.rs::explicit_configured_deny_cannot_be_bypassed_by_saved_broad_allow`; `current_tool_deny_cannot_be_bypassed_by_saved_allow`; `legacy_exec_mode_does_not_override_configured_deny` | pass | Covers Bash pattern Deny, non-shell mutating tool Deny, and compatibility headless mode. |
| Narrow family grants do not cover compound shell or hidden wrapper effects | `tests/permission.rs::narrow_saved_command_grant_does_not_cover_compound_shell`; `exact_saved_complex_command_grant_is_not_reused_for_a_variant`; scope normalization unit test | pass | `cmd:cargo` reuses only for a simple command. Compound commands receive a digest of full text; shell/interpreter/wrapper families are exact-scoped. |
| Practical simple command canonicalization | permission scope unit and `tests/policy_surface_m007.rs::scope_normalization_is_deterministic_and_bounded` | pass | Covers ordinary Cargo, `/usr/bin/cargo`, and `env FOO=1 /usr/bin/cargo`; persistence scope is bounded. |
| Signed store compatibility and scope tamper behavior remain valid | `cargo test -p codegg --lib permission`; `permission::tests::legacy_signed_broad_decisions_verify_under_new_scheme`; `permission::tests::legacy_decision_json_without_scope_stays_readable`; `tests/policy_surface_m007.rs::legacy_permission_file_without_scope_stays_readable` | pass | Existing HMAC/signature implementation and legacy unscoped JSON were retained; scoped signature stripping is rejected by the existing unit test. |
| Approval modes, child ceilings, and route behavior do not widen authority | `cargo test -p codegg --test approval_router -- --test-threads=1` | pass | 17/17, including Yolo explicit Deny and child-ceiling checks. |
| Current docs accurately describe invocation text and exact scopes | `architecture/permission.md`, `docs/security-semantics.md` | pass | Explicit text patterns are not represented as equivalent-effect or OS boundaries. |

## 3. Production implementation evidence

- `src/permission/mod.rs` now resolves matching specific Bash patterns before
  unpatterned fallback rules. Within the selected tier, matching Deny, Ask,
  and Allow rules use explicit conservative precedence.
- Current effective Deny is evaluated before the permission cache in both
  argument-bearing and ordinary permission checks. The legacy headless
  `with_exec_mode` broad defaults do not bypass a configured Deny.
- `decision_scope_for_shell_command` uses the existing
  `command_intent::shell_shape` parser. Simple commands use a validated
  `cmd:<executable>` family. Supported `env NAME=value <command>` and absolute
  executable paths retain family canonicalization. Complex shell forms and
  shell/interpreter/wrapper commands use `cmd:exact:<sha256>` over the full
  command string. No raw command is stored in the scope.
- `PermissionStore`, `PersistentDecision`, the signature scheme, scope field,
  and on-disk format were not changed. Legacy broad rows remain broad by
  compatibility and are explicitly described as such.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib permission
cargo test -p codegg --test permission
cargo test -p codegg --test policy_surface_m007
cargo test -p codegg --test approval_router -- --test-threads=1
python3 scripts/check_approval_router.py
python3 scripts/check_policy_surface.py
git diff --check
scripts/verify.sh quick
```

### Results

- `cargo fmt --all -- --check`: passed.
- `cargo test -p codegg --lib permission`: 125 passed, 0 failed.
- `cargo test -p codegg --test permission`: 51 passed, 0 failed.
- `cargo test -p codegg --test policy_surface_m007`: 14 passed, 0 failed.
- `cargo test -p codegg --test approval_router -- --test-threads=1`: 17 passed, 0 failed.
- Both manual Python guards passed; `git diff --check` passed.
- `scripts/verify.sh quick`: passed on the final implementation revision, including `cargo check --workspace --all-targets --locked` and all quick guards.

All results are local verification; no hosted CI result is claimed.

## 5. Invariant review

- Scope authority and existing tier order are preserved; a lower-tier Deny is
  not globally promoted over a higher-tier Allow.
- Within the effective tier, an invocation-specific Deny cannot be hidden by
  a generic fallback; current Deny cannot be hidden by a saved Allow.
- Approval mode and sandbox request remain orthogonal. No reviewer, Yolo,
  headless, or mode change widens a parent execution ceiling.
- Pattern and digest scopes classify the request text only. They do not
  establish shell equivalence, sandbox containment, or kernel enforcement.

## 6. Failure and recovery review

No asynchronous lifecycle, external process, scheduler, or durable state owner
changed. Permission decisions continue to use the existing bounded in-memory
store and atomic JSON persistence path. If the configured path cannot be
canonicalized, the existing default behavior remains in force. Invalid or
tampered signed decisions remain rejected by the existing verification logic.

## 7. Migration and compatibility review

No migration or DTO change. `PersistentDecision.scope` remains optional and
HMAC-signed. Existing unscoped signed rows continue to deserialize and verify
under the legacy signature path; they continue to represent explicitly broad
grants. New ambiguous shell invocations no longer fall back to an unscoped
grant when the scope is derived from tool arguments: their exact digest scope
is persisted instead.

## 8. Security review

The change closes a permission-cache precedence defect and prevents a
first-token saved grant from being reused for a compound invocation. Digest
scopes reveal no script body. Explicit textual filters remain invocation
preferences and can be bypassed by an equivalent script or alternate tool;
actual execution authority and obtained sandbox enforcement remain separate.
No credential, audit, process, or sandbox boundary changed.

## 9. Documentation and operations

- Updated `architecture/permission.md` with tier/rule precedence, current
  Deny behavior, exact shell scopes, and the limitation of text matching.
- Updated `docs/security-semantics.md` to state that configured patterns deny
  a matching invocation and do not prevent equivalent effects.
- Static approval-router and policy-surface guards pass.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Legacy explicitly unscoped saved Allow decisions remain broad by compatibility | A user-created legacy grant can still authorize compound shell; it is not silently revoked or reinterpreted | Keep the broad scope visible and preserve current effective Deny precedence; no migration is required for C001. |

No high- or medium-severity finding remains for C001.

## 11. Roadmap disposition

C001 is closed. C002 (advisory classification and preflight semantics) is
dependency-ready and may proceed. C003 remains blocked on C002. Parent M001
remains incomplete until C002 and C003 close. M002/M003 remain blocked on
parent M001. Linux execution-isolation dependencies and backend status are
unchanged.

## 12. Registry updates

- Marked implementation plan 004 implemented and added this closed record.
- Removed C001 from active implementation rows and added it to recently closed
  work.
- Registered plan 005 as ready because C001 is its only hard dependency.
- Kept plan 006 blocked on C002; no other blocked plan became ready.
- Updated the command-authorization roadmap milestone table and dependency
  graph to match.
