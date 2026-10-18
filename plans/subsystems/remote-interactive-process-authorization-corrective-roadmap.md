# Remote Interactive Process Authorization Corrective Roadmap

Status: proposed

Long-term references:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#41-authorization-and-principals`

Related ADRs:

- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md` (establishes `/core` as
  the remote-client protocol and names PTY output as a bounded ordered stream; it does **not**
  decide which principals may create interactive processes)
- A new ADR is required for this workstream before implementation. See §10.

## 1. Purpose and ownership boundary

This roadmap owns **one question**: which principals may create, drive, and terminate an
interactive process (PTY) on a host reached over a remote transport.

It exists because the interactive-process request family is authorized as `ScopeKind::Global`
with no capability, while the server's `/core` WebSocket endpoint is reachable by team
principals. The two facts combine into an authorization posture that has never been decided,
documented, or tested.

**Ownership boundary.** This roadmap owns the authorization policy for
`CoreRequest::InteractiveProcess*`, the `InteractiveAuthority` transport model, and the
`ServerCapabilities` advertisement for remote terminals. It does **not** own the TUI terminal
surface (that is `interactive-terminal-liveness-post-closure-corrective`), the PTY/ring/resync
mechanics (that is `interactive-process-sessions`), or the general daemon authorization service.

## 2. Work classification

### Invariants

- A remote principal must never obtain host code execution that the local owner would consider
  surprising. Terminal output, input, and process lifetime are equally powerful; authorizing one
  and denying another is not a defensible line.
- Authorization is decided at the daemon, from transport-bound identity, never from request
  payload fields.

### Capabilities

- An operator can determine, before acting, whether a given frontend and principal can open an
  interactive terminal.
- A remote client that cannot open a terminal receives a typed refusal that says so.

### Infrastructure

- A capability advertisement carried in `ServerCapabilities` (or an equivalent handshake), so
  clients learn the answer without trial and error. Currently absent: `ServerCapabilities` has no
  interactive-process field (`crates/codegg-protocol/src/frames.rs:102-135`).

### Polish

- Diagnostics naming why a remote terminal request was refused.

## 3. Non-goals

- Building terminal support on any additional frontend transport. Whether the TUI-attach frontend
  gains a terminal is a separate question; this roadmap decides only what happens *when* such a
  request arrives, on any transport.
- Any change to PTY mechanics, the output ring, resync semantics, or the terminal rendering.
- Replacing or weakening the existing attachment-ownership seam, which binds each attachment to
  its creating `client_id` and fails closed.
- Windows or macOS sandbox work.

## 4. Current state

Evidence, baseline `8b6191140`:

- **The interactive family is authorized globally.** All ten `CoreRequest::InteractiveProcess*`
  variants map to `ScopeKind::Global` with `capability: None`
  (`crates/codegg-core/src/authorization/policy.rs:911-940`). `ScopeKind::Global` is documented as
  *"No project data involved. Any active principal may proceed"*
  (`crates/codegg-core/src/authorization/policy.rs:11-13`).
- **The policy says so in its own words.** The comment above those arms states that the
  daemon-wide gate *"admits any authenticated caller; the attachment seam fails closed per
  process"* (`policy.rs:909-910`).
- **The daemon gate is real and runs before side effects.** `handle_request_with_client` calls
  `authorize_request` and emits an audit record for both denial and authorization
  (`src/core/daemon.rs:3785-3817`).
- **`/core` is reachable by team principals.** Its disposition is `CoreAdapter`, not
  `LocalOwnerOnly` (`src/server/authz.rs:358-364`), and it calls
  `daemon.handle_request_for_client(envelope, client_id)` with the authenticated connection id
  (`src/server/ws.rs:4429`).
- **`/core` routes arbitrary requests.** `handle_core_frame` has no variant allowlist; its only
  gates are a five-variant `is_secret_bearing()` check (`src/server/ws.rs:4352`, which does not
  cover interactive processes) and projection-specific checks that match only `Projection*`
  variants. Everything else reaches the daemon dispatch.
- **The daemon dispatches all ten variants**, binding a transport `client_id` and an
  `InteractiveAuthority` (`src/core/daemon.rs:4041-4050`).
- **The transport model exists and is only half-used.** `InteractiveAuthority::remote()` is
  documented as *"Authority for an authenticated remote principal"*
  (`src/interactive_process_attach.rs:312-319`), and `InteractiveTransport` is consulted in
  exactly one place: `can_terminate()`, where a remote principal is denied by default
  (`src/interactive_process_attach.rs:340-345`).

**The resulting posture.**

| Operation | Local owner | Authenticated remote principal |
|---|---|---|
| Create PTY | yes | yes — no transport check |
| Attach / input / resize / resume / list / detach | yes | yes — ownership seam only |
| Terminate / remove | yes | **no** |

A remote principal can spawn a shell on the host and run arbitrary commands, but cannot kill the
process it started. The asymmetry appears unintended: the guard prevents a remote principal from
terminating *host* processes, while nothing prevents it from *running* them. Remote principals
hold strictly more process-lifetime power than local ones.

**Why this has survived.** The interactive-terminal **frontend** only exists on the socket-daemon
transport, where every principal is the local owner, so the policy's Global classification is
currently unexercised by a remote caller. The exposure is reachable by a client speaking
`CoreFrame` on `/core` directly, without the TUI.

**Related, and out of this roadmap's boundary.** The `human-execution-corrective` C001 closure
records "unsupported remote targets fail explicitly" for finite human shell. This roadmap must not
contradict that contract; it extends the same principle to interactive processes.

## 5. Target architecture

```text
principal (transport-bound)
        │
        ▼
authorize_request  ──►  interactive_process_* operations
        │                      scoped, capability-bearing, auditable
        │                      (currently Global + None)
        ▼
ServerCapabilities.interactive_processes  ──►  client learns before acting
```

Two decisions must be made and recorded:

1. **Which principals may create an interactive process.** Options: local owner only (matches the
   terminate guard and the "locality by default" invariant); any principal with an explicit
   workspace-scoped execution capability; or remote principals with a named host-execution
   capability that is off by default.
2. **Whether remote terminals are ever advertised.** If a remote principal may create a PTY, the
   answer must be in `ServerCapabilities` so clients learn it rather than discovering it by
   failure.

## 6. Dependency graph

| Dependency | Kind | State |
|---|---|---|
| `interactive-process-sessions` M001–M003 | hard (interface) | closed; the attach/resume/resync contract is stable and is not reopened here |
| `human-execution-corrective` C001/C004 | hard (interface) | closed; finite human shell already fails explicitly for unsupported remote targets and is the precedent |
| Daemon M003 authorization gate and audit | interface | closed and stable; reused unchanged |
| `interactive-terminal-liveness-post-closure-corrective` C002 | soft | C002 closes the *frontend* half by refusing accurately; it does not decide this policy. Neither blocks the other |
| A new ADR | hard (decision) | **required before implementation** |

No implementation milestone may open before the ADR is accepted.

## 7. Milestones

### R001 — Decide and record the remote interactive-process authorization contract

Class: **invariant** (authorization semantics must not be left implicit), carrying the
capability-advertisement infrastructure.

Objective: produce an accepted ADR that states which principals may create, drive, and
terminate interactive processes over a remote transport; whether remote terminal support is
advertised; and what a denied request returns.

Deliverable boundary: the ADR, plus the route/capability disposition it implies. No production
code.

Exit conditions: ADR accepted; the create-vs-terminate asymmetry is explicitly resolved (either
retained with a stated rationale or removed); the answer states whether `/core` should advertise
terminal support.

**Not started.**

## 8. Cross-cutting requirements

- **Storage:** none. No migration.
- **Protocol:** at most a `ServerCapabilities` field and/or a typed error code, both defined by
  the ADR. No new `CoreRequest`/`CoreResponse` variant is expected.
- **Security/authorization:** this is the substance of the milestone. Any change goes through
  `operation_descriptor` and the canonical `AuthorizationService`; a bespoke check inside the
  interactive module is not acceptable as a substitute.
- **Audit:** `scripts/check_audit_coverage.py` already pins
  `interactive_process_create` and `interactive_process_input`. A denial of a remote create must
  be audited with the same completeness as an allow.
- **Observability:** denial must be observable to the operator as a typed refusal. Terminal
  **bytes** must never reach the bus, a projection, the transcript, or audit records — the
  existing boundary is unchanged by any decision here.
- **Docs:** `architecture/authorization.md`, `architecture/audit.md`, and the server/client
  architecture docs must state the decided posture.

## 9. Verification strategy

- **Security tests are the acceptance signal, not a supplement.** A test that an authenticated
  remote principal receives the decided disposition on `InteractiveProcessCreate` must be written
  first and demonstrated failing against the current Global classification.
- **Negative tests for each operation** in the family, not only create — the asymmetry is the
  defect, so each verb needs its own disposition test.
- **No regression to the local path:** the socket-daemon frontend's terminal behaviour must be
  unchanged.
- `scripts/check_audit_coverage.py` must stay green, and any new denial path must be covered by it.
- `bash scripts/verify.sh quick` locally, with CI labelled separately per `AGENTS.md` (CI is a
  strict subset of `quick`).

## 10. Risks and decision points

| Risk / decision | Disposition |
|---|---|
| The exposure is real today and reachable without the TUI | Must be stated plainly in the ADR. This roadmap exists because it was found during C002 research and deliberately not folded into that milestone |
| Removing remote create is a behaviour change for anyone relying on it | The current behaviour is undocumented and untested; the ADR must record the migration and the refusal users will now receive |
| A remote principal *should* be able to run commands remotely | Then it needs a named, auditable capability, off by default, and it must be advertised. Silence is not an option |
| `ServerCapabilities` cannot reach the `/tui` frontend | Correct — `src/client/attach.rs` performs no `ClientHello`/`ServerHello` handshake. Any advertisement must target `/core`, and the TUI-attach frontend's refusal remains local (`is_ws_attach_transport()`) |
| Folding this into the terminal-liveness C002 milestone | Prohibited. C002 is bounded to honest frontend refusal; this is an authorization contract change and needs its own ADR and milestone |
| Nobody decides, so it stays Global | Not an option. Undecided authorization semantics are a defect, and the terminate guard shows the intent was never "remote may do anything" |

## 11. Completion definition

This workstream completes when a reader can state, from an accepted ADR and a passing test, which
principals may create, drive, and terminate an interactive process over a remote transport; when
the create-versus-terminate asymmetry is resolved; when a remote client can learn the answer from
the protocol handshake; when every denial is audited and typed; and when the local terminal path
is demonstrably unchanged.

## 12. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| R001 | not started | — | — | new ADR required before implementation |

Additionally tracked, not owned here: the `interactive-terminal-liveness-post-closure-corrective`
C002 milestone closes the **frontend** half of the transport boundary by refusing accurately. It
explicitly does not decide this policy, and its closure must not claim that remote principals
cannot create interactive processes.