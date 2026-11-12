# CodeGG Embedding Host Roadmap

Status: proposed

Long-term references:

- `plans/000-long-term-specification.md` — host-neutral agent execution, provider ownership, secret handling.
- `plans/001-terminology-and-domain-model.md` — session, turn, provider connection, model.
- `plans/002-long-term-roadmap.md` — headless operation and adapter direction.

Related ADRs:

- ADR-0060 (accepted) — the embedding host wire is a public contract with its own stability policy. It governs E002, E003 and E004.

Research record: `research/codegg-embedding-host-2026-11-12.md`.

## 1. Purpose and ownership boundary

CodeGG owns the agent loop, provider adapters, model profiles, tool surface and
approval/sandbox policy. A **host** is a separate program that wants to drive
those capabilities programmatically: an editor, a desktop app, a local server
delegating coding work, a CI job.

This subsystem owns the *embedding contract*: the headless host mode, its wire
protocol, the SDK crate that speaks it, and the inline provider descriptor that
lets a host supply its own connection without writing CodeGG configuration.

It does not own the loop, the tool implementations, the providers or the
frontend protocol. It must not become a second daemon, a second provider stack,
or a place where host-specific concerns (UI, artifact capture, scheduling) leak
into CodeGG.

## 2. Work classification

### Invariants

- A host receives the same agent behaviour as the TUI and `exec`: same loop,
  same tools, same approval and sandbox semantics.
- Secrets never travel on argv and never enter a log, a frame dump or a
  transcript. The inline descriptor is read from stdin.
- A descriptor-declared connection is explicit. CodeGG never guesses a wire
  surface; the reviewed-wire-mapping rule remains for built-ins.
- The host protocol is versioned and additive within a major version.
- No daemon, no global config and no TUI state is required to run a host
  session.

### Capabilities

- Inline provider descriptor: endpoint, credential or env reference, headers,
  model catalog and declared wire surface.
- Per-session workspace root without a git or project requirement.
- Structured events with usage, and one typed error taxonomy.
- Turn cancellation that leaves the process usable.

### Infrastructure

- `codegg host --stdio` with `apiVersion` and capabilities.
- `codegg-sdk` crate: typed client and wire types.
- A conformance fixture that both the host mode and the SDK are tested against.

### Polish

- Documentation for third-party hosts.
- A version-negotiation story for a host pinned to an older SDK.

## 3. Non-goals

- Replacing ACP. The ACP adapter stays as-is for existing consumers.
- Exposing the frontend CoreFrame protocol to third parties.
- In-process embedding (linking the loop into another binary). The loop lives in
  the binary crate; extraction is a separate decision.
- Multi-tenant hosting, remote transport, or a network listener of any kind.
- Host-specific features: artifact capture, scheduling, chat UI, budgets.
- A general plugin or extension mechanism on the host protocol.

## 4. Current state

- ACP v1 (`src/acp.rs`) is the only documented programmatic surface. It has no
  provider input, no `_meta` handling, text-only prompts and no structured
  result or error taxonomy.
- The native protocol (`crates/codegg-protocol`, `crates/codegg-client`) is a
  frontend contract over a local daemon socket, modelled around projects,
  sessions and git worktrees.
- `codegg exec` (`src/exec.rs`) is a one-shot JSON surface over the real loop:
  `{prompt, model, agent}` in, `{success, result, toolsUsed, tokensUsed,
  durationMs, error, code}` out, with `-s` resume and policy flags. It has no
  workspace parameter, no provider input, no streaming and no cancellation.
- Provider configuration is file-based and closed: `ProviderConfig` has no
  headers and `models` carries only `{name, variants}`; custom provider ids are
  rejected; `opencode_go` is pinned to a shared-profile endpoint and a reviewed
  model list.
- The loop (`src/agent/`, ~40k lines) lives in the binary crate; `exec` and the
  daemon both drive it from there.

## 5. Target architecture

```text
host process                     codegg host --stdio
    |  JSON-RPC 2.0 lines over stdio  |
    |-------------------------------->|  initialize / session.open
    |                                 |      -> inline provider descriptor
    |                                 |         (endpoint, headers, credential,
    |                                 |          protocol, model catalog)
    |                                 |  turn.submit
    |<--------------------------------|  turn.text / turn.tool / turn.usage
    |<--------------------------------|  turn.done { stopReason, usage }
    |<--------------------------------|  turn.error { class, providerStatus }
    |  turn.cancel / session.close    |
                                     |
                                     v
                          existing agent loop + tools
                          descriptor-built provider registry
                          descriptor-supplied model profile
```

Ownership:

- `codegg-providers`: an `InlineProviderDescriptor` type, a bounded header list
  on the OpenAI-compatible client, and a registration function that builds a
  provider from a descriptor without config or credential-store lookups.
- `codegg-core`: a `ModelProfile` constructor from descriptor metadata.
- `src/host/` (new, binary crate): the wire, session state, turn lifecycle, and
  the mapping from loop events to notifications. It reuses `exec`'s loop setup
  rather than duplicating it.
- `crates/codegg-sdk`: wire types and a client. It depends on `codegg-protocol`
  only if a shared primitive is genuinely reusable; otherwise it stays small and
  independent.

## 6. Dependency graph

```text
E001 inline provider descriptor
    |
    +--> E002 headless host protocol v1
    |        |
    |        `--> E003 SDK crate and conformance
    |                 |
    |                 `--> E004 integration evidence and closure
    |
    `--> (independent) ACP and exec keep working
```

Classifications: E001 → E002 is hard (the protocol carries the descriptor).
E002 → E003 is hard (the SDK is the wire's client). E003 → E004 is hard (the
closure needs a real host). ACP/exec compatibility is an interface dependency
that must not regress.

## 7. Milestones

### E001 — Inline provider descriptor

Class: capability

Objective: a session can be given a provider connection inline — endpoint,
credential or env reference, headers, model catalog, declared wire surface — and
use any model id on it, without a config file and without the reviewed-model
restriction.

Dependencies: none hard. The per-turn registration seam
(`register_builtin_with_config_connections`, `TurnCredentialLocator`) and the
session-affinity header on the OpenAI-compatible provider are the interfaces.

Deliverable boundary: descriptor type and validation; descriptor registration;
bounded request headers; model-profile construction from descriptor metadata;
provider error taxonomy mapping. No wire protocol.

User or operator value: a host can point CodeGG at the connection it already
uses, with the model it already uses, with no configuration step.

Exit conditions: the plan's acceptance criteria; existing config-based
registration unchanged; secrets provably absent from logs and argv.

Deferred work: non-`chat_completions` surfaces; provider discovery through a
descriptor; proxying.

### E002 — Headless host protocol v1

Class: capability

Objective: `codegg host --stdio` runs a session with a workspace root, model,
agent, approval and sandbox policy, submits turns, streams structured events,
cancels, and closes — with no daemon and no global config.

Dependencies: E001 hard.

Deliverable boundary: wire types and versioning, session and turn lifecycle,
event mapping, cancellation, workspace binding without git. Not the SDK crate.

User or operator value: a host drives CodeGG over the pipe it already has.

Exit conditions: conformance fixture passes; ACP and `exec` unchanged; a turn
cancels within the stated bound and the process remains usable.

Deferred work: multiple concurrent turns per process, attachments in the wire,
tool allow-lists, remote transport.

### E003 — SDK crate and conformance

Class: infrastructure

Objective: `codegg-sdk` is the typed client for the host protocol, with golden
wire tests and a conformance fixture shared with the host implementation.

Dependencies: E002 hard.

Deliverable boundary: crate, client API, wire golden tests, a fake-host harness,
and `docs/embedding.md`. Not a publish decision.

User or operator value: a host can implement against a documented contract
without reading CodeGG internals.

Exit conditions: a host written only from the docs can open a session, submit a
turn, receive events, cancel and close; the golden tests fail on any wire change.

Deferred work: publishing to a registry; semver guarantees beyond the crate's
own version.

### E004 — Integration evidence and closure

Class: infrastructure

Objective: one real host drives one real session end to end on the pinned Linux
build: descriptor with headers, a model outside the reviewed list, streaming,
cancellation, a typed provider error, and a restart.

Dependencies: E003 hard.

Deliverable boundary: evidence and closure records only; no new surface.

Exit conditions: the closure record recommends `closed`; the host's own plan can
consume the SDK.

Deferred work: any second host.

## 8. Cross-cutting requirements

### Storage and migration

No new durable storage. Sessions live in memory for the process lifetime; a
host that wants resumption re-opens a session. Descriptor metadata is never
persisted by CodeGG.

### Protocol and compatibility

`apiVersion: 1`. Additive fields are allowed; a method or field removal requires
a new major version. `initialize` reports the version and capabilities, and an
unknown capability is a refusal, not a silent downgrade. ACP and `exec` remain
unchanged and independently versioned.

### Security and authorization

The descriptor arrives on stdin. A credential is never accepted on argv, never
written to a file, never logged, and never echoed in an error. Headers are a
bounded list of name/value pairs with validated names; a header value that looks
like a credential is accepted but treated as a secret (never logged). The
descriptor's declared wire surface is validated against a small allow-list.

### Concurrency, cancellation, and recovery

One active turn per session; a second submit is refused with a typed busy error.
Cancellation is cooperative, bounded, and leaves the process able to accept the
next turn. A provider stream failure ends the turn, not the process. A host
disconnect (stdin EOF) cancels the active turn and exits cleanly.

### Observability and audit

Events are the only transcript channel. Logs stay on stderr and are redacted.
Usage is reported when the provider reports it and explicitly marked absent
otherwise.

### Performance and resource use

One process per host session; no daemon. Descriptor registration is per session
and does not touch global state. The host mode must not add a startup cost
beyond the existing loop initialization.

### Documentation and operations

`docs/embedding.md` is the contract of record: methods, notifications, error
classes, versioning, a worked example, and the security rules.

## 9. Verification strategy

- Unit: descriptor validation, header bounds, model-profile construction, error
  mapping.
- Golden wire: every method and notification serialized and compared.
- Conformance: a scripted host drives a fake provider through open, submit,
  stream, cancel, close, error.
- Real: one live turn on the pinned Linux build with a descriptor-declared
  connection and a model outside the reviewed list.
- Compatibility: ACP and `exec` suites unchanged.

## 10. Risks and decision points

- **Stability policy.** A public wire needs a stated policy; this requires an ADR
  before E002 is handed off.
- **Loop extraction pressure.** Hosts will ask for in-process embedding. The
  answer is no for v1; extraction is a separate decision with its own cost.
- **Descriptor trust.** A declared wire surface must never widen into guessing.
- **Secret handling.** Inline credentials are a new carrier for an old secret;
  the redaction tests are the gate.
- **Reviewed-wire-mapping erosion.** The built-in rule must stay for built-ins;
  only a descriptor's explicit declaration bypasses it.

## 11. Completion definition

E001–E004 closed; `docs/embedding.md` published; a real host has driven a real
session with a descriptor-declared connection, streaming, cancellation and a
typed error; ACP and `exec` are unchanged; the ADR for the wire's stability is
accepted.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| E001 inline provider descriptor | proposed | `plans/implementation/codegg-embedding-host/001-inline-provider-descriptor.md` | — | none |
| E002 headless host protocol v1 | proposed | `plans/implementation/codegg-embedding-host/002-headless-host-protocol.md` | — | E001; ADR for wire stability |
| E003 SDK crate and conformance | proposed | `plans/implementation/codegg-embedding-host/003-sdk-crate-and-conformance.md` | — | E002 |
| E004 integration evidence and closure | proposed | `plans/implementation/codegg-embedding-host/004-integration-evidence-and-closure.md` | — | E003 |
