# Linux Execution Isolation M001 Corrective C003 — Authorized Remote Git Transport Recovery

Status: blocked (hard: C001, M005 native offline isolation, M006 brokered egress and remote-credential authorization contract; interface: C002 local state/transaction model)
Primary class: capability
Target closure: `plans/closure/linux-execution-isolation/011-status.md`
Repository baseline: Linux branch `c7c18181862d67c9e2a649399bbda9665869edd8` (M001 partial implementation), as compared with `main` on 2026-10-13. Source: `plans/subsystems/linux-execution-isolation-roadmap.md` and accepted ADR-0034; ADR-0033 was proposed at this baseline and accepted separately on 2026-10-14. Corrects M001 `plans/implementation/linux-execution-isolation/001-trusted-git-execution-boundary.md` and historical blocked `plans/closure/linux-execution-isolation/001-status.md` without revising its evidence. Long-term: `plans/000-long-term-specification.md` §§4,17,27–29 and `plans/001-terminology-and-domain-model.md` §8. New corrective closure must be independent and preserve original historical M001 findings.

## 1. Objective

Restore typed remote Git `fetch`/`pull`/`push`/`clone` and authenticated transport without running arbitrary repository-configured network helpers, SSH commands or credential programs with daemon authority, and without bypassing the Linux offline network boundary. Separate network authorization, credential identity and Git local side effects rather than classifying all network Git as inherently forbidden.

## 2. Evidence and blockers

Current M001 strict Git route refuses fetch/push/clone and `src/git_network_policy.rs` overlays inherited `GIT_CONFIG*`, proxy/askpass/SSH variables while `ALWAYS_STRIPPED_ENV_VARS` removes some after restoration. This does not constitute a trustworthy broker or credential boundary. M005 proves off-by-default child networking; M006 must supply actually mediated allowed destinations/protocols. Authenticated SSH Git needs either a separately constrained SSH transport and agent credential broker or an explicit independently approved trusted service operation; an HTTP(S)-only M006 does not magically enable SSH.

## 3. Invariants

- Network destinations/ports/protocols and credentials are authorized by immutable principal/workspace/run policy, not remote URL/user-controlled Git config, `GIT_SSH_COMMAND`, `core.sshCommand`, `url.*.insteadOf`, `credential.helper` or local hooks.
- Host DNS/HTTP proxy/private network/metadata reachability is never granted by command spelling or a Git server redirect. Every outbound connection uses the appropriate verified mediator or an independently admitted human/host authority, with truthful outcome.
- Credential contents and SSH agent descriptors are never put in model context, ordinary Git stdout/stderr, an untrusted child environment, or unredacted audit; credential broker offers only per-target scoped access when implemented.
- Remote operations may update local refs/index/working tree only through C002's verified Git state/lease path. No post-fetch checkout filter/merge helper runs with greater authority.
- If approved transport mechanism unavailable, return a typed denied/unavailable action, no fake executable and no automatic direct-network fallback.

## 4. Required production work

A. Audit `src/git_network_ops.rs`, `src/git_network_policy.rs`, `src/git_mutations_ops.rs`, `src/git_service.rs` and `egggit::process` for caller-provided URL, remote helper, transport protocol, subprocess and credential-bearing env. Inventory HTTPS, SSH, file/local remote and custom protocol helpers distinctly.

B. Add typed remote Git authority intent: approved destination, protocol/port, write/read, credential selector, network budget, existing worktree lease and policy receipt. Resolve Git remote URLs and rewrites under a *trusted* parser/config context before admission; revalidate effective transport destination at actual connect time. Prefer reusing proven M006 network mediator; if only HTTP(S) supported, ship HTTPS-only scoped Git and keep SSH transport unavailable or separately approved, not unmediated.

C. Bind a short-lived credential broker or use operator-approved trusted Git remote service only when the remote operation's origin and destination scope are independently proven. No arbitrary askpass/SSH/GIT_CONFIG env injection from repo or inherited process. Preserve redacted projected stdout/stderr and avoid reconstructing command lines as shell.

D. Exercise redirects, URL rewrites, custom remote helpers, mixed file/ssh/https, DNS rebinding, localhost/metadata targets, protected branches, abort/retry with failed auth, network timeout and local ref changes. Ensure ssh-agent / private-key path cannot escape into child with a "contained" label.

E. Provide separate HTTPS/SSH capability flags in status and docs, tests/guards and C003 closure; do not conflate M006 generic web mediation with Git transport protocol support.

## 5. Tests and verification

Fixture HTTPS remote with scoped credential and fixed DNS, authorized fetch/push, remote rejection and cancellation; negative redirects/rewrite to private address, shell-capable `ext::` helper, malicious `core.sshCommand`, `GIT_SSH_COMMAND`, `credential.helper`, trusted Git config include, proxy env override, agent socket and unexpected network egress. Exercise remote Git from a previously hostile repository. Distinguish `git://`, SSH, HTTPS and local file path support; deny unsupported protocols explicitly. No live production credentials or open internet required.

Commands: `cargo fmt --all -- --check`; `cargo test -p egggit`; `cargo test -p codegg --lib git`; `cargo test --test git_mutations_integration -- --test-threads=1`; `python3 scripts/check_git_forbidden_patterns.py`; `python3 scripts/check_execution_ownership.py`; `scripts/verify.sh quick`; and M005/M006 focused native tests with documented exact invocations.

## 6. Failure, recovery, compatibility

No network/credential authority may be gained from rephrasing a command as Python, Bash or alternate Git tool; the actual child still has M005/M006 limits. Network retry must preserve receipt and destination authorization, no transparent FullHost retry or broader Git protocol downgrade. File/local remotes can be qualified independently of networked transports. Cache credentials outside untrusted workspace, revocation invalidates in-flight bridge and late callbacks. Preserve existing operation types; additive versioned protocol only when necessary.

## 7. Acceptance

Typed HTTPS fetch/push/clone perform only approved mediated operations and never reach denied targets or reveal credentials. SSH either has separately native-proven restricted broker semantics or is honestly marked unavailable with actionable reason. All route changes preserve local trusted Git state/leases. No hidden subprocess executables or arbitrary repo config controls credentials/destinations. Record exact native transport/credential tests and residuals.

## 8. Stop

Stop if a generic proxy tunnel bypasses destination policy, Git transport helper can invoke arbitrary host executables, untrusted child inherits real SSH agent/credentials, a permitted remote rewrite lands on host-local service, or privileged daemon Git is presented as contained without real proof. Keep the affected protocol unavailable.
