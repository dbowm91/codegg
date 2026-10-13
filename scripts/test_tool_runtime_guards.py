#!/usr/bin/env python3
"""Offline positive and negative fixtures for tool-runtime source guards."""
from __future__ import annotations

import contextlib
import importlib.util
import io
import shutil
import tempfile
from pathlib import Path
from types import ModuleType

ROOT = Path(__file__).resolve().parent.parent


def load_guard(name: str) -> ModuleType:
    path = ROOT / "scripts" / f"{name}.py"
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def run_fixture(name: str, files: tuple[str, ...], mutation: tuple[str, str, str]) -> None:
    module = load_guard(name)
    with tempfile.TemporaryDirectory(prefix=f"{name}-") as temp_dir:
        fixture_root = Path(temp_dir)
        source_root = module.ROOT
        for rel in files:
            source = ROOT / rel
            destination = fixture_root / rel
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, destination)
        for key, value in tuple(vars(module).items()):
            if isinstance(value, Path):
                try:
                    relative = value.relative_to(source_root)
                except ValueError:
                    continue
                setattr(module, key, fixture_root / relative)

        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            baseline = module.main()
        if baseline != 0:
            raise AssertionError(f"{name}: valid source fixture rejected:\n{output.getvalue()}")

        rel, old, new = mutation
        target = fixture_root / rel
        text = target.read_text()
        if old not in text:
            raise AssertionError(f"{name}: negative fixture anchor missing in {rel}: {old!r}")
        target.write_text(text.replace(old, new))
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            mutated = module.main()
        if mutated == 0:
            raise AssertionError(f"{name}: forbidden mutation passed guard:\n{output.getvalue()}")
        print(f"OK: {name} accepts valid source and rejects its forbidden mutation.")


def main() -> int:
    try:
        run_fixture(
            "check_mcp_broker_adapter",
            ("src/agent/tool_batch.rs", "src/tool/mcp_adapter.rs"),
            ("src/agent/tool_batch.rs", ".execute(registry, &tool_name_clone", ".call_tool(registry"),
        )
        run_fixture(
            "check_tool_descriptor_snapshot",
            (
                "src/agent/request_preparation.rs",
                "src/agent/tool_descriptor_snapshot.rs",
                "src/agent/coordinator.rs",
            ),
            ("src/agent/request_preparation.rs", "snapshot_fingerprint", "descriptor_digest"),
        )
        run_fixture(
            "check_descendant_tool_ceiling",
            (
                "src/agent/worker.rs",
                "src/tool/task.rs",
                "src/agent/tool_batch.rs",
                "src/tool/backend.rs",
                "src/agent/tool_surface.rs",
                "crates/codegg-core/src/jobs/mod.rs",
                "src/scheduler/executors.rs",
                "src/job_dispatcher.rs",
                "src/tool/broker.rs",
                "src/agent/parent_authority.rs",
            ),
            ("src/tool/broker.rs", "parent.check_tool(", "parent.unchecked_tool("),
        )
        run_fixture(
            "check_tool_disclosure_wire",
            (
                "crates/codegg-providers/src/provider_core.rs",
                "crates/codegg-providers/src/wire.rs",
                "crates/codegg-providers/src/responses_api.rs",
                "src/agent/request_preparation.rs",
            ),
            ("crates/codegg-providers/src/provider_core.rs", "fn qualify_tool_disclosure", "fn missing_tool_disclosure"),
        )
        run_fixture(
            "check_tool_retry_admission",
            ("src/agent/tool_batch.rs",),
            ("src/agent/tool_batch.rs", ".execute(registry, &tool_name_clone", ".execute_with_retry(registry, &tool_name_clone"),
        )
    except (AssertionError, OSError, RuntimeError) as error:
        print(f"ERROR: tool-runtime guard self-test failed: {error}")
        return 1
    print("OK: all tool-runtime guards reject their negative fixtures.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
