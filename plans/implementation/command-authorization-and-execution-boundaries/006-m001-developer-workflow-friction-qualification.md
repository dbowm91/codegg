# Command Authorization M001 C003 — Low-Friction Everyday Development Workflow Qualification

Status: closed
Class: capability / compatibility
Closure: `plans/closure/command-authorization-and-execution-boundaries/006-status.md`.
Authority: accepted `plans/adrs/ADR-0034-command-syntax-and-execution-authority.md` and `plans/subsystems/command-authorization-and-execution-boundaries-roadmap.md`. Parent handoff: `plans/implementation/command-authorization-and-execution-boundaries/001-syntax-blocker-audit-and-classification.md`. Baseline inspected: `planning/linux-execution-isolation` @ `7b72582c4f01f8a53c7f20ed5dde1bc620f82965` (2026-10-13). No tests are represented as executed by this plan.

## 1. Objective and peer-informed rationale

Make Codegg feel like a competent day-to-day coding harness: normal inspection, edit/build/test/format/lint and local Git workflows should proceed with **few or no gratuitous approvals inside an already authorized workspace**, regardless of shell punctuation, interpreter choice or command style. Stop only when a real authority boundary is requested or the operator explicitly requested review. Codex's 2026 safety guidance describes frictionless low-risk execution inside a bounded workspace and approvals for boundary crossings; Gemini's current policy distinguishes defaults, explicit rules and sandbox expansion. Do not reproduce a peer's exact syntax allowlist or claim its trust model applies to Codegg's Landlock-only network-unrestricted baseline.

References researched 2026-10-13:
- `https://openai.com/index/running-codex-safely/` — routine sandboxed work and explicit boundary approval, scoped network.
- `https://alignment.openai.com/auto-review/` — dedicated review reducing human approval interruptions; not a substitute for kernel guarantees.
- `https://github.com/google-gemini/gemini-cli/blob/main/docs/reference/policy-engine.md` — explicit allow/ask/deny and precedence.
- `https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/sandbox.md` — sandbox expansion flow.

## 2. Concrete UX target: modes before command names

One authenticated `ExecutionPolicySnapshot` determines allowed workspace and approval mode; risk flags may alter review only when material effect/authority is unknown, not because a quote, pipeline or wrapper is present. In normal workspace-write **with obtained filesystem confinement** and where permission policy already allows the action, these workflows should not trigger redundant Ask solely from parser patterns:

| Ordinary workflow | Representative invocations | Required default user experience |
|---|---|---|
| Discovery / inspection | `pwd`, `ls`, `rg`, `find`, `cat`, `git status/log/diff`, `rustc --version`, `python --version` | Run when allowed by policy; no syntax-triggered review |
| Rust development | `cargo fmt/check/build/test/clippy`, `cargo metadata`, `cargo test --workspace`, `cargo test -p pkg --test foo` | Run inside granted sandbox; shell options/quotes/pipes not independent prompts |
| Python development | `python -m pytest`, `uv run pytest`, `ruff check`, `ruff format`, `mypy`, `python -c 'print(1)'` | Run with permitted profile; inline interpreter not automatically treated as unsafe |
| JS/TS, Go and generic | `npm test`, `pnpm test`, `yarn lint`, `go test ./...`, `make test`, `just test`, `cmake --build build` | Do not enumerate every package/task name in a globally privileged allowlist; use authority/obtained envelope |
| Local workspace maintenance | `mkdir -p target`, `rm -rf target`, `git diff --check`, `touch src/file`, `sed ...`, shell scripts and `bash -c 'cargo test'` | No reflexive shell-syntax Ask; destructive effects may receive operator-selected review/checkpoint if not previously approved |
| Local Git mutations | `git add`, `git commit`, `git switch`, `git stash`, `git merge` | Do not block by spelling, but C001 Git trusted policy and Linux C002 still require a **real contained executable-Git route** where needed |
| Credential, privilege and egress | `sudo`, raw devices, system packages, `git push`, package downloads, untrusted external services | Require independently authorized capability when outside the granted envelope; no blanket unsandboxed fallback |

**Important constraints:** Running `cargo test`, `npm test`, `make`, Git filters or `python -c` may execute arbitrary workspace code. Their low-friction handling is a UX policy **only because existing authorized sandbox/process restrictions apply**, not proof that they're inert. Landlock-only on Linux currently leaves network/IPC unrestricted; an "offline guaranteed" UX cannot be claimed before Linux M004/M005. A user selecting FullHost/Yolo knowingly has host authority; warn clearly and do not promise protection by a literal regex. ReadOnly/plan mode may reject mutating effects via real enforcement; don't treat "safe package name" as a waiver.

## 3. Required implementation

1. Build an *approval-friction baseline* with deterministic, no-model fixtures for a representative 50+ command corpus across Rust, Python, JS, Git, shell composition, tests and normal cleanup. Record (a) policy mode, (b) initial stored grants, (c) effective requested/obtained sandbox status, (d) user prompts per logical operation, (e) actual execution success/unavailability and (f) security denials. Do not treat missing backend as a prompt.
2. After C001/C002, remove any residual builtin Ask on harmless syntax, literal scripting forms and thin command wrappers. Implement minimal review aggregation: one risk/permission request per logical execution, with bounded reason summary and original whole invocation shown to human, and no reapproval for equivalent reroute/verified retry under identical scope. Avoid the false economy of "allowlist every executable basename".
3. Separate **normal local task** from **real boundary crossing**: writable scope changes, elevated OS rights, additional network/credential capability, protected Git metadata and denied project/worktree roots remain review/typed unavailable according to actual policy. Existing `ManagedProcessService` obtains enforcement before target; `ToolRegistry`, `command_intent`, Bash, terminal compatibility and typed tools preserve that receipt.
4. Define explicit routine developer workflow policy for Interactive, Automatic reviewer, YOLO, headless and plan modes without broadening `SandboxProfile`. Existing scoped `AlwaysAllow` remains useful for repeated single invocations, but no first-token compound-script grant. Provide actionable error reason instead of repeated "try a new spelling". If network isolation isn't available, show honest limitations; do not pretend classification makes egress safe.
5. Ensure normal intentional destructive workspace operations, e.g. clearing `target` or `dist`, can proceed with existing operator-authorized workspace-write permissions or an appropriately scoped Ask/Once/Session grant; do not broadly auto-allow out-of-root deletion or wipe an untracked source directory. Reuse existing Git checkpoints/undo when available; avoid inventing a new rollback engine.
6. Document practical user guidance in `architecture/permission.md`, `architecture/tool.md`, `docs/security-semantics.md` and `docs/tools.md`: "routine work in your approved sandbox", default vs explicit user rules, how to approve once/session, no sandbox=fail required, network warnings and still-unavailable Git subfeatures. Ensure TUI/CLI messages distinguish denied authority from missing capability and ordinary reviewer Ask.

## 4. Verification matrix and measurable acceptance

- **Zero syntax-only terminal rejections** for normal in-scope commands; harmless `bash -c`, `python -c`, quoted pipes, `$(pwd)`, redirection within granted workspace, `env VAR=...`, package/task runners and reusable single-command approvals do not auto-prompt merely for grammar.
- **Low-friction benchmark:** for the curated routine corpus in already-authorized `WorkspaceWrite` with a verified required filesystem boundary and no special stricter user rules, target zero unnecessary prompts attributable to syntactic heuristics. Record and explain exceptions (risky workspace deletion, new authority, explicit Ask rules, unknown tool execution contract); never alter deterministic Deny or requested sandbox to hit a metric.
- **Prompt integrity:** an explicitly requested Deny wins even if a command otherwise looks mundane; `default=ask` and strict operator preferences retain their intentional review semantics. Reusable `cmd:cargo` grant never silently blesses `cargo test; another-command`; persistent broad grants work only with explicit scope and audit.
- **ReadOnly/plan:** a mutating command cannot write and is not secretly retried as a privileged tool; a valid inspection command does not need excessive prompting.
- **Same effect via wrappers:** shell/compiled/Python equivalent outside-workspace write fails under actual Landlock/Seatbelt where obtained; verify negative marker. TCP/UDP is explicitly `Unrestricted` under Landlock-only—do not assert offline; a network-denied mode may be claimed only after Linux M005 native proof.
- **Git compatibility:** distinguish typed `BackendUnavailable` C002/C003 from arbitrary spelling blocker and report remaining unavailable flows without auto-trying `bash git ...` as a workaround.
- **No regression:** no privilege widening to daemon, filesystem/network outside-root, SSH/provider credentials, sessions, child agents, scheduler, worktree lease, cancellation or output handling.
- **Platform:** Linux real Landlock integration, portable policy unit tests for Windows/macOS and their available CI checks; no invented cross-platform namespace claims.

Baseline and post-change command-corpus results must record prompt counts, actual routing, policy source, exact SHA, host features and skipped tests. Suggested verification: `cargo fmt --all -- --check`; `cargo test -p codegg --lib permission`; `cargo test -p codegg --lib tool::bash`; `cargo test -p codegg --test approval_router -- --test-threads=1`; `cargo test -p codegg --test command_routing_adversarial`; `cargo test -p codegg --test sandbox_containment -- --test-threads=1`; `cargo test -p codegg --test policy_surface_m007`; `python3 scripts/check_sandbox_policy_wiring.py`; `python3 scripts/check_approval_router.py`; `scripts/verify.sh quick`.

## 5. Compatibility, failure, stop and closure

No new broad default persisted Allow, helper privilege, OS socket or network grant. A retained user rule that matches command text still controls the invocation and is clearly labeled best-effort. Keep persisted scopes and signature compatibility. Unsupported tools, new Git filters/remote operations and denied actual enforcement must return typed actionable results, not malformed command or fake spawn errors. Closure includes benchmark corpus, before/after prompt counts and denied/allowed fixture examples, exact test commands and residual Linux network limitation. Stop if a "low friction" fix suppresses an explicit Ask/Deny, promotes child scope, routes an unavailable typed Git operation through unsafe Bash, hides actual network/credential access, or requires global OS capability elevation.
