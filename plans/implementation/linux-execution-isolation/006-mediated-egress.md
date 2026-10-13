# Linux Execution Isolation M006 — Mediated Allowlisted Network Egress

Status: blocked — hard: M005
Primary class: capability
Repository baseline: `main` @ `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`, 2026-10-13.
Source: `plans/subsystems/linux-execution-isolation-roadmap.md`.
Long-term requirements: `plans/000-long-term-specification.md` §§4,17,27,29; `plans/001-terminology-and-domain-model.md` §8.
Applicable accepted decisions: ADR-0001, ADR-0002, ADR-0004, ADR-0016. ADR-0022 remains proposed, not accepted.
No production implementation or verification is asserted by this plan.
Target closure record: `plans/closure/linux-execution-isolation/006-status.md`.

## 1. Objective
Provide a separately operator-approved `ProxyAllowlist` capability for network-isolated child processes, allowing only explicitly authorized destinations/protocols through a trusted broker, while **direct** egress remains unavailable. Start with HTTP(S)-only if full arbitrary TCP protocol mediation cannot be safely proven.

## 2. Why this milestone and existing evidence
M005 establishes a verifiable offline namespace. `src/security/ssrf.rs` validates certain Codegg-owned requests but cannot constrain arbitrary child socket APIs. A proxy environment variable alone is not a security boundary. Consider reusing Eggress/Eggfetch transport pieces only after verifying policy mediation; do not assume their existing generic clients/proxies enforce this trust model.

## 3. Non-regression and security invariants
The trusted scheduler, not model text/child proxy env/Host header, owns policy identity and allowlist. Direct TCP/UDP/IPv6, localhost/metadata/private/link-local access and unauthorized Unix sockets remain denied. Proxy cannot serve as open CONNECT/SOCKS/HTTP Upgrade/websocket tunnel. Authenticate broker access per attempt and prevent cross-session borrowing; DNS answers must be pinned/validated at actual connect time, including IPv4-mapped IPv6 and redirects. Never pass provider tokens or broker credentials into model transcript. No firewall/root requirement. Required mediation unavailable => fail closed without fallback to host egress.

## 4. Scope, required production changes and ownership
Define typed policy: exact hostname or explicitly reviewed wildcard, ports, protocol, resolved address classes, redirect semantics, optional connection/time/byte budget and expiry. Implement a trusted broker on host side with a private transport exposed narrowly to only the owning isolated child; do not bind broad `/run`, `/tmp` or host root/socket directories. Require canonical job/lease authentication and policy narrowing for child agents. Broker resolves and validates every destination at connect time, prevents rebinding/local SSRF/redirect tunneling, and returns sanitized audit reason codes. Integrate lifetime and revocation under `ManagedProcessService` and scheduler, report `NetworkEnforcement` distinct from filesystem. Prefer deterministic offline fixtures rather than live internet tests.

Explicit non-goals: altering macOS/Windows execution defaults; creating a second independent process executor; changing worktree/session authorization; agent-selected escalation to FullHost; undocumented broadening of permissions; marking a requested but unproven enforcement result as applied.

Core/domain: retain typed policy snapshot, lease and immutable parent/child ceilings. Storage/migrations: no initial DB schema change; if durable preferences or public protocol/DTO fields change, design a versioned migration and compatibility test before shipping. Runtime/concurrency: execution belongs to `ManagedProcessService`, parent and helper must bind the status to the specific child before user exec. Frontend/operator surface: show requested and obtained dimensions and a safe explicit denial/next action. Security: untrusted workspace and child inputs never select helper, override enforcement, or control trusted broker metadata. Documentation/guards: update `architecture/security.md`, relevant jobs/tool docs, tests and static guards.

## 5. Ordered work packages and evidence
A. Threat model HTTP(S), CONNECT, Host/SNI confusion, DNS alias/CNAME, IDNA, IPv6/IP-literal, redirect, websocket/upgrade, HTTP/2, UDP/QUIC, abstract UDS and proxy protocol smuggling.
B. Decide minimum secure broker protocol, evaluate Eggress/Eggfetch reusability and reject unneeded new daemon.
C. Implement per-attempt signed/unguessable authorization against scheduler-owned leases; broker and resolver validate final address and policy.
D. Compose bridge with offline namespace; no alternate outbound interface; ensure child cannot bypass via env or inherited descriptor.
E. Add deterministic TLS/DNS/HTTP fixture, negative tests, cleanup and operator docs; close with exact-revision evidence.

## 6. Failure, cancellation, restart and contention
Broker crash or policy revocation fails requests and does not enable host-network fallback. Cancellation removes owned proxy sockets/auth, closes active relays and rejects stale generation. Concurrent jobs cannot share credentials, routes, allowlist or proxy endpoint. Unknown DNS, invalid TLS/SNI or unsupported protocols deny; no unbounded buffering/logging.

## 7. Compatibility and migration
Default `ReadOnly`/`WorkspaceWrite`/`FullHost` vocabulary, existing Landlock ABI>=3 baseline and legacy unrestricted shell network stay truthful and usable until explicit, reviewed adoption. No migration is implied by writing this plan. Platform unsupported or optional skipped test is not a proof of security. A new public protocol or persisted preference must be versioned; older consumers cannot silently treat unsupported required constraints as met.

## 8. Focused, integration, negative and qualification tests
Unit: policy parsing, DNS rebinding, IDNA/IP classification, port/protocol, redirect revalidation, auth lease and bounded quota. Real child: direct TCP/UDP denial, allowed HTTPS fixture success, blocked metadata/local/private targets, CONNECT tunnel rejection, hostname/SNI mismatch, redirect and DNS change, stale token/FD, concurrent jobs and broker restart. Commands: `cargo fmt --all -- --check`; `cargo test -p codegg --lib security::ssrf`; `cargo test -p codegg --lib managed_process`; `cargo test -p codegg --test sandbox_containment -- --test-threads=1`; `scripts/verify.sh quick`. Include a new opt-in Linux proxy integration test and exact command/host evidence.

## 9. Acceptance criteria and closure evidence
Real child can reach an explicitly approved HTTPS fixture **only** through the broker; all direct IP, denied domain, metadata/localhost, rebinding and tunnel paths fail. Per-job broker privileges expire and cannot be borrowed. Obtained network status is tied to enforced namespace and broker rather than environment alone.

Record implementation commits, host capability/kernel version, test output, skips/blocks, requested/obtained status, requirement-to-evidence table, compatibility, rollback, unresolved findings with severity and a justified closed/conditionally closed/blocked recommendation in `plans/closure/linux-execution-isolation/006-status.md`. Do not claim verification merely because fixtures exist.

## 10. Stop conditions
Stop if generic TCP tunneling remains possible, DNS checks are not bound to actual connected peer, a child can contact host networking directly, or a shared proxy exposes other sessions. Ship a deliberately narrow protocol or block M006 instead of claiming unrestricted safe network.
