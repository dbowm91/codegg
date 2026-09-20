# Playwright MCP companion

This package is intentionally opt-in. Installing it does not connect or
download anything. Enable the package only after the user has installed a
chosen `@playwright/mcp` version in the project/user environment and reviewed
the network/profile policy. The imported `--no-install` command makes a
missing local package fail closed instead of downloading from npm.

Playwright MCP is for persistent state and rich page introspection. For most
coding tasks, use the sibling `playwright-browser-testing` CLI skill because it
loads less schema/context.
