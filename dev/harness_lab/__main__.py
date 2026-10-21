from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import sys
import time
from typing import Optional

from .supervisor import HarnessError, run_exec_trial, run_smoke, run_tool_trial
from . import budget
from . import ledger
from . import improve
from . import pilot as pilot_module
from . import sustained as sustained_module


def _sustained_plans(trials: int) -> list:
    """Build the trial plan for a sustained scenario.

    The sequence alternates the two frozen tasks and injects the two
    interruption kinds at fixed positions, so the scenario always covers a
    normal trial, a cancelled trial, and an abruptly dropped ACP connection.
    """
    tasks = list(pilot_module.FROZEN_TASKS)
    plans = []
    for index in range(trials):
        task = tasks[index % len(tasks)]
        fault = None
        if index == 3:
            fault = "cancel"
        elif index == 7:
            fault = "abrupt_acp_drop"
        elif index == 11:
            fault = "cancel"
        elif index == 15:
            fault = "abrupt_acp_drop"
        plans.append(sustained_module.TrialPlan(
            trial_id=f"trial-{index:02d}-{task}-{fault or 'clean'}",
            task=task, fault=fault))
    return plans


def _add_budget_arguments(parser: argparse.ArgumentParser) -> None:
    """Attach the budget admission gate to a command that can reach a provider.

    The gate is fail-closed: a live trial must present a budget policy whose
    enforceable cap the controller verified, and the trial is refused before
    anything is spawned when it cannot.
    """
    parser.add_argument("--budget", type=Path,
                        help="JSON budget policy document for the trial's enforceable caps")
    parser.add_argument("--live-provider", action="store_true",
                        help="declare that this trial reaches a live provider; requires --budget and a verified cap")


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="python -m dev.harness_lab")
    commands = parser.add_subparsers(dest="command", required=True)
    doctor = commands.add_parser("doctor", help="check local harness prerequisites")
    doctor.add_argument("--binary", type=Path, default=os.environ.get("CODEGG_HARNESS_BINARY"))
    smoke = commands.add_parser("smoke", help="prove two isolated native ACP lanes")
    smoke.add_argument("--binary", type=Path, default=os.environ.get("CODEGG_HARNESS_BINARY"))
    smoke.add_argument("--subjects", type=int, default=2)
    smoke.add_argument("--timeout", type=float, default=20.0)
    smoke.add_argument("--output-dir", type=Path)
    smoke.add_argument("--unattended-profile", choices=("workspace-write", "full-host"))
    tool = commands.add_parser("tool-trial", help="run a local fixture provider through the native ACP tool broker")
    tool.add_argument("--binary", type=Path, default=os.environ.get("CODEGG_HARNESS_BINARY"))
    tool.add_argument("--profile", choices=("workspace-write", "full-host"), required=True)
    tool.add_argument("--timeout", type=float, default=45.0)
    tool.add_argument("--deny-bash", action="store_true", help="prove explicit bash deny remains effective")
    _add_budget_arguments(tool)
    evaluate = commands.add_parser("evaluate", help="run and independently verify the deterministic native tool fixture")
    evaluate.add_argument("--binary", type=Path, default=os.environ.get("CODEGG_HARNESS_BINARY"))
    evaluate.add_argument("--ledger", type=Path, required=True)
    evaluate.add_argument("--profile", choices=("workspace-write", "full-host"), default="workspace-write")
    evaluate.add_argument("--candidate", default="current")
    evaluate.add_argument("--task-id", default="safe-bash-write")
    evaluate.add_argument("--group-id", default="native-tool-smoke")
    evaluate.add_argument("--suite", type=Path, help="versioned JSONL case manifest (one task per line)")
    evaluate.add_argument("--timeout", type=float, default=60.0)
    evaluate.add_argument("--max-wallclock", type=float, default=600.0)
    evaluate.add_argument("--max-trials", type=int, default=100)
    evaluate.add_argument("--deny-bash", action="store_true")
    evaluate.add_argument("--resume-candidate", action="store_true",
                          help="resume missing task/repeat pairs for an interrupted candidate")
    _add_budget_arguments(evaluate)
    report = commands.add_parser("report", help="summarize a frozen ledger without provider calls")
    report.add_argument("--ledger", type=Path, required=True)
    replay = commands.add_parser("replay", help="re-run a deterministic verifier from captured artifacts")
    replay.add_argument("--ledger", type=Path, required=True)
    replay.add_argument("--run-id", required=True)
    compare = commands.add_parser("compare", help="compare candidates on paired task IDs")
    compare.add_argument("--ledger", type=Path, required=True)
    compare.add_argument("--left", required=True)
    compare.add_argument("--right", required=True)
    improve_cmd = commands.add_parser("improve", help="run a bounded manual dry-run or one scoped OpenCode proposal")
    improve_cmd.add_argument("--proposer", choices=("manual", "opencode"), required=True)
    improve_cmd.add_argument("--max-iterations", type=int, default=1)
    improve_cmd.add_argument("--max-candidate-retries", type=int, default=0)
    improve_cmd.add_argument("--dry-run", action="store_true")
    improve_cmd.add_argument("--checkpoint", type=Path, default=Path("/tmp/codegg-harness-improve.json"))
    improve_cmd.add_argument("--worktree", type=Path)
    improve_cmd.add_argument("--prompt")
    improve_cmd.add_argument("--allow-prefix", action="append", default=[])
    improve_cmd.add_argument("--timeout", type=float, default=300.0)
    improve_cmd.add_argument("--model", default=None,
                             help="pin the OpenCode proposer to one approved provider/model route")
    _add_budget_arguments(improve_cmd)
    optimize = commands.add_parser("optimize", help="run a bounded multi-iteration assess/correct/retest controller")
    optimize.add_argument("--repository", type=Path, default=Path.cwd())
    optimize.add_argument("--suite", type=Path, required=True)
    optimize.add_argument("--ledger", type=Path, required=True)
    optimize.add_argument("--baseline", required=True)
    optimize.add_argument("--candidate-root", type=Path, required=True)
    optimize.add_argument("--target-root", type=Path, required=True)
    optimize.add_argument("--checkpoint", type=Path, required=True)
    optimize.add_argument("--allow-prefix", action="append", required=True)
    optimize.add_argument("--prompt", required=True)
    optimize.add_argument("--max-iterations", type=int, default=3)
    optimize.add_argument("--max-candidate-retries", type=int, default=0)
    optimize.add_argument("--max-wallclock", type=float, default=3600.0)
    optimize.add_argument("--timeout", type=float, default=900.0)
    optimize.add_argument("--resume", action="store_true")
    optimize.add_argument("--model", default=None,
                          help="pin the OpenCode proposer to one approved provider/model route")
    _add_budget_arguments(optimize)
    qualify = commands.add_parser("qualify", help="build, test, and paired-ACP-evaluate a scoped candidate")
    qualify.add_argument("--worktree", type=Path, required=True)
    qualify.add_argument("--suite", type=Path, required=True)
    qualify.add_argument("--ledger", type=Path, required=True)
    qualify.add_argument("--baseline", required=True)
    qualify.add_argument("--candidate", required=True)
    qualify.add_argument("--target-dir", type=Path, required=True)
    qualify.add_argument("--allow-prefix", action="append", default=[])
    qualify.add_argument("--proposer", choices=("manual", "opencode"), default="manual")
    qualify.add_argument("--prompt")
    qualify.add_argument("--timeout", type=float, default=900.0)
    qualify.add_argument("--max-wallclock", type=float, default=3600.0)
    qualify.add_argument("--checkpoint", type=Path, default=Path("/tmp/codegg-harness-qualification.json"))
    qualify.add_argument("--resume", action="store_true")
    qualify.add_argument("--max-candidate-retries", type=int, default=0,
                         help="bounded OpenCode compile-correction retries; zero disables retries")
    qualify.add_argument("--model", default=None,
                         help="pin the OpenCode proposer to one approved provider/model route")
    _add_budget_arguments(qualify)
    budget_check = commands.add_parser(
        "budget-check",
        help="report the budget admission decision for a trial policy; provider-free")
    budget_check.add_argument("--budget", type=Path, required=True,
                              help="JSON budget policy document for the trial's enforceable caps")
    candidate_tree = commands.add_parser("candidate-worktree", help="create a new isolated local candidate branch/worktree")
    candidate_tree.add_argument("--repository", type=Path, default=Path.cwd())
    candidate_tree.add_argument("--branch", required=True)
    candidate_tree.add_argument("--destination", type=Path, required=True)
    candidate_tree.add_argument("--base-revision", required=True)
    pilot = commands.add_parser(
        "pilot",
        help="run one frozen task fixture against a live approved route and record a receipt")
    pilot.add_argument("--task", required=True, choices=list(pilot_module.FROZEN_TASKS),
                       help="frozen pilot task id")
    pilot.add_argument("--model", required=True,
                       help="provider/model composite, for example opencode_go/muse-spark-1.3-contributor")
    pilot.add_argument("--binary", type=Path, default=None,
                       help="CodeGG binary under test; defaults to the debug build")
    pilot.add_argument("--profile", default="workspace-write",
                       choices=["workspace-write", "full-host"])
    pilot.add_argument("--timeout", type=float, default=pilot_module.DEFAULT_TIMEOUT_SECONDS)
    pilot.add_argument("--output", type=Path, default=None,
                       help="where to write the receipt JSON; stdout when absent")
    pilot.add_argument("--lane-root", type=Path, default=None,
                       help="private lane directory; a temporary lane when absent")
    _add_budget_arguments(pilot)
    sustained = commands.add_parser(
        "sustained",
        help="run an unattended sequence of live trials with bounded resource limits")
    sustained.add_argument("--model", required=True,
                           help="provider/model composite for every trial in the scenario")
    sustained.add_argument("--binary", type=Path, default=None)
    sustained.add_argument("--profile", default="workspace-write",
                           choices=["workspace-write", "full-host"])
    sustained.add_argument("--trials", type=int, default=24,
                           help="number of live trials in the scenario")
    sustained.add_argument("--per-trial-wallclock", type=float, default=600.0)
    sustained.add_argument("--max-provider-requests", type=int, default=None,
                           help="scenario-level provider-request ceiling, independent of the policy")
    sustained.add_argument("--output", type=Path, default=None)
    sustained.add_argument("--lane-root", type=Path, default=None)
    _add_budget_arguments(sustained)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        # The budget admission gate runs before anything else so a live trial
        # that cannot prove an enforceable cap never reaches a provider, a
        # daemon, or a subprocess.
        admitted_budget = _require_budget_admission(args)
        spend_guard = None if admitted_budget is None else budget.SpendGuard(admitted_budget)
        if args.command == "doctor":
            binary = _binary(args.binary)
            print(json.dumps({"ok": True, "binary": str(binary), "sha256": _sha256(binary)}, sort_keys=True))
            return 0
        if args.command == "report":
            print(json.dumps(ledger.report(args.ledger), sort_keys=True))
            return 0
        if args.command == "replay":
            print(json.dumps(ledger.replay(args.ledger, args.run_id), sort_keys=True))
            return 0
        if args.command == "compare":
            print(json.dumps(ledger.compare(args.ledger, args.left, args.right), sort_keys=True))
            return 0
        if args.command == "improve":
            if args.proposer == "manual" and args.dry_run:
                result = improve.manual_dry_run(args.checkpoint, args.max_iterations, args.max_candidate_retries)
            elif args.proposer == "opencode" and args.worktree and args.prompt and args.allow_prefix:
                result = improve.opencode_propose(args.worktree, args.prompt, tuple(args.allow_prefix),
                                                 args.timeout, model=args.model)
            else:
                raise HarnessError("manual mode requires --dry-run; OpenCode mode requires --worktree, --prompt, and at least one --allow-prefix")
            print(json.dumps(result, sort_keys=True))
            return 0 if result.get("status") != "BLOCKED" else 2
        if args.command == "sustained":
            if not getattr(args, "live_provider", False):
                raise HarnessError(
                    "the sustained command always reaches a live provider; pass --live-provider "
                    "together with an admitted --budget policy")
            policy = _budget_policy(getattr(args, "budget", None))
            plans = _sustained_plans(args.trials)
            receipt = sustained_module.run_sustained_scenario(
                _binary(args.binary), args.model, budget_policy=policy, plans=plans,
                profile=args.profile, root=args.lane_root,
                per_trial_wallclock=args.per_trial_wallclock,
                max_provider_requests=args.max_provider_requests)
            rendered = json.dumps(receipt, indent=2, sort_keys=True)
            if args.output is not None:
                args.output.parent.mkdir(parents=True, exist_ok=True)
                args.output.write_text(rendered, encoding="utf-8")
                print(json.dumps({"attempted": receipt["attempted_trials"],
                                  "unattempted": receipt["unattempted_trials"],
                                  "solved": receipt["solved_trials"],
                                  "stopped": receipt["stopped_trials"],
                                  "elapsed_ms": receipt["elapsed_ms"],
                                  "stopped_reason": receipt["stopped_reason"],
                                  "output": str(args.output)}, sort_keys=True))
            else:
                print(rendered)
            return 0 if receipt["unattempted_trials"] == 0 else 2
        if args.command == "budget-check":
            admission = _budget_admission(args.budget)
            print(json.dumps(admission.as_dict(), sort_keys=True))
            return 0 if admission.status == budget.STATUS_ADMITTED else 2
        if args.command == "pilot":
            # The pilot always reaches a live provider, so the declaration is
            # not optional: a run that did not name a live provider and an
            # admitted budget never starts.
            if not getattr(args, "live_provider", False):
                raise HarnessError(
                    "the pilot command always reaches a live provider; pass --live-provider "
                    "together with an admitted --budget policy")
            policy = _budget_policy(getattr(args, "budget", None))
            receipt = pilot_module.run_live_task(
                _binary(args.binary), args.task, args.model, budget_policy=policy,
                profile=args.profile, root=args.lane_root, timeout=args.timeout)
            rendered = json.dumps(receipt, indent=2, sort_keys=True)
            if args.output is not None:
                args.output.parent.mkdir(parents=True, exist_ok=True)
                args.output.write_text(rendered, encoding="utf-8")
                print(json.dumps({"ok": receipt["ok"], "output": str(args.output),
                                  "task_id": receipt["task_id"], "model": receipt["model"],
                                  "terminal": receipt["terminal"]}, sort_keys=True))
            else:
                print(rendered)
            return 0 if receipt["ok"] else 2
        if args.command == "optimize":
            result = improve.run_optimizer(
                args.repository, args.suite, args.ledger,
                baseline_candidate=args.baseline, candidate_root=args.candidate_root,
                target_root=args.target_root, checkpoint_path=args.checkpoint,
                allowed_prefixes=tuple(args.allow_prefix), prompt=args.prompt,
                max_iterations=args.max_iterations,
                max_candidate_retries=args.max_candidate_retries,
                max_wallclock=args.max_wallclock, timeout=args.timeout,
                resume=args.resume, model=args.model,
                budget_policy=_budget_policy(getattr(args, "budget", None)))
            print(json.dumps(result, sort_keys=True))
            return 0 if result.get("status") == "ACCEPTED" else 2
        if args.command == "qualify":
            if not args.allow_prefix or args.timeout <= 0 or args.max_wallclock <= 0 or args.max_candidate_retries < 0:
                raise HarnessError("qualification requires an allowlist and positive time budgets")
            if args.max_candidate_retries and args.proposer != "opencode":
                raise HarnessError("compile-correction retries require --proposer opencode")
            suite = args.suite.expanduser().resolve(strict=True)
            baseline_rows = [row for row in ledger.load_runs(args.ledger.expanduser().resolve(strict=True))
                             if row["candidate"] == args.baseline]
            if not baseline_rows or not baseline_rows[0].get("subject", {}).get("revision"):
                raise HarnessError("qualification baseline lacks a source revision receipt")
            suite_digest = __import__("hashlib").sha256(suite.read_bytes()).hexdigest()
            if args.resume:
                checkpoint = improve.Checkpoint.resume(args.checkpoint, baseline_rows[0]["subject"]["revision"], suite_digest)
            else:
                checkpoint = improve.Checkpoint(args.checkpoint, baseline_rows[0]["subject"]["revision"], suite_digest)
                checkpoint.transition("DIAGNOSED", {"baseline": args.baseline, "candidate": args.candidate,
                                                      "suite_sha256": suite_digest})
                checkpoint.transition("PROPOSING", {"proposer": args.proposer})
            try:
                worktree = args.worktree.expanduser().resolve(strict=True)
                if args.resume:
                    if checkpoint.state not in {"PATCHED", "BUILDING", "QUALIFYING", "EVALUATING", "COMPARING"}:
                        raise HarnessError("checkpoint has no safely resumable candidate patch")
                elif args.proposer == "opencode":
                    if not args.prompt or len(args.prompt.encode()) > 8192:
                        raise HarnessError("OpenCode qualification requires a non-empty prompt of at most 8 KiB")
                    proposal = improve.opencode_propose(worktree, args.prompt, tuple(args.allow_prefix),
                                                       args.timeout, model=args.model)
                    paths = proposal["changed_paths"]
                    patch_digest = proposal["patch_sha256"]
                else:
                    paths = improve.changed_paths(worktree)
                    patch_digest = improve.candidate_patch_sha256(worktree, paths)
                if not args.resume:
                    checkpoint.transition("PATCHED", {"changed_paths": paths, "patch_sha256": patch_digest,
                                                       "proposer_receipt": proposal if args.proposer == "opencode" else "manual"})
                result = improve.qualify_candidate(
                    worktree, suite, args.ledger, baseline_candidate=args.baseline,
                    candidate=args.candidate, target_dir=args.target_dir,
                    allowed_prefixes=tuple(args.allow_prefix), timeout=args.timeout,
                    max_wallclock=args.max_wallclock, checkpoint=checkpoint, resume=args.resume,
                    proposer=args.proposer, correction_prompt=args.prompt or "",
                    max_candidate_retries=args.max_candidate_retries, model=args.model,
                    budget_policy=_budget_policy(getattr(args, "budget", None)))
            except (ValueError, RuntimeError, TimeoutError, OSError) as error:
                if checkpoint.state not in improve.TERMINAL and "REJECTED" in improve.ALLOWED[checkpoint.state]:
                    checkpoint.transition("REJECTED", {"failure_class": type(error).__name__})
                raise HarnessError(str(error)) from error
            result["checkpoint"] = str(args.checkpoint)
            print(json.dumps(result, sort_keys=True))
            return 0 if result.get("status") == "ACCEPTED" else 2
        if args.command == "candidate-worktree":
            result = improve.create_candidate_worktree(args.repository, args.branch,
                                                        args.destination, args.base_revision)
            print(json.dumps(result, sort_keys=True))
            return 0
        binary = _binary(args.binary)
        if args.command == "smoke":
            if args.subjects < 2:
                raise HarnessError("smoke requires at least two subjects")
            result = run_smoke(binary, args.subjects, args.timeout, args.output_dir, args.unattended_profile)
        elif args.command == "evaluate":
            if args.suite:
                cases = [json.loads(line) for line in args.suite.read_text(encoding="utf-8").splitlines() if line.strip()]
                if not cases or len(cases) > 100 or any(not isinstance(case, dict) or case.get("schema_version") != 1 for case in cases):
                    raise HarnessError("suite must contain 1-100 schema_version=1 JSONL cases")
                if len({case.get("task_id") for case in cases}) != len(cases):
                    raise HarnessError("suite task_id values must be unique")
                if args.max_wallclock <= 0 or args.timeout <= 0:
                    raise HarnessError("evaluation time budgets must be positive")
                suite_sha256 = __import__("hashlib").sha256(args.suite.read_bytes()).hexdigest()
                expanded_files_by_task = {}
                for case in cases:
                    if not isinstance(case.get("task_id"), str) or not case["task_id"] or not isinstance(case.get("group_id", args.group_id), str):
                        raise HarnessError("suite cases require non-empty task_id and string group_id")
                    if case.get("profile", args.profile) not in {"workspace-write", "full-host"} or not isinstance(case.get("deny_bash", args.deny_bash), bool):
                        raise HarnessError("suite case profile or deny_bash has an unsupported shape")
                    if not isinstance(case.get("content", "harness-lab-tool-ok"), str) or len(case.get("content", "harness-lab-tool-ok").encode()) > 4096:
                        raise HarnessError("suite case content must be a bounded string")
                    if case.get("context_text") is not None and (not isinstance(case["context_text"], str)
                                                                  or not case["context_text"]
                                                                  or len(case["context_text"].encode()) > 65536):
                        raise HarnessError("suite context_text must be a bounded non-empty string")
                    sequence = case.get("tool_sequence")
                    if sequence is not None and (not isinstance(sequence, list) or not 1 <= len(sequence) <= 8 or any(
                            not isinstance(item, dict) or not isinstance(item.get("name"), str)
                            or not isinstance(item.get("arguments"), dict) for item in sequence)):
                        raise HarnessError("suite tool_sequence must contain 1-8 named tool calls with object arguments")
                    initial_files = case.get("initial_files", {})
                    if not isinstance(initial_files, dict) or any(not isinstance(path, str) for path in initial_files):
                        raise HarnessError("suite initial_files must map relative paths to text fixtures")
                    expanded_initial_files = {}
                    for path, value in initial_files.items():
                        if isinstance(value, str) and len(value.encode()) <= 65536:
                            expanded_initial_files[path] = value
                        elif isinstance(value, dict) and set(value) == {"repeat", "count", "suffix"}:
                            repeat_text, count, suffix = value["repeat"], value["count"], value["suffix"]
                            if (not isinstance(repeat_text, str) or not isinstance(suffix, str)
                                    or not isinstance(count, int) or isinstance(count, bool) or not 1 <= count <= 10000
                                    or (len(repeat_text.encode()) * count + len(suffix.encode())) > 65536):
                                raise HarnessError("suite repeated initial file is outside its bounded shape")
                            expanded_initial_files[path] = repeat_text * count + suffix
                        else:
                            raise HarnessError("suite initial_files must map paths to bounded text or repeat descriptors")
                    expanded_files_by_task[case["task_id"]] = expanded_initial_files
                    if not isinstance(case.get("expected_file", "tool-receipt.txt"), str):
                        raise HarnessError("suite expected_file must be a relative path")
                    if case.get("transport", "acp") not in {"acp", "exec"}:
                        raise HarnessError("suite case transport must be acp or exec")
                    if not isinstance(case.get("expected_exists", not case.get("deny_bash", args.deny_bash)), bool):
                        raise HarnessError("suite expected_exists must be boolean")
                    if not isinstance(case.get("cancel", False), bool):
                        raise HarnessError("suite cancel must be boolean")
                    if case.get("expected_terminal", "passed") not in {"passed", "assertion_failed", "cancelled", "timeout", "execution_failed"}:
                        raise HarnessError("suite expected_terminal is unsupported")
                    if case.get("cancel", False) != (case.get("expected_terminal") == "cancelled"):
                        raise HarnessError("cancel cases must expect the cancelled terminal")
                    if not isinstance(case.get("timeout_task", False), bool) or case.get("timeout_task", False) != (case.get("expected_terminal") == "timeout"):
                        raise HarnessError("timeout cases must declare timeout_task and expect timeout")
                    case_timeout = case.get("timeout_seconds", args.timeout)
                    if not isinstance(case_timeout, (int, float)) or isinstance(case_timeout, bool) or case_timeout <= 0 or case_timeout > args.timeout:
                        raise HarnessError("suite timeout_seconds must be positive and no greater than --timeout")
                    expected_failures = case.get("expected_tool_failures", 1 if case.get("deny_bash", args.deny_bash) else 0)
                    call_count = len(sequence) if sequence is not None else 1
                    if not isinstance(expected_failures, int) or isinstance(expected_failures, bool) or not 0 <= expected_failures <= call_count:
                        raise HarnessError("suite expected_tool_failures must be within the tool call count")
                    repeats = case.get("repeats", 1)
                    if not isinstance(repeats, int) or isinstance(repeats, bool) or not 1 <= repeats <= 10:
                        raise HarnessError("suite case repeats must be between 1 and 10")
                total_trials = sum(case.get("repeats", 1) for case in cases)
                if not 1 <= args.max_trials <= 100 or total_trials > args.max_trials:
                    raise HarnessError("suite trial count exceeds --max-trials")
                result = {"ok": True, "suite": str(args.suite), "trials": []}
                started = time.monotonic()
                exhausted = False
                existing = {}
                if args.resume_candidate:
                    existing = ledger.validate_candidate_resume_rows(
                        args.ledger.expanduser().resolve(strict=True), args.candidate,
                        cases, suite_sha256, args.group_id)
                    for case in cases:
                        for repeat in range(case.get("repeats", 1)):
                            row = existing.get((case["task_id"], repeat))
                            if row is None:
                                continue
                            expected_terminal = case.get("expected_terminal", "passed")
                            matched = row["terminal"] == expected_terminal
                            result["trials"].append({"task_id": case["task_id"], "repeat": repeat,
                                                     "ledger_manifest": row,
                                                     "expected_terminal_matched": matched,
                                                     "resumed": True})
                            if not matched or row["terminal"] != "passed":
                                result["ok"] = False
                for case in cases:
                    case_sha256 = ledger.canonical_digest(case)
                    for repeat in range(case.get("repeats", 1)):
                        if (case["task_id"], repeat) in existing:
                            continue
                        remaining = args.max_wallclock - (time.monotonic() - started)
                        if remaining <= 0:
                            result["unattempted_task_ids"] = sorted({item["task_id"] for item in cases
                                                                       if item["task_id"] not in
                                                                       {trial.get("task_id") for trial in result["trials"]}})
                            result["budget_exhausted"] = "max_wallclock"
                            result["ok"] = False
                            exhausted = True
                            break
                        if spend_guard is not None:
                            # The provider-request cap is enforced BEFORE the
                            # trial starts, so a run never crosses the ceiling
                            # and stops afterwards.
                            try:
                                spend_guard.admit()
                            except budget.BudgetExhausted as error:
                                result["unattempted_task_ids"] = sorted({item["task_id"] for item in cases
                                                                           if item["task_id"] not in
                                                                           {trial.get("task_id") for trial in result["trials"]}})
                                result["budget_exhausted"] = "max_provider_requests"
                                result["budget_error"] = str(error)
                                result["budget"] = spend_guard.as_dict()
                                result["ok"] = False
                                exhausted = True
                                break
                        try:
                            if case.get("transport", "acp") == "exec":
                                trial = run_exec_trial(binary, case.get("profile", args.profile),
                                                      min(args.timeout, remaining, case.get("timeout_seconds", args.timeout)),
                                                      ledger_root=args.ledger, candidate=args.candidate,
                                                      task_id=case["task_id"], group_id=case.get("group_id", args.group_id),
                                                      repeat=repeat, suite_sha256=suite_sha256,
                                                      case_sha256=case_sha256,
                                                      content=case.get("content", "harness-lab-tool-ok"),
                                                      expected_text=case.get("expected_text", "harness-lab-tool-ok"),
                                                      expected_file=case.get("expected_file", "tool-receipt.txt"),
                                                      expected_exists=case.get("expected_exists"),
                                                      expected_terminal=case.get("expected_terminal", "passed"),
                                                      expected_tool_failures=case.get("expected_tool_failures", 0),
                                                      tool_sequence=case.get("tool_sequence"),
                                                      initial_files=expanded_files_by_task[case["task_id"]])
                            else:
                                trial = run_tool_trial(binary, case.get("profile", args.profile), min(args.timeout, remaining, case.get("timeout_seconds", args.timeout)),
                                                       bool(case.get("deny_bash", args.deny_bash)), args.ledger,
                                                       args.candidate, case["task_id"], case.get("group_id", args.group_id),
                                                       repeat, suite_sha256, case_sha256,
                                                       case.get("content", "harness-lab-tool-ok"),
                                                       case.get("expected_text", "harness-lab-tool-ok"),
                                                       case.get("tool_sequence"), expanded_files_by_task[case["task_id"]],
                                                       case.get("expected_file", "tool-receipt.txt"),
                                                       case.get("expected_exists"), case.get("cancel", False),
                                                       case.get("expected_terminal", "passed"), case.get("timeout_task", False),
                                                       case.get("context_text"),
                                                       case.get("expected_tool_failures", 1 if case.get("deny_bash", args.deny_bash) else 0),
                                                       agent_steps=case.get("agent_steps"))
                        except HarnessError as error:
                            trial = {"ok": False, "task_id": case["task_id"], "repeat": repeat,
                                     "failure_type": type(error).__name__}
                        result["trials"].append(trial)
                        if spend_guard is not None:
                            usage = trial.get("codegg_usage") or {}
                            spend_guard.observe(
                                provider_requests=int(trial.get("provider_calls") or 0),
                                input_tokens=int(usage.get("input_tokens") or 0),
                                output_tokens=int(usage.get("output_tokens") or 0))
                            result["budget"] = spend_guard.as_dict()
                        terminal = trial.get("ledger_manifest", {}).get("terminal")
                        expected_terminal = case.get("expected_terminal", "passed")
                        trial["expected_terminal_matched"] = terminal == expected_terminal
                        if terminal != expected_terminal or terminal != "passed":
                            result["ok"] = False
                    if exhausted:
                        break
            else:
                result = run_tool_trial(binary, args.profile, args.timeout, args.deny_bash,
                                        args.ledger, args.candidate, args.task_id, args.group_id)
        else:
            result = run_tool_trial(binary, args.profile, args.timeout, args.deny_bash)
        print(json.dumps(result, sort_keys=True))
        return 0 if result.get("ok", True) else 2
    except (HarnessError, ValueError, RuntimeError, TimeoutError, OSError) as error:
        print(json.dumps({"ok": False, "error": str(error)}, sort_keys=True), file=sys.stderr)
        return 2


def _binary(value: Path | None) -> Path:
    if value is None:
        raise HarnessError("pass --binary or set CODEGG_HARNESS_BINARY to a built CodeGG executable")
    binary = value.expanduser().resolve()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise HarnessError(f"candidate binary is not executable: {binary}")
    return binary


def _budget_policy(path: Path | None) -> budget.BudgetPolicy:
    """Load and strictly validate a budget policy document."""
    if path is None:
        raise HarnessError(
            "a live trial requires --budget pointing at a budget policy document "
            "with at least one enforceable hard cap")
    try:
        raw = json.loads(path.expanduser().read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise HarnessError(f"budget policy is unreadable: {error}") from error
    if not isinstance(raw, dict):
        raise HarnessError("budget policy must be a JSON object")
    try:
        return budget.budget_policy_from_mapping(raw)
    except ValueError as error:
        raise HarnessError(f"budget policy is invalid: {error}") from error


def _budget_admission(path: Path | None) -> budget.BudgetAdmission:
    return budget.admit_budget(_budget_policy(path))


def _require_budget_admission(args: argparse.Namespace) -> Optional[budget.BudgetPolicy]:
    """Fail closed before a live trial starts.

    Runs before any provider request, daemon spawn, or subprocess launch, so a
    trial that cannot prove an enforceable cap never reaches the provider.
    Returns the admitted policy so the caller can enforce it during the run.
    """
    if not getattr(args, "live_provider", False):
        return None
    admission = _budget_admission(getattr(args, "budget", None))
    if admission.status != budget.STATUS_ADMITTED:
        raise HarnessError(
            "budget admission refused before the trial started: "
            + json.dumps(admission.as_dict(), sort_keys=True))
    return _budget_policy(getattr(args, "budget", None))


def _sha256(path: Path) -> str:
    import hashlib

    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


if __name__ == "__main__":
    raise SystemExit(main())
