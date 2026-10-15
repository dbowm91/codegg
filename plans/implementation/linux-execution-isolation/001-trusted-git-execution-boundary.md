# Linux Execution Isolation M001 — Trusted Git Execution and Hostile Repository Metadata

Status: blocked (historical M001 blocked; C001–C003 now split recovery)
Primary class: invariant
Hard dependencies: none; no ADR-0033 acceptance required for this existing-authority corrective.
Repository baseline: `main` at `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03` (2026-10-13). Source roadmap: `plans/subsystems/linux-execution-isolation-roadmap.md`. Long-term requirements: `plans/000-long-term-specification.md` §§4,17,27,29 and `plans/001-terminology-and-domain-model.md` §8. Applicable accepted ADRs: ADR-0001, ADR-0002, ADR-0004, ADR-0016. Proposed design gate: ADR-0033; do not treat it as accepted. Target closure: `plans/closure/linux-execution-isolation/NNN-status.md` (use milestone number). Historical initial implementation `eb0f18c` and blocked closure `plans/closure/linux-execution-isolation/001-status.md` remain evidence of a partial security pass and Git integration breakage. C001/C002/C003 own corrective remediation.

## 1. Objective

Prevent agent-controlled Git configuration, hooks, external filters, diff drivers, filesystem monitors, or repository aliases from being promoted into a more privileged **Codegg-owned Git subprocess** after a sandboxed workspace mutation. Delineate the trusted Git service's authority from Bash, Python, and user-authorized interactive Git. Do not claim that a fully writable `.git` directory is protected by Landlock: M003 owns a protected mount view.

## 2. Why ready; current implementation evidence

`crates/egggit/src/process.rs::GitEnvPolicy` clears and selectively restores environment; `src/git_mutations.rs`, `src/tool/git.rs`, `src/git_network_ops.rs`, `src/git_network_policy.rs`, and worktree helpers initiate Git commands. `docs/validation/git-security-review.md` acknowledges that Git configuration can invoke external filter/textconv commands. `src/security/sandbox/policy.rs` does not blanket-deny `.git` in a writable workspace. Environment clearing does not neutralize repository-local Git config or `.gitattributes`; user-owned Git and an agent-controlled repository are distinct trust levels. Audit all actual entrypoints, including `egggit::run_with_timeout`, sync/async paths, raw Git fallback, Git hooks, worktree maintenance and any job launched outside `ManagedProcessService`.

## 3. Invariants

- Agent-controlled workspace bytes, Git configuration and attributes cannot autonomously cause execution with daemon/provider/user credential authority through trusted Codegg operations.
- Git read/status/diff operations must not be trusted merely because they are called "read-only"; filesystem monitors, textconv and filters can execute programs.
- Existing daemon credential ownership, secret redaction, approval receipts, worktree leases, Git argv fidelity and cancellation remain unchanged.
- Shell/git commands deliberately run under an already-contained child retain that child's restrictions; no retry through an unsandboxed Git executor.
- Genuine user-requested hook/filter/toolchain workflows require a separate, explicit authorized route and truthful execution description, never invisible promotion of an agent-written config.
- No arbitrary Git args may override fixed safety flags via `-c`, `--config-env`, environment entries or configuration files.

## 4. Scope and boundaries

In: inventory Git subprocess owners; classify safe, privileged and intentionally user-authorized execution; implement a canonical non-executable Git policy for trusted background operations (or fail closed on unsupported Git features); strict network Git policy and Git config key review; tests/guards. Out: rewriting Git engine, disabling all interactive user Git globally, protecting `.git` against writes by the sandboxed target (M003), changing approvals, assuming `git -c core.hooksPath=/dev/null` alone blocks all execution vectors.

## 5. Production changes

### A. Entry-point and authority inventory

Trace `src/tool/git.rs`, `src/git_mutations.rs`, `src/git_service.rs`, `src/git_network_ops.rs`, `src/git_recovery.rs`, `crates/egggit/src/process.rs`, `crates/codegg-core/src/repository_lineage.rs`, worktree creation/status services, and command-intent routed Git. For each classify requested caller, intended authority, repo-controlled configuration, inherited environment, hooks/textconv/fsmonitor/filter/credential-helper/transport/alias risk, and whether process runs under a sandbox. Produce a machine-checkable owned-entrypoint list (extend existing Git static guard rather than create a second competing one).

### B. Canonical safe-Git execution

Introduce a narrowly typed trusted-Git launch/policy selection in the existing `egggit` process builder. For privileged-to-the-agent operations disable all reachable repo-controlled command-execution surfaces, *including* hooks, fsmonitor, external diff/textconv, filters, credential helpers, git aliases, pager/editor, ssh commands, and shell-capable transports, or explicitly deny the operation when the feature cannot be suppressed safely. Inspect Git's effective configuration and command-specific semantics rather than claim a few environment variables are complete containment. Explicitly handle `.git/config`, config includes, `.gitattributes`, per-worktree config, `GIT_CONFIG*` and `-c` precedence. Preserve Git's necessary read/index/ref behavior. For difficult operations such as checkout smudge filters or signed commits, block trusted unattended execution until reviewed authority exists instead of silently running config code.

### C. Authorized exceptional Git

Keep user-initiated operations that genuinely require hooks or credentials behind existing typed approval/identity policies. Their execution class and obtained sandbox status must reflect **their actual spawn boundary**, with no concealed elevation and no credential values in logs. Do not let agent-written files define the approved executable without a separate validation step.

### D. Evidence and docs

Extend `docs/validation/git-security-review.md`, `architecture/security.md`, `architecture/git.md` and maintained `scripts/check_git_forbidden_patterns.py` (if present) with the inventory and guard. Do not rewrite older security review as proof of new protections.

## 6. Ordered work packages

1. Construct fixtures in disposable repositories with executable `pre-commit`/checkout hooks, `core.fsmonitor`, `diff.*.textconv`, clean/smudge/process filters, alias expansion, local/linked worktree Git config, remote helper and a hostile `include.path`. Use marker files and a synthetic secret, never real credentials.
2. Run benign `status`, `diff`, `add`, `commit`, `checkout`, `fetch` shapes against current and proposed Git service and record exactly which entrypoints spawn unintended executables. Mark unsafe operation families for deny until complete.
3. Centralize trusted-Git subprocess policy; remove bypassable raw/sync ownership drift; preserve noninteractive status and cancellation.
4. Distinguish approved user execution from trusted agent-triggered maintenance; wire the correct policy for each.
5. Verify tests and static guards, document unsupported intentionally executable Git features, write closure record.

## 7. Failure, cancellation, restart, contention

Unsafe/ambiguous effective Git config causes terminal denial before a privileged child runs. Failures do not automatically retry through raw shell, local command or FullHost. Concurrent repository modifications cannot swap checked config for executed config unnoticed; if a race cannot be excluded, use OS-backed protected configuration/isolated Git context, pin identity, or deny this command pending M003. Worktree operations keep their lease checks. Timeout and cancellation own spawned descendants and output limits as in existing services. Restart does not replay a rejected operation.

## 8. Migration and compatibility

No schema/protocol migration. Trusted background Git may refuse formerly accepted hooks/filters; produce explicit reasons and an operator-authorized alternative. Do not claim this milestone alone renders `.git` immutable; it prevents **trusted Codegg Git** from executing untrusted repository configuration. Normal user-operated shells outside Codegg's trusted Git authority are unchanged.

## 9. Required tests and verification

Focused: sanitized argv and environment, process policy precedence, per-operation hook/filter/fsmonitor disabled/rejected, no raw fallback, worktree gitdir/common-dir. Integration: real `git status`, `diff`, `add`, `commit`, `checkout`, `fetch` with each hostile feature and marker absence, benign cases still succeed; agent writes malicious metadata before a later trusted Git call. Negative: alias, `-c`, `GIT_CONFIG_COUNT`, includes, `includeIf`, diff/textconv, filter, hook, SSH helper, prompt injection and symlink races. Concurrent mutation: adversarial config replacement during trusted execution; fail closed if unprovable. Linux native evidence required; portable policy unit tests on macOS/Windows or exact non-regression where available.

```bash
cargo fmt --all -- --check
cargo test -p egggit
cargo test -p codegg --lib git
cargo test -p codegg --test sandbox_containment -- --test-threads=1
python3 scripts/check_sandbox_policy_wiring.py
scripts/verify.sh quick
```
Use the actual workspace package name for `egggit` if Cargo metadata shows otherwise; document any modified command. Do not claim tests executed until closure.

## 10. Acceptance criteria

Every inventoried trusted Git entrypoint either disables repo-controlled execution or denies it; synthetic hook/filter/fsmonitor/alias/transport markers remain absent for its accepted operations. Benign authorized Git workflows retain expected changes and redacted auditing. A later trusted Git command cannot execute a payload planted by a preceding workspace-write child. No bypass exists through an alternative Git library/CLI path. Closure records the exact host, commit, test matrix, unsupported exception routes and residual TOCTOU/security risks.

## 11. Stop conditions

Stop if a required Git behavior cannot be made non-executable under its current trust level; a known race survives without enforceable isolation; a fix would require redefining user approval or agent authority; or the remaining issue requires read-only mount overlays (M003). Record blocked behavior rather than broadening trust.

## 12. Current stop condition

The initial boundary work is implemented, but closure is blocked because the
safe policy denies filter/merge-driver and network Git operations, including
ordinary typed add/checkout/fetch/push workflows. The existing approval route
does not yet provide a distinct contained execution class for those operations.
Do not mark M001 closed until the maintainer selects an authorized contained
route or explicitly accepts the compatibility loss and the roadmap is revised.

## 13. Superseding execution-path clarification (2026-10-13)

Accepted ADR-0032 removes the assumption that Git command-name refusal is a valid security goal. Do not implement additional subcommand blocklists. C001 `009-m001-typed-git-policy-reconciliation.md` fixes typed authorization and fake-executable failures; C002 `010-m001-contained-local-git-recovery.md` restores executable *local* Git only after real namespace/process/offline guarantees; C003 `011-m001-authorized-remote-git-recovery.md` separately handles mediated transport/credentials. M001 strict closure follows these correctives rather than blocking M002 launch-contract work. Historical blocked closure remains unchanged. A high-privilege raw Git fallback is prohibited.
