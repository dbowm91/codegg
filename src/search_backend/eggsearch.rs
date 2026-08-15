//! Eggsearch backend adapter.
//!
//! Translates Codegg's stable `websearch` / `webfetch` argument shapes
//! into the eggsearch MCP tool argument shapes, calls into the live
//! `McpService`, and applies output framing/capping. The actual schema
//! difference between Codegg's native tools and eggsearch's MCP tools
//! is hidden from the model.

use serde_json::{json, Value};

use crate::error::ToolError;

use super::framing::{
    clamp_output, frame_batch_results, frame_evidence_bundle, frame_fetched_page, frame_repo_file,
    frame_repo_map, frame_repo_results, frame_research_results, frame_search_results,
    frame_security_results,
};

fn copy_fields(args: &mut Value, input: &Value, fields: &[&str]) {
    for field in fields {
        if let Some(value) = input.get(*field).filter(|value| !value.is_null()) {
            args[*field] = value.clone();
        }
    }
}

fn non_empty_string(input: &Value, field: &str) -> Result<Option<String>, ToolError> {
    match input.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| ToolError::Execution(format!("'{field}' must be a non-empty string")))
            .map(Some),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct RepositoryLocator {
    host: Option<String>,
    owner: Option<String>,
    repo: String,
    ref_name: Option<String>,
}

fn repository_locator(input: &Value, require_owner: bool) -> Result<RepositoryLocator, ToolError> {
    let explicit_owner = non_empty_string(input, "owner")?;
    let repo = non_empty_string(input, "repo")?
        .ok_or_else(|| ToolError::Execution("missing 'repo' parameter".to_string()))?;

    let (owner, repo) = match explicit_owner {
        Some(_owner) if repo.contains('/') => {
            return Err(ToolError::Execution(
                "'repo' must be a repository name when 'owner' is provided; use one combined owner/repo locator or separate fields, not both".to_string(),
            ));
        }
        Some(owner) => (Some(owner), repo),
        None if repo.matches('/').count() == 1 => {
            let (owner, repo_name) = repo.split_once('/').expect("one slash implies split");
            if owner.is_empty() || repo_name.is_empty() {
                return Err(ToolError::Execution(format!(
                    "invalid repository locator '{repo}'; expected owner/repo"
                )));
            }
            (Some(owner.to_string()), repo_name.to_string())
        }
        None if repo.contains('/') => {
            return Err(ToolError::Execution(format!(
                "ambiguous repository locator '{repo}'; provide explicit 'owner' and 'repo'"
            )));
        }
        None => (None, repo),
    };

    if require_owner && owner.is_none() {
        return Err(ToolError::Execution(
            "repository fetch/map requires explicit 'owner' and 'repo' or an unambiguous 'owner/repo' locator".to_string(),
        ));
    }

    Ok(RepositoryLocator {
        host: non_empty_string(input, "host")?,
        owner,
        repo,
        ref_name: non_empty_string(input, "ref_name")?,
    })
}

fn add_repository_locator(args: &mut Value, locator: RepositoryLocator) {
    if let Some(host) = locator.host {
        args["host"] = Value::String(host);
    }
    if let Some(owner) = locator.owner {
        args["owner"] = Value::String(owner);
    }
    args["repo"] = Value::String(locator.repo);
    if let Some(ref_name) = locator.ref_name {
        args["ref_name"] = Value::String(ref_name);
    }
}

fn range_alias(input: &Value, current: &str, legacy: &str) -> Result<Option<u64>, ToolError> {
    let current_value = input.get(current).and_then(Value::as_u64);
    let legacy_value = input.get(legacy).and_then(Value::as_u64);
    if input
        .get(current)
        .is_some_and(|_value| current_value.is_none())
    {
        return Err(ToolError::Execution(format!(
            "'{current}' must be a non-negative integer"
        )));
    }
    if input
        .get(legacy)
        .is_some_and(|_value| legacy_value.is_none())
    {
        return Err(ToolError::Execution(format!(
            "'{legacy}' must be a non-negative integer"
        )));
    }
    if let (Some(current_value), Some(legacy_value)) = (current_value, legacy_value) {
        if current_value != legacy_value {
            return Err(ToolError::Execution(format!(
                "'{current}' and legacy '{legacy}' disagree"
            )));
        }
    }
    Ok(current_value.or(legacy_value))
}

fn build_web_search_args(input: &Value) -> Result<Value, ToolError> {
    let query = input
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .ok_or_else(|| ToolError::Execution("missing 'query' parameter".to_string()))?;
    let max_results = input
        .get("num_results")
        .or_else(|| input.get("max_results"))
        .and_then(Value::as_u64)
        .unwrap_or(8)
        .min(30);
    let mut args = json!({"query": query, "max_results": max_results});
    if let Some(providers) = translate_provider_hint(input.get("provider").and_then(Value::as_str))
    {
        if !providers.is_empty() {
            args["providers"] = Value::Array(providers.into_iter().map(Value::String).collect());
        }
    }
    copy_fields(&mut args, input, &["intent", "freshness", "safe_search"]);
    Ok(args)
}

fn build_web_fetch_args(input: &Value) -> Result<Value, ToolError> {
    let url = input
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::Execution("missing 'url' parameter".to_string()))?;
    validate_fetch_url(url)?;
    let max_chars = input
        .get("max_length")
        .or_else(|| input.get("max_chars"))
        .and_then(Value::as_u64)
        .unwrap_or(10_000);
    let mut args = json!({"url": url, "max_chars": max_chars});
    args["extract_mode"] = input
        .get("extract_mode")
        .cloned()
        .unwrap_or_else(|| json!("text"));
    args["include_links"] = input
        .get("include_links")
        .cloned()
        .unwrap_or_else(|| json!(false));
    Ok(args)
}

fn translate_legacy_research_domains(input: &Value, args: &mut Value) -> Result<(), ToolError> {
    let Some(domains) = input.get("domains") else {
        return Ok(());
    };
    let domains = domains.as_array().ok_or_else(|| {
        ToolError::Execution("legacy 'domains' must be an array of strings".to_string())
    })?;
    if domains.is_empty() {
        return Err(ToolError::Execution(
            "legacy 'domains' is empty; use 'research_domain' or 'providers' instead".to_string(),
        ));
    }
    let values = domains
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    ToolError::Execution(
                        "legacy 'domains' must contain non-empty strings".to_string(),
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if values.iter().all(|value| is_known_provider(value)) {
        if input.get("providers").is_some() {
            return Err(ToolError::Execution(
                "legacy 'domains' and current 'providers' cannot both be supplied".to_string(),
            ));
        }
        let provider_values = values
            .iter()
            .filter_map(|value| translate_provider_hint(Some(value)))
            .flatten()
            .map(Value::String)
            .collect();
        args["providers"] = Value::Array(provider_values);
    } else if values.len() == 1 && input.get("research_domain").is_none() {
        args["research_domain"] = Value::String(values[0].to_string());
    } else {
        return Err(ToolError::Execution(
            "legacy 'domains' is ambiguous; use one current 'research_domain' or an explicit 'providers' list".to_string(),
        ));
    }
    Ok(())
}

fn is_known_provider(value: &str) -> bool {
    matches!(
        value,
        "duckduckgo"
            | "mojeek"
            | "wikipedia"
            | "arxiv"
            | "openalex"
            | "pubmed"
            | "hn_algolia"
            | "google_news"
            | "github"
            | "exa"
            | "tavily"
            | "brave"
            | "brave_api"
            | "kagi"
            | "serpapi"
    )
}

fn normalize_batch_item(item: &Value, index: usize) -> Result<Value, ToolError> {
    let item_type = item.get("type").and_then(Value::as_str);
    if item_type == Some("web") || (item_type.is_none() && item.get("url").is_some()) {
        let url = item
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::Execution(format!("batch item {index} is missing 'url'")))?;
        validate_fetch_url(url)?;
        let mut normalized = json!({"type": "web", "url": url});
        copy_fields(
            &mut normalized,
            item,
            &["extract_mode", "include_links", "max_chars"],
        );
        return Ok(normalized);
    }

    if item_type == Some("repo") || item_type.is_none() && item.get("repo").is_some() {
        let path = non_empty_string(item, "path")?.ok_or_else(|| {
            ToolError::Execution(format!("batch repo item {index} is missing 'path'"))
        })?;
        let mut normalized = json!({"type": "repo", "path": path});
        add_repository_locator(&mut normalized, repository_locator(item, true)?);
        copy_fields(
            &mut normalized,
            item,
            &[
                "commit_sha",
                "line_start",
                "line_end",
                "context_before",
                "context_after",
                "max_chars",
            ],
        );
        if let Some(line_start) = range_alias(item, "line_start", "start_line")? {
            normalized["line_start"] = json!(line_start);
        }
        if let Some(line_end) = range_alias(item, "line_end", "end_line")? {
            normalized["line_end"] = json!(line_end);
        }
        return Ok(normalized);
    }

    Err(ToolError::Execution(format!(
        "batch item {index} must be a tagged 'web' or 'repo' item"
    )))
}

fn build_batch_fetch_args(input: &Value) -> Result<Value, ToolError> {
    let mut raw_items = Vec::new();
    if let Some(urls) = input.get("urls") {
        let urls = urls.as_array().ok_or_else(|| {
            ToolError::Execution("legacy 'urls' must be an array of HTTP(S) strings".to_string())
        })?;
        raw_items.extend(urls.iter().map(|url| json!({"type": "web", "url": url})));
    }
    if let Some(items) = input.get("items") {
        let items = items
            .as_array()
            .ok_or_else(|| ToolError::Execution("'items' must be an array".to_string()))?;
        raw_items.extend(items.iter().cloned());
    }
    if raw_items.is_empty() {
        return Err(ToolError::Execution(
            "batch_fetch requires a non-empty 'items' array (legacy 'urls' is also accepted)"
                .to_string(),
        ));
    }
    let items = raw_items
        .iter()
        .enumerate()
        .map(|(index, item)| normalize_batch_item(item, index))
        .collect::<Result<Vec<_>, _>>()?;
    let mut args = json!({"items": items});
    copy_fields(&mut args, input, &["continue_on_error"]);
    args["max_items"] = json!(bounded_u64(input, "max_items", 100, 100)?);
    args["max_chars_per_item"] = json!(bounded_u64(input, "max_chars_per_item", 10_000, 50_000,)?);
    if input.get("max_total_chars").is_some() {
        args["max_total_chars"] = json!(bounded_u64(input, "max_total_chars", 100_000, 500_000,)?);
    }
    Ok(args)
}

fn bounded_u64(input: &Value, field: &str, default: u64, cap: u64) -> Result<u64, ToolError> {
    match input.get(field) {
        None => Ok(default),
        Some(value) => value.as_u64().map(|value| value.min(cap)).ok_or_else(|| {
            ToolError::Execution(format!("'{field}' must be a non-negative integer"))
        }),
    }
}

fn build_evidence_bundle_args(input: &Value) -> Result<Value, ToolError> {
    let sources = input
        .get("sources")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let fetches = input.get("fetches").and_then(Value::as_array);
    if sources.is_empty() && fetches.map_or(true, Vec::is_empty) {
        return Err(ToolError::Execution(
            "evidence_bundle requires at least one current source or fetch input".to_string(),
        ));
    }
    for (index, source) in sources.iter().enumerate() {
        if source.get("type").is_some() {
            return Err(ToolError::Execution(format!(
                "sources[{index}].type is a legacy pseudo-source field; pass current source-card fields instead"
            )));
        }
    }
    let mut args = json!({"sources": sources});
    copy_fields(
        &mut args,
        input,
        &[
            "goal",
            "fetches",
            "include_unfetched_sources",
            "max_sources",
            "max_fetched_items",
            "max_total_chars",
        ],
    );
    Ok(args)
}

/// Translate a native `websearch` call into an eggsearch `web_search`
/// call and execute it.
///
/// Eggsearch argument shape (subject to upstream change):
///
/// ```json
/// {
///   "query": "...",
///   "max_results": 8,
///   "providers": ["duckduckgo", "mojeek"],
///   "timeout_ms": null
/// }
/// ```
pub async fn call_web_search(
    mcp_server: &str,
    input: &Value,
    max_output_chars: usize,
    timeout_ms: u64,
) -> Result<String, ToolError> {
    let args = build_web_search_args(input)?;

    let svc = super::state::mcp_service()
        .ok_or_else(|| eggsearch_unavailable("McpService is not initialized"))?;
    let raw = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
        let guard = svc.read().await;
        guard.call_tool(mcp_server, "web_search", args).await
    })
    .await
    .map_err(|_| {
        ToolError::Timeout(format!(
            "eggsearch web_search timed out after {timeout_ms}ms"
        ))
    })?
    .map_err(|e| ToolError::Execution(format!("eggsearch web_search: {e}")))?;

    let (capped, truncated) = clamp_output(&raw, max_output_chars, "max_search_output_chars");
    super::state::set_last_truncated(truncated);
    Ok(frame_search_results(&capped, "eggsearch"))
}

/// Translate a native `webfetch` call into an eggsearch `web_fetch`
/// call and execute it.
pub async fn call_web_fetch(
    mcp_server: &str,
    input: &Value,
    max_output_chars: usize,
    timeout_ms: u64,
) -> Result<String, ToolError> {
    let args = build_web_fetch_args(input)?;

    let svc = super::state::mcp_service()
        .ok_or_else(|| eggsearch_unavailable("McpService is not initialized"))?;
    let raw = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
        let guard = svc.read().await;
        guard.call_tool(mcp_server, "web_fetch", args).await
    })
    .await
    .map_err(|_| {
        ToolError::Timeout(format!(
            "eggsearch web_fetch timed out after {timeout_ms}ms"
        ))
    })?
    .map_err(|e| ToolError::Execution(format!("eggsearch web_fetch: {e}")))?;

    let (capped, truncated) = clamp_output(&raw, max_output_chars, "max_fetch_output_chars");
    super::state::set_last_truncated(truncated);
    Ok(frame_fetched_page(&capped, "eggsearch"))
}

/// Translate a native `repo_search` call into an eggsearch `repo_search`
/// call and execute it.
pub async fn call_repo_search(
    mcp_server: &str,
    input: &Value,
    max_output_chars: usize,
    timeout_ms: u64,
) -> Result<String, ToolError> {
    let query = input
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .ok_or_else(|| ToolError::Execution("missing 'query' parameter".to_string()))?;
    let mut args = json!({"query": query});
    if input.get("include_snippets").is_some() {
        return Err(ToolError::Execution(
            "'include_snippets' is not part of the current eggsearch repo_search contract; omit it and use the returned source snippets".to_string(),
        ));
    }
    if input.get("repo").is_some() || input.get("owner").is_some() {
        add_repository_locator(&mut args, repository_locator(input, false)?);
    }
    copy_fields(
        &mut args,
        input,
        &[
            "host",
            "path",
            "file",
            "language",
            "symbol",
            "profile",
            "include_local",
            "mode",
        ],
    );
    args["max_results"] = json!(input
        .get("max_results")
        .and_then(Value::as_u64)
        .unwrap_or(10)
        .min(30));

    let svc = super::state::mcp_service()
        .ok_or_else(|| eggsearch_unavailable("McpService is not initialized"))?;
    let raw = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
        let guard = svc.read().await;
        guard.call_tool(mcp_server, "repo_search", args).await
    })
    .await
    .map_err(|_| {
        ToolError::Timeout(format!(
            "eggsearch repo_search timed out after {timeout_ms}ms"
        ))
    })?
    .map_err(|e| ToolError::Execution(format!("eggsearch repo_search: {e}")))?;

    let (capped, truncated) = clamp_output(&raw, max_output_chars, "max_repo_output_chars");
    super::state::set_last_truncated(truncated);
    Ok(frame_repo_results(&capped, "eggsearch"))
}

/// Translate a native `repo_fetch` call into an eggsearch `repo_fetch`
/// call and execute it.
pub async fn call_repo_fetch(
    mcp_server: &str,
    input: &Value,
    max_output_chars: usize,
    timeout_ms: u64,
) -> Result<String, ToolError> {
    let path = non_empty_string(input, "path")?
        .ok_or_else(|| ToolError::Execution("missing 'path' parameter".to_string()))?;
    let mut args = json!({"path": path});
    add_repository_locator(&mut args, repository_locator(input, true)?);
    copy_fields(
        &mut args,
        input,
        &[
            "commit_sha",
            "context_before",
            "context_after",
            "max_chars",
            "symbol",
            "symbol_kind",
            "match_text",
            "expand_to_block",
            "max_block_lines",
            "prefer_local",
        ],
    );
    if let Some(line_start) = range_alias(input, "line_start", "start_line")? {
        args["line_start"] = json!(line_start);
    }
    if let Some(line_end) = range_alias(input, "line_end", "end_line")? {
        args["line_end"] = json!(line_end);
    }

    let svc = super::state::mcp_service()
        .ok_or_else(|| eggsearch_unavailable("McpService is not initialized"))?;
    let raw = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
        let guard = svc.read().await;
        guard.call_tool(mcp_server, "repo_fetch", args).await
    })
    .await
    .map_err(|_| {
        ToolError::Timeout(format!(
            "eggsearch repo_fetch timed out after {timeout_ms}ms"
        ))
    })?
    .map_err(|e| ToolError::Execution(format!("eggsearch repo_fetch: {e}")))?;

    let (capped, truncated) = clamp_output(&raw, max_output_chars, "max_repo_output_chars");
    super::state::set_last_truncated(truncated);
    Ok(frame_repo_file(&capped, "eggsearch"))
}

/// Translate a native `repo_map` call into an eggsearch `repo_map`
/// call and execute it.
pub async fn call_repo_map(
    mcp_server: &str,
    input: &Value,
    max_output_chars: usize,
    timeout_ms: u64,
) -> Result<String, ToolError> {
    if input
        .get("path")
        .and_then(Value::as_str)
        .is_some_and(|path| !path.trim().is_empty())
    {
        return Err(ToolError::Execution(
            "repo_map does not support a subdirectory 'path' in the current eggsearch contract; use repo_search or repo_fetch for a path-scoped request".to_string(),
        ));
    }
    let mut args = json!({});
    add_repository_locator(&mut args, repository_locator(input, true)?);
    copy_fields(
        &mut args,
        input,
        &[
            "commit_sha",
            "max_entries",
            "include_files",
            "include_directories",
            "include_ci",
            "include_security",
        ],
    );
    let depth = input
        .get("max_depth")
        .or_else(|| input.get("depth"))
        .and_then(Value::as_u64)
        .unwrap_or(2)
        .min(3);
    args["max_depth"] = json!(depth);

    let svc = super::state::mcp_service()
        .ok_or_else(|| eggsearch_unavailable("McpService is not initialized"))?;
    let raw = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
        let guard = svc.read().await;
        guard.call_tool(mcp_server, "repo_map", args).await
    })
    .await
    .map_err(|_| ToolError::Timeout(format!("eggsearch repo_map timed out after {timeout_ms}ms")))?
    .map_err(|e| ToolError::Execution(format!("eggsearch repo_map: {e}")))?;

    let (capped, truncated) = clamp_output(&raw, max_output_chars, "max_repo_output_chars");
    super::state::set_last_truncated(truncated);
    Ok(frame_repo_map(&capped, "eggsearch"))
}

/// Translate a native `security_search` call into an eggsearch
/// `security_search` call and execute it.
pub async fn call_security_search(
    mcp_server: &str,
    input: &Value,
    max_output_chars: usize,
    timeout_ms: u64,
) -> Result<String, ToolError> {
    let query = input
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .ok_or_else(|| ToolError::Execution("missing 'query' parameter".to_string()))?;
    let mut args = json!({"query": query});
    copy_fields(
        &mut args,
        input,
        &[
            "ecosystem",
            "package",
            "version",
            "cve_id",
            "ghsa_id",
            "osv_id",
            "rustsec_id",
            "severity_min",
            "include_kev",
            "include_exploit_context",
            "include_defensive_guidance",
            "include_vendor_advisories",
            "max_per_group",
            "freshness",
            "providers",
            "assess_applicability",
            "dependency_files",
            "workflow",
        ],
    );
    if let Some(cve) = non_empty_string(input, "cve")? {
        if input
            .get("cve_id")
            .is_some_and(|value| value != &Value::String(cve.clone()))
        {
            return Err(ToolError::Execution(
                "'cve' and 'cve_id' disagree; use the current 'cve_id' field".to_string(),
            ));
        }
        args["cve_id"] = Value::String(cve);
    }
    args["max_results"] = json!(input
        .get("max_results")
        .and_then(Value::as_u64)
        .unwrap_or(10)
        .min(20));

    let svc = super::state::mcp_service()
        .ok_or_else(|| eggsearch_unavailable("McpService is not initialized"))?;
    let raw = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
        let guard = svc.read().await;
        guard.call_tool(mcp_server, "security_search", args).await
    })
    .await
    .map_err(|_| {
        ToolError::Timeout(format!(
            "eggsearch security_search timed out after {timeout_ms}ms"
        ))
    })?
    .map_err(|e| ToolError::Execution(format!("eggsearch security_search: {e}")))?;

    let (capped, truncated) = clamp_output(&raw, max_output_chars, "max_security_output_chars");
    super::state::set_last_truncated(truncated);
    Ok(frame_security_results(&capped, "eggsearch"))
}

/// Translate a native `research_search` call into an eggsearch
/// `research_search` call and execute it.
pub async fn call_research_search(
    mcp_server: &str,
    input: &Value,
    max_output_chars: usize,
    timeout_ms: u64,
) -> Result<String, ToolError> {
    let query = input
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .ok_or_else(|| ToolError::Execution("missing 'query' parameter".to_string()))?;
    let mut args = json!({"query": query});
    copy_fields(
        &mut args,
        input,
        &[
            "research_domain",
            "desired_source_types",
            "include_counterpoints",
            "include_primary_sources",
            "include_recent_discussion",
            "include_security_considerations",
            "freshness",
            "providers",
            "workflow",
            "depth",
            "compare_targets",
            "constraints",
            "known_context",
        ],
    );
    translate_legacy_research_domains(input, &mut args)?;
    args["max_results"] = json!(input
        .get("max_results")
        .and_then(Value::as_u64)
        .unwrap_or(10)
        .min(15));

    let svc = super::state::mcp_service()
        .ok_or_else(|| eggsearch_unavailable("McpService is not initialized"))?;
    let raw = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
        let guard = svc.read().await;
        guard.call_tool(mcp_server, "research_search", args).await
    })
    .await
    .map_err(|_| {
        ToolError::Timeout(format!(
            "eggsearch research_search timed out after {timeout_ms}ms"
        ))
    })?
    .map_err(|e| ToolError::Execution(format!("eggsearch research_search: {e}")))?;

    let (capped, truncated) = clamp_output(&raw, max_output_chars, "max_research_output_chars");
    super::state::set_last_truncated(truncated);
    Ok(frame_research_results(&capped, "eggsearch"))
}

/// Translate a native `batch_fetch` call into an eggsearch
/// `batch_fetch` call and execute it.
pub async fn call_batch_fetch(
    mcp_server: &str,
    input: &Value,
    max_output_chars: usize,
    timeout_ms: u64,
) -> Result<String, ToolError> {
    let args = build_batch_fetch_args(input)?;

    let svc = super::state::mcp_service()
        .ok_or_else(|| eggsearch_unavailable("McpService is not initialized"))?;
    let raw = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
        let guard = svc.read().await;
        guard.call_tool(mcp_server, "batch_fetch", args).await
    })
    .await
    .map_err(|_| {
        ToolError::Timeout(format!(
            "eggsearch batch_fetch timed out after {timeout_ms}ms"
        ))
    })?
    .map_err(|e| ToolError::Execution(format!("eggsearch batch_fetch: {e}")))?;

    let (capped, truncated) = clamp_output(&raw, max_output_chars, "max_batch_output_chars");
    super::state::set_last_truncated(truncated);
    Ok(frame_batch_results(&capped, "eggsearch"))
}

/// Translate a native `build_evidence_bundle` call into an eggsearch
/// `build_evidence_bundle` call and execute it.
pub async fn call_build_evidence_bundle(
    mcp_server: &str,
    input: &Value,
    max_output_chars: usize,
    timeout_ms: u64,
) -> Result<String, ToolError> {
    let args = build_evidence_bundle_args(input)?;

    let svc = super::state::mcp_service()
        .ok_or_else(|| eggsearch_unavailable("McpService is not initialized"))?;
    let raw = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
        let guard = svc.read().await;
        guard
            .call_tool(mcp_server, "build_evidence_bundle", args)
            .await
    })
    .await
    .map_err(|_| {
        ToolError::Timeout(format!(
            "eggsearch build_evidence_bundle timed out after {timeout_ms}ms"
        ))
    })?
    .map_err(|e| ToolError::Execution(format!("eggsearch build_evidence_bundle: {e}")))?;

    let (capped, truncated) = clamp_output(&raw, max_output_chars, "max_evidence_output_chars");
    super::state::set_last_truncated(truncated);
    Ok(frame_evidence_bundle(&capped, "eggsearch"))
}

/// Best-effort translation of the historical Codegg `provider` hint to
/// eggsearch's `providers` list. Returns `Some(vec)` when the user
/// pinned a specific provider, `Some(vec![])` (i.e. "let eggsearch
/// auto-pick") for `auto`/missing or any hint that eggsearch does not
/// recognize, or `None` to mean "omit the field entirely".
///
/// Note: The model-facing enum advertises a long list of historical
/// provider hints, but we do not have ground truth on which of them
/// eggsearch supports today. Hints that eggsearch does not recognize
/// are intentionally mapped to an empty list (auto-pick) so that the
/// search still succeeds with a sensible default provider.
pub(crate) fn translate_provider_hint(hint: Option<&str>) -> Option<Vec<String>> {
    let h = hint.unwrap_or("auto");
    match h {
        "auto" => Some(Vec::new()),
        "duckduckgo" => Some(vec!["duckduckgo".to_string()]),
        "mojeek" => Some(vec!["mojeek".to_string()]),
        "wikipedia" => Some(vec!["wikipedia".to_string()]),
        "arxiv" => Some(vec!["arxiv".to_string()]),
        "openalex" => Some(vec!["openalex".to_string()]),
        "pubmed" => Some(vec!["pubmed".to_string()]),
        "hn_algolia" => Some(vec!["hn_algolia".to_string()]),
        "google_news" => Some(vec!["google_news".to_string()]),
        "github" => Some(vec!["github".to_string()]),
        "exa" => Some(vec!["exa".to_string()]),
        "tavily" => Some(vec!["tavily".to_string()]),
        "brave" | "brave_api" => Some(vec!["brave_api".to_string()]),
        "kagi" => Some(vec!["kagi".to_string()]),
        "serpapi" => Some(vec!["serpapi".to_string()]),
        // Unknown hint: let eggsearch decide.
        _ => Some(Vec::new()),
    }
}

pub fn eggsearch_unavailable(detail: &str) -> ToolError {
    ToolError::Execution(format!(
        "eggsearch backend is configured but unavailable: {detail}. \
         Install eggsearch or set [search].backend = \"builtin\" / \"disabled\"."
    ))
}

/// Validate a URL before forwarding it to eggsearch for fetching.
/// Rejects empty URLs, non-HTTP(S) schemes, and overlong URLs (>2048 bytes).
fn validate_fetch_url(url: &str) -> Result<(), ToolError> {
    if url.is_empty() {
        return Err(ToolError::Execution(
            "fetch URL must not be empty".to_string(),
        ));
    }
    if url.len() > 2048 {
        return Err(ToolError::Execution(format!(
            "fetch URL is too long ({} bytes, max 2048)",
            url.len()
        )));
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(ToolError::Execution(format!(
            "fetch URL must use http or https scheme, got: {}",
            &url[..url.len().min(64)]
        )));
    }
    Ok(())
}

/// Produce an actionable error when eggsearch is connected but a
/// specific MCP tool is not advertised by the server.
pub fn eggsearch_tool_missing(
    server_name: &str,
    codegg_tool: &str,
    upstream_tool: &str,
    discovered_tools: &[String],
) -> ToolError {
    let tool_list = if discovered_tools.is_empty() {
        "(none discovered)".to_string()
    } else {
        discovered_tools.join(", ")
    };
    ToolError::Execution(format!(
        "eggsearch backend is connected but tool {upstream_tool} is not advertised \
         by server {server_name}. Discovered tools: {tool_list}. \
         Requested by Codegg wrapper: {codegg_tool}. \
         Upgrade eggsearch or disable Codegg {codegg_tool}."
    ))
}

/// Check that a specific upstream MCP tool is available on the
/// server. Returns `Ok(())` if the tool is present, or a descriptive
/// error if it is missing.
pub fn ensure_tool_available(
    mcp_server: &str,
    codegg_tool: &str,
    upstream_tool: &str,
) -> Result<(), ToolError> {
    let svc = match super::state::mcp_service() {
        Some(s) => s,
        None => return Ok(()), // will be caught by the call itself
    };
    // Read the tool list synchronously (RwLock read is cheap).
    let guard = svc.try_read().map_err(|_| {
        ToolError::Execution(format!(
            "eggsearch: could not check tool availability for {upstream_tool}"
        ))
    })?;
    let tools = guard.server_tools();
    let discovered: Vec<String> = tools
        .get(mcp_server)
        .map(|t| t.iter().map(|x| x.name.clone()).collect())
        .unwrap_or_default();
    drop(guard);
    if discovered.iter().any(|t| t == upstream_tool) {
        Ok(())
    } else {
        Err(eggsearch_tool_missing(
            mcp_server,
            codegg_tool,
            upstream_tool,
            &discovered,
        ))
    }
}

/// Best-effort provider_status query, used by the doctor command.
pub async fn call_provider_status(mcp_server: &str, timeout_ms: u64) -> Result<String, ToolError> {
    let svc = super::state::mcp_service()
        .ok_or_else(|| eggsearch_unavailable("McpService is not initialized"))?;
    let raw = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), async {
        let guard = svc.read().await;
        guard
            .call_tool(mcp_server, "provider_status", json!({}))
            .await
    })
    .await
    .map_err(|_| {
        ToolError::Timeout(format!(
            "eggsearch provider_status timed out after {timeout_ms}ms"
        ))
    })?
    .map_err(|e| ToolError::Execution(format!("eggsearch provider_status: {e}")))?;
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_hint_auto_returns_empty() {
        let v = translate_provider_hint(Some("auto"));
        assert_eq!(v, Some(vec![]));
    }

    #[test]
    fn provider_hint_duckduckgo_passes_through() {
        assert_eq!(
            translate_provider_hint(Some("duckduckgo")),
            Some(vec!["duckduckgo".to_string()])
        );
    }

    #[test]
    fn provider_hint_mojeek_passes_through() {
        assert_eq!(
            translate_provider_hint(Some("mojeek")),
            Some(vec!["mojeek".to_string()])
        );
    }

    #[test]
    fn provider_hint_brave_maps_to_brave_api() {
        let v = translate_provider_hint(Some("brave"));
        assert_eq!(v, Some(vec!["brave_api".to_string()]));
    }

    #[test]
    fn provider_hint_unsupported_returns_empty_for_auto_pick() {
        // The adapter intentionally treats unknown historical hints as "auto"
        // (empty list) so that eggsearch can pick a provider.
        assert_eq!(
            translate_provider_hint(Some("unsupported_historical")),
            Some(vec![])
        );
    }

    #[test]
    fn provider_hint_unknown_returns_empty() {
        let v = translate_provider_hint(Some("mystery"));
        assert_eq!(v, Some(vec![]));
    }

    /// Verify that when a `websearch` call dispatches to the
    /// eggsearch backend and eggsearch is unavailable, we return
    /// the documented actionable error (used to drive the
    /// "missing eggsearch" acceptance criterion).
    ///
    /// The test is intentionally permissive about the exact error
    /// text because the failure surface depends on whether a stale
    /// `McpService` from a previous test is still in the global
    /// state slot. We assert the *contract*: when the eggsearch
    /// backend is selected and the underlying service is not
    /// usable, the error must mention "eggsearch" so the user
    /// can debug.
    #[tokio::test]
    async fn web_search_unavailable_returns_actionable_error() {
        let _cp = crate::search_backend::test_support::acquire_cross_process_lock();
        let _g = crate::search_backend::test_support::SHARED_TEST_LOCK
            .lock()
            .await;
        crate::search_backend::state::reset_for_tests();
        crate::search_backend::state::install_search_config(crate::config::schema::SearchConfig {
            backend: Some(crate::config::schema::SearchBackendConfig::Eggsearch),
            ..Default::default()
        });
        let res = super::super::dispatch_web_search(&serde_json::json!({"query": "test"})).await;
        let err = res.expect_err("should be unavailable");
        let msg = err.to_string();
        // Either: the documented actionable "eggsearch backend is
        // configured but unavailable" error, or a downstream
        // "server eggsearch not found" error from a stale
        // McpService. Both surface actionable information about
        // eggsearch.
        assert!(
            msg.contains("eggsearch backend is configured but unavailable")
                || msg.contains("server eggsearch not found")
                || msg.contains("McpService is not initialized"),
            "expected actionable eggsearch error, got: {msg}"
        );
    }

    #[test]
    fn validate_fetch_url_rejects_empty() {
        let err = super::validate_fetch_url("").unwrap_err();
        assert!(err.to_string().contains("must not be empty"));
    }

    #[test]
    fn validate_fetch_url_rejects_non_http_scheme() {
        let err = super::validate_fetch_url("ftp://example.com/file").unwrap_err();
        assert!(err.to_string().contains("http or https scheme"));
    }

    #[test]
    fn validate_fetch_url_rejects_overlong_url() {
        let long_url = format!("https://example.com/{}", "x".repeat(2100));
        let err = super::validate_fetch_url(&long_url).unwrap_err();
        assert!(err.to_string().contains("too long"));
    }

    #[test]
    fn validate_fetch_url_accepts_valid_http() {
        assert!(super::validate_fetch_url("http://example.com").is_ok());
    }

    #[test]
    fn validate_fetch_url_accepts_valid_https() {
        assert!(super::validate_fetch_url("https://example.com/path?q=1").is_ok());
    }

    #[test]
    fn repository_locator_splits_unambiguous_legacy_form() {
        let locator = repository_locator(&json!({"repo": "owner/name"}), true).unwrap();
        assert_eq!(
            locator,
            RepositoryLocator {
                host: None,
                owner: Some("owner".to_string()),
                repo: "name".to_string(),
                ref_name: None,
            }
        );
    }

    #[test]
    fn repository_locator_rejects_ambiguous_nested_legacy_form() {
        let err = repository_locator(&json!({"repo": "group/subgroup/name"}), true).unwrap_err();
        assert!(err.to_string().contains("ambiguous"));
    }

    #[test]
    fn repository_locator_prefers_explicit_fields_and_forwards_host_ref() {
        let locator = repository_locator(
            &json!({
                "host": "gitlab",
                "owner": "group/subgroup",
                "repo": "project",
                "ref_name": "main",
            }),
            true,
        )
        .unwrap();
        assert_eq!(locator.owner.as_deref(), Some("group/subgroup"));
        assert_eq!(locator.host.as_deref(), Some("gitlab"));
        assert_eq!(locator.ref_name.as_deref(), Some("main"));
    }

    #[test]
    fn range_aliases_translate_and_conflicts_fail() {
        assert_eq!(
            range_alias(&json!({"start_line": 4}), "line_start", "start_line").unwrap(),
            Some(4)
        );
        let err = range_alias(
            &json!({"line_start": 4, "start_line": 5}),
            "line_start",
            "start_line",
        )
        .unwrap_err();
        assert!(err.to_string().contains("disagree"));
    }

    #[test]
    fn legacy_research_provider_domains_translate_without_stale_field() {
        let mut args = json!({"query": "papers"});
        translate_legacy_research_domains(&json!({"domains": ["arxiv"]}), &mut args).unwrap();
        assert_eq!(args["providers"], json!(["arxiv"]));
        assert!(args.get("domains").is_none());
    }

    #[test]
    fn legacy_research_ambiguous_domains_fail() {
        let mut args = json!({"query": "papers"});
        let err = translate_legacy_research_domains(
            &json!({"domains": ["machine learning", "systems"]}),
            &mut args,
        )
        .unwrap_err();
        assert!(err.to_string().contains("ambiguous"));
    }

    #[test]
    fn batch_urls_become_tagged_web_items() {
        let args = build_batch_fetch_args(&json!({"urls": ["https://example.com"]})).unwrap();
        assert_eq!(
            args["items"],
            json!([{"type": "web", "url": "https://example.com"}])
        );
        assert!(args.get("urls").is_none());
    }

    #[test]
    fn evidence_bundle_accepts_fetch_only_current_input() {
        let args = build_evidence_bundle_args(&json!({
            "fetches": [{"url": "https://example.com", "fetched": true}]
        }))
        .unwrap();
        assert_eq!(args["sources"], json!([]));
        assert!(args["fetches"].is_array());
    }

    #[test]
    fn eggsearch_tool_missing_includes_upstream_name() {
        let err = super::eggsearch_tool_missing(
            "eggsearch",
            "websearch",
            "web_search",
            &["web_fetch".to_string()],
        );
        let msg = err.to_string();
        assert!(msg.contains("web_search"));
        assert!(msg.contains("websearch"));
        assert!(msg.contains("web_fetch"));
    }

    #[test]
    fn eggsearch_tool_missing_empty_tools_list() {
        let err = super::eggsearch_tool_missing("eggsearch", "repo_search", "repo_search", &[]);
        let msg = err.to_string();
        assert!(msg.contains("(none discovered)"));
    }

    #[test]
    fn eggsearch_unavailable_message_contains_actionable_hint() {
        let err = super::eggsearch_unavailable("test detail");
        let msg = err.to_string();
        assert!(msg.contains("eggsearch"));
        assert!(msg.contains("test detail"));
        assert!(msg.contains("builtin") || msg.contains("disabled"));
    }
}
