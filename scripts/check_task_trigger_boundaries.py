#!/usr/bin/env python3
"""Static guard: external task-trigger capability boundaries (Project Work Orders M005).

The trigger bearer is a narrow capability, not a principal credential.
This guard pins the structural properties that keep it narrow:

- distinct bearer prefix (`cggtr_`) that never collides with the
  personal-token prefix (`cggt_`);
- verifier-only persistence (no plaintext/verifier-echo columns, Debug
  omits the verifier);
- POST-only fire route (no GET registration, no query extraction);
- no Authorization/secret/bearer/verifier content in log/event lines;
- the fire bearer never enters principal resolution;
- firing is NOT a Core operation (no `WorkOrderTriggerFire` request
  variant), so trigger capabilities can never authorize general Core APIs;
- trigger list/get/event/audit shapes carry no secret or verifier;
- storage layout tracks the M005 migration.

Run:

  python3 scripts/check_task_trigger_boundaries.py

Exit code 0 on success, 1 on failure.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TRIGGER_CORE = ROOT / "crates" / "codegg-core" / "src" / "work_order" / "trigger.rs"
STORE_CORE = ROOT / "crates" / "codegg-core" / "src" / "work_order" / "store.rs"
IDENTITY = ROOT / "crates" / "codegg-core" / "src" / "identity.rs"
TRANSPORT_AUTH = ROOT / "crates" / "codegg-core" / "src" / "transport_auth.rs"
PROTOCOL_WORK_ORDER = ROOT / "crates" / "codegg-protocol" / "src" / "work_order.rs"
PROTOCOL_CORE = ROOT / "crates" / "codegg-protocol" / "src" / "core.rs"
SCHEMA = ROOT / "crates" / "codegg-core" / "src" / "session" / "schema.rs"
STORAGE_MOD = ROOT / "crates" / "codegg-core" / "src" / "storage" / "mod.rs"
DAEMON_TRIGGERS = ROOT / "src" / "core" / "daemon_work_orders.rs"
FIRE_ROUTE = ROOT / "src" / "server" / "routes" / "task_trigger.rs"
HTTP_SERVER = ROOT / "src" / "server" / "http.rs"
SAFE_PUBLICATION = (
    ROOT / "crates" / "codegg-core" / "src" / "projection_replay" / "safe_publication.rs"
)


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _production(source: str) -> str:
    """Strip trailing `#[cfg(test)]` test modules (test code may name secrets)."""
    marker = "#[cfg(test)]"
    idx = source.find(marker)
    return source[:idx] if idx != -1 else source


def _code_lines(source: str) -> list[tuple[int, str]]:
    """Non-comment source lines with 1-based numbers (doc prose may name concepts)."""
    out = []
    for lineno, line in enumerate(source.splitlines(), start=1):
        if line.strip().startswith("//"):
            continue
        out.append((lineno, line))
    return out


def check_prefix_separation() -> list[str]:
    failures = []
    trigger = _read(TRIGGER_CORE)
    auth = _read(TRANSPORT_AUTH)
    if 'TASK_TRIGGER_PREFIX: &str = "cggtr_"' not in trigger:
        failures.append("trigger.rs: TASK_TRIGGER_PREFIX must be `cggtr_`")
    if 'PERSONAL_TOKEN_PREFIX: &str = "cggt_"' not in auth:
        failures.append("transport_auth.rs: PERSONAL_TOKEN_PREFIX must stay `cggt_`")
    # The trigger bearer must never match the personal-token path.
    if "is_personal_token_presentation" not in trigger:
        failures.append("trigger.rs: must pin non-collision with personal-token routing")
    identity = _read(IDENTITY)
    if "TaskTriggerId" not in identity:
        failures.append("identity.rs: TaskTriggerId typed identity is missing")
    return failures


def check_verifier_only_storage() -> list[str]:
    failures = []
    store = _production(_read(STORE_CORE))
    if "secret_verifier" not in store:
        failures.append("store.rs: trigger rows must persist a `secret_verifier`")
    for forbidden in ("secret_plaintext", "plaintext_secret TEXT", "secret_secret"):
        if forbidden in store:
            failures.append(f"store.rs: forbidden plaintext column marker `{forbidden}`")
    trigger = _production(_read(TRIGGER_CORE))
    if "finish_non_exhaustive" not in trigger:
        failures.append("trigger.rs: TaskTrigger Debug impl must omit the verifier")
    if "secret_verifier_hex" in trigger and ".secret_verifier_hex" not in _read(STORE_CORE):
        failures.append("trigger.rs/store.rs: verifier field drift")
    return failures


def check_post_only_no_query() -> list[str]:
    failures = []
    route = "\n".join(
        line for _, line in _code_lines(_production(_read(FIRE_ROUTE)))
    )
    if "post(fire_task_trigger)" not in route:
        failures.append("task_trigger.rs: fire route must register POST")
    if re.search(r'"/api/v1/task-triggers[^"]*"\s*,\s*get\s*\(', route):
        failures.append("task_trigger.rs: fire path must never register GET")
    if "Query<" in route:
        failures.append("task_trigger.rs: fire handler must never read query strings")
    if "cookie" in route.lower():
        failures.append("task_trigger.rs: fire handler must never consult cookies")
    if "Idempotency-Key" not in route and "idempotency-key" not in route:
        failures.append("task_trigger.rs: fire handler must honor Idempotency-Key")
    return failures


def check_no_secret_in_logs() -> list[str]:
    failures = []
    for path in (FIRE_ROUTE, TRIGGER_CORE, DAEMON_TRIGGERS):
        source = _production(_read(path))
        for lineno, line in _code_lines(source):
            if re.search(r"\btracing::(info|warn|error|debug)", line) or "eprintln!" in line:
                lowered = line.lower()
                for marker in ("presented", "secret", "bearer", "verifier", "plaintext"):
                    # `secret_free`, `verifier-only`, and redaction helpers
                    # are descriptive, not disclosures.
                    if marker in lowered and "redact" not in lowered:
                        failures.append(
                            f"{path.relative_to(ROOT)}:{lineno}: log line mentions `{marker}`"
                        )
    return failures


def check_no_principal_resolution() -> list[str]:
    failures = []
    route = "\n".join(
        line for _, line in _code_lines(_production(_read(FIRE_ROUTE)))
    )
    for forbidden in (
        "verify_for_client",
        "verify_personal_token",
        "AuthenticatedPrincipal",
        "resolve_bearer_principal",
        "auth_middleware",
    ):
        if forbidden in route:
            failures.append(
                f"task_trigger.rs: fire path must not touch principal auth (`{forbidden}`)"
            )
    daemon = _production(_read(DAEMON_TRIGGERS))
    fire_fn = daemon[daemon.find("pub async fn fire_work_order_trigger") :]
    for forbidden in ("verify_for_client", "request_authority_for_client", "register_with_principal"):
        if forbidden in fire_fn:
            failures.append(
                f"daemon_work_orders.rs fire path must not resolve principals (`{forbidden}`)"
            )
    return failures


def check_fire_is_not_a_core_operation() -> list[str]:
    failures = []
    protocol = _read(PROTOCOL_CORE)
    if "TriggerFire" in protocol:
        failures.append("protocol/core.rs: firing must NOT be a CoreRequest variant")
    for required in (
        "WorkOrderTriggerCreate",
        "WorkOrderTriggerList",
        "WorkOrderTriggerGet",
        "WorkOrderTriggerRevoke",
    ):
        if required not in protocol:
            failures.append(f"protocol/core.rs: management variant `{required}` is missing")
    if "WorkOrderTriggerChanged" not in protocol:
        failures.append("protocol/core.rs: WorkOrderTriggerChanged event is missing")
    if "WorkOrderTriggerChanged" not in _read(SAFE_PUBLICATION):
        failures.append("safe_publication.rs: trigger event must be classified Safe")
    return failures


def check_shapes_carry_no_secret() -> list[str]:
    failures = []
    protocol = _read(PROTOCOL_WORK_ORDER)
    list_struct = protocol[protocol.find("pub struct TaskTriggerMetadataDto") :]
    list_struct = list_struct[: list_struct.find("\n}\n") + 3]
    for forbidden in ("secret", "verifier"):
        if forbidden in list_struct.lower():
            failures.append(
                f"work_order.rs TaskTriggerMetadataDto mentions `{forbidden}`"
            )
    fire_struct = protocol[protocol.find("pub struct TaskTriggerFireResultDto") :]
    fire_struct = fire_struct[: fire_struct.find("\n}\n") + 3]
    for forbidden in ("secret", "verifier", "project", "session"):
        if forbidden in fire_struct.lower():
            failures.append(
                f"work_order.rs TaskTriggerFireResultDto mentions `{forbidden}`"
            )
    return failures


def check_migration_and_layout() -> list[str]:
    failures = []
    schema = _read(SCHEMA)
    if "migrate_v63" not in schema:
        failures.append("schema.rs: migrate_v63 (task triggers) is missing")
    if "TASK_TRIGGER_SCHEMA_STATEMENTS" not in schema:
        failures.append("schema.rs: migrate_v63 must apply TASK_TRIGGER_SCHEMA_STATEMENTS")
    storage = _read(STORAGE_MOD)
    if "STORAGE_LAYOUT_VERSION: u32 = 63" not in storage:
        failures.append("storage/mod.rs: STORAGE_LAYOUT_VERSION must be 63")
    return failures


def check_server_wiring() -> list[str]:
    failures = []
    http = _read(HTTP_SERVER)
    if "task_trigger_router" not in http:
        failures.append("http.rs: trigger router must be merged into the server app")
    if "into_make_service_with_connect_info" not in http:
        failures.append(
            "http.rs: server must serve with connect info (rate limiter extracts ConnectInfo)"
        )
    route = _read(FIRE_ROUTE)
    if "RequestBodyLimitLayer" not in route:
        failures.append("task_trigger.rs: fire route must bound the request body")
    return failures


def main() -> int:
    checks = [
        check_prefix_separation,
        check_verifier_only_storage,
        check_post_only_no_query,
        check_no_secret_in_logs,
        check_no_principal_resolution,
        check_fire_is_not_a_core_operation,
        check_shapes_carry_no_secret,
        check_migration_and_layout,
        check_server_wiring,
    ]
    failures: list[str] = []
    for check in checks:
        try:
            failures.extend(check())
        except OSError as error:
            failures.append(f"{check.__name__}: could not read source: {error}")
    if failures:
        print("task-trigger boundary guard failed:")
        for line in failures:
            print(f"  {line}")
        return 1
    print("task-trigger boundary guard ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
