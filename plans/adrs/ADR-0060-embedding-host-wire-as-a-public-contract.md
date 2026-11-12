# ADR-0060: The Embedding Host Wire Is a Public Contract

Status: accepted

Date: 2026-11-12

Accepted: 2026-11-12, by the project maintainer, as the gate the embedding-host
line's own plans require before `codegg host --stdio` is handed off
(`plans/implementation/codegg-embedding-host/002-headless-host-protocol.md`,
"Applicable ADRs"). The acceptance covers the decision below and nothing wider:
no embedding surface ship date, no crate publication and no loop extraction is
authorized by this record.

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#44-frontends-render-projections`
- `plans/001-terminology-and-domain-model.md` — provider connection, session,
  turn

Affected subsystem roadmaps:

- `plans/subsystems/codegg-embedding-host-roadmap.md`

Related ADRs:

- ADR-0010 (accepted) — desktop frontend and shared client boundary. This ADR
  does not revisit it: the desktop stays a daemon client, and the embedding host
  is a separate surface offered to programs that are not frontends.
- ADR-0053 (accepted) — the daemon's published projection surfaces. The
  embedding wire deliberately publishes **less** than a frontend projection; it
  carries execution events and usage, not session or project projections.
- ADR-0047 (accepted) — restricted-agent shared-checkout writer contract. The
  workspace root the host binds is a turn's writable root and carries no
  checkout coordination.

## Context

CodeGG has two programmatic surfaces today. ACP v1 (`src/acp.rs`) is a stdio
JSON-RPC adapter but is a *client* of the daemon socket: it buys a running
daemon, a project and a session before it can delegate anything, and it has no
provider input, no `_meta` and no structured error or usage taxonomy. The
native frontend protocol (`crates/codegg-protocol`, `crates/codegg-client`) is
richer but is a daemon contract modelled around projects, sessions and git
worktrees.

Neither surface lets a program that already owns a provider connection say so.
Provider configuration is file-based and closed: `ProviderConfig` has no headers
(`crates/codegg-config/src/schema.rs`), `ModelConfig` is `{name, variants}`, a
custom provider id is rejected, and the `opencode_go` built-in pins its endpoint
and refuses a model that has no reviewed wire mapping. A host that needs one
connection-level header therefore has to write configuration, run a relay inside
its sandbox, and shadow the connection's identity.

The embedding-host line answers that gap with a descriptor and a headless host
mode. Once a host programs against the wire, the wire becomes a contract that
CodeGG is expected to keep: a host pinned to an older SDK must not be broken by
a CodeGG upgrade that renames a method, drops a field or reclassifies an error.
That is an architectural commitment about a durable public surface, so it is
recorded here rather than assumed in a milestone.

## Decision drivers

- **A host cannot read CodeGG's source to integrate.** The contract has to be
  written down, versioned and testable without the repository.
- **The wire must not become a second frontend protocol.** Every field added is
  a field CodeGG owes forever; the surface stays small.
- **ACP and `exec` must not move.** Existing consumers of both are unaffected by
  this decision.
- **A secret carrier needs a stable rule, not a convention.** Versioning,
  redaction and the no-argv rule are part of the contract or they will drift.

## Considered options

### Option A — Document ACP as the embedding surface and extend it with `_meta`

Rejected. ACP is bound to the daemon by construction: a host would still need a
daemon, a project and a git-shaped workspace, and the adapter's method set is
smaller than the events the loop already produces. `_meta` is an untyped
extension bag; a stability policy over it would be a policy over arbitrary
payloads.

### Option B — Publish the native CoreFrame protocol as the embedding surface

Rejected. It is a frontend contract: projects, worktrees, session projections and
a daemon socket. Exposing it would freeze a much larger surface and would make
every frontend change an embedding-contract change.

### Option C — A separate, small, versioned host wire (`codegg host --stdio`)

Accepted. One process, one workspace root, one session, JSON-RPC 2.0 over
newline-delimited JSON on stdio. Five methods (`initialize`, `session/open`,
`turn/submit`, `turn/cancel`, `session/close`) and five notification kinds
(`turn/text`, `turn/tool`, `turn/usage`, `turn/done`, `turn/error`). The
provider connection arrives inline in `session/open` and never on argv.

### Option D — Extract the loop into a library and let hosts link it

Rejected for now. It is the largest possible commitment (a 40k-line loop, its
tool surface and its provider stack become a library API), it gives up process
isolation, and it does not help the first host, which runs CodeGG inside a Linux
VM and could not link it even if the API existed. Extraction stays a separate
decision with its own ADR if it is ever wanted.

## Decision

**The embedding host wire is a public contract of CodeGG, versioned by
`apiVersion`, with its own stability policy, and is independent of ACP, `exec`
and the frontend protocol.**

1. **Version.** The wire declares `apiVersion: 1`. `initialize` returns the
   version, a capability list and CodeGG's own version. A host that does not
   understand the version refuses the session; CodeGG never silently downgrades.
2. **Additive within a major version.** Adding an optional field, a new
   notification kind or a new error class is allowed inside `1`. Removing or
   renaming a method, a notification, a required field or an error class
   requires `apiVersion: 2`. A host must ignore an unknown notification and an
   unknown field; CodeGG must not require a host to understand an optional
   field it did not ask for.
3. **One definition.** The wire types live in one crate that both the host
   binary and the SDK depend on. A second copy is a defect, not a fork.
4. **Golden frames are the gate.** Every request, result and notification has a
   serialized fixture. A change that alters a fixture fails the suite unless the
   change is additive and the fixture records the addition.
5. **The capability list is a refusal surface.** A host asks for a capability;
   CodeGG either has it or refuses. Declaring a capability CodeGG does not
   implement is a protocol error, not a warning.
6. **The secret rule is part of the contract.** A descriptor travels on stdin.
   A credential never appears on argv, in a log, in a stderr diagnostic, in an
   error frame or in a dumped frame. Header values are treated as secrets even
   when they are not, because a host may put a credential in one.
7. **Scope of the guarantee.** This record binds the wire, not the tool surface,
   not the model catalogue and not the loop's behaviour. A change in which tools
   exist, which models a built-in provider accepts, or how the loop plans is not
   a wire change.
8. **ACP, `exec` and the daemon protocol are out of scope.** They keep their own
   versioning and their own tests. This ADR neither freezes nor unfreezes them.

## Consequences

- A host may pin `codegg-sdk` and depend on the wire surviving a CodeGG upgrade
  within a major version.
- Every future wire addition is reviewed as a durable commitment; the cheap
  option is a notification CodeGG already produces, not a new method.
- `docs/embedding.md` becomes the contract of record and must be updated in the
  same change as any wire edit.
- The capability list is now a place where a half-finished feature can leak into
  a promise; a capability is declared only when it is exercised by the
  conformance fixture.
- CodeGG gains a third documented surface and therefore a third compatibility
  suite to run.

## Compatibility and migration

The wire is new, so it has no migration path and no legacy consumer. The
descriptor type is additive in `codegg-providers`; `ProviderConfig` gains no
field and the config schema does not change. ACP (`protocolVersion: 1`) and
`codegg exec`'s JSON shape (`{success, result, toolsUsed, tokensUsed,
durationMs, error, code}`) are unchanged and are pinned by their own tests. A
host that wants the pre-existing behaviour does not pass a descriptor.

## Security and reliability implications

- The descriptor is the only new secret carrier. It arrives on stdin, is never
  written to disk, and is redacted in `Debug`, logs and error text.
- Header names are validated and values are bounded and rejected on CR/LF, so a
  descriptor cannot split a request or smuggle a second header block.
- Only an explicit descriptor declaration may bypass the reviewed-wire-mapping
  rule; built-in providers keep that rule unchanged.
- A provider failure ends a turn and not the process; a host disconnect cancels
  the active turn and exits, so no orphan work continues after the host is gone.
- One active turn per session, refused with a typed busy class rather than
  queued, keeps the wire's ordering guarantees honest.

## Verification

- `cargo test -p codegg-host-protocol` — golden frames for every message.
- `cargo test -p codegg-sdk` — the conformance fixture through the client.
- `cargo test -p codegg --bin codegg host` — the host implementation against the
  same fixture, plus cancellation, EOF and error-class tests.
- `scripts/verify.sh scoped` — the repository's own guard suite.
- The line's closure records carry the real-host transcripts
  (`plans/closure/codegg-embedding-host/`).

## Supersession

This ADR supersedes nothing. It is bound by ADR-0010 and ADR-0053 in the sense
that it adds a surface beside theirs without changing them. A later decision
that changes the wire's stability policy, extracts the loop, or replaces stdio
with a transport must be a new ADR that supersedes this one and links to it.
