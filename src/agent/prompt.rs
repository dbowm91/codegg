use crate::agent::Agent;
use crate::config::schema::Config;
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
        "You are a build agent. Execute commands, edit files, and perform tasks to build and test the project.",
    );
    map.insert(
        "plan",
        "You are a planning agent. Analyze the codebase and create detailed implementation plans. Do not modify files or execute commands.",
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
    parts
}

fn builtin_prompts() -> &'static HashMap<&'static str, &'static str> {
    &BUILTIN_PROMPTS
}
