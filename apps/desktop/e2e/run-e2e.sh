#!/usr/bin/env bash
# run-e2e.sh — Canonical built-app E2E entry point for C003 qualification.
#
# Usage (from anywhere; paths resolve internally):
#   CODEGG_DAEMON_EXECUTABLE=/path/to/codegg apps/desktop/e2e/run-e2e.sh
#
# Flow per phase (lifecycle, then autostart — separate phases because the
# embedded WebDriver provider spawns the desktop app once per WebdriverIO
# invocation in the launcher process, so each phase needs its own app
# environment):
#
#   1. create an isolated phase home (temp-scoped);
#   2. start the socket fixture server (`desktop_e2e_fixture serve`);
#   3. lifecycle: RPC `start` (isolated daemon + observer + probe project);
#      autostart: RPC `start` then `stop_daemon` (warm home, dead daemon, so
#      the fresh app must autostart through the explicit executable);
#   4. export the phase env and run the phase spec — the launcher, the
#      service, and the app all inherit it, so the app can only ever see the
#      isolated home;
#   5. RPC `shutdown` (identity-checked kill when the pid is known, scoped
#      best-effort fallback otherwise) and remove the phase home.
#
# Exit status is nonzero if any phase fails. `e2e/logs/` holds supplementary
# runner logs; the machine assertions in e2e/specs are the closure authority.
set -euo pipefail
app_root="$(cd "$(dirname "$0")/.." && pwd)"
repo_root="$(cd "$app_root/../.." && pwd)"
fixture_bin="$app_root/src-tauri/target/debug/desktop_e2e_fixture"

if [[ -z "${CODEGG_DAEMON_EXECUTABLE:-}" ]]; then
  candidate="$repo_root/target/debug/codegg"
  if [[ -x "$candidate" ]]; then
    export CODEGG_DAEMON_EXECUTABLE="$candidate"
  else
    echo "CODEGG_DAEMON_EXECUTABLE is required (build the root codegg binary first)" >&2
    exit 2
  fi
fi
if [[ ! -x "$CODEGG_DAEMON_EXECUTABLE" ]]; then
  echo "daemon executable is missing or not executable: $CODEGG_DAEMON_EXECUTABLE" >&2
  exit 2
fi

echo "==> building E2E fixture helper"
rustup run 1.90.0 cargo build --manifest-path "$app_root/src-tauri/Cargo.toml" \
  --locked --bin desktop_e2e_fixture
echo "==> building desktop app with the desktop-e2e feature"
"$app_root/e2e/build-e2e-app.sh" build

cleanup_build() {
  "$app_root/e2e/build-e2e-app.sh" clean >/dev/null 2>&1 || true
}
trap cleanup_build EXIT

# RPC helper: one JSON command to the phase fixture over its socket.
fixture_rpc() {
  local socket="$1" cmd="$2"
  CODEGG_E2E_SOCKET="$socket" node --input-type=module -e "
import('$app_root/e2e/fixture-client.ts').then(async ({ FixtureClient }) => {
  const client = await FixtureClient.connect();
  try {
    const response = await client.command('$cmd');
    console.log(JSON.stringify(response));
  } finally {
    client.disconnect();
  }
}).catch((error) => { console.error(error.message); process.exit(1); });
"
}

wait_for_socket() {
  local socket="$1" deadline=$((SECONDS + 30))
  while [[ ! -S "$socket" ]]; do
    if ((SECONDS >= deadline)); then
      echo "fixture socket never appeared: $socket" >&2
      return 1
    fi
    sleep 0.2
  done
}

run_phase() {
  local name="$1" spec="$2" prestart="$3" wdio_port="$4"
  local home server_pid status=0
  home="$(mktemp -d "${TMPDIR:-/tmp}/codegg-e2e-XXXXXX")"
  export CODEGG_E2E_HOME="$home"
  export CODEGG_E2E_SOCKET="$home/fixture.sock"
  export CODEGG_DAEMON_HOME="$home/daemon-home"
  export CODEGG_E2E_WDIO_PORT="$wdio_port"
  echo "==> phase $name: home $home"
  "$fixture_bin" serve >"$home/server.log" 2>&1 &
  server_pid=$!
  wait_for_socket "$CODEGG_E2E_SOCKET"
  if [[ "$prestart" == "start" ]]; then
    echo "==> phase $name: starting isolated daemon"
    fixture_rpc "$CODEGG_E2E_SOCKET" start >"$home/start.json"
  elif [[ "$prestart" == "start-then-stop" ]]; then
    echo "==> phase $name: starting then stopping isolated daemon (warm home, dead daemon)"
    fixture_rpc "$CODEGG_E2E_SOCKET" start >"$home/start.json"
    fixture_rpc "$CODEGG_E2E_SOCKET" stop_daemon >/dev/null
  fi
  export CODEGG_E2E_STATE_FILE="$home/start.json"
  echo "==> phase $name: running $spec"
  cd "$app_root"
  # shellcheck disable=SC2086
  if ! npx wdio run "$app_root/e2e/wdio.conf.ts" --spec "$app_root/e2e/specs/$spec"; then
    echo "phase $name FAILED" >&2
    status=1
  fi
  echo "==> phase $name: shutting down fixture"
  fixture_rpc "$CODEGG_E2E_SOCKET" shutdown >/dev/null 2>&1 || true
  for _ in $(seq 1 50); do
    kill -0 "$server_pid" 2>/dev/null || break
    sleep 0.2
  done
  kill -9 "$server_pid" 2>/dev/null || true
  wait "$server_pid" 2>/dev/null || true
  # Keep the fixture server log and the isolated daemon log beside the
  # runner logs (supplementary artifacts for post-mortems); the phase home
  # itself is always removed.
  mkdir -p "$app_root/e2e/logs"
  cp "$home/server.log" "$app_root/e2e/logs/$name-fixture.log" 2>/dev/null || true
  if [[ -f "$home/daemon-home/daemon.log" ]]; then
    cp "$home/daemon-home/daemon.log" "$app_root/e2e/logs/$name-daemon.log" 2>/dev/null || true
  fi
  # The launcher log is overwritten by every `wdio run` invocation, so keep
  # a per-phase copy beside the per-spec worker logs; without it the earlier
  # phase's spawn-to-ready evidence is lost.
  if [[ -f "$app_root/e2e/logs/wdio.log" ]]; then
    cp "$app_root/e2e/logs/wdio.log" "$app_root/e2e/logs/$name-wdio.log" 2>/dev/null || true
  fi
  rm -rf "$home"
  unset CODEGG_E2E_HOME CODEGG_E2E_SOCKET CODEGG_DAEMON_HOME CODEGG_E2E_STATE_FILE CODEGG_E2E_WDIO_PORT
  return $status
}

overall=0
run_phase lifecycle m003-lifecycle.e2e.ts start 4445 || overall=1
run_phase autostart m003-autostart.e2e.ts start-then-stop 4446 || overall=1
if ((overall != 0)); then
  echo "E2E trajectory FAILED" >&2
fi
exit $overall
