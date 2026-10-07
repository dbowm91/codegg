# Tools

Everything the agent can call is a **tool**: a named, schema'd operation that
passes through one boundary. CodeGG ships a large native registry, takes tools
from MCP servers and plugins, and pulls deterministic validators from the
`eggsact` crate. This page is the map. For what any one tool may actually do,
see `architecture/` and the per-module docs.

## How a tool call is gated

Every production tool call — whether it came from the agent loop or from a Tool
Program — is dispatched through `ToolBroker`, which is the single execution
boundary. The pipeline is ordered:

1. Registry lookup and contract snapshot
2. Caller-policy check
3. Input schema validation
4. Authority, permission, and path policy
5. Deadline/cancellation precheck
6. Route selection (inline native, or scheduler-owned)
7. Execution with invocation identity and provenance
8. Output validation
9. Artifact registration
10. Terminal result recording and event emission

Three configuration layers can narrow what a tool is allowed to do, and they
compose:

| Layer | Where | Effect |
|-------|-------|--------|
| Per-tool enable/disable | `"tools"` block in the config | Turns individual tools on or off |
| Per-tool permission level | `[permission]` in the config | Sets `allow`, `deny`, or `ask` per tool |
| Agent permission table | The agent's `[agent.permissions]` | Narrows the surface for that agent specifically |

The config's `tools` map takes booleans:

```jsonc
{
  "tools": {
    "bash": true,
    "read": true,
    "edit": false
  }
}
```

The `permission` block takes levels, plus path rules and bash pattern rules:

```jsonc
{
  "permission": {
    "read": "allow",
    "glob": "allow",
    "grep": "allow",
    "bash": "ask",
    "edit": "ask",
    "bash_allow_patterns": ["git status", "git diff", "git log"],
    "bash_deny_patterns": ["rm -rf", "git push --force"],
    "tools": {
      "webfetch": "allow"
    }
  }
}
```

Three things worth internalizing:

- **`deny` is a hard stop.** It is enforced ahead of, and independently of, the
  approval mode. No confirmation dialog overrides it.
- **Absent is not the same as `deny`.** A tool you do not mention inherits the
  ambient configuration. Agents that need to be genuinely read-only must deny
  explicitly — see `docs/agents-skills.md`.
- **Command metadata never grants authority.** A skill's `allowed-tools`
  frontmatter, a command's declared agent, and a plugin manifest are
  descriptive. They do not widen the permission envelope.

On top of all three, the deterministic security pipeline can classify a call
and escalate it. That layer is additive and never replaces the permission
system; see `docs/security-semantics.md`.

## Tool families

| Family | Tools | What it covers |
|--------|-------|----------------|
| Read files | `read`, `list`, `glob`, `grep`, `codesearch`, `diff` | Inspecting the workspace |
| Write files | `write`, `edit`, `apply_patch`, `replace` | Creating and changing files |
| Shell | `bash`, `terminal`, `python_script`, `tool_program` | Shell commands, capability-controlled Python, and read-only Tool Programs. Interactive terminals are a TUI/daemon surface, not a tool |
| Git | `git_query` (model-facing), `git_read` (program-only) | Typed read-only repository facts |
| Git mutation | `git`, `commit` | Mutating operations, routed through the risk-classified mutation executor |
| Test and verify | `test`, `verify` | Running the project's test suite, and bounded offline `check`/`build`/`lint`/`typecheck`/`format_check` passes |
| Code intelligence | LSP-backed tools, `lsp_preview_apply` | Diagnostics, definitions, references, impact, repair |
| Deterministic | 13 `eggsact`-backed validators | Text, config, and identifier checking |
| Web and research | `websearch`, `webfetch`, `repo_search`, `repo_fetch`, `repo_map`, `security_search`, `research`, `research_search`, `batch_fetch`, `evidence_bundle` | Evidence-backed external lookups |
| Higher-level workflows | `review`, `security`, `skill`, `task`, `work_order`, `goal_get`, `goal_update_progress`, `goal_request_completion`, `plan_enter`, `plan_exit`, `todowrite`, `memory_search`, `question`, `tool_search` | Delegation, planning, and discovery |

Run `codegg doctor` for the live state of the subsystems behind these.

## Deterministic tools (`eggsact`)

CodeGG links the `eggsact` crate as an in-process library at version 1.2.5. No
external executable is resolved, so these tools work without an install step.

```bash
codegg doctor deterministic-tools
```

On a default install that reports:

```text
enabled: true
backend: native
profile: codegg_core
expose_expert_tools: false
max_output_chars: 12000
model tools: 8 always-visible, 5 deferred
```

Thirteen tools are registered, split by whether the model sees them up front:

| Always visible | Deferred — discover via `tool_search` |
|----------------|---------------------------------------|
| `text_equal` | `text_inspect` |
| `text_diff_explain` | `config_preflight` |
| `text_replace_check` | `identifier_inspect` |
| `validate_json` | `structured_data_compare` |
| `validate_toml` | `text_fingerprint` |
| `command_preflight` | |
| `path_normalize` | |
| `text_security_inspect` | |

`tool_search` is how the model finds the deferred half.

`command_preflight` and `config_preflight` are also driven from the harness
side by the eggsact preflight service, which checks shell commands, patches,
and config writes before they take effect. It is enabled by default in
`warn` mode, and its mode is configurable from `off` through `observe` and
`warn` to `block_on_definite`. Note that this is a different surface from the
`!` human-shell confirmation, which is a separate regex-based screen in the
shell policy.

The whole family is configured under `[deterministic_tools]`, whose defaults
match the report above:

```jsonc
{
  "deterministic_tools": {
    "enabled": true,
    "backend": "native",       // or "disabled"
    "profile": "codegg_core",
    "model_audience": "model",
    "harness_audience": "harness",
    "expose_expert_tools": false,
    "max_output_chars": 12000
  }
}
```

Setting `backend` to `disabled` replaces the real tools in the registry with
stubs that return a clear error, rather than silently routing calls somewhere
else — the model sees the failure instead of getting a different answer than it
asked for.

## Git

The `egggit` crate is **read-only by construction**. It exposes branch,
status, diff summary, changed files, log, blame, refs, worktree facts,
operation state, and patch validation. It does not mutate a repository: commit,
worktree create/remove, and every other mutating workflow stay with the host
application, which owns its own permission and approval policy.

Two read surfaces sit on top:

- `git_query` — the model-facing semantic surface. Bounded read operations
  with a fixed schema: `status`, `diff`, `log`, and `branches`.
- `git_read` — the program-only adapter over the same canonical execution
  service. Hidden from ordinary model turns.

Mutations live in `src/git_mutations.rs` behind the `git` and `commit` tools, and
share one execution model: resolve
and policy-check the repository root, snapshot pre-operation state (HEAD,
branch, index, worktree), validate preconditions, render argv shell-free, run
with a timeout and noninteractive controls, snapshot post-operation state, and
return a typed state delta.

Network operations — fetch, pull, push, remote config — carry an extra policy
layer in `src/git_network_policy.rs`: `GIT_TERMINAL_PROMPT` is pinned to `0` so
a credential helper cannot block, credentials embedded in URLs are redacted
before persistence or display, and transport failures are classified into DNS,
connect, auth, ref-rejection, and timeout categories so the UI can say
something actionable.

Force pushes, hard resets, and similar operations are classified by risk. See
`architecture/git.md` and the `git` skill.

## Language servers

LSP is a native backend (`backend=native`, `kind=active`), but exposure to the
model is behind a gate that is **off by default**:

```jsonc
{
  "experimental": {
    "lsp_tool": true
  }
}
```

```bash
codegg doctor lsp
```

reports the gate, and when it is `false` the model tool is hidden — the registry
still knows about it. This is deliberate: LSP changes what the agent can see
about the workspace, so it is opt-in.

Configure servers under the `[lsp]` block. `docs/LSP.md` covers server setup and
configuration; the module contract is `architecture/lsp.md`.

If a sandbox profile is requested without a workspace root, bash runs without
OS containment and that is reported as *unavailable*, never as full host
access — the absence of a workspace never silently widens authority.

## Testing and the scheduler

The `test` tool runs the project's suite through the daemon scheduler rather
than spawning a process directly. That is the general rule: process-heavy work
— test runs, managed processes, subagent dispatch, tool programs — is submitted
to `JobSubmissionService` and admitted by `JobScheduler`, which owns global
admission control for the single daemon.

Consequences:

- Admission is decided in one place rather than at each call site, so a busy
  daemon queues work instead of running everything at once.
- Submission is idempotent: re-submitting the same job returns the original.
- Under `--standalone` there is no daemon scheduler, so the test tool refuses
  with an explicit error rather than running unsupervised. Run builds and
  tests through the normal daemon path.

The result is classified into `passed`, `failed`, `cancelled`, or `timed_out`.
See `architecture/scheduler.md`, `architecture/jobs.md`, and
`architecture/testing.md`.

## Web search and research

CodeGG's web, repository, security, research, batch-fetch, and evidence-bundle
tools are **wrappers** around the external `eggsearch` MCP server. Prebuilt
installer bundles carry a pinned `codegg-eggsearch` sidecar at version 0.3.9;
a source install must provide it separately.

```bash
codegg doctor search
```

The raw `mcp__eggsearch__*` tools are **hidden from the model by default**
(`Raw MCP tools exposed to model: no`). The wrappers exist to bound and
trust-frame the output instead of passing an unbounded third-party response
straight into context.

Output is capped per tool family, so no single lookup can dominate the context
window:

| Tool family | Cap (chars) |
|-------------|-------------|
| `search` | 12,000 |
| `fetch` | 20,000 |
| `repo_search`, `repo_fetch`, `repo_map` | 15,000 |
| `security` | 10,000 |
| `research` | 15,000 |
| `batch` | 50,000 |
| `evidence` | 100,000 |

The default provider timeout is 60,000 ms. All of these are configurable under
`[search]`.

`research` is the long-horizon entry point and is what the built-in `research`
agent uses; `websearch` is the quick lookup. `docs/providers.md` covers
credential and connection setup.

## MCP and plugins

Tools from configured MCP servers and installed plugins join the same registry
and pass through the same broker, so they inherit the same permission gating.
A disabled domain stays visible in the catalog but returns an actionable error
if called. See `docs/MCP.md` and `docs/PLUGINS.md`.

Tool Programs — reusable, reviewable bundles of tool calls — are executed
through the broker like anything else. See `architecture/tool_programs.md` and
the `tool-program-harness` skill.

## Diagnosing what you have

```bash
codegg doctor                      # everything
codegg doctor deterministic-tools # the eggsact family and preflight
codegg doctor lsp                  # the LSP gate and active servers
codegg doctor search               # the eggsearch backend and caps
codegg doctor mcp                  # configured MCP servers
codegg doctor providers            # configured providers, read-only
```

Inside the TUI, `/tool-backends` reports which tools are disabled and why, and
`/tool-contracts` reports the registered contracts.

## See also

- `architecture/git.md` — typed Git operations and risk classification
- `architecture/permission.md` — permission levels and the approval path
- `architecture/security.md` — deterministic security semantics
- `architecture/testing.md` — test taxonomy and execution strategy
- `architecture/scheduler.md` — global admission control
- `architecture/tool_programs.md` — reusable tool call bundles
- `docs/security-semantics.md` — the security pipeline and its gates
- `docs/agents-skills.md` — narrowing a tool surface with an agent
- `docs/configuration.md` — the full config schema
