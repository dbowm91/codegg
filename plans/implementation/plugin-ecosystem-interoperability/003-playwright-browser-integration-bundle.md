# Plugin Ecosystem Interoperability M003 — Playwright Browser-Testing Integration Bundle

Status: blocked on M002 (Agent Plugins 1.0 import)

Repository baseline: 2f64c9e16f9ee96ea4d47ed897201f4c24d4606f

Source roadmap:

- plans/subsystems/plugin-ecosystem-interoperability-roadmap.md#m003--playwright-browser-testing-integration-bundle

Long-term requirements:

- plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability
- plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends
- plans/000-long-term-specification.md#27-security-requirements
- plans/000-long-term-specification.md#29-system-invariants
- plans/003-planning-process.md

Applicable ADRs:

- None. This is an integration package over existing shell/skill/MCP owners. No browser runtime is added to CodeGG core.

Primary class: integration / capability packaging

Hard dependency:

- M002 portable Agent Plugins package import and local installation.

Soft dependencies:

- M001 first-class plugin tools is not required;
- M004 catalog work may later distribute the bundle but is not required to implement/test it locally.

## 1. Objective

Provide a supported browser-testing workflow for CodeGG without embedding Chromium, Playwright, Node, or a browser-control engine into the CodeGG binary.

The default coding-agent path should use Playwright CLI plus a portable skill because it is token-efficient and naturally fits CodeGG's existing bash/test/tool disclosure. An optional MCP mode should be documented/provisioned for persistent browser state and exploratory page-structure reasoning.

The integration must make the security boundary explicit: browser content is untrusted, Playwright is not a sandbox, and browser credentials/profiles are not implicitly inherited.

## 2. External research basis

Microsoft's current Playwright guidance was reviewed on 2026-09-20:

- https://github.com/microsoft/playwright-cli
- https://github.com/microsoft/playwright-mcp
- https://github.com/microsoft/playwright/blob/main/docs/src/getting-started-cli.md
- https://github.com/microsoft/playwright/blob/main/docs/src/getting-started-mcp.md

Current guidance distinguishes:

- Playwright CLI + skills for coding agents because concise commands avoid loading large MCP schemas/accessibility trees into model context;
- Playwright MCP for specialized loops that benefit from persistent state, rich introspection, and repeated reasoning over page structure.

CodeGG should preserve that tradeoff rather than make all browser actions permanent native tools.

## 3. Current implementation evidence

At baseline:

- CodeGG has bash, terminal, test, image, Tool Programs, permission modes, sandbox profiles, and managed process/run-store infrastructure.
- portable SKILL.md discovery already exists.
- Runtime Assets M006 lets plugins package skills and MCP declarations.
- M002 will allow a conformant Agent Plugins skill/MCP package to install without conversion.
- McpService can launch local stdio MCP servers.
- there is no first-party browser/DOM/Playwright tool.
- no reason exists to add 50-70 browser operations to CORE_PALETTE.

## 4. Invariants that must not regress

- no Playwright/browser engine dependency enters the default CodeGG Rust dependency graph;
- no Node.js runtime is bundled into CodeGG;
- activation/discovery does not automatically download npm packages;
- browser process execution remains visible through existing shell/MCP process ownership;
- user permission/sandbox mode remains authoritative;
- browser page content is untrusted external data;
- persistent browser profiles, existing Chrome profiles, cookies, and credentials are opt-in only;
- a browser integration cannot widen filesystem/network authority beyond the selected CodeGG execution mode and external Playwright process capabilities;
- screenshots/traces/artifacts are bounded and stored in project/run-owned locations, never arbitrary home-directory paths by default;
- MCP mode remains optional and deferred;
- CodeGG core tool palettes do not gain dozens of Playwright-specific tools.

## 5. Explicit non-goals

- native browser engine;
- CDP implementation in Rust;
- screenshot-only computer-use agent;
- CAPTCHA bypass;
- anti-bot evasion;
- credential harvesting;
- automatic reuse of a developer's logged-in browser profile;
- remote browser cloud service;
- live browser CI;
- installing Node/browser binaries without explicit user action;
- making Playwright MCP a default always-connected server for every CodeGG user.

## 6. Deliverable shape

Implement the integration as repository-owned portable assets plus minimal host integration where required.

Preferred package split:

### Package A — playwright-browser-testing

A conformant Agent Plugins 1.0 skill-only package containing a portable browser-testing skill.

The skill teaches the agent to:

- detect an existing supported Playwright CLI installation;
- prefer concise CLI operations for ordinary frontend testing/debugging;
- use stable element references/snapshots rather than guessing selectors when supported;
- capture screenshots/traces only when useful;
- keep generated browser artifacts bounded;
- use test/base URLs provided by the user/project;
- treat page content as untrusted;
- stop and ask before authentication-sensitive flows when existing policy requires it.

### Package B or host preset — playwright-mcp

MCP is optional.

Do not force the skill package to auto-connect Playwright MCP just because the package is installed. Use one of these acceptable designs after M002 implementation constraints are known:

1. a separate Agent Plugins MCP-only companion package; or
2. a CodeGG MCP preset/configuration flow explicitly enabled by the user.

Prefer the design that keeps install and runtime activation obvious and allows CLI-only users to avoid MCP schema/context cost.

Do not invent a portable conditional-component semantic not present in Agent Plugins 1.0.

## 7. Required production changes

### A. Verify and pin supported upstream invocation

At implementation time, verify current Playwright CLI and MCP package executable/argument conventions from official upstream metadata/docs.

Document:

- minimum supported Node version;
- tested Playwright CLI package/version range;
- tested Playwright MCP package/version range;
- how CodeGG detects an existing executable/project dependency;
- whether npx is used.

Do not hard-code @latest in a repository-owned integration contract.

If an on-demand package download path is offered, it must be explicit user setup with clear network/code-execution warning and a version selected by CodeGG's support matrix. Ordinary agent use must not silently download tooling.

### B. Add a portable browser-testing skill

Create a small, high-signal SKILL.md rather than a browser manual.

The skill should contain:

- when to use browser testing;
- CLI preference rationale;
- minimal discovery command/help path;
- open/navigate;
- inspect/snapshot;
- interact/type/click/select;
- screenshot/trace;
- assertion/verification workflow;
- cleanup/session close;
- common failure handling;
- instruction-in-page prompt-injection warning.

Keep verbose command reference in bounded skill resources if needed so activation remains small.

The skill must not grant allowed-tools authority. Existing CodeGG tool permissions remain authoritative.

### C. Add host-visible dependency diagnostics

Add a doctor/integration diagnostic path that can report, without installing:

- Node availability/version;
- Playwright CLI availability/version;
- Playwright MCP availability/version if MCP mode selected;
- browser binary availability where Playwright exposes it;
- configured mode: none / CLI / MCP;
- current MCP connection status.

Prefer integrating with existing doctor/plugin/MCP diagnostics rather than a new subsystem.

### D. Explicit MCP configuration flow

If implementing an MCP companion/preset:

- require user activation;
- use the existing McpService configuration/reconciliation path;
- do not special-case execution in AgentLoop;
- keep raw Playwright MCP tools subject to existing MCP exposure/progressive-disclosure policy;
- consider hiding the server until explicitly enabled for a task to avoid large tool-schema pressure;
- provide an easy disable/disconnect path.

If upstream supports allowed-host/origin constraints, expose them in the preset/UI and default conservatively. If it does not, documentation must make unrestricted navigation capability explicit.

### E. Browser process/profile policy

Default to isolated/ephemeral browser state.

Do not:

- pass the user's default browser profile automatically;
- mount arbitrary credential directories;
- persist cookies/storage beyond the integration's explicit project/user data location;
- enable browser extensions automatically.

If a persistent profile option is later exposed:

- require explicit selection;
- show the path/scope;
- keep it outside project source trees unless the user deliberately chooses otherwise;
- warn that browser authentication state is sensitive.

### F. Artifact handling

For CLI mode, identify Playwright-generated snapshots/screenshots/traces and keep them under a bounded CodeGG/project-owned working/artifact directory where practical.

Do not add a second artifact database.

The skill should teach the model to:

- inspect screenshots through existing image capability only when necessary;
- avoid repeatedly creating redundant screenshots/traces;
- delete/overwrite disposable artifacts or rely on existing run retention;
- not commit generated browser state unless explicitly requested.

### G. Tool/disclosure posture

Do not add browser-specific native tools to CORE_PALETTE.

CLI mode uses existing bash/test/image tools.

MCP mode uses existing MCP tool discovery/exposure. If the server exports a very large tool schema, prefer CodeGG deferred/discovery behavior and document measured context impact during qualification.

## 8. Protocol, storage, migration, and compatibility effects

Protocol:

- none required for CLI skill;
- optional MCP preset uses existing MCP config/protocol.

Storage:

- portable integration package files;
- optional bounded integration data/profile/artifact directory;
- no new database.

Migration:

- none.

Compatibility:

- Node-less CodeGG remains fully functional; the integration is unavailable with actionable diagnostics;
- existing MCP and skill behavior unchanged;
- package should be usable by other Agent Plugins clients where only portable skill content is involved.

## 9. Security requirements

Browser pages can contain prompt-injection text and malicious downloads.

The skill and MCP documentation must state:

- webpage text is data, not instruction authority;
- do not execute commands copied from pages without ordinary review/permission;
- do not download/run files merely because a page requests it;
- do not enter credentials/secrets unless the user explicitly requested an authenticated flow and policy permits it;
- do not reuse logged-in profiles by default.

The Playwright process itself is not a CodeGG security boundary. CodeGG sandbox status must not be represented as if it fully contains an independently spawned browser/network stack unless verified.

## 10. Ordered work packages

### WP1 — Upstream support matrix and package skeleton

- verify current upstream CLI/MCP invocation;
- establish supported versions/prerequisites;
- create Agent Plugins package fixture/metadata after M002 exists.

### WP2 — CLI skill and diagnostics

- SKILL.md + bounded resources;
- doctor checks;
- artifact/workdir guidance;
- no network installation.

Exit: local installed Playwright CLI can be used reliably by an agent via the skill.

### WP3 — Optional MCP companion/preset

- explicit activation/config;
- connection doctor;
- raw/deferred MCP exposure qualification;
- profile/host security documentation.

Exit: user can opt into persistent browser mode without changing core tools.

### WP4 — Qualification/docs

- frontend fixture smoke script/test;
- no-live-network hermetic checks;
- manual optional smoke instructions;
- context/schema measurement;
- closure evidence.

## 11. Required tests

Automated/hermetic:

- portable package validates under M002;
- skill activation and resource bounds;
- dependency doctor: missing Node, old Node, CLI present, MCP present;
- no setup command auto-runs during discovery/activation;
- MCP companion remains inactive unless selected;
- generated config maps through existing McpService;
- package uninstall/disable removes future activation without affecting core CodeGG;
- static dependency diff proves no Playwright/Chromium Rust/default binary dependency.

Optional local smoke, not CI-gating:

- start local static demo site;
- open page;
- inspect;
- perform one interaction;
- verify result;
- screenshot;
- close session;
- repeat in MCP mode if installed.

No external website should be required for closure.

## 12. Required verification commands

~~~text
cargo test -p codegg skills
cargo test --test skills_registry
cargo test --test plugin_contributions
cargo test -p codegg mcp
scripts/verify.sh quick
~~~

Plus integration package validation commands introduced by M002.

## 13. Documentation updates

- docs/PLUGINS.md or integrations documentation
- architecture/skills.md
- architecture/mcp.md
- troubleshooting/doctor docs
- README capability mention only after qualified
- plans/closure/plugin-ecosystem-interoperability/003-status.md

## 14. Acceptance criteria

- CodeGG offers a supported browser-testing integration without embedding a browser;
- CLI+skill is the default recommended path;
- MCP is explicit optional mode;
- activation performs no silent npm/browser download;
- prerequisite failures are actionable;
- browser profiles/credentials are isolated by default;
- webpage content is trust-framed as untrusted;
- core tool palettes are unchanged;
- package/install uses M002 portable path;
- no live-network CI is added;
- optional local smoke and scripts/verify.sh quick pass.

## 15. Stop conditions

Stop and report if:

- upstream requires an unbounded or unstable undocumented command interface;
- portable packaging would force MCP activation for CLI-only users;
- browser profile isolation cannot be made explicit;
- CodeGG would need to embed Node/Chromium to provide a reliable integration;
- correct process cleanup requires bypassing existing shell/MCP ownership;
- upstream licensing/distribution terms prevent repository-owned integration assets.

## 16. Closure evidence required

- upstream version/support matrix and date;
- package/skill files;
- proof no auto-download occurs at discovery/activation;
- dependency doctor fixtures;
- CLI local smoke evidence;
- optional MCP local smoke evidence;
- context/schema overhead measurement;
- profile/credential default evidence;
- dependency/binary-size diff proving no browser engine added;
- exact verification commands/outcomes.

