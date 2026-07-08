# Eggsearch/Eggsact Crates.io Dependency Switch Plan

## Purpose

Codegg currently pins `eggsact` by git revision and invokes `eggsearch` as an external MCP binary. Since `eggsact` is now published on crates.io at `1.1.3` and `eggsearch` is now published on crates.io at `0.3.4`, this pass should move Codegg to the release-ready dependency/install story and verify that neither upstream release introduced breaking API or CLI/MCP changes relative to Codegg's current integration.

This is a narrow release-readiness pass. Do not redesign the eggsearch/eggsact architecture.

## Current Codegg state

At the time this plan was written:

- Codegg uses `eggsact` as a direct Rust dependency from git:

```toml
eggsact = { git = "https://github.com/eggstack/eggsact", rev = "cef5d41cd647cd8bac35c97a404918f4854d215e" }
```

- Codegg does not link against `eggsearch` as a Rust library dependency.
- Codegg treats eggsearch as an external MCP server and defaults to invoking:

```text
eggsearch mcp stdio
```

- This out-of-process eggsearch design should remain intact.

## Target state

- Replace the git-pinned eggsact dependency with the crates.io release:

```toml
eggsact = "1.1.3"
```

- Do not add `eggsearch = "0.3.4"` as a Codegg library dependency unless a concrete in-process library integration is intentionally designed later.
- Update documentation to require installing eggsearch CLI/MCP from crates.io:

```bash
cargo install eggsearch --version 0.3.4
```

- Verify that eggsact `1.1.3` exposes the API Codegg uses.
- Verify that eggsearch `0.3.4` exposes the CLI and MCP tool surface Codegg expects.
- Run locked dependency and test validation after the switch.

## Task 1: Inspect eggsact 1.1.3 for API compatibility

### Required Codegg API surface

Codegg currently relies on these eggsact APIs and types:

- `eggsact::agent::ToolRegistry`
- `eggsact::agent::ToolRegistry::with_profile_and_audience(profile, audience)`
- `eggsact::agent::ToolRegistry::call_json(tool, args)`
- `eggsact::agent::ToolRegistry::has_tool(tool)`
- `eggsact::agent::Profile`
- `eggsact::agent::Profile::from_str_opt(&str)`
- `eggsact::agent::ToolAudience`
- `eggsact::mcp::response::ToolResponse`
- `ToolResponse` fields used by Codegg:
  - `ok`
  - `tool`
  - `result`
  - `error_type`
  - `error`
  - `hints`
  - `warnings`
  - `limits_applied`
  - `findings`
  - `machine_code`
  - `recommended_next_tool`

Codegg also assumes these eggsact profile names exist:

- `codegg_core`
- `codegg_core_min`
- `default`
- `full`

Codegg also assumes the default model/harness deterministic palette can resolve these underlying eggsact tools, depending on configured profile/audience:

- `text_equal`
- `text_diff_explain`
- `text_replace_check`
- `validate_json`
- `validate_toml`
- `command_preflight`
- `path_normalize`
- `text_security_inspect`
- deferred/contextual wrappers such as `text_inspect`, `config_preflight`, `identifier_inspect`, `structured_data_compare`, and `text_fingerprint` if registered by Codegg.

### Inspection steps

1. Inspect the published `eggsact` `1.1.3` crate API locally with:

```bash
cargo info eggsact@1.1.3
cargo tree -p codegg --edges normal | grep eggsact
```

2. After switching Cargo.toml, run:

```bash
cargo check -p codegg --locked
cargo test --test eggsact_adapter --locked
cargo test --test eggsact_deterministic_tools --locked
cargo test --test preflight_integration --locked
```

3. If compilation fails due to API drift, inspect the upstream eggsact repo/tag and adapt Codegg minimally:

```bash
git clone https://github.com/eggstack/eggsact /tmp/eggsact-api-check
cd /tmp/eggsact-api-check
git tag --list
git checkout <tag-or-rev-for-1.1.3>
rg "pub enum Profile|pub enum ToolAudience|struct ToolResponse|impl ToolRegistry|from_str_opt|with_profile_and_audience|call_json|has_tool"
```

4. Record any API difference in the commit message or a small note in this plan if the implementation updates the plan.

### Acceptance criteria

- Codegg compiles against `eggsact = "1.1.3"` without using git overrides.
- The adapter tests pass against crates.io eggsact.
- The deterministic/preflight tool tests pass against crates.io eggsact.
- No `[patch.crates-io]` override is required.

## Task 2: Switch eggsact dependency to crates.io

### Implementation

Update root `Cargo.toml`:

```toml
eggsact = "1.1.3"
```

Remove the git dependency and explicit `rev`.

Then update `Cargo.lock` with:

```bash
cargo update -p eggsact --precise 1.1.3
```

If that command fails because the current lock source is git, use:

```bash
cargo update
```

then inspect the lockfile to confirm `eggsact` comes from crates.io.

### Lockfile acceptance criteria

`Cargo.lock` should contain a crates.io source entry for eggsact, for example:

```toml
[[package]]
name = "eggsact"
version = "1.1.3"
source = "registry+https://github.com/rust-lang/crates.io-index"
```

It should not contain an eggsact git source.

## Task 3: Inspect eggsearch 0.3.4 CLI/MCP compatibility

### Required Codegg eggsearch contract

Codegg does not import eggsearch as a Rust library. It expects an external CLI/MCP server with:

- executable command: `eggsearch`
- default args: `mcp stdio`
- an MCP server name defaulting to `eggsearch`
- required tools:
  - `web_search`
  - `web_fetch`
- recommended tools:
  - `batch_fetch`
  - `repo_search`
  - `repo_fetch`
  - `repo_map`
  - `security_search`
  - `research_search`
  - `build_evidence_bundle`
- provider/status diagnostic support if available through Codegg's current bootstrap status path.

### Inspection steps

Install and inspect the published binary:

```bash
cargo install eggsearch --version 0.3.4 --locked
which eggsearch
eggsearch --help
eggsearch mcp --help
eggsearch mcp stdio --help || true
```

Then run Codegg's fake and adapter tests:

```bash
cargo test --test fake_eggsearch_mcp --locked
cargo test search_backend --locked
```

If Codegg has a doctor command available in local CLI form, run:

```bash
cargo run --locked -- doctor search
```

or the current equivalent command documented in Codegg.

### Optional live MCP smoke test

If safe and non-flaky locally, run a minimal MCP listing against installed eggsearch. Keep it outside default CI unless a deterministic harness exists.

The test should verify that eggsearch `0.3.4` advertises the required and recommended tool names that Codegg expects.

### Breaking-change checks

Inspect the eggsearch repo/tag if the binary behavior appears inconsistent:

```bash
git clone https://github.com/eggstack/eggsearch /tmp/eggsearch-api-check
cd /tmp/eggsearch-api-check
git tag --list
git checkout <tag-or-rev-for-0.3.4>
rg "web_search|web_fetch|batch_fetch|repo_search|repo_fetch|repo_map|security_search|research_search|build_evidence_bundle|provider_status|mcp stdio|stdio"
```

Check for:

- renamed MCP tool names.
- changed argument schema for Codegg wrapper calls.
- changed CLI subcommand shape.
- changed provider status behavior.
- MSRV incompatibility if Codegg docs recommend `cargo install eggsearch` from the same toolchain.

### Acceptance criteria

- `cargo install eggsearch --version 0.3.4 --locked` succeeds on the intended toolchain, or docs note the required newer toolchain if eggsearch's MSRV is higher than Codegg's.
- `eggsearch mcp stdio` remains the correct command shape.
- Codegg's required MCP tools are present.
- Missing recommended tools, if any, are handled as partial support and documented.
- No Codegg Rust dependency on eggsearch is added.

## Task 4: Update documentation

Update all docs that mention eggsact/eggsearch dependency state.

### README

Add or update release install guidance:

```bash
cargo install eggsearch --version 0.3.4
```

State explicitly:

- Codegg links to crates.io `eggsact = "1.1.3"` for local deterministic tools.
- Codegg invokes `eggsearch` out-of-process as an MCP server.
- Users need `eggsearch` installed on PATH when `[search].backend = "eggsearch"`.
- Users can set `[search].backend = "builtin"` or `"disabled"` if eggsearch is not installed.

### Architecture docs

Update:

- `architecture/deterministic_tools.md`
- `architecture/search_backend.md`
- `architecture/config.md`
- `architecture/preflight.md` if it mentions eggsact dependency source.
- `AGENTS.md` if it contains contributor dependency notes.

Remove stale references to git-pinned eggsact as the preferred release path.

### Acceptance criteria

- Docs do not claim eggsact is git-pinned unless documenting old history.
- Docs do not claim eggsearch is linked in-process.
- Docs mention the exact supported release versions: eggsact `1.1.3`, eggsearch `0.3.4`.

## Task 5: Validate clean dependency resolution

Run:

```bash
cargo metadata --locked
cargo tree -p codegg --locked | grep -E "eggsact|eggsearch"
cargo check --workspace --locked
```

Expected:

- `eggsact v1.1.3` appears from crates.io.
- No `eggsact` git source remains.
- `eggsearch` should not appear in `cargo tree` unless another crate legitimately depends on it as a library. If it appears, inspect why.

## Task 6: Run targeted integration validation

Run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features --locked
cargo test --test eggsact_adapter --locked
cargo test --test eggsact_deterministic_tools --locked
cargo test --test preflight_integration --locked
cargo test --test fake_eggsearch_mcp --locked
cargo test search_backend --locked
```

If the full workspace all-features gate is too broad, document the exact failure and narrow it only with a reason. Do not silently skip the full gate.

## Task 7: Failure handling

If eggsact `1.1.3` has breaking API changes:

1. Prefer a small compatibility shim in `src/eggsact/adapter.rs` rather than spreading changes through Codegg.
2. If the break is larger, create a follow-up plan and keep the git-pinned dependency until eggsact publishes a compatible version.
3. Do not partially switch to crates.io with failing tests.

If eggsearch `0.3.4` has breaking CLI/MCP changes:

1. Keep Codegg's out-of-process architecture.
2. Update command/args defaults only if the new CLI shape is stable and documented.
3. Update tool wrappers only if tool argument/response schemas changed.
4. Add compatibility handling for old/new tool names only if needed.

## Final acceptance criteria

This pass is complete when:

- `Cargo.toml` uses `eggsact = "1.1.3"`.
- `Cargo.lock` resolves eggsact from crates.io, not git.
- Codegg does not add `eggsearch` as a Rust library dependency.
- Docs instruct users to install `eggsearch 0.3.4` as an external CLI/MCP binary.
- Codegg's eggsact adapter and deterministic/preflight tests pass against crates.io eggsact.
- Codegg's eggsearch wrapper/fake MCP tests pass against the expected eggsearch MCP contract.
- Any upstream breaking changes discovered in eggsact or eggsearch are documented and handled before merging the dependency switch.
