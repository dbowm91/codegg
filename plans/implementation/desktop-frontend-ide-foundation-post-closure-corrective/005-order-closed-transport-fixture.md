# Desktop Frontend and IDE Post-Closure Corrective C005 — Order Closed-Transport Fixture

Status: closed — closed-transport fixture ordering qualified

Repository baseline: `70405abdcc71e079dac053256854d0ff6efe7ad0`

Source corrective: `plans/subsystems/desktop-frontend-ide-foundation-post-closure-corrective-addendum.md`
Controlling process: `plans/003-planning-process.md#7-corrective-passes`
Owning module: `architecture/client.md`

## Trigger

Hosted CI run `38848051476` failed at
`codegg-client::session_projection_driver::closed_transport_publishes_disconnected`.
The fake daemon wrote the subscribe response and immediately dropped its socket.
The client could observe EOF before `SessionProjectionDriver::attach` consumed
the response, so the test failed during attach instead of exercising the
post-attach disconnected state.

## Scope and invariants

- Keep the production client/driver behavior unchanged.
- Have the fake daemon wait for an explicit test signal after the attach call
  returns, then close the socket without an unsubscribe handshake.
- Continue asserting that the attached driver publishes `Disconnected` and
  stop reports the subscription released.
- Do not increase timeouts or skip the test.

## Verification and closure

Run formatting and obtain green hosted `CI / verify` on the exact source head
with the complete workspace test suite. Record the exact run and count in
`plans/closure/desktop-frontend-ide-foundation-post-closure-corrective/005-status.md`.

Closure: `plans/closure/desktop-frontend-ide-foundation-post-closure-corrective/005-status.md`.
Hosted CI `38850114895` and Desktop E2E `38850114846` passed on the exact
source head.

