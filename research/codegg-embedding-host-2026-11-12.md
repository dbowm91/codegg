# CodeGG Embedding Host — Research Record (2026-11-12)

Status: research artifact; informs `plans/subsystems/codegg-embedding-host-roadmap.md`.

Baseline: `1d2d9dce2`. Read-only investigation of the CodeGG tree plus a live
integration attempt from a host project (EggUI) driving the pinned Linux image.

## Why this record exists

A host project needed to drive CodeGG as a coding agent inside a Linux VM. It
used ACP v1, the only documented programmatic surface, and hit five hard limits
that are properties of CodeGG's current embedding story rather than of that
host:

1. **No connection-level request headers.** The host's provider gateway routes
   by `x-opencode-session`. `ProviderConfig` has no headers field
   (`crates/codegg-config/src/schema.rs`), `CreateProviderConnectionRequest`
   carries `endpoint` and `credential` only (`crates/codegg-protocol/src/provider.rs`),
   and the generic `openai` provider has no per-request header control. The host
   had to add a loopback relay inside the VM to inject the header.
2. **A closed provider registry.** The model's provider segment is resolved in a
   fixed built-in registry and a custom id is rejected with
   `provider_not_found` (`src/core/daemon_turns.rs`). The `opencode_go` provider
   is pinned to a shared-profile base URL (`crates/codegg-providers/src/opencode_go.rs`)
   and refuses any model without a reviewed wire mapping: `model "…" on provider
   "opencode_go" has no reviewed wire mapping`.
3. **No model metadata injection.** `ProviderConfig.models` is
   `HashMap<String, ModelConfig>` where `ModelConfig` is `{name, variants}` — no
   context window, output budget or tool support. A host that already knows
   those facts has nowhere to put them.
4. **No embedding contract.** ACP v1 (`src/acp.rs`, 1,695 lines) has no
   `_meta`/extension handling, text-only prompts, a handful of `sessionUpdate`
   kinds and no structured result or error taxonomy. The native CoreFrame
   protocol (`crates/codegg-protocol`, 16,848 lines; client
   `crates/codegg-client`, 5,418 lines) is rich but is a *frontend* contract:
   documented as frontend-side client contracts, bound to a daemon over a local
   unix socket or Windows pipe (`LocalDaemonPaths`), and modelled around
   projects, sessions and git worktrees (`CoreResponse::SessionLaunch` carries
   `workspace_id`, `worktree_id`, `checkout_path`).
5. **Cross-boundary reachability.** The daemon transport is local-only; there is
   no TCP or WebSocket listener in the daemon. A host on the other side of a VM
   boundary can only reach a guest process over stdio or a forwarded socket.

## The three surfaces as they stand

| Surface | Size / shape | Strengths | Limits for embedding |
|---|---|---|---|
| ACP v1 (`src/acp.rs`) | 1,695 lines, JSON-RPC over stdio | Simple, PTY-free, already used for headless runs | No provider input, no headers, no structured errors/results, no extension surface |
| Native protocol (`codegg-protocol` + `codegg-client`) | 16,848 + 5,418 lines, daemon over unix socket | Full control: sessions, turns, provider connections, approvals, documents, LSP, git | Frontend contract, daemon required, project/worktree model, local socket only, no stability statement |
| `codegg exec` (`src/exec.rs`) | one-shot JSON in/out, `-s` resume, policy flags | Small, deterministic shape: `{prompt, model, agent}` → `{success, result, toolsUsed, tokensUsed, durationMs, error, code}` | No workspace parameter (uses cwd), no provider input, no streaming, no cancellation, one error string |

`codegg exec` is the closest thing to an SDK, and it already drives the real
agent loop. It reads its provider registry from `Config::load()` plus
`register_builtin_with_config_connections` (`src/exec.rs`), which is exactly the
seam an inline descriptor needs.

## What a host actually needs to control

| Control | Today | Needed |
|---|---|---|
| Provider endpoint | config file only | per-session descriptor |
| Provider credential | config file or credential store | descriptor, never argv |
| Request headers | not expressible | descriptor header list |
| Model identity | built-in registry + reviewed wire list | any model id on a declared connection |
| Model metadata | guessed by the resolver | descriptor catalog: context window, output budget, tool support |
| Wire surface | fixed per built-in provider | descriptor declares `chat_completions` (v1) |
| Workspace | cwd / project + worktree | absolute root, git optional |
| Session lifecycle | daemon socket + config | in-process host session over stdio |
| Approval / sandbox | CLI flags (exec) or TUI state | per-session fields |
| Progress | ACP notifications only | structured events with usage |
| Cancellation | ACP cancel / daemon control | host cancel for one turn |
| Errors | `provider_api` string | stable taxonomy + provider status |
| Versioning | ACP v1 only | host `apiVersion` + capability list |

## Options considered

**A. Extract the agent loop into a library crate and link it from the host.**
Rejected for now. `src/agent/` is 40,492 lines inside the binary crate and is
entangled with the bus, TUI and daemon. It also does not help a host on the
other side of a VM boundary, which cannot link a macOS-side binary to Linux
guest code.

**B. Extend ACP with namespaced extensions.** Rejected. ACP is deliberately
minimal and has no `_meta` handling today; adding provider descriptors,
streaming events and a result taxonomy would either fork the protocol or make
the adapter a second, undocumented protocol wearing ACP's name.

**C. Use the native daemon protocol directly.** Rejected for v1. It requires a
daemon, is bound to projects/worktrees, and is a frontend contract with no
stability commitment. Reusing it would import a large surface a host does not
need and cannot reach across the boundary.

**D. A headless host mode over stdio, plus an SDK crate as its client.**
Selected. It reuses the loop where it lives, needs no daemon or global config,
rides the stdio pipe a VM host already has, and can be versioned independently
of both ACP and the frontend protocol.

## Selected shape

- `codegg host --stdio`: newline-delimited JSON-RPC 2.0, `apiVersion: 1`,
  additive-only within a major version.
- `initialize` → capabilities and CodeGG version.
- `session/open` carries the workspace root, model, agent, approval, sandbox and
  an **inline provider descriptor**: `{id, base_url, headers, credential |
  credential_env, protocol, models[]}`.
- `turn/submit`, `turn/cancel`, `session/close`; notifications for text, tool
  use, usage, completion and a typed error.
- `codegg-sdk`: a typed client over that protocol, usable by any host and by
  CodeGG's own conformance tests.
- The reviewed-wire-mapping rule stays for built-ins; a descriptor-declared
  connection declares its surface explicitly, which is a statement, not a guess.

## Evidence that this is bounded work

- Per-turn provider registration with a locator already exists
  (`register_builtin_with_config_connections`, `TurnCredentialLocator`), so an
  inline descriptor is a new registration path, not a new architecture.
- Arbitrary header injection already exists for one built-in: the
  OpenAI-compatible provider has `with_session_affinity_header`
  (`crates/codegg-providers/src/openai_compatible.rs`). Generalising it to a
  bounded header list is a small change.
- Model profiles are already a crate-level concept
  (`crates/codegg-core/src/model_profile/{types,adapter,resolve}.rs`), so
  descriptor-supplied metadata has a natural home.
- `exec` already proves the loop can run headless from a small JSON input.

## Residual risks

- CodeGG has no published stability policy for embedding; the host protocol must
  define one before a second consumer appears.
- The loop lives in the binary crate; a later in-process embedding would require
  extraction, and this record does not claim it.
- Inline credentials cross the same trust boundary as the config file today.
  They must never travel on argv (`ps`-visible) and never enter a log.
- A descriptor-declared wire surface must be validated, not trusted blindly:
  only `chat_completions` is in scope for v1.
