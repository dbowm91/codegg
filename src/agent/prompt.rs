use crate::agent::Agent;
use crate::config::schema::Config;
use crate::model_profile::{PromptProfileKind, ResolvedModelProfile};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::path::Path;

const INSTRUCTION_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md", "CONTEXT.md"];

pub fn render_prompt_template(template: &str, variables: &HashMap<&str, &str>) -> String {
    let mut result = template.to_string();
    for (key, value) in variables {
        result = result.replace(&format!("{{{{{key}}}}}"), value);
        result = result.replace(&format!("{{{key}}}"), value);
    }
    result
}

static BUILTIN_PROMPTS: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let mut map = HashMap::new();
    map.insert(
        "build",
        "You are a build agent. Execute commands, edit files, and perform tasks to build and test the project. \
         For in-depth, comparative, or multi-hop research questions, spawn a `research` subagent via \
         `task({action: 'spawn', agent: 'research', prompt: '…'})` — the subagent runs the full research \
         pipeline and returns a synthesized answer with citations. For quick lookups, use the `websearch` \
         tool directly (default: DuckDuckGo, no key required).",
    );
    map.insert(
        "plan",
        "You are a planning agent. Analyze the codebase and create detailed implementation plans. Do not modify files or execute commands. \
         For in-depth research, spawn a `research` subagent via `task({action: 'spawn', agent: 'research', prompt: '…'})`. \
         For quick lookups, use `websearch` (default: DuckDuckGo, no key required).",
    );
    map.insert(
        "general",
        "You are a general-purpose subagent. Complete the assigned task efficiently without managing todos.",
    );
    map.insert(
        "explore",
        "You are an exploration agent. Read and analyze code to understand structure and relationships. Do not modify files.",
    );
    map.insert(
        "title",
        "Generate a concise, descriptive title for the conversation. Return only the title text.",
    );
    map.insert(
        "summary",
        "Generate a concise summary of the conversation. Include key decisions, changes made, and remaining tasks.",
    );
    map.insert(
        "compaction",
        "Compress the conversation history while preserving essential context, decisions, and state.",
    );
    map.insert(
        "debug",
        "You are a debugging agent. Investigate errors, trace issues to their root cause, and propose fixes. Analyze logs, stack traces, and code flow.",
    );
    map.insert(
        "refactor",
        "You are a refactoring agent. Improve code structure, readability, and maintainability without changing behavior. Focus on clean code principles.",
    );
    map.insert(
        "review",
        "You are a code review agent. Analyze code for bugs, security issues, performance problems, and style inconsistencies. Provide constructive feedback.",
    );
    map.insert(
        "test",
        "You are a testing agent. Write and maintain tests, verify bug fixes, and ensure code correctness. Focus on edge cases and coverage.",
    );
    map.insert(
        "document",
        "You are a documentation agent. Improve code documentation, README files, and inline comments. Make complex code more understandable.",
    );
    map
});

pub fn select_provider_prompt(model_id: &str) -> &'static str {
    let id = model_id.to_lowercase();
    if id.starts_with("gpt-4")
        || id.starts_with("o1")
        || id.starts_with("o3")
        || id.starts_with("o4")
    {
        include_str!("prompts/beast.txt")
    } else if id.starts_with("codex") || id.contains("/codex") {
        include_str!("prompts/codex.txt")
    } else if id.starts_with("gpt") {
        include_str!("prompts/gpt.txt")
    } else if id.starts_with("gemini") || id.starts_with("gemini-2") {
        include_str!("prompts/gemini.txt")
    } else if id.contains("claude")
        || id.contains("sonnet")
        || id.contains("opus")
        || id.contains("haiku")
    {
        include_str!("prompts/anthropic.txt")
    } else if id.starts_with("trinity") || id.contains("/trinity") {
        include_str!("prompts/trinity.txt")
    } else if id.starts_with("kimi") || id.contains("/kimi") {
        include_str!("prompts/kimi.txt")
    } else {
        include_str!("prompts/default.txt")
    }
}

pub fn assemble_system_prompt(
    agent: &Agent,
    config: &Config,
    tools: &[String],
    skills: &[String],
    custom_instructions: Option<&str>,
) -> String {
    let mut parts = Vec::new();

    if let Some(prompt) = &agent.system_prompt {
        parts.push(prompt.clone());
    }

    parts.push(format!(
        "You are the {} agent. {}",
        agent.name, agent.description
    ));

    if !tools.is_empty() {
        let tool_list = tools.join(", ");
        parts.push(format!("Available tools: {tool_list}"));
    }

    if !skills.is_empty() {
        let skill_list = skills.join(", ");
        parts.push(format!("Available skills: {skill_list}"));
    }

    if let Some(model) = &agent.model {
        parts.push(format!("Using model: {model}"));
    }

    if let Some(instructions) = config.instructions.as_ref() {
        for instruction in instructions {
            parts.push(instruction.clone());
        }
    }

    if let Some(instructions) = custom_instructions {
        parts.push(instructions.to_string());
    }

    parts.join("\n\n")
}

pub fn load_instructions(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

pub fn find_instructions_file() -> Option<String> {
    let candidates = [".codegg/instructions.md", "INSTRUCTIONS.md"];

    if let Ok(cwd) = std::env::current_dir() {
        for candidate in &candidates {
            let path = cwd.join(candidate);
            if path.exists() {
                return load_instructions(&path);
            }
        }
    }

    if let Some(config_dir) = dirs::config_dir() {
        let global = config_dir.join("codegg").join("instructions.md");
        if global.exists() {
            return load_instructions(&global);
        }
    }

    None
}

pub fn find_all_instruction_files() -> Vec<String> {
    let mut contents = Vec::new();
    let Ok(cwd) = std::env::current_dir() else {
        return contents;
    };
    let current = cwd.as_path();
    let mut git_root: Option<&Path> = None;
    let mut walker = current;
    loop {
        if walker.join(".git").exists() {
            git_root = Some(walker);
            break;
        }
        match walker.parent() {
            Some(parent) => walker = parent,
            None => break,
        }
    }
    let stop_at = git_root.unwrap_or(current);
    let mut search = current;
    loop {
        for file in INSTRUCTION_FILES {
            let path = search.join(file);
            if path.exists() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    contents.push(content);
                }
            }
        }
        if search == stop_at {
            break;
        }
        match search.parent() {
            Some(parent) => search = parent,
            None => break,
        }
    }
    for candidate in &[".codegg/instructions.md", "INSTRUCTIONS.md"] {
        let path = cwd.join(candidate);
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                contents.push(content);
            }
        }
    }
    if let Some(config_dir) = dirs::config_dir() {
        let global = config_dir.join("codegg").join("instructions.md");
        if global.exists() {
            if let Ok(content) = std::fs::read_to_string(&global) {
                contents.push(content);
            }
        }
    }
    contents
}

pub async fn fetch_remote_instruction(url: &str) -> Option<String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return None;
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;

    match client.get(url).send().await {
        Ok(resp) => resp.text().await.ok(),
        Err(_) => None,
    }
}

pub fn is_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

pub async fn load_agent_prompt_async(agent: &Agent, config: &Config, model_id: &str) -> String {
    let mut parts = base_prompt_parts(agent, model_id);
    for content in find_all_instruction_files() {
        parts.push(content);
    }
    if let Some(instructions) = config.instructions.as_ref() {
        let urls: Vec<_> = instructions.iter().filter(|i| is_url(i)).collect();
        let non_urls: Vec<_> = instructions.iter().filter(|i| !is_url(i)).collect();

        for instruction in non_urls {
            parts.push(instruction.clone());
        }

        if !urls.is_empty() {
            let futures: Vec<_> = urls
                .iter()
                .map(|url| fetch_remote_instruction(url))
                .collect();
            let results = futures::future::join_all(futures).await;

            for (url, result) in urls.iter().zip(results) {
                match result {
                    Some(content) => parts.push(content),
                    None => parts.push(format!("[Failed to fetch remote instruction: {url}]")),
                }
            }
        }
    }
    parts.join("\n\n")
}

pub fn load_agent_prompt(agent: &Agent, config: &Config, model_id: &str) -> String {
    let mut parts = base_prompt_parts(agent, model_id);
    for content in find_all_instruction_files() {
        parts.push(content);
    }
    if let Some(instructions) = config.instructions.as_ref() {
        for instruction in instructions {
            if is_url(instruction) {
                parts.push(format!(
                    "[Remote instruction: {instruction} - fetched at runtime]"
                ));
            } else {
                parts.push(instruction.clone());
            }
        }
    }
    parts.join("\n\n")
}

fn base_prompt_parts(agent: &Agent, model_id: &str) -> Vec<String> {
    let mut parts = Vec::new();
    parts.push(select_provider_prompt(model_id).to_string());

    if let Some(prompt) = &agent.system_prompt {
        parts.push(prompt.clone());
        return parts;
    }

    let builtin_prompts = builtin_prompts();
    if let Some(prompt) = builtin_prompts.get(agent.name.as_str()) {
        parts.push(prompt.to_string());
    } else {
        parts.push(format!(
            "You are the {} agent. {}",
            agent.name, agent.description
        ));
    }
    if let Some(role) = agent.role.as_deref() {
        parts.push(subagent_output_contract(role).to_string());
    }
    parts
}

fn builtin_prompts() -> &'static HashMap<&'static str, &'static str> {
    &BUILTIN_PROMPTS
}

pub struct PromptContext<'a> {
    pub agent: &'a Agent,
    pub config: &'a Config,
    pub model_profile: &'a ResolvedModelProfile,
    pub tools: &'a [String],
    pub skills: &'a [String],
    pub custom_instructions: Option<&'a str>,
    /// Whether the agent is in plan mode. When true, a plan-mode contract
    /// is appended that tells the model what tools are available and what
    /// the planning surface looks like.
    pub is_plan_mode: bool,
    /// All known agent kinds. Used to inject the research-subagent
    /// addendum when a `research` subagent is spawnable.
    pub agents: &'a [Agent],
}

pub fn assemble_system_prompt_with_profile(ctx: PromptContext<'_>) -> String {
    let mut parts = Vec::new();

    parts.push(base_harness_contract().to_string());
    parts.push(goal_and_todos_contract().to_string());
    parts.push(role_contract(ctx.agent).to_string());
    if let Some(role) = ctx.agent.role.as_deref() {
        parts.push(subagent_output_contract(role).to_string());
    }
    parts.push(profile_contract(ctx.model_profile).to_string());

    if ctx.is_plan_mode {
        parts.push(plan_mode_contract().to_string());
    }

    // Inject the websearch contract whenever the model has access to
    // the `websearch` tool. This steers the model away from `curl` /
    // `wget` for web search and page retrieval.
    if ctx.tools.iter().any(|t| t == "websearch") {
        parts.push(websearch_contract().to_string());
    }

    // Inject the research-subagent addendum whenever the model can
    // spawn a `research` subagent via the `task` tool. The `task` tool
    // is always present for non-minimal agents, so the only gating
    // condition is "is `research` a known subagent kind".
    let research_spawnable = !ctx.is_plan_mode && ctx.agents.iter().any(|a| a.name == "research");
    if research_spawnable && ctx.tools.iter().any(|t| t == "task") {
        parts.push(research_subagent_contract().to_string());
    }

    if let Some(prompt) = &ctx.agent.system_prompt {
        parts.push(prompt.clone());
    }

    parts.push(format!(
        "You are the {} agent. {}",
        ctx.agent.name, ctx.agent.description
    ));

    if !ctx.tools.is_empty() {
        parts.push(format!("Available tools: {}", ctx.tools.join(", ")));
    }

    if !ctx.skills.is_empty() {
        parts.push(format!("Available skills: {}", ctx.skills.join(", ")));
    }

    parts.push(format!("Using model: {}", ctx.model_profile.model));

    if let Some(instructions) = ctx.config.instructions.as_ref() {
        for instruction in instructions {
            parts.push(instruction.clone());
        }
    }

    if let Some(instructions) = ctx.custom_instructions {
        parts.push(instructions.to_string());
    }

    parts
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn base_harness_contract() -> &'static str {
    "You are operating inside codegg, a coding agent harness. Use tools to inspect the repository before making claims about files, code, or project structure. Do not claim tests passed unless tool output confirms the test result. Prefer minimal, correct changes over broad rewrites."
}

/// Steering contract for long-horizon planning. Two surfaces:
///
/// * **In-flight planning** — use the `todo` tool. A todo is a single
///   step the user can check off within the current turn. Update
///   todos as you complete steps so the user can see progress.
///
/// * **Long-horizon planning** — when work spans many turns, many
///   sessions, or exceeds the budget of a single in-flight todo,
///   call `goal_set` (or the `/goal` slash command) to set a
///   long-running goal with an objective, success criteria, and
///   optional budget. As work progresses, call `goal_update_progress`
///   with phase/next-action updates. When the objective is met,
///   call `goal_request_completion` with concrete evidence (commands
///   run, files changed, tests passing) and `remaining_risks`.
///
/// Do not mark a goal complete from a todo check-off alone. A
/// successful todo is one of many steps toward the goal, not
/// the goal itself. The runtime will validate evidence before
/// transitioning the goal to `Complete`.
fn goal_and_todos_contract() -> &'static str {
    "Planning surfaces: use the `todo` tool for in-flight steps the user can check off within this turn. When work spans many turns or sessions, set a long-horizon goal with `goal_set` (or `/goal set <objective>`), then track phase/next-action with `goal_update_progress`. Mark completion with `goal_request_completion` carrying concrete evidence (commands run, files changed, tests passing) and an explicit `remaining_risks` list. A finished todo is a step toward a goal, not the goal itself — the runtime validates goal completion against evidence."
}

/// Contract injected into the system prompt when the agent is in plan mode.
///
/// Plan mode hides mutating tools from the model and exposes a planning
/// surface (todowrite/todoread) plus read-only inspection tools (read, glob,
/// grep, list, codesearch, websearch, webfetch, lsp, skill) and read-only
/// bash. The model is told explicitly so it doesn't try to use tools that
/// don't exist in its schema and doesn't attempt workarounds like writing
/// a plan file via bash heredoc when todowrite is the intended surface.
pub fn plan_mode_contract() -> &'static str {
    "PLAN MODE ACTIVE. You are in a read-only planning environment. Available tools: read, glob, grep, list, codesearch, websearch, webfetch, lsp, skill (information gathering), todowrite, todoread (use todowrite to record plan steps — this is the recommended way to communicate the plan to the user), bash for read-only commands only (ls, cat, grep, git status, cargo check, etc.; destructive shell is rejected automatically), and plan_enter/plan_exit (toggle plan mode). You MUST NOT: edit, write, or modify source files; run mutating shell commands (rm, mv, install scripts, etc.); or spawn subagents that modify state. To switch back to build mode, call plan_exit (typically after the user has approved the plan)."
}

/// Contract injected when the agent has access to the `websearch` tool.
///
/// The `websearch` tool defaults to DuckDuckGo (no API key required) with
/// Mojeek as a last-resort fallback. If `EXA_API_KEY` / `TAVILY_API_KEY` /
/// `BRAVE_API_KEY` / `KAGI_API_KEY` / `SERPAPI_API_KEY` is set in the
/// environment, that backend is used first. The tool can also route to
/// Wikipedia, arXiv, OpenAlex, PubMed, Hacker News, Google News, and
/// GitHub for domain-specific queries. Use `webfetch` only for a specific
/// known URL. **Do not use `curl` / `wget` for web search or page
/// retrieval** — they are rate-limited, blocked, or unsafe.
pub fn websearch_contract() -> &'static str {
    "**Web access contract**: For web information needs, prefer the `websearch` tool. It defaults to DuckDuckGo (no API key required) with Mojeek as a last-resort fallback. If `EXA_API_KEY` / `TAVILY_API_KEY` / `BRAVE_API_KEY` / `KAGI_API_KEY` / `SERPAPI_API_KEY` is set, that backend is used first. The tool can also route to Wikipedia, arXiv, OpenAlex, PubMed, Hacker News, Google News, and GitHub for domain-specific queries (the `provider` parameter selects explicitly: e.g. `provider: 'arxiv'`). Use `webfetch` only for a specific known URL. **Do not use `curl` / `wget` for web search or page retrieval** — they are rate-limited, blocked, or unsafe."
}

/// Optional addendum injected when the `research` subagent is available.
/// The main `build` / `plan` agent can spawn a `research` subagent via
/// `task({action: 'spawn', agent: 'research', prompt: '…'})` for in-depth,
/// multi-source research with synthesis and citations.
pub fn research_subagent_contract() -> &'static str {
    "**Long-horizon research**: You can spawn a `research` subagent via `task({action: 'spawn', agent: 'research', prompt: '<question>'})` for in-depth, multi-source research. The subagent runs the full research pipeline (source collection, evidence extraction, claim construction, synthesis) and returns a structured answer with citations. Use it when the question is open-ended, comparative, or requires more than a quick web lookup. For a single quick lookup, use the `websearch` tool directly."
}

fn role_contract(agent: &Agent) -> &'static str {
    match agent.role.as_deref().unwrap_or("executor") {
        "planner" => "Role contract: You are a planning agent. Analyze the repository and produce an implementation plan. Do not modify files.",
        "explorer" => "Role contract: You are an exploration agent. Inspect and explain repository structure. Do not modify files.",
        "summarizer" => "Role contract: You are a summarization agent. Preserve decisions, state, changed files, remaining risks, and next actions.",
        "compactor" => "Role contract: You are a context compaction agent. Preserve task state, decisions, file paths, tool results, and unresolved issues.",
        "reviewer" => "Role contract: You are a review agent. Look for correctness, safety, regression risk, missing tests, and excessive scope.",
        "security_reviewer" => "Role contract: You are a security review agent. Focus on realistic exploit paths, affected surfaces, and mitigations. Distinguish confirmed issues from speculative risks.",
        "title" => "Role contract: You are a title generation agent. Produce a concise session title.",
        "researcher" => "Role contract: You are a research agent. Produce long-horizon, multi-source answers with citations. Use the `research` tool for in-depth synthesis; use `websearch` for quick lookups. Avoid `curl`/`wget` for web search.",
        _ => "Role contract: You are an implementation agent. Inspect relevant files, make targeted changes, and verify them when possible.",
    }
}

pub fn subagent_output_contract(role: &str) -> &'static str {
    match role {
        "explore" | "explorer" => "Output contract: Return a compact report with: files examined, key symbols/modules found, relevant relationships, and uncertainties. Do not include raw file contents.",
        "review" | "reviewer" => "Output contract: Return findings by severity (critical/high/medium/low/info). For each: file path, line number if applicable, title, rationale, and suggested patch scope. Prioritize correctness and security over style.",
        "debug" => "Output contract: Return: commands/logs that revealed the issue, failure signature, root-cause candidates ranked by likelihood, and next experiment to try.",
        "test" => "Output contract: Return: tests added or run, pass/fail status per test, coverage gaps identified, and any flaky or skipped tests.",
        "security" | "security_reviewer" => "Output contract: Return: finding category, exploitability assessment, affected surface/files, and mitigation recommendation. Distinguish confirmed issues from speculative risks.",
        "planner" => "Output contract: Return: implementation plan with ordered steps, estimated complexity per step, dependencies between steps, files to create/modify, and verification criteria.",
        "researcher" => "Output contract: Return a synthesized answer with: question, evidence, conclusion, and citations. Distinguish confirmed claims from speculative ones. Prefer concrete, citable sources.",
        "executor" | _ => "Output contract: Return a compact summary with: work performed, key findings, files touched, and suggested next steps.",
    }
}

fn profile_contract(profile: &ResolvedModelProfile) -> &'static str {
    match profile.prompt_profile {
        PromptProfileKind::FrontierReasoning => {
            "Model profile: Strong reasoning model. Use concise planning, then execute. Avoid unnecessary verbosity."
        }
        PromptProfileKind::FrontierExecutor => {
            "Model profile: Strong coding executor. Prefer direct repository inspection, targeted edits, and verification."
        }
        PromptProfileKind::FastExecutor => {
            "Model profile: Fast executor. Keep changes bounded. Always emit structured tool calls when action is required. Never narrate intent (\"I will use the X tool\") without a corresponding structured tool call. Do not describe steps in prose when a tool call can express the same intent."
        }
        PromptProfileKind::LocalStrict => {
            "Model profile: Strict local/open model mode. Use one step at a time. Prefer small patches. Do not infer file contents without reading them."
        }
        PromptProfileKind::ToolFragile => {
            "Model profile: Tool-fragile mode. Use structured tool calls exactly. Do not describe tool calls in prose when a tool call is required."
        }
        PromptProfileKind::LongContextPlanner => {
            "Model profile: Long-context planning mode. Synthesize repository context carefully. Separate facts from recommendations."
        }
        PromptProfileKind::Reviewer => {
            "Model profile: Review mode. Look for correctness, safety, regression risk, missing tests, and excessive scope."
        }
        PromptProfileKind::Summarizer => {
            "Model profile: Summarizer mode. Preserve relevant state densely and avoid adding unsupported claims."
        }
        PromptProfileKind::Default => {
            "Model profile: Default coding model. Use tools for repository facts and keep edits targeted."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_profile::resolve::infer_builtin_profile;

    fn test_agent(name: &str) -> Agent {
        test_agent_with_role(name, None)
    }

    fn test_agent_with_role(name: &str, role: Option<&str>) -> Agent {
        Agent {
            name: name.to_string(),
            role: role.map(|r| r.to_string()),
            description: format!("Test {name} agent"),
            mode: crate::agent::AgentMode::All,
            mode_name: None,
            model: Some("test-model".to_string()),
            variant: None,
            temperature: None,
            top_p: None,
            color: None,
            steps: None,
            system_prompt: None,
            permissions: std::collections::HashMap::new(),
            hidden: false,
            thinking_budget: None,
            reasoning_effort: None,
        }
    }

    fn test_config() -> Config {
        Config::default()
    }

    #[test]
    fn test_profile_contract_local_strict() {
        let profile = infer_builtin_profile("ollama/qwen2.5-coder:32b");
        let contract = profile_contract(&profile);
        assert!(contract.contains("Strict local"));
        assert!(contract.contains("small patches"));
        assert!(contract.contains("Do not infer file contents"));
    }

    #[test]
    fn test_profile_contract_tool_fragile() {
        let mut profile = infer_builtin_profile("some-model");
        profile.prompt_profile = PromptProfileKind::ToolFragile;
        let contract = profile_contract(&profile);
        assert!(contract.contains("Tool-fragile"));
        assert!(contract.contains("structured tool calls exactly"));
    }

    #[test]
    fn test_assemble_system_prompt_with_profile_includes_all_parts() {
        let agent = test_agent("build");
        let config = test_config();
        let profile = infer_builtin_profile("openai/gpt-5");
        let tools = vec!["bash".to_string(), "read".to_string()];
        let skills = vec!["git".to_string()];

        let prompt = assemble_system_prompt_with_profile(PromptContext {
            agent: &agent,
            config: &config,
            model_profile: &profile,
            tools: &tools,
            skills: &skills,
            custom_instructions: Some("Custom instruction here"),
            is_plan_mode: false,
            agents: &[],
        });

        assert!(prompt.contains("codegg"));
        assert!(prompt.contains("Role contract"));
        assert!(prompt.contains("Model profile"));
        assert!(prompt.contains("You are the build agent"));
        assert!(prompt.contains("Available tools: bash, read"));
        assert!(prompt.contains("Available skills: git"));
        assert!(prompt.contains("Using model:"));
        assert!(prompt.contains("Custom instruction here"));
        // Planning contract is always included so the model knows
        // about todos vs. long-horizon goals.
        assert!(prompt.contains("Planning surfaces"));
        assert!(prompt.contains("todo"));
        assert!(prompt.contains("goal_request_completion"));
    }

    #[test]
    fn test_planning_contract_mentions_both_surfaces() {
        let agent = test_agent("build");
        let config = test_config();
        let profile = infer_builtin_profile("anthropic/claude-sonnet");
        let prompt = assemble_system_prompt_with_profile(PromptContext {
            agent: &agent,
            config: &config,
            model_profile: &profile,
            tools: &[],
            skills: &[],
            custom_instructions: None,
            is_plan_mode: false,
            agents: &[],
        });
        // In-flight planning goes through todos.
        assert!(prompt.contains("in-flight"));
        // Long-horizon planning goes through goal_set / goal_update_progress.
        assert!(prompt.contains("long-horizon"));
        assert!(prompt.contains("goal_set"));
        assert!(prompt.contains("goal_update_progress"));
        // Completion requires concrete evidence and remaining_risks.
        assert!(prompt.contains("evidence"));
        assert!(prompt.contains("remaining_risks"));
    }

    #[test]
    fn test_assemble_system_prompt_with_profile_empty_tools_skills() {
        let agent = test_agent("explore");
        let config = test_config();
        let profile = infer_builtin_profile("minimax/minimax-2.7");

        let prompt = assemble_system_prompt_with_profile(PromptContext {
            agent: &agent,
            config: &config,
            model_profile: &profile,
            tools: &[],
            skills: &[],
            custom_instructions: None,
            is_plan_mode: false,
            agents: &[],
        });

        assert!(prompt.contains("explore"));
        assert!(prompt.contains("Fast executor"));
        assert!(!prompt.contains("Available tools:"));
        assert!(!prompt.contains("Available skills:"));
    }

    #[test]
    fn test_role_contract_planner() {
        let agent = test_agent_with_role("myplan", Some("planner"));
        let contract = role_contract(&agent);
        assert!(contract.contains("planning agent"));
        assert!(contract.contains("Do not modify files"));
    }

    #[test]
    fn test_role_contract_explorer() {
        let agent = test_agent_with_role("myexplore", Some("explorer"));
        let contract = role_contract(&agent);
        assert!(contract.contains("exploration agent"));
        assert!(contract.contains("Do not modify files"));
    }

    #[test]
    fn test_role_contract_summarizer() {
        let agent = test_agent_with_role("mysummary", Some("summarizer"));
        let contract = role_contract(&agent);
        assert!(contract.contains("summarization agent"));
    }

    #[test]
    fn test_role_contract_compactor() {
        let agent = test_agent_with_role("mycompact", Some("compactor"));
        let contract = role_contract(&agent);
        assert!(contract.contains("compaction agent"));
    }

    #[test]
    fn test_role_contract_reviewer() {
        let agent = test_agent_with_role("myreview", Some("reviewer"));
        let contract = role_contract(&agent);
        assert!(contract.contains("review agent"));
    }

    #[test]
    fn test_role_contract_title() {
        let agent = test_agent_with_role("mytitle", Some("title"));
        let contract = role_contract(&agent);
        assert!(contract.contains("title generation agent"));
    }

    #[test]
    fn test_role_contract_executor_default() {
        let agent = test_agent_with_role("mybuild", Some("executor"));
        let contract = role_contract(&agent);
        assert!(contract.contains("implementation agent"));
    }

    #[test]
    fn test_role_contract_none_defaults_to_executor() {
        let agent = test_agent("unknown");
        let contract = role_contract(&agent);
        assert!(contract.contains("implementation agent"));
    }

    #[test]
    fn test_plan_mode_contract_is_included_when_active() {
        let agent = test_agent("build");
        let config = test_config();
        let profile = infer_builtin_profile("anthropic/claude-sonnet");
        let prompt = assemble_system_prompt_with_profile(PromptContext {
            agent: &agent,
            config: &config,
            model_profile: &profile,
            tools: &["read".to_string(), "todowrite".to_string()],
            skills: &[],
            custom_instructions: None,
            is_plan_mode: true,
            agents: &[],
        });
        // The plan mode contract is appended.
        assert!(prompt.contains("PLAN MODE ACTIVE"));
        // Mentions the planning surface.
        assert!(prompt.contains("todowrite"));
        // Tells the model about read-only bash.
        assert!(prompt.contains("read-only"));
    }

    #[test]
    fn test_plan_mode_contract_is_omitted_when_inactive() {
        let agent = test_agent("build");
        let config = test_config();
        let profile = infer_builtin_profile("anthropic/claude-sonnet");
        let prompt = assemble_system_prompt_with_profile(PromptContext {
            agent: &agent,
            config: &config,
            model_profile: &profile,
            tools: &["read".to_string()],
            skills: &[],
            custom_instructions: None,
            is_plan_mode: false,
            agents: &[],
        });
        // The plan mode contract is NOT included.
        assert!(!prompt.contains("PLAN MODE ACTIVE"));
    }

    #[test]
    fn test_websearch_contract_included_when_websearch_tool_present() {
        let agent = test_agent("build");
        let config = test_config();
        let profile = infer_builtin_profile("openai/gpt-5");
        let prompt = assemble_system_prompt_with_profile(PromptContext {
            agent: &agent,
            config: &config,
            model_profile: &profile,
            tools: &["websearch".to_string(), "read".to_string()],
            skills: &[],
            custom_instructions: None,
            is_plan_mode: false,
            agents: &[],
        });
        assert!(prompt.contains("Web access contract"));
        assert!(prompt.contains("DuckDuckGo"));
        assert!(prompt.contains("curl"));
    }

    #[test]
    fn test_websearch_contract_omitted_when_websearch_tool_absent() {
        let agent = test_agent("build");
        let config = test_config();
        let profile = infer_builtin_profile("openai/gpt-5");
        let prompt = assemble_system_prompt_with_profile(PromptContext {
            agent: &agent,
            config: &config,
            model_profile: &profile,
            tools: &["read".to_string()],
            skills: &[],
            custom_instructions: None,
            is_plan_mode: false,
            agents: &[],
        });
        assert!(!prompt.contains("Web access contract"));
    }

    #[test]
    fn test_research_subagent_contract_included_when_research_kind_known() {
        let mut research_agent = test_agent("research");
        research_agent.mode = crate::agent::AgentMode::All;
        let build_agent = test_agent("build");
        let agents = vec![build_agent, research_agent];
        let config = test_config();
        let profile = infer_builtin_profile("openai/gpt-5");
        let prompt = assemble_system_prompt_with_profile(PromptContext {
            agent: &agents[1],
            config: &config,
            model_profile: &profile,
            tools: &["task".to_string(), "websearch".to_string()],
            skills: &[],
            custom_instructions: None,
            is_plan_mode: false,
            agents: &agents,
        });
        assert!(prompt.contains("Long-horizon research"));
        assert!(prompt.contains("research pipeline"));
    }

    #[test]
    fn test_research_subagent_contract_omitted_in_plan_mode() {
        let mut research_agent = test_agent("research");
        research_agent.mode = crate::agent::AgentMode::All;
        let build_agent = test_agent("build");
        let agents = vec![build_agent, research_agent];
        let config = test_config();
        let profile = infer_builtin_profile("openai/gpt-5");
        let prompt = assemble_system_prompt_with_profile(PromptContext {
            agent: &agents[1],
            config: &config,
            model_profile: &profile,
            tools: &["task".to_string()],
            skills: &[],
            custom_instructions: None,
            is_plan_mode: true,
            agents: &agents,
        });
        // Plan mode → research subagent hint is suppressed.
        assert!(!prompt.contains("Long-horizon research"));
    }

    #[test]
    fn test_role_contract_unknown_role_defaults_to_executor() {
        let agent = test_agent_with_role("custom", Some("custom_role"));
        let contract = role_contract(&agent);
        assert!(contract.contains("implementation agent"));
    }
}
