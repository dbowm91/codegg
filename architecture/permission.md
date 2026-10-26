# Permission Module

The `permission` module enforces access control for tool execution and
file paths. It provides a multi-layered ruleset system with cached
decisions, HMAC-signed persistence, DoomLoop detection, and
mode-based permission workflows.

## Purpose

Enforce per-tool, per-path, and per-command permission decisions before
any tool executes. Prevent prompt fatigue via cached decisions, detect
stuck-agent loops, and provide mode-based permission envelopes
(review/debug/docs) for specialized workflows.

## Where It Lives

| Artifact | Location |
|----------|----------|
| `PermissionChecker`, `PermissionStore`, `DoomLoopDetector`, ruleset types | `src/permission/mod.rs` |
| `ModeDefinition`, `BuiltinModes` (review/debug/docs) | `src/permission/modes.rs` |
| `PermissionRegistry` (ask-response broker) | `crates/codegg-core/src/bus/mod.rs` |
| `ApprovalRouter`, `ApprovalMode`, `ApprovalRequest/Decision` | `src/permission/approval.rs` (domain: `crates/codegg-core/src/approval.rs`) |
| `ApprovalReviewer` (Automatic Escalate-only helper) | `src/permission/reviewer.rs` (see `architecture/approval_reviewer.md`) |
| `RuntimePreferenceStore` (daemon-owned, principal-scoped) | `crates/codegg-core/src/approval.rs` (table `runtime_preferences`, v58) |
| `ExecutionPolicySnapshot` (immutable turn/batch) | `crates/codegg-core/src/approval.rs` |
| `PermissionDecision` (bus DTO) | `crates/codegg-core/src/bus/mod.rs` |
| `ToolCategory` enum | `src/tool/mod.rs` |
| Destructive bash patterns | `src/tool/destructive.rs` |

> **Note:** `PermissionRegistry` is in `codegg-core`, not in the
> permission module. The permission module defines domain types; the bus
> owns the ask-response broker.

## How It Works

### Tool-Category Short-Circuit

Every `Tool` reports a `ToolCategory` (`ReadOnly | SafeMutating |
Mutating | ShellExec`). Native model-surface resolution and catalog metadata
use that instance-level category. The function `tool_category_for_name()` in
`src/permission/mod.rs` is only the conservative name-based fallback for
paths without a native instance; unknown names remain `Mutating`.

Categories with `is_permission_free() == true` (`ReadOnly`,
`SafeMutating`) short-circuit `PermissionChecker::check()` to `Allow`
before any store/rule/glob lookup, unless a persistent `Deny` is in
the store.

`ReadOnly` (28 names): `read`, `glob`, `grep`, `list`, `webfetch`,
`websearch`, `codesearch`, `lsp`, `diff`, `security`, `skill`,
`tool_search`, `plan_enter`, `plan_exit`, `repo_search`, `repo_fetch`,
`repo_map`, `research`, `research_search`, `batch_fetch`,
`security_search`, `evidence_bundle`, `mcp_resource_search`,
`mcp_resource_read`, `memory_search`, `memory_get`, `context_read`,
`extension_search`.

`SafeMutating` (5 names): `todowrite`, `todoread`, `question`,
`invalid`, `extension_install_request`.

The short-circuit is **name-based** and lives only in `check()`. The
args-carrying entry point `check_with_args()` (used for `bash`,
`terminal`, `test`, `git`) does **not** short-circuit — it has its own
scope lookup and its own shell-specific fallback. In practice the only
args-carrying callers are `bash` and `git`
(`src/agent/tool_batch.rs`), neither of which is permission-free, so the
two paths do not disagree.

### Check Flow

```
ToolCallRequested
    │
    ▼
PermissionChecker::check()  (or check_with_args for shell)
    │
    ├──► Tool category short-circuit (ReadOnly / SafeMutating)   [check() only]
    │         │
    │         ├── Persistent Deny in store → Deny
    │         └── otherwise                → Allow
    │
    ├──► Policy/sandbox denial (effective_policy_denies)
    │         │
    │         └── Deny → Return Deny
    │
    ├──► Check PermissionStore (cached, HMAC-verified)
    │         │
    │         ├── Allow → Return Allow
    │         └── Deny  → Return Deny
    │
    ├──► Check tool rules (agent > session > config priority)
    │         │
    │         ├── Allow → Return Allow
    │         ├── Deny  → Return Deny
    │         └── Ask   → Continue
    │
    ├──► Check path globs (on canonicalized paths)
    │         │
    │         ├── Allow → Continue
    │         └── Deny  → Return Deny
    │
    ├──► (shell tools only) Destructive-pattern fallback
    │         │
    │         ├── Matches DESTRUCTIVE_BASH_PATTERNS → Ask
    │         └── Non-destructive                   → Allow
    │
    └──► Return default (Ask/Allow/Deny)

--- If result is Ask, the tool batch handles the dialog: ---

ToolBatch::check_tool_permission()  (src/agent/tool_batch.rs)
    │
    ├──► Create oneshot channel
    │
    ├──► PermissionRegistry::register_with_session(session, turn, perm_id, tx)
    │         [Registration-before-publish; sync fn, NOT async]
    │
    ├──► GlobalEventBus::publish(PermissionPending { ... })
    │
    ├──► Wait for response (unbounded by default; see
    │     permission.approval_timeout_secs to bound it.
    │     Always bounded below by the registry orphan guard —
    │     see "Orphan Guard" below)
    │
    ├──► User responds → PermissionRegistry::respond_scoped(session_id, perm_id, choice)
    │
    └──► Cache decision if AlwaysAllow/AlwaysDeny
```

The single production human-wait owner is
`ApprovalRouter::request_human_approval()` (`src/permission/approval.rs`),
which performs the register → publish → wait → unregister sequence.
`scripts/check_approval_router.py` enforces that it stays the only owner.

### Orphan Guard

The human wait itself is unbounded by default — a user may take arbitrarily long
to decide, and that is intended. The registry is nevertheless *always* given a
non-zero `pending_ttl` so a waiter that can never be woken is eventually
released rather than parked forever:

- When `permission.approval_timeout_secs` is set, the TTL is that value plus a
  60s grace, so the router's own deadline fires first and the waiter
  unregisters itself cleanly rather than being evicted mid-wait.
- Otherwise the TTL is a long orphan guard (24h) that will not fire against a
  user who is still deciding, but does release a genuinely stranded waiter.

Evicting an entry drops its oneshot sender, which the awaiting task observes as
`CHANNEL_CLOSED_DENY` — a typed deny that reaches the model — instead of the
turn spinning on "waiting for command/tool output..." with no error. This is the
backstop for the case `respond_scoped` cannot repair: a response dropped because
its `session_id` did not match leaves the waiter with no sender to wake it.

### Bash Command Handling

Structured native **mutations** (`write`, `edit`, `apply_patch`, `replace`)
may target the authenticated workspace or the session's private
or explicitly shared scratch directories. Mutations inside those
host-validated roots do not trigger the general permission default, including
`permission.default = "ask"` — `check_trusted_root_mutation()` relaxes
*only* the implicit default `Ask`, and returns `Deny` outright for any
tool outside that four-tool set. Stored decisions, explicit tool or path
rules, and hard denials still apply.
The broker supplies session identity; each tool resolves the CodeGG-managed
directory and applies the same symlink-aware validator beneath that root.
Relative paths continue to resolve from the workspace — a relative path is
resolved against the workspace root only, never against scratch or a
secondary root. Scratch access does
not apply to another session, human shell, plugins, or arbitrary absolute
paths.

If a structured `write` reaches the path boundary with an absolute target under
a recognized host temporary root, but that root is not authorized, the tool
returns a non-authoritative recovery hint to retry under the current private
scratch path. It does not rewrite the target or add a grant. The hint is shown
only when the host scratch directory exists. An explicit Deny is resolved
before the tool runs and is never converted into a scratch write. Structured
reads and shell commands keep their existing path and execution semantics.
This hint does not intercept shell writes: an unsandboxed FullHost process can
address the real host temp directory, and shell syntax inspection is not
containment.

### The temp-root redirect offer

The hint above lives inside the write tool, so it is only reachable once the
permission layer has already said Allow. Under the default policy a write
outside every authorized root is the implicit `Ask`, so the human was asked
first and the model never received the suggestion.

The redirect offer closes that gap without moving any authority. When the
pending decision is the *implicit default* `Ask`, and the target is a
structured `write` of a recognized host temporary root that is outside every
authorized root, the agent loop answers the **model** first: the call is
refused with the same actionable message, naming the caller's own private
scratch root, and the turn continues. A model that retries at the suggested
path succeeds with no human involved. A model that repeats the same restricted
path is not offered again and reaches the normal human approval path, which is
unchanged.

The offer grants nothing and rewrites nothing: the restricted path is still
refused, and the human is still the only authority for it. It is issued only
for the implicit default — an operator tool rule, an operator path rule, a
stored decision, or a hard deny keeps its authority, and the human is asked
directly. The offer is scoped to the structured `write` tool, because the
edit family is rooted at the workspace and cannot write into a scratch root;
offering a scratch path there would be a broken promise. The offer state is
in-memory and per session: a daemon restart clears it, so the next attempt is
offered again.

The offer sits *after* the security and sensitive-path escalations, never
before them: a write to a system directory, a sensitive path, or a security
finding reaches the human first, unchanged. The recognized host temp roots
this milestone covers are `/tmp`-style paths, which are not system paths in
that sense, while the product's own scratch roots live under the user cache
directory.

The guidance the model receives goes through the same `redact_local_paths`
pass as any tool output, so a scratch root under `/var` or `/tmp` is shown
redacted. The model already knows its own scratch root — the capability
contract in its prompt names the private session scratch, and a delegated
run's prompt block names its private run scratch — so the offered file name
remains actionable without disclosing a path the model was not given.

`check_with_args()` adds a destructive-pattern fallback for
`ShellExec` tools. If the user's ruleset would allow the command but
it matches a destructive pattern (`rm -rf /`, `mkfs`, `dd of=...`,
`:(){:|:&};:`, `shutdown`, etc.), the result is `Ask`. If the command
is non-destructive, the result is `Allow` — auto-approving safe
commands like `ls`, `cat`, `cargo build`, `git status` even in a
strict `default = "ask"` config.

Safe bash patterns are defined in `default_bash_allow_patterns()`
(`src/permission/mod.rs`, `default_bash_allow_patterns()`). Users can extend
or override these via
`bash_allow_patterns` and `bash_deny_patterns` config fields.

For Bash, rules retain the tier order agent > session > config. Within one
tier, a matching command pattern takes precedence over an unpatterned fallback;
among matching patterns, Deny outranks Ask, which outranks Allow. Thus a
current effective Deny cannot be hidden by `default = "allow"`, an `ask`
fallback, `allow_all_bash`, or a saved Allow. This resolves one invocation
string only: glob patterns are preferences and do not prevent an equivalent
effect through a script, wrapper, or another tool.

When a workspace and action are already authorized, routine development
commands do not receive an additional Ask solely for quotes, shell wrappers,
interpreter flags, environment assignments, pipes, or substitutions. Explicit
`ask`/`deny` rules, meaningful risk signals, and a missing required sandbox
still follow their normal review or denial paths. Interactive, Automatic, and
Yolo change how an Ask is resolved; they do not widen the captured execution
scope. A FullHost request has host authority and must not be described as
protected by command-pattern review.
Operator-selected `bash_deny_patterns` are best-effort invocation filters; they
are not kernel enforcement. Built-in command risk classification may request
review, but ordinary wrappers, quotes, expansions, and pipelines do not prompt
solely because of their syntax.

Remembered shell approvals use `cmd:<executable>` only when the existing
`command_intent::shell_shape` parser classifies the request as one simple
command. A simple `env NAME=value <command>` wrapper and an absolute
executable path are canonicalized. Shells, interpreters, and command wrappers
use exact-invocation scopes even when their argument text has simple argv
shape.

A compound command contributes **one scope per segment**
(`decision_scopes_for_shell_command`). The command is split on control
operators found outside quotes, and a segment is scoped independently, so
approving `a && b` persists `cmd:a` and `cmd:b` rather than a single digest
that matches neither variation later. This is what makes "Allow always"
converge instead of re-prompting on every slightly different pipeline.

Segments are combined with **intersection** semantics: the compound command is
authorized only when *every* segment resolves to Allow, and an explicit Deny on
any one segment denies the whole. Segmenting therefore cannot widen a grant
beyond what the operator approved.

A command is **not** segmented when it contains a construct whose interior the
splitter cannot see — command substitution, variable expansion, heredoc, or
unbalanced quotes — because a `;` or `|` inside such a construct could be cut as
if it were a real separator and leave the tail looking like a clean
`cmd:<argv0>` family grant. Those keep a single `cmd:exact:<sha256>` scope over
the complete command text, as does a segment that is itself complex — so a
redirection, glob, or interpreter invocation stays pinned to its literal text
and a saved `cmd:cargo` family grant still cannot cover `cargo test; ./other-script`.
Explicit legacy unscoped grants remain broad and readable for compatibility;
current effective Deny rules still win. The digest is a cache scope, not shell
parsing or OS enforcement.

Escalation signals from the shell policy (`tool::bash::policy::find_ask_pattern`)
are consulted on the same path, immediately after `destructive_match`: a hit
returns `Ask` carrying an `ask_pattern` field rather than being rejected inside
the tool. `bash_ask_patterns` adds operator-defined signals. These are review
controls, not boundaries — see `architecture/tool.md` for why a pattern match
constrains the invocation rather than the program, and why containment belongs
to `SandboxProfile` plus Landlock.

### Command preflight is advisory

`PreflightService` runs on every `bash` call and reports findings. Findings
remain advisory and may add a reason to a request already requiring approval;
they never create an escalation solely because the service ran. The resolved
`block_on_definite` mode is a separate caller policy: its typed `blocked` result
is enforced consistently at the agent boundary and in direct `bash` calls.
Other modes carry only the bounded advisory summary. The service is wired by
default, so `CommandPreflightResult` may be present for a clean command too.

That arm is also reached from `PermissionResult::Allow`, not only `Ask`, so a
presence test silently discarded a stored `AlwaysAllow` grant. An advisory may
add a reason to a request that is already escalating; it may never create one
on its own or override a stored operator decision. Explicit permission Deny,
security Deny, and the separately configured `block_on_definite` policy remain
authoritative. Surfaced command-risk findings are labeled `advisory-block` and stay `Warn` so an advisory never reads as an enforcement verdict.

## Key Types & APIs

### PermissionLevel (`src/permission/mod.rs`)

```rust
pub enum PermissionLevel {
    Deny,
    Ask,
    Allow,
}
```

### PermissionResult (`src/permission/mod.rs`)

```rust
pub enum PermissionResult {
    Allow,
    Deny,
    Ask(PermissionRequest),
}
```

### PermissionDecisionReceipt (`src/permission/mod.rs`)

Ephemeral receipt produced when the permission boundary accepts a call.
Contains `decision_id`, `outcome` (a `PermissionLevel`), `source`,
`issued_at`, and optional `policy_revision`. Callers must not manufacture
policy revisions from unrelated session identifiers after evaluation.

### PermissionChoice (`src/permission/mod.rs`)

```rust
pub enum PermissionChoice {
    AllowOnce,
    AlwaysAllow,
    DenyOnce,
    AlwaysDeny,
}
```

Bidirectional `From` impls convert between `PermissionChoice` (domain)
and `PermissionDecision` (bus DTO).

### PermissionRuleset (`src/permission/mod.rs`)

```rust
pub struct PermissionRuleset {
    pub default: PermissionLevel,
    pub tool_rules: Vec<ToolRule>,
    pub path_rules: Vec<PathRule>,
}
```

### ToolRule (`src/permission/mod.rs`)

```rust
pub struct ToolRule {
    pub tool: String,                       // Supports glob patterns
    pub level: PermissionLevel,
    pub paths: Option<Vec<String>>,         // Path restrictions (canonicalized)
    pub bash_patterns: Option<Vec<String>>, // Bash command patterns
}
```

`matches()` supports `*` wildcard and glob compilation.
`matches_bash_command()` checks bash command patterns similarly.

### PermissionChecker (`src/permission/mod.rs`)

Main enforcement point:

```rust
pub struct PermissionChecker {
    config_rules: PermissionRuleset,
    session_rules: PermissionRuleset,
    agent_rules: PermissionRuleset,
    store: Arc<RwLock<PermissionStore>>,
    compiled_globs: Vec<(globset::GlobMatcher, PermissionLevel)>,
    canonicalized_config_tool_rules: Vec<CanonicalizedToolRule>,
    canonicalized_explicit_config_tool_rules: Vec<CanonicalizedToolRule>,
    canonicalized_session_tool_rules: Vec<CanonicalizedToolRule>,
    canonicalized_agent_tool_rules: Vec<CanonicalizedToolRule>,
    legacy_exec_mode: bool,
    path_cache: Arc<RwLock<HashMap<String, (PathBuf, Instant)>>>,
    temp_root_offers: Arc<std::sync::Mutex<HashSet<String>>>,
}
```

`canonicalized_explicit_config_tool_rules` holds only the levels the
operator actually wrote (`explicit_config_ruleset()`), so a built-in
default never looks like a deliberate operator decision.
`legacy_exec_mode` records that `with_exec_mode()` was applied.
`temp_root_offers` is the in-memory, per-session "already offered this
redirect" set (see "The temp-root redirect offer" above).

Key methods:
- `check(tool, path, session_id)` — main entry point (async)
- `check_legacy(tool, path)` — no-session variant
- `check_bash(path, command, session_id)` — bash-specific with
  destructive-pattern fallback
- `check_bash_legacy(path, command)` — no-session bash variant
- `check_git(path, subcommand, session_id)` — git-specific
- `check_with_args(tool, path, args, session_id)` — generic with args
- `check_trusted_root_mutation(tool, path, allowed_roots, session_id)` —
  relaxes only the implicit default `Ask` for a structured mutation
  addressed to an authenticated root
- `temp_root_redirect_offer(tool, path, allowed_roots, private_scratch, session_id)` —
  returns the non-authoritative redirect message, or `None`
- `with_session_rules(rules)` — per-session overrides
- `with_agent_rules(rules)` — per-agent overrides
- `with_active_mode(config)` — wires built-in modes into agent rules
- `with_exec_mode()` — CI/CD mode, all destructive tools auto-allowed
- `always_allow(tool, path, session_id)` / `always_deny(...)` — persist
- `always_allow_scoped(...)` / `always_deny_scoped(...)` — persist with a
  capability scope
- `clear_decisions()` — wipe cached decisions

### Agent declarations vs. operator config

An agent's `[agent.permissions]` declaration is authoritative **when the
operator has not spoken**. The distinction that matters is *operator-wrote-it*
versus *built-in default*: `config_ruleset()` folds the `default_ruleset()`
`Ask` entries in, so passing it as "the config's opinion" made a default look
like a deliberate operator decision and clamped every agent `allow` back to
`ask`. `explicit_config_ruleset()` reports only levels the operator actually
wrote, and the safety envelope merges that.

The envelope still takes the most restrictive level across agent, session, and
config, so agent declarations never widen what narrows. Ordering is unchanged:
policy/sandbox denial wins, a remembered user Deny/Ask wins over an agent
allow, an operator-explicit level wins over the agent's default, and security
escalation stays reachable from an `Allow` outcome. A tool **no** agent declares
keeps the existing default prompt surface.

Both the main loop (`agent_loop_factory`, the exec path in `main.rs`) and a
subagent's own loop (`worker.rs`) must load the active agent's rules. The
per-agent blocks used to be parsed, generated, and then ignored on the main
loop, which silently disabled every one of them.

`[agent.permissions]` is keyed by *tool*, which cannot express "this tool, but
only to look". `git = "allow"` also authorises `git push`; `git = "deny"` costs a
read-only agent the repository facts it needs. The `git_read_only = true`
declaration is the middle, and it is enforced at the **tool** boundary rather
than here — see [git.md](git.md) "Read-only Git". A permission level answers
"may this run?"; the read-only gate answers "may this agent run *this* git
invocation?", and only the tool sees the argv that separates `branch` from
`branch -d`. It never widens anything: an agent must also be permitted to use
`git` at all, and any `deny` on `git` still wins.

### PermissionStore (`src/permission/mod.rs`)

HMAC-signed persistent decision cache:

```rust
pub struct PermissionStore {
    decisions: Vec<PersistentDecision>,
    store_path: Option<PathBuf>,
}

pub struct PersistentDecision {
    pub tool: String,
    pub path: Option<String>,
    pub scope: Option<String>,     // M007 capability scope (cmd:<argv0>, git:<sub>, …)
    pub level: PermissionLevel,
    pub created_at: i64,
    pub signature: String,           // HMAC-SHA256 via CODEGG_PERM_KEY
    pub session_id: Option<String>,  // Per-session isolation
}
```

- Session-specific decisions checked first, then global
- HMAC signature prevents tampering (`CODEGG_PERM_KEY` env var)
- Persists to `~/.config/codegg/permissions.json` — via
  `canonical_permission_store_path()` (`src/permission/approval.rs`),
  which honors the `CODEGG_PERMISSIONS_PATH` override. Production
  `PermissionChecker` constructors use it; subagents get an ephemeral
  store (`None`) so a child cannot write a global grant.
- `scope` is `#[serde(default)]`: a pre-M007 file stays readable, and a
  legacy broad row (no scope) keeps matching every command family for
  its tool/path. The scope is part of the HMAC input, and
  `DECISION_SCOPE_MAX_LEN` (80) bounds the serialized scope so a scope
  can never carry raw command text.

### DoomLoopDetector (`src/permission/mod.rs`)

Detects repetitive tool call patterns using window-based counting:

```rust
pub struct DoomLoopDetector {
    history: VecDeque<String>,
    counts: HashMap<String, usize>,
    max_window: usize,    // Capped at 1000
    threshold: usize,     // Capped at 100, min 1
}
```

- `record_tool_call(tool_name, arguments)` — records a composite key
  (tool + JSON argument hash) into the sliding window
- `is_doom_loop()` — returns true if the **most recent** tool has been
  called `threshold` or more times anywhere in the window
- `current_doom_tool()` — returns the tool name from the last call
- `reset()` — clears history

### Mode System (`src/permission/modes.rs`)

```rust
pub struct ModeDefinition {
    pub name: String,
    pub description: String,
    pub default: PermissionLevel,
    pub allowed_tools: Vec<String>,
    pub restricted_tools: Vec<String>,
    pub tool_overrides: Vec<(String, PermissionLevel)>,
}
```

**Built-in Modes:**

| Mode | Default | Restricted Tools |
|------|---------|------------------|
| `review` | Ask | edit, apply_patch, replace, write, bash, terminal, git, commit, image, task |
| `debug` | Allow | task, image, commit |
| `docs` | Ask | bash, terminal, git, commit, task, image |

All three modes allow todowrite/todoread for in-flight planning.
Modes are activated via `[mode.review]` (etc.) in config; the
permission checker merges mode rules into agent-level rules via
`with_active_mode()`.

### PermissionRegistry (`crates/codegg-core/src/bus/mod.rs`)

The ask-response broker. **All methods are synchronous (`fn`), NOT
async.** Uses a `DashMap`; age-based eviction is **off by default**
(`pending_ttl = PENDING_TTL_UNBOUNDED`) and is armed by the caller (see
"Orphan Guard").

```rust
pub struct PermissionRegistry {
    senders: DashMap<String, PendingPermission>,
    last_cleanup_ms: AtomicU64,
    pending_ttl: AtomicU64,
}
```

Key methods:
- `register(perm_id, tx)` — backward-compatible, session = `"default"`
- `register_with_session(session_id, turn_id, perm_id, tx)` — full
  session/turn scoping
- `respond(perm_id, choice)` — backward-compatible
- `respond_scoped(session_id, perm_id, choice)` — session-verified; this
  is the production reply path (`src/core/daemon_turns.rs`,
  `src/server/routes/permission.rs`, `src/server/ws.rs`)
- `unregister(perm_id)` / `unregister_scoped(session_id, perm_id)`
- `is_registered(perm_id)` / `is_registered_scoped(session_id, perm_id)`
- `pending_permission_ids()` — all pending across sessions
- `get_pending_for_session(session_id)` — session-scoped with metadata
- `resolve_pending_owner(perm_id)` — recovers `(session_id, turn_id)` for a
  **simple** (unprefixed) id; returns `None` when the id is unknown *or*
  when more than one session owns it, so an ambiguous reply fails closed
- `set_pending_ttl(Option<Duration>)` / `pending_ttl()` — the orphan
  guard's age bound

`QuestionRegistry` sits next to it in the same module with the same
shape: `register`/`register_with_session`/`unregister`/
`unregister_scoped`/`is_registered`/`is_registered_scoped`/
`resolve_pending_owner`/`get_pending_for_session`, plus
`answer_question`/`answer_question_scoped` for the reply.

### Response Identifier Shapes

`CoreRequest::PermissionRespond` / `QuestionRespond` accept two id forms:

1. **Scoped (protocol form)** — `perm:<session_id>:<turn_id>:<perm_id>`. This
   is what `AppEvent::PermissionPending` → `CoreEvent` re-prefixing
   (`src/core/mod.rs`) produces, so remote/WS frontends echo it verbatim. The
   session and turn are taken from the id itself.
2. **Simple (bare)** — the unprefixed `{tool_call_id}-{tool}` id, which is what
   `AppEvent::PermissionPending` carries on the bus and therefore exactly what
   the **local TUI** receives and sends back. It carries no session, so
   ownership is recovered via `resolve_pending_owner`.

The controller-lease check runs against the *resolved* owner in both forms, so
an opaque bare id cannot bypass session-control authority. Anything that
resolves to no pending request is rejected with `invalid_permission_id` /
`invalid_question_id` — and because the human wait is unbounded by default, the
TUI surfaces that refusal as an error toast rather than letting the tool hang
with no feedback (`TuiCommand::PermissionResponseFinished`).

### Approval Dialog Keys

The dialog accepts direct codex-style keys in addition to `↑↓`/`j`/`k` +
`Enter`:

| Key | Effect |
|---|---|
| `y` | Allow once |
| `p` | Always allow — arms the `⚠ Confirm & Persist` interstitial |
| `n` | Deny once (cancels out of the interstitial when it is open) |
| `1`–`4` | Positional equivalents of the rows above; answer in one keypress |
| `↑↓`/`j`/`k`, `Enter` | Navigate / commit the highlighted row |
| `Esc` | Deny once — the same decision `n` commits; back out of the interstitial when it is open |

Every answering key commits directly — no second `Enter` is required. The
digits are guarded on no modifier, so `Ctrl`+digit stays available to the
host, and `0` is deliberately not a row so it can never underflow into
"Allow Once".

Persistent grants (the `Always Allow` / `Always Deny` rows and `p`) always
require the interstitial's second keypress, so no single keypress creates a
standing permission.

**Every dialog exit is an answer.** The human wait is unbounded unless
`permission.approval_timeout_secs` is configured, so a path that tears the
dialog down without answering strands the tool until the turn is cancelled.
`Esc` therefore commits `DenyOnce` rather than emitting a bare close. This also
covers indirect dismissal (parent dialog close, focus change), because the
dialog no longer has a close-only branch to fall through.

### Registration-Before-Publish Pattern

```rust
// CORRECT
let (tx, rx) = tokio::sync::oneshot::channel();
PermissionRegistry::register(perm_id.clone(), tx);
GlobalEventBus::publish(AppEvent::PermissionPending { ... });
let choice = match tokio::time::timeout(Duration::from_secs(300), rx).await {
    Ok(Ok(choice)) => choice,
    _ => PermissionDecision::DenyOnce,
};
PermissionRegistry::unregister(&perm_id);
```

## Configuration Surface

```toml
[permission]
default = "ask"
read = "allow"
edit = "ask"
glob = "allow"
grep = "allow"
list = "allow"
bash = "ask"
task = "ask"
lsp = "ask"
skill = "allow"
todowrite = "ask"
question = "ask"
webfetch = "ask"
websearch = "ask"
codesearch = "ask"
doom_loop = "ask"

# Custom tool rules
[permission.tools]
"custom_tool" = "deny"

# Path-based rules
[permission.paths]
"/home/user/project/**" = "ask"

# DoomLoop threshold
[permission.doomloop_threshold]
5

# Bash-specific patterns
bash_allow_patterns = ["cargo *"]
bash_deny_patterns = ["rm -rf *"]
# Extra shell shapes that escalate to an approval prompt (additive to the
# built-in risk signals; a review signal, not an execution boundary).
bash_ask_patterns = ["curl * | sh"]
allow_all_bash = false

# Bound the human wait. Omit (the default) to wait indefinitely.
approval_timeout_secs = 300
```

The per-tool keys accept either a plain action string or an object form
(`PermissionRule` in `crates/codegg-config/src/schema.rs` is an untagged
`Action(String) | Object(HashMap<String, String>)`); a wrong shape
silently discards the whole file, so validate with
`codegg validate --config <file>`.

`permission.sandbox_mode` is also part of this block and is read by the
sandbox-profile resolution, not by the permission checker.

### PERMISSION_TYPES Constant (`src/permission/mod.rs`)

27 recognized tool permission names: `read`, `edit`, `glob`, `grep`,
`list`, `bash`, `git`, `task`, `todowrite`, `todoread`, `question`,
`webfetch`, `websearch`, `codesearch`, `repo_search`, `repo_fetch`,
`repo_map`, `research`, `research_search`, `batch_fetch`,
`security_search`, `evidence_bundle`, `lsp`, `doom_loop`, `skill`,
`plan_enter`, `plan_exit`.

`PERMISSION_TYPES` is the *config* vocabulary — it is not the same list
as the permission-free categories in `tool_category_for_name()`, which
also names `diff`, `security`, `tool_search`, `context_read`,
`mcp_resource_search`, `mcp_resource_read`, `memory_search`,
`memory_get`, `extension_search`, `extension_install_request`, and
`invalid`.

## Invariants & Gotchas

0. **A delegated run's continuation cannot widen the ceiling.** A continued
   attempt runs under the *attempt's own* payload: the same `denied_tools`,
   `allowed_paths`, model, sandbox profile and worktree ceiling the first
   attempt ran under. The `task action=continue` request may narrow the
   recorded path set; anything wider is refused with the typed reason
   `ceiling_widened`. The delegated job's own admission is unchanged too: a
   continuation is an explicitly authorized new attempt, not a scheduler
   retry, so it keeps `RetryPolicy::no_retry()` and
   `IdempotencyClass::NonIdempotent`. See `architecture/agent.md` §"Continuing
   a delegated run".
1. **PermissionRegistry is synchronous.** `register()`, `respond()`,
   `answer_question()` are `fn`, not `async fn`. Do NOT `.await` them.
2. **Registration-before-publish.** Always register the oneshot channel
   BEFORE publishing the `PermissionPending` event. Only
   `ApprovalRouter::request_human_approval()` performs this in production
   (`scripts/check_approval_router.py` enforces the single owner).
3. **Agent loop does not treat external origin as approval.** Unknown
   raw MCP tools follow the normal mutating default and remain `Ask`
   until explicit policy or user decision allows them.
4. **Discovery never widens authority (M002).** `tool_search` may reveal
   only tools the current agent/session policy already allows; a denied,
   plan-excluded, model-disabled, backend-unavailable, or
   parent-ceiling-exceeded tool stays undiscoverable and non-callable.
   Deferred/profile-specific disclosure changes advertisement, never the
   broker/permission/contract boundary. Hidden tools (`invalid`,
   `DisabledTool` stubs) are never discoverable.
5. **Path canonicalization TTL is 1 second** (`PATH_CANONICALIZE_CACHE_TTL_SECS`).
   Not-found entries also cache for 1s.
6. **Session isolation.** Session-specific decisions are checked before
   global decisions. A Deny at any layer overrides allows at lower
   layers.
7. **Exec mode** (`with_exec_mode()`) sets `default = Allow` and allows
   bash, edit, task, todowrite — for CI/CD where no TUI is available.
   It is a headless compatibility adapter, distinct from user `Yolo`
   (M003 preserves `with_exec_mode()` and documents the separation).
8. **M003 approval routing (ADR-0004).** Deterministic
   `PermissionChecker`/`SecurityService` evaluation normalizes to
   `Allow | Deny | Escalate(ApprovalRequest)` before routing. `Allow`
   returns immediately, `Deny` never becomes `Escalate`, and only
   `Escalate` reaches the router. `Interactive` performs the single
   human wait, `Yolo` auto-allows only `Escalate` within the resolved
   ceiling, and `Automatic` safely defers to the human until M006 (never
   fail-open, even with rollout enabled). The turn/batch captures an
   immutable `ExecutionPolicySnapshot` (mode + sandbox + policy revision);
   concurrent mode changes apply on the next boundary. Child loops narrow
   via `narrow_for_child()` and can never broaden the parent. Approval
   mode and sandbox profile are separate fields. Production `Always`
   decisions use `canonical_permission_store_path()` (env
   `CODEGG_PERMISSIONS_PATH` override or `~/.config/codegg/permissions.json`);
   subagents use an ephemeral store. Write failure applies in-memory but
   reports `user_choice_unpersisted`. Frontend state is a projection, not
   authority; durable preference lives in daemon-owned
   `runtime_preferences` (revision/CAS, secret-free, additive v58).
9. **M005 sandbox wiring (ADR-0004).** The M003 snapshot profile becomes
   a production execution property: `ToolRegistryOptions.sandbox_profile`
   (default `WorkspaceWrite`) configures `BashTool` via
   `sandbox_config_for_profile()` over the authoritative workspace root;
   `FullHost` carries no containment and is explicit/auditable.
   Enforcement is reported separately (`SandboxEnforcement`:
   filesystem `Enforced/Unavailable/FullHost` + network
   `Unrestricted` for shell). `ApprovalMode` cannot mutate the profile.
   Child profiles narrow via `resolve_child_sandbox()`; a worktree
   child's writable root is its leased worktree. Outside-path needs
   return a bounded `SandboxEscalationRequest`, never a silent turn-wide
   `FullHost` switch. Protocol exposes `SandboxEnforcementDto` on the
   snapshot (additive `sandbox_enforcement`).
**M002 accepted execution context.** An accepted agent tool call carries
`ExecutionAuthorizationContext`: the immutable batch snapshot, permission
receipt, and bounded command-preflight advisory. The value travels through
`ToolExecutionContext` and `BrokerInvocationContext`; broker retries reuse
it unchanged. Bash consumes the preflight result already evaluated before
approval and does not run a second scan afterward. It refuses production
calls when the registered sandbox profile differs from the accepted
snapshot or required filesystem enforcement is unavailable. Raw Bash passes
the context to `ManagedProcessRequest`, which refuses a disabled sandbox for
a constrained snapshot. Typed Git mutation executors also retain the accepted
context while their own Git policy remains the capability authority. The
context records authorization inputs; it does not claim every downstream
effect was observed or contained.

For brokered calls, the immutable workspace path policy is fingerprinted from
its trusted canonical, read, and write roots and carried with the accepted
permission decision through each broker attempt. The broker rejects a
changed policy identity or revision. Missing policy, workspace, decision, or
permission revision data fails closed; identity strings and audit correlation
IDs are not used as substitute authority.

The command M004 route audit at
`research/command-authorization-m004-route-coverage.md` records the current
specialized-tool gap. The production typed Test and Python entrypoints now
validate the accepted receipt and snapshot before scheduler submission. They
admit only explicit `FullHost`; a missing/invalid receipt is denied and a
constrained snapshot returns `ToolError::BackendUnavailable` before a job is
created because the downstream executors do not consume that snapshot. Active
command-intent routes use the same admission predicate and remain
FullHost-only. Use constrained Bash for equivalent test commands; there is no
automatic reroute. Git filter/hook and remote authority remain gated by Linux
C002/C003.
**M003 command outcome observability.** The command execution event correlates
the permission receipt and decision source with the requested profile, safe
route label, advisory risk level, and actual obtained filesystem/network
outcome. Raw shell reports the completed managed-process enforcement result;
timeouts are explicitly unavailable, and Landlock-only networking remains
unrestricted. Completed raw-shell events also carry exit code,
output-truncation status, cleanup status, and helper identity. The event stores
structural digests and bounded labels, never command text, arguments, output,
or generated script content. This is launch and route provenance, not a trace
of every nested syscall; see
`architecture/audit.md` and the M003 validation record.
10. **M006 automatic reviewer (ADR-0004).** `Automatic` escalations resolve
   through the dedicated bounded reviewer (`src/permission/reviewer.rs`,
   see `approval_reviewer.md`) when a reviewer model is configured and
   registry-validated; otherwise Automatic keeps the M003 defer behavior.
   The reviewer sees only `Escalate` (never deterministic `Allow`/`Deny`),
   investigates only via `read/glob/grep/list/diff/git_read`, returns
   strict Allow/Deny/DeferUser JSON, and fails closed (Defer interactive,
   deny headless) on malformed/timeout/unavailable/stale/cancelled
   output. Valid Allow applies only to the unchanged original
   request/policy revision; Deny returns bounded feedback to the primary
   model; repeated equivalent denials backstop to defer/deny. Guard:
   `scripts/check_approval_reviewer.py`.
11. **M007 user surfaces (ADR-0004).** Mode/profile selection is
    frontend-neutral: the `CoreRequest::RuntimePolicySet` request
    (`crates/codegg-protocol/src/core.rs`) carries
    `{ approval_mode?, sandbox_profile?, expected_revision? }`
    (single-revision atomic write via `RuntimePreferenceStore::set_policy`,
    CAS `expected_revision`, fail-closed validation) plus the existing
    single-dimension sets; `ExecutionPolicyGet` additionally projects
    `reviewer_available`/`reviewer_detail` (daemon-resolved from
    `[approval_reviewer].model`) and the M004 last-model identity now
    reaches frontends through `RuntimePreferenceDto`. Presentation logic
    lives in `src/policy_surface.rs` (warning/confirmation matrix,
    effective-state rendering, restore summaries, CLI resolution; no
    daemon/TUI/server/authority imports — guard:
    `scripts/check_policy_surface.py`). The TUI exposes `/approval`,
    `/sandbox`, `/policy` (`src/tui/commands/policy.rs`, async
    start/apply pattern, stale completions dropped), renders the cached
    daemon-resolved line in the status bar and `/status`, announces the
    restored preference once at startup, and chains one confirmation for
    `Yolo`+contained/`FullHost` and two for `Yolo`+`FullHost`; cancelling
    changes nothing and concurrent changes fail CAS for reload. Headless
    flags (`--approval-mode`, `--sandbox`, `--yolo` alias; `codegg exec`
    equivalents) resolve through the same contract; bare `exec` keeps the
    legacy permissive behavior as a documented compatibility alias.
    Remembered approvals are capability-scoped where deterministic
    command data exists (`PersistentDecision.scope`: `cmd:<argv0>` for simple
    shell commands, `cmd:exact:<sha256>` for complex shell text, and
    `git:<subcommand>` for git; exact-scope first, legacy broad rows still
    match; scope is HMAC-signed, legacy signatures still verify for unscoped
    rows). A current effective Deny is evaluated before the cache so a
    legacy broad Allow cannot mask it. The TUI manifest model hint stays
    display-only (`reconcile_tab_model_with_daemon`).
12. **M008 qualification (invariant/closure).** `tests/agent_loop/reliability_qualification_m008.inc`
    proves deterministic Deny across Interactive/Automatic/Yolo, reviewer
    Allow/Deny/Defer plus malformed/unavailable/injection never-Allow,
    reviewer-cannot-mutate-sandbox, captured-snapshot wins on mode races,
    preference/permission restart durability, and child ceilings. No
    production change.

## Testing

```bash
cargo test -p codegg --lib permission    # unit tests for permission module
cargo test -p codegg --lib permission::tests  # specific test module
```

Key test patterns:
- `read_only_tools_short_circuit_to_allow` — 15 read-only tools
- `safe_mutating_tools_short_circuit_to_allow` — todowrite, todoread,
  question, invalid
- `destructive_bash_prompts` — rm -rf /, mkfs, fork bomb, shutdown
- `non_destructive_bash_auto_allows` — ls, cat, cargo test, git status
- `builtin_mode_review_blocks_mutation` — review mode Deny rules
- `builtin_modes_all_allow_todo_tools` — all modes allow todos
- `trusted_root_write_relaxes_only_the_default_ask` — the scratch-root
  relaxation never overrides an explicit decision
- `temp_root_redirect_offer_*` — the redirect offer is scoped to the
  implicit default, to unauthorized temp roots, and to one offer per
  (session, tool, target)

## Related Docs

- [tool.md](tool.md) — Tools that use PermissionChecker
- [bus.md](bus.md) — PermissionRegistry pattern
- [security.md](security.md) — Additional security measures
- [approval_reviewer.md](approval_reviewer.md) — The bounded Automatic reviewer
- [authorization.md](authorization.md) — Operation descriptors and scopes
- [audit.md](audit.md) — The durable audit trail

## Source Verification

Verified 2026-10-23 against `src/permission/mod.rs`, `src/permission/modes.rs`,
`src/permission/approval.rs`, `src/permission/reviewer.rs`,
`src/agent/tool_batch.rs`, `crates/codegg-core/src/bus/mod.rs`,
`crates/codegg-core/src/approval.rs`, `crates/codegg-config/src/schema.rs`,
and `crates/codegg-protocol/src/core.rs`.

- **Replaced drifting `file.rs:line` references with symbol names.** The
  module grew again since the 2026-10-06 pass; every remaining line
  anchor was off by 3–700 lines. Symbols now carry the claim, so the
  text does not rot on the next edit.
- **Corrected the permission-free tool list.** `tool_category_for_name()`
  names 28 `ReadOnly` and 5 `SafeMutating` tools; the previous list
  omitted `mcp_resource_search`, `mcp_resource_read`, `memory_search`,
  `memory_get`, `extension_search`, `extension_install_request`, and
  `invalid`.
- **Corrected the `PermissionRegistry` TTL claim.** The registry no
  longer expires entries on a fixed 310s age: `pending_ttl` defaults to
  `PENDING_TTL_UNBOUNDED` and is armed by the caller. The 310s constant
  survives only in the core-bus unit tests.
- **Added the policy-denial step to the check flow.** `check()` and
  `check_with_args()` both consult `effective_policy_denies()` *before*
  the cached store, so a policy/sandbox denial is never masked by a
  remembered Allow.
- **Added the M007 `scope` field** to `PersistentDecision` and
  `DECISION_SCOPE_MAX_LEN` to the store description.
- **Added `canonicalized_explicit_config_tool_rules`, `legacy_exec_mode`,
  and `temp_root_offers`** to the `PermissionChecker` field list.
- **Corrected the read-only test count** from 14 to 15
  (`read_only_tools_short_circuit_to_allow`).
- **Corrected `RuntimePolicySet`** — it is a `CoreRequest` variant, not a
  struct, and it carries `expected_revision`.
- **Added missing config keys** `bash_ask_patterns`,
  `approval_timeout_secs`, and `sandbox_mode`, and the untagged
  `PermissionRule` shape warning.
- **Renamed the dialog owner** from `AgentLoop::check_tool_permission()`
  to `ToolBatch::check_tool_permission()` (`src/agent/tool_batch.rs`);
  the agent loop delegates to the tool batch.
- **Confirmed correct as written:** the `PERMISSION_TYPES` list (all 27
  names, in order), the three built-in modes and their restricted-tool
  sets, the `DoomLoopDetector` bounds (window ≤ 1000, threshold 1–100),
  `decision_scopes_for_shell_command` intersection semantics, the
  approval-dialog key map (`y`/`p`/`n`/`1`–`4`/`Esc`), the orphan-guard
  TTL arithmetic (timeout + 60s grace, else 24h), the
  `runtime_preferences` v58 table, and
  `canonical_permission_store_path()`'s `CODEGG_PERMISSIONS_PATH`
  override with `~/.config/codegg/permissions.json` as the default.
