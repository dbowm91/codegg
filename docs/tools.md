# Tools

Tools are the operations available to the agent. Native tools, configured MCP servers, and installed plugins use CodeGG's permission and safety controls. See [docs/security-semantics.md](security-semantics.md) for command classification and escalation behavior.

## Permissions and workspace scope

The `tools` config map enables or disables individual tools. The `permission` map sets a per-tool action, such as `allow`, `ask`, or `deny`. Agent permissions can narrow the tools available to that agent. A hard deny cannot be overridden by an approval prompt. Omitted settings use the active configuration and agent policy.

A skill's `allowed-tools` field is descriptive metadata. It does not grant tool permission.

File tools resolve relative paths from the session workspace. The `session_working_directory` tool can set a default subdirectory for later work, but it does not change the workspace security boundary. If CodeGG has no workspace root, workspace tools refuse instead of using the process's current directory.

## Tool families

| Family | Tools | Purpose |
|---|---|---|
| Filesystem | `read`, `write`, `edit`, `apply_patch`, `replace`, `diff`, `glob`, `grep`, `list` | Inspecting and changing workspace files |
| Shell and execution | `bash`, `terminal`, `test`, `batch`, `python_script`, `verify` | Shell commands, tests, scripts, and bounded project checks. Interactive PTYs are a TUI surface. |
| Programs and discovery | `tool_program`, `tool_search`, `question`, `skill`, `skill_proposal`, `image`, `context_read`, `invalid` | Reusable read-only tool sequences, discovery, questions, skills, image and context reads |
| Source intelligence | `lsp`, `lsp_read`, `lsp_preview_apply`, `codesearch`, `repo_map`, `ci_watch` | Code navigation, diagnostics, previews, and repository inspection |
| Version control | `git`, `git_query`, `git_read`, `commit` | Repository facts, changes, and commits |
| Research and evidence | `research`, `research_search`, `repo_search`, `repo_fetch`, `security_search`, `batch_fetch`, `evidence_bundle`, `websearch`, `webfetch` | Local and external evidence lookup |
| Planning and tasks | `task`, `job`, `todoread`, `todowrite`, `plan_enter`, `plan_exit`, `goal_get`, `goal_request_completion`, `goal_update_progress`, `work_order`, `work_plan_get`, `work_plan_update_item`, `session_working_directory` | Delegation, plans, goals, and progress |
| Review and validation | `review`, `security`, `text_equal`, `text_diff_explain`, `text_replace_check`, `text_inspect`, `text_fingerprint`, `text_security_inspect`, `validate_json`, `validate_toml`, `path_normalize`, `identifier_inspect`, `structured_data_compare`, `command_preflight`, `config_preflight`, `math_eval`, `unit_convert` | Reviews and deterministic checks |
| Extensions and resources | `mcp_resource_read`, `mcp_resource_search`, `extension_search`, `extension_install_request`, `memory_get`, `memory_search` | MCP resources, extensions, and memory |

The list can vary with enabled tools, provider and server configuration, and the active session. Use `tool_search` to discover available tools, or run `codegg doctor` for subsystem status.

## Choosing a tool

- Use structured file tools for workspace paths. Use `bash` when you need shell syntax.
- Use `git_query` for bounded repository facts. Use `git` or `commit` for supported mutations.
- Use `lsp` and `lsp_read` for language-server information. LSP exposure is controlled by `experimental.lsp_tool`; see [docs/LSP.md](LSP.md).
- Use `repo_search`, `repo_fetch`, and `repo_map` for repository search. CodeGG uses the Eggsearch sidecar for search; check `codegg doctor search` if it is unavailable.
- Use configured MCP servers and installed plugins for their contributed tools. See [docs/MCP.md](MCP.md) and [docs/PLUGINS.md](PLUGINS.md).

## Process tools

The `test` tool and other managed process work use the daemon's scheduler. Under `--standalone`, tools that require daemon scheduling refuse rather than starting unsupervised work. A required filesystem sandbox that cannot be enforced also refuses before the process starts.

## Source verification

Checked 2026-11-23 against `crates/codegg-core/src/command_catalog.rs`, the tool implementations under `src/tool/`, `crates/codegg-tui/src/command.rs`, `crates/codegg-config/src/schema.rs`, and the LSP gate in the config schema.
