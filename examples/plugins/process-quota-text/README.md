# process-quota-text — Zero-SDK Plain Text Plugin

Demonstrates the simplest possible codegg plugin: a script that prints
plain text to stdout. No SDK, no JSON protocol, no dependencies.

## What it shows

- A `command/quota.md` frontmatter file that registers a process command
- `stdout: text` mode — the script's stdout is rendered as an `EmitChat`
  effect with `ChatFormat::Markdown`
- Argument parsing via `sys.argv`
- SIGTERM handling for graceful shutdown

## Installation

Copy the two directories into your project root:

```
<project>/command/quota.md
<project>/scripts/quota.py
```

The frontmatter tells codegg to run `python3 scripts/quota.py` when the
user invokes `/quota`.

## Usage

```
/quota
/quota --provider openai
```

## Stdout mode behavior

With `stdout: text` in the frontmatter, the process runtime captures
stdout and emits it as an `EmitChat` effect. The TUI renders this in the
chat surface as a plain text block. If the output is short (≤3 lines) it
appears as a toast; otherwise it opens in the scrollable info dialog.

## Safety

- No network access
- No secrets or env passthrough
- No file I/O
- Reads only command-line arguments
