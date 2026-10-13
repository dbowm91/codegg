# Command Authorization and Execution Boundaries M003 — Effect-Equivalence and Observability Qualification

Status: conditionally closed (Linux filesystem evidence complete; macOS cross-target compile awaits an Apple SDK/sysroot)
Primary class: invariant
Target closure: `plans/closure/command-authorization-and-execution-boundaries/003-status.md`.
Status and acceptance are planning declarations, not execution evidence. Source roadmap: `plans/subsystems/command-authorization-and-execution-boundaries-roadmap.md`. Repository baseline: `main` 2026-10-13 and Linux branch `c7c18181862d67c9e2a649399bbda9665869edd8`. Long-term: `plans/000-long-term-specification.md` §§4,17,27–29 and `plans/001-terminology-and-domain-model.md` §8. Accepted: ADR-0001/0002/0004/0016/0024; Linux mechanism ADR-0023 proposed. Closure belongs under `plans/closure/command-authorization-and-execution-boundaries/`.

## 1. Objective

Prove that execution *effects and authority limits* rather than command spelling determine security outcomes. Qualify end-to-end user/operator explanations, risk signals, route provenance and retained audit, including the honest limitations of Landlock-only vs future namespace/network backends.

## 2. Evidence and dependencies

Current shell regex rules can be bypassed syntactically; Git M001 blocker rejects subcommand names rather than requiring a particular actual execution capability. M001 and M002 establish typed advisory/authority semantics and route fidelity. Existing `SandboxEnforcement` reports filesystem and network separately, with `NetworkEnforcement::Unrestricted` on Landlock-only. The completed Linux M003–M005, once implemented and native-tested, provide separate stronger claims; do not manufacture these from mocks.

## 3. Core requirements

Build a deterministic *effect-equivalence corpus* in existing test infrastructure rather than a new always-on CI security framework. For each prohibited effect, generate alternate expressions using (a) direct Bash text, (b) wrapped shell and split command, (c) generated script file, (d) Python or compiled test helper, and (e) typed Git/native tool wherever applicable. Assert the same actual filesystem/network/credential/IPC authority outcome, though advisory risk and prompts may legitimately differ when intent is opaque. An advisory classifier miss is an observability weakness, **not** a change in granted kernel capabilities.

Include allowed workspace modifications, denied outside-root writes, nested credential access, Git planted executable config/hook/filter, outbound TCP/UDP (only assert denial with a verified network backend), host process/Unix IPC (only assert denied with verified process/IPC backend), and child agent/nested worktree scope. Keep fixtures synthetic and safe: never run a real fork bomb or `rm -rf /` outside purely parsed non-executing tests.

Trace exact decision source, approved capability, helper identity, executed binary/argv digest, route, actual backend and obtained enforcement, process exit, output truncation/cleanup and allowed metadata. Demonstrate a generated script does not disappear from audit when launching under Bash; audit may record hash/path/executable, not pretend to know every nested syscall. Protect tokens/private commands from log disclosure and excessive retention. Add post-run checkpoint/undo/link-to-Git trace where existing infrastructure supports it; do not invent a new rollback engine.

## 4. Ordered work

A. Assert M001/M002 closure and inventory risk-to-authorization mapping. Reuse already proven native Landlock test fixtures; do not claim network enforcement absent M005.
B. Exercise same-effect variants and adversarial reroutes including unknown git alias and raw-command fallback. Verify operator textual Deny is honored for matching invocation and label failure to catch other forms as expected best-effort limitation.
C. End-to-end test CLI/TUI/daemon/agent tool surfaces with snapshot and outcome; redact script contents and secret-bearing paths. Verify deterministic reason taxonomy, private status channel and job-cancel semantics.
D. Run Linux supported and unsupported host probes and platform compile/nonregression for macOS/Windows; report missing evidence per dimension rather than marking all mechanisms enforced.
E. Update `architecture/security.md`, `architecture/permission.md`, validation docs and plans closure/registry. No default profile migration.

## 5. Acceptance / stop

An equally scoped shell, wrapper, script, Git operation and native tool cannot perform **a resource access that the actual backend denies**. Authoritative Deny survives all routing and reviewer modes; current Landlock-only reports unrestricted networking. The operator sees why execution was allowed, denied, escalated, or unavailable and the actual containment used. Legitimate local coding workflows still execute when the selected authority permits them. Exact-revision tests, host identity, feature matrix and unresolved findings recorded in closure.

Stop for any unauthorized privilege promotion, false "network offline" claim, hidden child execution in audit under a known owner, lost cancellation, or inability to test a purported enforced mechanism on a native host. Leave that mechanism blocked rather than accepting mock-only proof.

Commands: `cargo fmt --all -- --check`; `cargo test -p codegg --lib security`; `cargo test -p codegg --lib permission`; `cargo test -p codegg --test sandbox_containment -- --test-threads=1`; `cargo test -p codegg --test approval_router -- --test-threads=1`; `python3 scripts/check_policy_surface.py`; `python3 scripts/check_sandbox_contract.py`; `scripts/verify.sh quick`. Add targeted equivalence integration tests with documented exact names, no excessive full matrices.
