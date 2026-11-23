# CodeGG

CodeGG is a Rust-native coding agent for terminal workflows. It combines an interactive terminal UI, model-provider connections, project tools, and permission and sandbox controls.

## Quick start

1. Install from source. See the [installation guide](docs/install.md); no GitHub release or prebuilt bundle is currently available.

2. Set a credential for a provider, then check what CodeGG can reach.

   ```bash
   export ANTHROPIC_API_KEY='...'
   codegg providers
   codegg models -p anthropic
   ```

3. Start the TUI and choose a model from the provider's available list.

   ```bash
   codegg -m anthropic/<model-id>
   ```

   Use `/help` or press `Space` in the TUI for commands and keybindings. Press `Ctrl+C` twice to exit.

For one-shot use, run `codegg --run "Explain this project"`. Use `codegg doctor` to diagnose setup problems.

## Install options

The managed installer supports Linux and macOS, but currently has no release bundle to download. Source installation is available with Rust and Cargo. See [docs/install.md](docs/install.md) for supported hosts, installer settings, and upgrade behavior.

## Guides

| Guide | Covers |
|---|---|
| [Installation](docs/install.md) | Source and managed installation, upgrades, completions |
| [CLI](docs/cli.md) | Commands and flags |
| [Configuration](docs/configuration.md) | Config files, precedence, and key settings |
| [Providers](docs/providers.md) | Provider credentials, accounts, and model selection |
| [TUI](docs/tui.md) | Projects, sessions, commands, panels, and keybindings |
| [Tools](docs/tools.md) | Tool families and safety behavior |
| [Agents and skills](docs/agents-skills.md) | Custom agents and instruction packages |
| [Troubleshooting](docs/TROUBLESHOOTING.md) | Common startup, provider, permission, and integration issues |
| [Daemon](docs/daemon.md) | Local daemon and transports |
| [MCP](docs/MCP.md) | MCP server setup |
| [LSP](docs/LSP.md) | Language-server setup |
| [Plugins](docs/PLUGINS.md) | Plugin setup and capabilities |
| [Themes](docs/themes.md) | Theme selection and authoring |
| [Repository initialization](docs/repository-init.md) | Repository evidence and AGENTS.md proposals |
| [Security semantics](docs/security-semantics.md) | Permission and command-safety behavior |
| [Architecture](architecture/overview.md) | Module map and implementation contracts |

## Safety

CodeGG can modify files, run processes, operate Git, and contact configured services. Review the approval mode, sandbox profile, and tool permissions before granting access. See [docs/security-semantics.md](docs/security-semantics.md).

## Source verification

Checked 2026-11-23 against the CLI definitions in `crates/codegg-cli/src/main.rs`, installer behavior in `install.sh`, and the GitHub release listing. The release listing was empty when checked.
