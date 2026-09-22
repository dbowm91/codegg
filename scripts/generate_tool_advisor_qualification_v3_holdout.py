#!/usr/bin/env python3
"""Generate the C001 v3 semantic holdout for tool-advisor qualification.

This generator is deliberately local-only: no model, no tokenizer, no
inference, no BM25/linear scoring to choose labels, and no remote
teacher/provider call. Every label is authored by the deterministic
scenario tables below, and every special slice carries the behavior its
tag claims:

- no-tool cases never imperatively request a candidate and always state a
  human-readable abstention rationale;
- counterfactual cases are explicit A/B pairs sharing one candidate
  universe whose single changed cue flips the expected label;
- unknown-renamed cases label a genuinely novel synthetic identity;
- long-session cases carry AdvisorContextV2-shaped state fields with a
  stale origin superseded by the current task;
- hard-negative cases hide the relevant name and argue the distractor;
- multi-tool cases carry graded relevance for a real two-step task.
"""

from __future__ import annotations

import hashlib
import json
import sys
from collections import Counter
from pathlib import Path


OUTPUT = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("assets/tool-advisor/qualification-v3-holdout.jsonl")
MANIFEST = Path(sys.argv[2]) if len(sys.argv) > 2 else Path("assets/tool-advisor/qualification-v3-holdout-manifest.json")

PROVENANCE = "generated-local-qualification-v3-template-v1"

CORE = [
    {"name": "grep", "description": "Search literal text in files", "category": "ReadOnly", "disclosure": "core", "synthetic_identity": False},
    {"name": "plan_get", "description": "Show the active work plan", "category": "ReadOnly", "disclosure": "core", "synthetic_identity": False},
]


def deferred(name: str, description: str) -> dict[str, object]:
    return {"name": name, "description": description, "category": "ReadOnly", "disclosure": "deferred", "synthetic_identity": False}


def novel(name: str, description: str) -> dict[str, object]:
    row = deferred(name, description)
    row["synthetic_identity"] = True
    return row


# Deferred catalog: historical concrete tools only. Novel identities appear
# solely in the unknown-renamed slice.
CATALOG = {
    "plugin_search": ("Search the plugin catalog for an installable extension", "plugin"),
    "plugin_install": ("Install a plugin from the catalog", "plugin"),
    "plugin_enable": ("Enable an installed plugin capability", "plugin"),
    "lsp_definition": ("Jump to the definition of a symbol", "lsp"),
    "lsp_references": ("Find all references to a symbol", "lsp"),
    "lsp_hover": ("Show hover documentation for a symbol", "lsp"),
    "research_search": ("Search local research notes and citations", "research"),
    "docs_lookup": ("Look up repository documentation", "search"),
    "memory_recall": ("Recall an earlier session decision from memory", "research"),
    "web_search": ("Search current public web documentation", "search"),
    "semantic_search": ("Search repository text by semantic similarity", "search"),
    "json_query": ("Extract a field from a structured JSON record", "structured"),
    "schema_validate": ("Validate a record against its schema", "structured"),
    "json_edit": ("Edit a field of a structured record", "structured"),
    "data_query": ("Run a query over a tabular dataset", "data"),
    "table_filter": ("Filter rows of a data table", "data"),
    "git_log": ("Show repository history for a path", "git"),
    "git_diff": ("Show uncommitted changes", "git"),
    "git_blame": ("Show who changed each line", "git"),
    "git_status": ("Show working tree status", "git"),
    "read": ("Read a file from disk", "filesystem"),
    "glob": ("List files matching a pattern", "filesystem"),
    "list_dir": ("List directory contents", "filesystem"),
    "symbol_search": ("Find symbols by name across the workspace", "filesystem"),
    "shell_exec": ("Run a shell command", "shell"),
    "shell_history": ("Show previously run shell commands", "shell"),
    "process_list": ("List running processes", "shell"),
}

NOVEL_TOOLS = [
    ("tool_x17", "Obscured-identity catalog probe with no lexical hint"),
    ("tool_x23", "Obscured-identity history probe with no lexical hint"),
    ("tool_x31", "Obscured-identity record probe with no lexical hint"),
    ("tool_x42", "Obscured-identity session probe with no lexical hint"),
    ("mcp__novel_lsp__jump_target", "Novel MCP server that resolves jump targets for symbols"),
    ("plugin__atlas__catalog_lookup", "New Atlas plugin that looks up catalog entries"),
    ("search__atlas__doc_find", "New Atlas search provider over project documents"),
    ("lsp__novel__symbol_jump", "Novel language server hop for symbol navigation"),
    ("data__atlas__frame_slice", "New Atlas data tool that slices record frames"),
    ("git__novel__history_walk", "Novel history walker over repository commits"),
    ("mcp__novel_search__repo_seek", "Novel MCP search endpoint over the repository"),
    ("shell__atlas__history_lens", "New Atlas lens over shell command history"),
]


def case(
    case_id: str,
    context: str,
    candidates: list[dict[str, object]],
    relevance: dict[str, int],
    preferred: list[str],
    none: bool,
    tags: list[str],
    group_suffix: str,
    skeleton: str,
    family: str,
    semantic: str | None = None,
    leakage: str | None = None,
) -> dict[str, object]:
    return {
        "schema_version": 1,
        "case_id": case_id,
        "context": context,
        "candidates": candidates,
        "relevance": relevance,
        "preferred_order": preferred,
        "none": none,
        "tags": sorted(set(tags + ["qualification-v3"])),
        "group_id": f"v3-group-{group_suffix}",
        "provenance": PROVENANCE,
        "semantic_group": semantic or f"v3-semantic-{group_suffix}",
        "leakage_group": leakage or f"v3-leakage-{group_suffix}",
        "task_family": family,
        "tool_family": family,
        "generated_variant_family": skeleton,
        "teacher_probabilities": {},
    }


def with_core(*extra: dict[str, object]) -> list[dict[str, object]]:
    return CORE + list(extra)


# ---------------------------------------------------------------------------
# Slice builders. Each scenario yields two variants (a/b) with distinct
# record tokens; the skeleton id is shared per scenario (or per pair).
# ---------------------------------------------------------------------------

def build_no_tool() -> list[dict[str, object]]:
    scenarios = [
        ("plugin", "plugin_search", "whether the editor already has a CSV preview extension installed",
         "The installed-extension list is already visible in this conversation. Rationale: answering from already-present context requires no tool action."),
        ("mcp", "plugin_search", "what an MCP server is in general terms",
         "This is a conceptual question for discussion, not a lookup task. Rationale: explanation grounded in general knowledge needs no tool call."),
        ("lsp", "lsp_definition", "why a rename was already applied to the Config type",
         "The rename diff is quoted verbatim above and the work is done. Rationale: summarizing completed state requires no further action."),
        ("research", "research_search", "a summary of the tradeoff paragraph pasted above",
         "All material to summarize is present in the prompt. Rationale: reasoning over already-present context needs no retrieval."),
        ("search", "web_search", "the meaning of the error message already quoted in full",
         "The full message and stack are visible here. Rationale: interpretation of shown text is not a lookup task."),
        ("structured", "json_query", "which field of the shown record holds the retry count",
         "The record is printed above with three fields. Rationale: reading a displayed record is not a tool operation."),
        ("data", "data_query", "whether the two-row table above contains a refund row",
         "Both rows are visible in this message. Rationale: inspection of given data needs no query tool."),
        ("git", "git_log", "who approved the already-merged change described above",
         "The approval note is quoted in the conversation. Rationale: restating recorded history needs no repository access."),
        ("filesystem", "read", "what the three-line snippet above prints when run mentally",
         "The snippet is short and fully shown. Rationale: mental execution of shown code is not file access."),
        ("shell", "shell_exec", "whether the command quoted above is safe to run",
         "This asks for judgment about wording, not execution. Rationale: safety discussion must not itself run anything."),
        ("lsp", "lsp_hover", "an explanation of the hover text already pasted into the thread",
         "The hover text is reproduced above. Rationale: explaining visible documentation needs no further lookup."),
        ("shell", "shell_history", "a recap of the plan agreed earlier in this conversation",
         "The agreed steps are listed above. Rationale: recapping conversation state is not a history query."),
    ]
    rows: list[dict[str, object]] = []
    for index, (family, irrelevant, situation, rationale) in enumerate(scenarios):
        desc, _ = CATALOG[irrelevant]
        for variant, token in (("a", f"v3-nt-{index:02d}a"), ("b", f"v3-nt-{index:02d}b")):
            closer = " The question stands alone." if variant == "a" else " A follow-up stays within the same shown material."
            context = (
                f"In workspace record {token}, advise on {situation}. "
                f"Consider only what is already shown in this conversation and do not fetch anything else. "
                f"{rationale}{closer}"
            )
            rows.append(case(
                f"qualification-v3-no-tool-{index:02d}{variant}",
                context,
                with_core(deferred(irrelevant, f"Irrelevant deferred {desc.lower()}"), deferred("symbol_search", "Find symbols by name across the workspace")),
                {}, [], True, ["no-tool", "abstention"],
                f"nt-{index:02d}{variant}", f"v3-skeleton-nt-{index:02d}{variant}", family,
            ))
    return rows


def build_counterfactual_pairs() -> list[dict[str, object]]:
    # (skeleton_suffix, family, tool_a, tool_b, neutral, stem, cue_a, cue_b)
    # Each pair shares the stem and most wording; only the cue sentence flips
    # the expected label.
    pairs = [
        ("cf-01", "search", "docs_lookup", "web_search", "semantic_search",
         "In workspace record {tok}, look up the retry policy.",
         "Consult the local documentation index rather than the public web.",
         "Consult the public web index rather than the local documentation."),
        ("cf-02", "structured", "json_query", "semantic_search", "schema_validate",
         "In workspace record {tok}, resolve the retry limit.",
         "Read the parsed record field rather than matching surrounding text.",
         "Match the surrounding text rather than reading the parsed record field."),
        ("cf-03", "lsp", "lsp_definition", "semantic_search", "lsp_hover",
         "In workspace record {tok}, locate ConfigLoader.",
         "Resolve the declaration site rather than ranking text occurrences.",
         "Rank the text occurrences rather than resolving the declaration site."),
        ("cf-04", "git", "git_log", "git_blame", "git_status",
         "In workspace record {tok}, investigate the retry change.",
         "List the commit history rather than attributing single lines.",
         "Attribute the single lines rather than listing commit history."),
        ("cf-05", "data", "table_filter", "json_query", "data_query",
         "In workspace record {tok}, scope the refunds data.",
         "Select the rows over fifty rather than extracting one nested field.",
         "Extract the one nested field rather than selecting rows over fifty."),
        ("cf-06", "research", "research_search", "memory_recall", "docs_lookup",
         "In workspace record {tok}, settle the backoff debate.",
         "Consult the written project notes rather than recalling session memory.",
         "Recall the session memory rather than consulting written project notes."),
        ("cf-07", "plugin", "plugin_search", "plugin_install", "plugin_enable",
         "In workspace record {tok}, get the CSV preview capability.",
         "Locate the catalog entry rather than performing installation.",
         "Perform the installation rather than merely locating the catalog entry."),
        ("cf-08", "lsp", "lsp_references", "lsp_definition", "lsp_hover",
         "In workspace record {tok}, audit the retry helper.",
         "Enumerate the call sites rather than resolving its declaration.",
         "Resolve its declaration rather than enumerating the call sites."),
        ("cf-09", "shell", "shell_exec", "shell_history", "process_list",
         "In workspace record {tok}, handle the migration command.",
         "Execute it now rather than reviewing past invocations.",
         "Review the past invocations rather than executing anything now."),
        ("cf-10", "shell", "shell_history", "process_list", "shell_exec",
         "In workspace record {tok}, check prior test runs.",
         "Review the past invocations rather than inspecting live processes.",
         "Inspect the live processes rather than reviewing past invocations."),
        ("cf-11", "structured", "schema_validate", "json_edit", "json_query",
         "In workspace record {tok}, process record seven.",
         "Check conformance against the schema rather than changing any field.",
         "Change the flagged field rather than checking schema conformance."),
        ("cf-12", "research", "memory_recall", "research_search", "semantic_search",
         "In workspace record {tok}, recover the naming decision.",
         "Use session memory rather than searching written notes.",
         "Search the written notes rather than using session memory."),
        ("cf-13", "filesystem", "read", "glob", "list_dir",
         "In workspace record {tok}, open configs/app.toml.",
         "Read that exact path rather than searching for candidate files.",
         "Search for candidate files rather than reading one exact path."),
    ]
    rows: list[dict[str, object]] = []
    for skeleton, family, tool_a, tool_b, neutral, stem, cue_a, cue_b in pairs:
        desc_a, _ = CATALOG[tool_a]
        desc_b, _ = CATALOG[tool_b]
        desc_n, _ = CATALOG[neutral]
        shared = with_core(deferred(tool_a, desc_a), deferred(tool_b, desc_b), deferred(neutral, desc_n))
        tok_a = skeleton.replace("cf-", "v3-cf-") + "a"
        tok_b = skeleton.replace("cf-", "v3-cf-") + "b"
        rows.append(case(
            f"qualification-v3-counterfactual-{skeleton}a", f"{stem.format(tok=tok_a)} {cue_a}",
            shared, {tool_a: 3}, [tool_a], False, ["counterfactual"],
            f"{skeleton}a", f"v3-skeleton-{skeleton}", family,
            semantic=f"v3-counterfactual-pair-{skeleton}",
        ))
        rows.append(case(
            f"qualification-v3-counterfactual-{skeleton}b", f"{stem.format(tok=tok_b)} {cue_b}",
            shared, {tool_b: 3}, [tool_b], False, ["counterfactual"],
            f"{skeleton}b", f"v3-skeleton-{skeleton}", family,
            semantic=f"v3-counterfactual-pair-{skeleton}",
        ))
    return rows


def build_unknown() -> list[dict[str, object]]:
    scenarios = [
        ("plugin", 0, "Look up the catalog entry for CSV preview in record {tok} through the new Atlas catalog endpoint, which understands bundle metadata the old index cannot parse."),
        ("mcp", 4, "Resolve the jump target for the ConfigLoader symbol in record {tok} through the novel MCP endpoint, which returns declaration sites for workspace symbols."),
        ("lsp", 7, "Hop to the symbol target of the retry helper in record {tok} with the novel navigation service, which resolves across generated files."),
        ("search", 6, "Find the deployment note of record {tok} with the new Atlas document index, which covers the migrated archive the legacy index misses."),
        ("research", 6, "Search the consolidated decision log of record {tok} through the Atlas document service, which merges notes the old search cannot see."),
        ("structured", 8, "Slice the refund frame of record {tok} with the new Atlas frame tool, which pages nested batches the legacy reader truncates."),
        ("data", 8, "Slice the usage frame of record {tok} with the Atlas frame service, which handles the wide export the old query path rejects."),
        ("git", 9, "Walk the retry-module history of record {tok} with the novel history walker, which follows renames the plain log misses."),
        ("filesystem", 10, "Seek the archived config of record {tok} through the novel MCP repository endpoint, which indexes snapshots the local listing omits."),
        ("shell", 11, "Review yesterday's deploy invocations for record {tok} through the Atlas history lens, which annotates failures the plain history hides."),
        ("lsp", 1, "Probe the renamed helper of record {tok} with the obscured-identity catalog probe, whose numeric handle carries no lexical hint about its purpose."),
        ("search", 2, "Probe the archived thread of record {tok} with the obscured-identity history probe, whose numeric handle reveals nothing about its target."),
    ]
    rows: list[dict[str, object]] = []
    for index, (family, novel_index, template) in enumerate(scenarios):
        name, desc = NOVEL_TOOLS[novel_index]
        distractors = {
            "plugin": ("plugin_search", "lsp_definition"),
            "mcp": ("plugin_search", "semantic_search"),
            "lsp": ("lsp_references", "lsp_hover"),
            "search": ("web_search", "semantic_search"),
            "research": ("research_search", "docs_lookup"),
            "structured": ("json_query", "schema_validate"),
            "data": ("data_query", "table_filter"),
            "git": ("git_log", "git_diff"),
            "filesystem": ("read", "glob"),
            "shell": ("shell_history", "process_list"),
        }[family]
        for variant, token in (("a", f"v3-un-{index:02d}a"), ("b", f"v3-un-{index:02d}b")):
            d0, _ = CATALOG[distractors[0]]
            d1, _ = CATALOG[distractors[1]]
            closer = " Start with the new endpoint before any legacy fallback." if variant == "a" else " Prefer the new endpoint over the legacy fallback for this record."
            rows.append(case(
                f"qualification-v3-unknown-{index:02d}{variant}",
                template.format(tok=token) + closer,
                with_core(novel(name, desc), deferred(distractors[0], d0), deferred(distractors[1], d1)),
                {name: 3}, [name], False, ["unknown-renamed"],
                f"un-{index:02d}{variant}", f"v3-skeleton-un-{index:02d}{variant}", family,
            ))
    return rows


def build_hard_negative() -> list[dict[str, object]]:
    # (family, relevant, distractor, template) — templates never repeat the
    # relevant name; each argues why the distractor is plausible but wrong.
    scenarios = [
        ("plugin", "plugin_install", "plugin_search",
         "The workspace record {tok} needs the CSV preview capability actually installed, not merely located. Although catalog search looks plausible because it finds the right entry, only performing the installation changes the workspace; search alone leaves nothing installed, so installation is required instead."),
        ("mcp", "plugin_search", "web_search",
         "Record {tok} needs the MCP catalog entry for snapshot storage. Although public web results look plausible because they describe similar servers, the workspace catalog holds the installable entry; web matches cannot be installed, so the catalog lookup is required instead."),
        ("lsp", "lsp_definition", "semantic_search",
         "Record {tok} needs the exact declaration site of ConfigLoader before editing. Although ranked text matches look plausible because they mention the symbol everywhere, only the declaration shows the field layout; occurrence ranking is plausible but the definition jump is required instead."),
        ("lsp", "lsp_references", "lsp_definition",
         "Record {tok} needs every call site of the retry helper audited. Although the declaration looks plausible because it is the canonical location, one declaration cannot enumerate usages; the single site is plausible but the reference enumeration is required instead."),
        ("research", "research_search", "web_search",
         "Record {tok} needs the project-local decision on retry backoff. Although public documentation looks plausible because it covers backoff well, the workspace decision lives only in project notes; public pages are plausible but the local notes search is required instead."),
        ("search", "docs_lookup", "semantic_search",
         "Record {tok} needs the authored migration guide section. Although similarity-ranked snippets look plausible because they quote nearby text, only the guide section carries the ordered steps; snippets are plausible but the documentation lookup is required instead."),
        ("structured", "json_query", "semantic_search",
         "Record {tok} needs the parsed retry limit from the record. Although text search looks plausible because the number appears in the file, surrounding prose quotes stale values; matching text is plausible but reading the parsed field is required instead."),
        ("structured", "schema_validate", "json_edit",
         "Record {tok} needs the import batch checked for conformance before landing. Although editing the odd field looks plausible because it silences the complaint, an edit without validation can hide a second violation; the quick edit is plausible but conformance validation is required instead."),
        ("data", "table_filter", "data_query",
         "Record {tok} needs the refunds table narrowed to rows over fifty for review. Although a full-table query looks plausible because it returns everything, the reviewer asked for the narrowed subset; the broad query is plausible but row filtering is required instead."),
        ("data", "data_query", "table_filter",
         "Record {tok} needs the total across all regions computed server-side. Although narrowing rows looks plausible because the region of interest is one of many, a client-side subset cannot produce the grand total; filtering is plausible but the aggregate query is required instead."),
        ("git", "git_log", "git_status",
         "Record {tok} needs the commit that introduced the retry flag. Although the working-tree status looks plausible because it shows what changed recently, uncommitted state cannot name the introducing commit; status is plausible but history listing is required instead."),
        ("git", "git_blame", "git_log",
         "Record {tok} needs the author of the exact fallback line. Although the file history looks plausible because it lists nearby commits, commit lists do not attribute single lines; history is plausible but line attribution is required instead."),
        ("filesystem", "read", "glob",
         "Record {tok} needs the exact contents of configs/app.toml. Although pattern listing looks plausible because it confirms the file exists, existence alone does not reveal the pinned flag value; listing is plausible but opening the file is required instead."),
        ("shell", "shell_history", "shell_exec",
         "Record {tok} needs to know which migration command ran last night. Although re-running the command looks plausible because it reproduces the effect, execution cannot report the past; running is plausible but history review is required instead."),
    ]
    rows: list[dict[str, object]] = []
    for index, (family, relevant, distractor, template) in enumerate(scenarios):
        desc_r, _ = CATALOG[relevant]
        desc_d, _ = CATALOG[distractor]
        for variant, token in (("a", f"v3-hn-{index:02d}a"), ("b", f"v3-hn-{index:02d}b")):
            closer = f" Treat this as the {token} instance." if variant == "a" else f" The {token} follow-up keeps the same requirement."
            rows.append(case(
                f"qualification-v3-hard-negative-{index:02d}{variant}",
                template.format(tok=token) + closer,
                with_core(deferred(relevant, desc_r), deferred(distractor, desc_d)),
                {relevant: 3}, [relevant], False, ["hard-negative"],
                f"hn-{index:02d}{variant}", f"v3-skeleton-hn-{index:02d}{variant}", family,
            ))
    return rows


def build_multi_tool() -> list[dict[str, object]]:
    scenarios = [
        ("plugin", "plugin_search", "plugin_install",
         "For record {tok}, first locate the CSV preview entry in the catalog, then install it into the workspace. The locating step comes first and the installation completes the task."),
        ("lsp", "lsp_definition", "lsp_references",
         "For record {tok}, first resolve the declaration of the retry helper, then enumerate its call sites from that anchor. Declaration first, usage audit second."),
        ("research", "research_search", "memory_recall",
         "For record {tok}, first search the written project notes for the backoff decision, then recall the follow-up agreed in session. Notes first, session memory second."),
        ("search", "docs_lookup", "web_search",
         "For record {tok}, first read the local migration guide, then check the public release notes for newer caveats. Local guide first, public notes second."),
        ("structured", "json_query", "schema_validate",
         "For record {tok}, first extract the retry limit from the record, then validate the edited batch against the schema. Extraction first, validation second."),
        ("data", "table_filter", "data_query",
         "For record {tok}, first narrow the refunds table to the region of interest, then compute the total over that subset. Narrowing first, aggregation second."),
        ("git", "git_log", "git_diff",
         "For record {tok}, first list the commits touching the retry module, then review the uncommitted changes on top. History first, working-tree diff second."),
        ("filesystem", "glob", "read",
         "For record {tok}, first list the candidate config paths, then read the pinned one. Discovery first, reading second."),
        ("shell", "shell_history", "shell_exec",
         "For record {tok}, first review which deploy command ran last, then re-run it with the fixed flag. Review first, execution second."),
        ("lsp", "lsp_hover", "lsp_definition",
         "For record {tok}, first read the hover summary of the helper, then jump to its declaration for the full body. Summary first, declaration second."),
        ("structured", "schema_validate", "json_edit",
         "For record {tok}, first validate the import batch, then fix the flagged field. Validation first, repair second."),
        ("git", "git_status", "git_blame",
         "For record {tok}, first check which files are dirty, then attribute the suspect line in the dirty file. Status first, attribution second."),
    ]
    rows: list[dict[str, object]] = []
    for index, (family, first, second, template) in enumerate(scenarios):
        desc_a, _ = CATALOG[first]
        desc_b, _ = CATALOG[second]
        for variant, token in (("a", f"v3-mt-{index:02d}a"), ("b", f"v3-mt-{index:02d}b")):
            closer = f" Both steps belong to record {token}." if variant == "a" else f" Record {token} tracks the two steps as one task."
            rows.append(case(
                f"qualification-v3-multi-tool-{index:02d}{variant}",
                template.format(tok=token) + closer,
                with_core(deferred(first, desc_a), deferred(second, desc_b)),
                {first: 3, second: 2}, [first, second], False, ["multi-tool"],
                f"mt-{index:02d}{variant}", f"v3-skeleton-mt-{index:02d}{variant}", family,
            ))
    return rows


def build_long_session() -> list[dict[str, object]]:
    scenarios = [
        ("plugin", "plugin_install", "repair the failing extension host",
         "install the CSV preview plugin build", "run the catalog install and restart the host",
         "the host log reports the preview plugin missing", "document the extension API"),
        ("lsp", "lsp_definition", "repair the failing Rust build",
         "locate the definition of ConfigLoader", "inspect the symbol definition before editing",
         "the compiler reports an unknown field in ConfigLoader", "document the architecture"),
        ("research", "research_search", "settle the retry backoff debate",
         "find the backoff decision in the project notes", "quote the agreed values into the thread",
         "two reviewers cite different numbers", "draft the release announcement"),
        ("search", "docs_lookup", "finish the migration runbook",
         "read the local rollback section", "copy the ordered steps into the runbook",
         "the staging run stopped at the rollback step", "write the conference abstract"),
        ("structured", "json_query", "fix the import rejection",
         "read the retry limit from the rejected record", "compare it against the schema default",
         "the importer rejects batch 7 with a limit error", "plan the team offsite"),
        ("data", "table_filter", "answer the refund audit",
         "narrow the refunds table to large rows", "export the narrowed subset for review",
         "the auditor asks for rows over fifty only", "redesign the landing page"),
        ("git", "git_log", "explain the regression window",
         "list the commits touching the retry module", "identify the first bad build",
         "the nightly run turned red after Tuesday", "update the hiring rubric"),
        ("filesystem", "read", "restore the pinned configuration",
         "open configs/app.toml at the pinned revision", "diff it against the working copy",
         "the service reads a stale flag at startup", "organize the photo library"),
        ("shell", "shell_history", "reconstruct last night's deploy",
         "review which deploy command ran last", "re-run it with the fixed flag",
         "the deploy log rotated before dawn", "catalog the bookshelf"),
        ("lsp", "lsp_references", "audit the retry helper rollout",
         "enumerate every call site of the helper", "mark each site verified",
         "three call sites still use the old arity", "compose the newsletter"),
        ("search", "web_search", "check the upstream fix status",
         "read the current public release notes", "compare against the pinned version",
         "the vendored copy may predate the fix", "prepare the quarterly report"),
        ("structured", "schema_validate", "land the import batch",
         "validate the batch against the schema", "report conformance before landing",
         "the previous landing failed conformance", "schedule dentist appointments"),
        ("git", "git_blame", "attribute the fallback line",
         "attribute the exact fallback line", "ask the author about intent",
         "reviewers disagree about who wrote it", "plan the garden layout"),
        ("shell", "process_list", "diagnose the stuck worker",
         "list the live worker processes", "restart the wedged one",
         "the queue depth keeps growing", "review the poetry submissions"),
    ]
    rows: list[dict[str, object]] = []
    for index, (family, relevant, objective, task, step, signal, stale) in enumerate(scenarios):
        desc, _ = CATALOG[relevant]
        for variant, token, with_error in (
            ("a", f"v3-ls-{index:02d}a", True),
            ("b", f"v3-ls-{index:02d}b", False),
        ):
            signal_line = f"Unresolved signal: {signal}." if with_error else "Unresolved signal: none. No unresolved errors."
            context = (
                f"Current objective: {objective} in record {token}.\n"
                f"Current task: {task}.\n"
                f"Next step: {step}.\n"
                f"{signal_line}\n"
                f"Original session topic: {stale}.\n"
                f"The original topic is stale and is superseded by the current task; "
                f"follow the current task rather than the original topic."
            )
            rows.append(case(
                f"qualification-v3-long-session-{index:02d}{variant}",
                context,
                with_core(deferred(relevant, desc), deferred("semantic_search", "Search repository text by semantic similarity")),
                {relevant: 3}, [relevant], False, ["context-v2-long-session", "contextual"],
                f"ls-{index:02d}{variant}", f"v3-skeleton-ls-{index:02d}{variant}", family,
            ))
    return rows


def build_general() -> list[dict[str, object]]:
    scenarios = [
        ("plugin", "plugin_enable", "Enable the installed CSV preview capability for record {tok} so spreadsheets render inline."),
        ("mcp", "plugin_search", "Find the snapshot-storage MCP entry for record {tok} in the workspace catalog."),
        ("lsp", "lsp_hover", "Show the hover summary of the retry helper for record {tok} before editing its docs."),
        ("research", "memory_recall", "Recall the dependency-freeze decision of record {tok} from the earlier session."),
        ("search", "semantic_search", "Find textually similar retry handlers for record {tok} across the repository."),
        ("structured", "json_edit", "Update the retry limit field of record {tok} to the agreed value."),
        ("data", "data_query", "Compute the refund total for record {tok} grouped by region."),
        ("git", "git_diff", "Review the uncommitted retry changes of record {tok} before committing."),
    ]
    rows: list[dict[str, object]] = []
    for index, (family, relevant, template) in enumerate(scenarios):
        desc, _ = CATALOG[relevant]
        for variant, token in (("a", f"v3-gn-{index:02d}a"), ("b", f"v3-gn-{index:02d}b")):
            closer = f" Scope the answer to record {token}." if variant == "a" else f" Keep the change limited to record {token}."
            rows.append(case(
                f"qualification-v3-general-{index:02d}{variant}",
                template.format(tok=token) + closer,
                with_core(deferred(relevant, desc), deferred("glob", "List files matching a pattern")),
                {relevant: 3}, [relevant], False, [],
                f"gn-{index:02d}{variant}", f"v3-skeleton-gn-{index:02d}{variant}", family,
            ))
    return rows


def canonical_json(row: dict[str, object]) -> str:
    # Match ToolAdvisorCase serde field order and compact form. Content is
    # ASCII-only so Python and serde_json agree byte-for-byte.
    ordered = {
        "schema_version": row["schema_version"],
        "case_id": row["case_id"],
        "context": row["context"],
        "candidates": row["candidates"],
        "relevance": {key: row["relevance"][key] for key in sorted(row["relevance"])},
        "preferred_order": row["preferred_order"],
        "none": row["none"],
        "tags": row["tags"],
        "group_id": row["group_id"],
        "provenance": row["provenance"],
        "semantic_group": row["semantic_group"],
        "leakage_group": row["leakage_group"],
        "task_family": row["task_family"],
        "tool_family": row["tool_family"],
        "generated_variant_family": row["generated_variant_family"],
        "teacher_probabilities": row["teacher_probabilities"],
    }
    return json.dumps(ordered, ensure_ascii=False, separators=(",", ":"))


def dataset_fingerprint(rows: list[dict[str, object]]) -> str:
    payload = "".join(canonical_json(row) + "\n" for row in sorted(rows, key=lambda row: str(row["case_id"])))
    return hashlib.sha256(payload.encode()).hexdigest()


def main() -> None:
    rows: list[dict[str, object]] = []
    rows.extend(build_no_tool())
    rows.extend(build_counterfactual_pairs())
    rows.extend(build_unknown())
    rows.extend(build_hard_negative())
    rows.extend(build_multi_tool())
    rows.extend(build_long_session())
    rows.extend(build_general())
    assert len(rows) == 170, f"expected 170 cases, got {len(rows)}"
    for row in rows:
        row["context"].encode("ascii")
        assert row["generated_variant_family"].startswith("v3-skeleton-")
        assert row["provenance"] == PROVENANCE
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text("".join(json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n" for row in rows))
    raw_sha = hashlib.sha256(OUTPUT.read_bytes()).hexdigest()
    tags = Counter(tag for row in rows for tag in row["tags"])
    family_counts = Counter(str(row["tool_family"]) for row in rows)
    skeletons = {str(row["generated_variant_family"]) for row in rows}
    pair_ids = {str(row["semantic_group"]) for row in rows if "counterfactual" in row["tags"]}
    manifest = {
        "schema_version": 1,
        "dataset": str(OUTPUT),
        "dataset_fingerprint": dataset_fingerprint(rows),
        "raw_sha256": raw_sha,
        "case_count": len(rows),
        "minimum_leakage_groups": 120,
        "minimum_skeletons": 64,
        "skeleton_count": len(skeletons),
        "counterfactual_pairs": len(pair_ids),
        "construction": "deterministic local labels; no selected-model inference; no scoring-based label choice; no remote teacher",
        "provenance": PROVENANCE,
        "family_counts": dict(sorted(family_counts.items())),
        "tag_counts": dict(sorted(tags.items())),
        "required_families": ["plugin", "lsp", "research/search", "structured/data"],
        "required_slices": ["hard-negative", "counterfactual", "unknown-renamed", "no-tool", "multi-tool", "context-v2-long-session"],
    }
    MANIFEST.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    print(json.dumps(manifest, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
