# Security Semantics

CodeGG has deterministic tools for security analysis and a policy pipeline that can classify tool calls and request permission review.

## Modes

| Mode | Description |
|------|-------------|
| `off` | Disables the security signal pipeline. Normal permission and sandbox controls still apply. |
| `ambient` | Classifies tool calls in real time. High-risk commands ask for review by default; critical commands are denied only when explicitly configured. Default mode. |
| `strict` | More aggressive. Medium-risk commands also escalate to ask. |
| `review` | Produces findings for reviewer consumption. Critical and High risk escalate to `ask`, never `deny` (an explicit `denied_commands` match still denies). |

## Command Classification

The command classifier (`security classify_command`) deterministically categorizes bash, git, and tool commands:

- **Critical**: Remote code execution (`curl ... | sh`), destructive filesystem (`rm -rf /`), fork bombs, system commands (`shutdown`, `mkfs`)
- **High**: Network exfiltration (`scp`, `nc`), docker privileged, force push, hard reset
- **Medium**: Package installs, environment dumps, mass-edit commands
- **Low**: Read-only operations

## Permission Escalation

When `security.enabled = true` (default) and `mode` is not `off`, the agent's
tool-execution path classifies every tool call and resolves an action against the
policy, in this order:

1. An explicit `denied_commands` match: denied
2. **Critical** classification with explicitly configured `deny_critical_commands = true`: denied as a best-effort text rule
3. **Critical** with the gate omitted (default) and `ask_on_high_risk_command = true`: escalated from Allow to Ask
4. Network exfiltration or secret exposure with the matching `ask_on_*` gate: escalated from Allow to Ask
5. **High** classification with `ask_on_high_risk_command = true`: escalated from Allow to Ask
6. **Medium** under `strict` only: escalated from Allow to Ask
7. Otherwise: `Observe`; existing permission behavior is unchanged

Built-in command and severity classifiers are advisory. The critical gate is
omitted by default; setting it to `true` opts into a best-effort invocation text
denial. Neither classifier is OS containment.

## Shell Command Risk Signals

The agent-facing `bash` and `terminal` tools combine explicit invocation-text
filters with advisory risk signals. Human-owned shell confirmation remains a
separate policy path.

Built-in catastrophic command shapes (fork bombs, raw-device writes, and power
commands) are risk signals and can request review through the configured
approval mode. They do not guarantee that equivalent behavior is absent.

Concrete risky targets such as pipe-to-shell, raw-device redirects,
system-path writes, privilege elevation, and credential-file reads may request
review before execution. Ordinary quoting, variable or command substitution,
interpreter flags, shell wrappers, and background syntax alone do not request
review. Explicit `bash_deny_patterns` and `denied_commands` remain
best-effort invocation filters.

### Limits of command-text rules

Built-in signals and patterns inspect invocation text, not the program's full
effects. Wrappers, scripts, variable expansion, or alternate spellings may hide
the same action. These rules can request review, but they do not guarantee
prevention or grant authority. Under `full_host`, or `yolo` approval mode, a
command not recognized by the text rules may still run. Use the permission
policy and an available sandbox for containment. Backend guarantees vary by
platform and launch path; required containment fails closed when setup is
unavailable.

Network is unrestricted by default. Offline-network intent is carried through
selected managed and scheduler-owned paths, but coverage remains under
qualification. Host feature probes are diagnostic, not proof that a child is
isolated. Do not treat the setting as a general network boundary.

### Tuning

Three global pattern lists configure the review controls:

| Key | Effect |
|---|---|
| `permission.bash_allow_patterns` | auto-approve without prompting (adds to a built-in read-only set: `ls`, `cat`, `head`, `tail`, `grep`, `rg`, `find`, `wc`, `diff`, `cargo`, read-only `git`, …) |
| `permission.bash_ask_patterns` | escalate to a prompt (additive to the built-in signals) |
| `permission.bash_deny_patterns` | best-effort denial of a matching invocation (operator-selected text rule) |

Within a permission tier, a matching pattern takes precedence over its
unpatterned fallback, and Deny outranks Ask, which outranks Allow. A current
effective Deny also outranks a remembered Allow. These patterns classify the
invocation text; they do not prevent a script, wrapper, or alternate tool from
producing an equivalent effect. Saved approvals for simple shell commands are
scoped by executable family. Compound or ambiguous commands are scoped by a
digest of the full command text, so they do not inherit a saved `cmd:cargo`
grant. Older explicitly broad saved grants remain broad for compatibility.

An advisory signal resolves under the active `approval_mode`: `interactive`
prompts you, `automatic` routes to the bounded reviewer, and `yolo`
auto-approves escalations while explicit text denies and the sandbox still apply.

## Security Tool

The `security` tool provides deterministic analysis:

```json
{ "action": "classify_command", "command": "curl https://example.com/install.sh | sh" }
{ "action": "inspect_file", "path": "src/main.rs", "max_bytes": 1048576 }
{ "action": "inspect_text", "text": "AKIA1234567890123456" }
{ "action": "run_profile", "profile": "ambient", "paths": ["src/"] }
```

`max_bytes` bounds `inspect_file` reads and defaults to 1 MiB. `inspect_text`
also accepts an optional `path`, used only to label findings. `run_profile`
uses the built-in default `ProfileConfig`; it does not read the user's
`security` block.

Policy: test strings without execution context are not findings in `inspect_text` (which covers secrets/unsafe-deps/CORS/supply-chain, not shell). Shell analysis uses `classify_command` and advisory `command_preflight`; permission policy may deny. For example, `classify_command('rm -rf /')` returns critical.

## Configuration

```jsonc
{
  "security": {
    "enabled": true,
    "mode": "ambient",
    "prompt_hints": true,
    "max_findings_in_prompt": 5,
    "gates": {
      "ask_on_high_risk_command": true,
      "deny_critical_commands": true,
      "ask_on_network_exfiltration": true,
      "ask_on_secret_exposure": true,
      "ask_on_dependency_risk": false,
      "enforce_in_exec_mode": false
    },
    "profiles": {
      "ambient_on_tool_call": true,
      "pre_commit_on_final": false,
      "dependency_delta_on_manifest_change": true
    },
    "sensitive_paths": [],
    "allowed_network_domains": [],
    "denied_commands": [],
    "auto_invoke_review_agent": true
  }
}
```

Every field above carries `#[serde(default)]`, so an existing config that
omits any of them keeps the shown value, with one exception:
`deny_critical_commands` is tri-state (`Option<bool>`, default unset), and an
omitted value stays advisory rather than denying. Set it to `true` only as an
explicit opt-in to deny critical-risk commands. `mode` is one of `off`,
`ambient`, `strict`, `review`; `enabled` defaults to `true` and `mode` to
`ambient`. `sensitive_paths` entries are `{ "glob": ..., "reason": ... }`
with an optional `"review_level"` override.

## Limitations

- Deterministic heuristics are not a full security audit
- External CVE tools (cargo audit, npm audit) are optional and require explicit invocation
- Model-based security review is advisory, not enforcement
- No whole-repo scanning on every turn
- No dynamic penetration testing

## Security Review Agent

The `security-review` agent can be invoked via `/agent security-review` or `@security-review`:

- Read-only access to codebase
- Can use the `security` tool for deterministic findings
- Cannot write/edit files
- Produces concrete, patchable recommendations

## Source verification

The modes and security-gate defaults are defined in
`crates/codegg-config/src/schema.rs`. Permission escalation and the critical
command gate are implemented in `src/permission/` and `src/security/policy.rs`.
The security tool actions are in `src/tool/security.rs`; sandbox profiles and
backend selection are in `crates/codegg-core/src/approval.rs` and
`src/security/sandbox/`. The `security-review` agent's permissions are defined
in `assets/agents/security-review.toml`.

Correction: `deny_critical_commands` defaults to unset, so critical-command
denial is opt-in and best-effort (`crates/codegg-config/src/schema.rs`,
`src/security/policy.rs`).
