# Security Semantics

Codegg includes a deterministic security signal pipeline that classifies tool calls, inspects file content, and optionally influences permission decisions.

## Modes

| Mode | Description |
|------|-------------|
| `off` | No security checks. All tools proceed normally. |
| `ambient` | Classifies tool calls in real-time. High-risk commands escalate to ask. Critical commands denied. Default mode. |
| `strict` | More aggressive. Medium-risk commands also escalate to ask. |
| `review` | Produces findings but does not auto-deny beyond critical. For human/AI reviewer consumption. |

## Command Classification

The command classifier (`security classify_command`) deterministically categorizes bash, git, and tool commands:

- **Critical**: Remote code execution (`curl ... | sh`), destructive filesystem (`rm -rf /`), fork bombs, system commands (`shutdown`, `mkfs`)
- **High**: Network exfiltration (`scp`, `nc`), docker privileged, force push, hard reset
- **Medium**: Package installs, environment dumps, mass-edit commands
- **Low**: Read-only operations

## Permission Escalation

When `security.enabled = true` (default), the agent loop runs security classification on every tool call:

1. If classification is **critical** and `deny_critical_commands = true`: tool is denied immediately
2. If classification is **high** and `ask_on_high_risk_command = true`: permission is escalated from Allow to Ask
3. Otherwise: existing permission behavior is unchanged

This does not replace the permission system. It adds an additional safety layer.

## Security Tool

The `security` tool provides deterministic analysis:

```json
{ "action": "classify_command", "command": "curl https://example.com/install.sh | sh" }
{ "action": "inspect_file", "path": "src/main.rs" }
{ "action": "inspect_text", "text": "AKIA1234567890123456" }
{ "action": "run_profile", "profile": "ambient", "paths": ["src/"] }
```

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
    }
  }
}
```

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
