# Eggsentry

Deterministic security scanning — secrets, commands, dependencies, unsafe code.

## Purpose

`eggsentry` (`crates/eggsentry/`) is a self-contained security scanning crate. It classifies shell commands, scans text/files for secrets and unsafe patterns, inspects dependency files, and produces structured findings. It is consumed by Codegg's `security` tool and gate policy.

## Module Structure

```
crates/eggsentry/src/
├── lib.rs          # Crate root, re-exports
├── command.rs      # Shell command classification
├── dependency.rs   # Dependency file detection and audit
├── finding.rs      # Finding data model (severity, confidence, categories)
├── profile.rs      # Security profiles for scan configuration
└── scanner.rs      # Regex-based text/file scanner
```

## Key Types

### SecurityFinding

| Field | Type | Description |
|-------|------|-------------|
| `id` | `String` | Deterministic SHA-256-based ID |
| `severity` | `Severity` | Info / Low / Medium / High / Critical |
| `confidence` | `Confidence` | Low / Medium / High |
| `category` | `SecurityCategory` | 16 categories (see below) |
| `source` | `FindingSource` | How the finding was discovered |
| `mode` | `FindingMode` | Deterministic or Agentic |
| `file` | `Option<PathBuf>` | Source file |
| `line_range` | `Option<(usize, usize)>` | Line range |
| `evidence` | `String` | Supporting evidence |
| `recommendation` | `String` | Remediation advice |

### SecurityCategory (16 variants)

| Category | Description |
|----------|-------------|
| `SecretExposure` | API keys, tokens, passwords |
| `DangerousCommand` | Shell injection, destructive commands |
| `DestructiveFilesystem` | Destructive file operations |
| `NetworkExfiltration` | Data exfiltration risk |
| `RemoteCodeExecution` | RCE vectors |
| `DependencyVulnerability` | Known vulnerable deps |
| `DependencyRisk` | Risky dependency patterns |
| `UnsafeCode` | `unsafe` blocks/fns |
| `PathTraversal` | Directory traversal |
| `InsecureTls` | TLS misconfigurations |
| `SsrfRisk` | Server-side request forgery |
| `AuthzRisk` | Authorization issues |
| `SandboxEscapeRisk` | Sandbox bypass |
| `SupplyChainRisk` | Supply chain attack vectors |
| `ConfigRisk` | Configuration security |
| `Unknown` | Unclassified |

### Severity (ordered)

`Info` < `Low` < `Medium` < `High` < `Critical`

### Command Classification

| Function | Purpose |
|----------|---------|
| `classify_bash_command(cmd)` | Classify a bash command string |
| `classify_git_subcommand(argv)` | Classify a git subcommand |
| `classify_tool_call(name, args)` | Classify a tool invocation |

Returns `CommandClassification` with risk level and rationale.

## Scanner

Regex-based scanner with compiled `LazyLock<Regex>` patterns:

**Secret patterns**: AWS keys, GitHub tokens, OpenAI keys, private keys, passwords, API keys, generic secrets.

**Unsafe patterns**: `unsafe` blocks, `unsafe fn`, `danger_accept`, `Command::new("sh")`, CORS wildcards, `bind_all`.

**Public functions**:
- `inspect_text(path, text) -> Vec<SecurityFinding>` — scan raw text
- `inspect_file(path) -> Vec<SecurityFinding>` — scan a file

## SecurityReport

Aggregated scan result:

| Field | Description |
|-------|-------------|
| `profile` | Applied security profile name |
| `findings` | All discovered findings |
| `summary` | Count breakdown by severity |

Method: `summarize()` — populates summary with severity counts.

## Finding Quality

`is_high_signal()` returns true when:
- severity ≥ High AND confidence ≥ Medium, OR
- severity = Medium AND confidence = High

## See Also

- [Security](security.md) — Codegg-side security integration
- [Permission](permission.md) — Access control that gates security operations
- [Tool](tool.md) — Security tool wrapper
