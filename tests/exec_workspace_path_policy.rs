use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const PROVIDER_ID: &str = "openai";
const MODEL_ID: &str = "harness-fixture";
const MARKER_FILE: &str = "harness-marker.txt";
const MARKER_BODY: &str = "CODEGG-HARNESS-MARKER-4f21c0d8";
const POLICY_DEFECT: &str = "missing workspace";
const EXEC_TIMEOUT: Duration = Duration::from_secs(120);
const PROVIDER_READ_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_REPORTED_CHARS: usize = 4000;

const AMBIENT_PROVIDER_ENV: &[&str] = &[
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "GOOGLE_API_KEY",
    "OPENROUTER_API_KEY",
    "OPENCODE_ZEN_API_KEY",
    "MISTRAL_API_KEY",
    "GROQ_API_KEY",
    "DEEPINFRA_API_KEY",
    "CEREBRAS_API_KEY",
    "COHERE_API_KEY",
    "TOGETHERAI_API_KEY",
    "PERPLEXITY_API_KEY",
    "XAI_API_KEY",
    "VENICE_API_KEY",
    "MINIMAX_API_KEY",
    "OPENCODE_GO_API_KEY",
    "GENERALCOMPUTE_API_KEY",
    "CODEGG_CORE_ENDPOINT",
];

#[derive(Debug, Clone)]
struct ProviderRequest {
    sequence: usize,
    request_line: String,
    body: serde_json::Value,
}

fn read_http_request(stream: &mut TcpStream) -> std::io::Result<(String, serde_json::Value)> {
    stream.set_read_timeout(Some(PROVIDER_READ_TIMEOUT))?;
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "provider closed before sending request",
            ));
        }
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(offset) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break offset + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]).into_owned();
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if !name.eq_ignore_ascii_case("content-length") {
                return None;
            }
            value.trim().parse::<usize>().ok()
        })
        .unwrap_or(0);
    while bytes.len() < header_end + content_length {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "provider closed during request body",
            ));
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    let request_line = headers.lines().next().unwrap_or_default().to_string();
    let body = if content_length == 0 {
        serde_json::json!({})
    } else {
        serde_json::from_slice(&bytes[header_end..header_end + content_length]).map_err(
            |error| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("provider request JSON: {error}"),
                )
            },
        )?
    };
    Ok((request_line, body))
}

fn write_sse_response(stream: &mut TcpStream, body: &str) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body.as_bytes())?;
    stream.flush()?;
    let _ = stream.shutdown(std::net::Shutdown::Write);
    Ok(())
}

fn write_json_response(stream: &mut TcpStream, body: &str) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body.as_bytes())?;
    stream.flush()?;
    let _ = stream.shutdown(std::net::Shutdown::Write);
    Ok(())
}

fn tool_call_sse() -> String {
    let arguments = serde_json::json!({ "path": MARKER_FILE }).to_string();
    let call = serde_json::json!({
        "choices": [{
            "delta": {"tool_calls": [{
                "index": 0,
                "id": "call-read-1",
                "type": "function",
                "function": {"name": "read", "arguments": arguments}
            }]},
            "finish_reason": "tool_calls"
        }]
    });
    format!("data: {call}\n\ndata: [DONE]\n\n")
}

fn completion_sse() -> String {
    "data: {\"choices\":[{\"delta\":{\"content\":\"Marker inspected.\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"
        .to_string()
}

fn spawn_scripted_openai(
    listener: TcpListener,
    requests: Arc<Mutex<Vec<ProviderRequest>>>,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut chat_sequence = 0usize;
        while !stop.load(Ordering::Relaxed) {
            let accepted = match listener.accept() {
                Ok(accepted) => accepted,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(_) => break,
            };
            let (mut stream, _) = accepted;
            let _ = stream.set_nonblocking(false);
            let Ok((request_line, body)) = read_http_request(&mut stream) else {
                continue;
            };
            if request_line.contains("/chat/completions") {
                chat_sequence += 1;
                requests
                    .lock()
                    .expect("provider request log")
                    .push(ProviderRequest {
                        sequence: chat_sequence,
                        request_line,
                        body,
                    });
                let response = if chat_sequence == 1 {
                    tool_call_sse()
                } else {
                    completion_sse()
                };
                let _ = write_sse_response(&mut stream, &response);
            } else {
                let catalog = serde_json::json!({
                    "object": "list",
                    "data": [{"id": MODEL_ID, "object": "model"}]
                })
                .to_string();
                let _ = write_json_response(&mut stream, &catalog);
            }
        }
    })
}

fn observed_tool_results(requests: &[ProviderRequest]) -> Vec<String> {
    let mut results = Vec::new();
    for request in requests {
        let Some(messages) = request
            .body
            .get("messages")
            .and_then(|value| value.as_array())
        else {
            continue;
        };
        for message in messages {
            if message.get("role").and_then(|role| role.as_str()) != Some("tool") {
                continue;
            }
            if let Some(content) = message.get("content").and_then(|value| value.as_str()) {
                results.push(content.to_string());
            }
        }
    }
    results
}

fn bounded(text: &str) -> String {
    if text.chars().count() <= MAX_REPORTED_CHARS {
        return text.to_string();
    }
    let head: String = text.chars().take(MAX_REPORTED_CHARS).collect();
    format!("{head}\n... [truncated]")
}

fn isolated_environment(root: &Path, config_path: &Path) -> Vec<(String, String)> {
    let home = root.join("home");
    let scratch = root.join("scratch");
    vec![
        ("HOME".to_string(), home.to_string_lossy().into_owned()),
        (
            "USERPROFILE".to_string(),
            home.to_string_lossy().into_owned(),
        ),
        (
            "XDG_CONFIG_HOME".to_string(),
            root.join("config").to_string_lossy().into_owned(),
        ),
        (
            "XDG_DATA_HOME".to_string(),
            root.join("xdg-data").to_string_lossy().into_owned(),
        ),
        (
            "XDG_CACHE_HOME".to_string(),
            root.join("cache").to_string_lossy().into_owned(),
        ),
        (
            "XDG_STATE_HOME".to_string(),
            root.join("state").to_string_lossy().into_owned(),
        ),
        (
            "XDG_RUNTIME_DIR".to_string(),
            root.join("runtime").to_string_lossy().into_owned(),
        ),
        (
            "CODEGG_DAEMON_HOME".to_string(),
            root.join("daemon").to_string_lossy().into_owned(),
        ),
        (
            "CODEGG_DATA_HOME".to_string(),
            root.join("data").to_string_lossy().into_owned(),
        ),
        (
            "CODEGG_TUI_CONFIG".to_string(),
            config_path.to_string_lossy().into_owned(),
        ),
        ("TMPDIR".to_string(), scratch.to_string_lossy().into_owned()),
        ("TMP".to_string(), scratch.to_string_lossy().into_owned()),
        ("TEMP".to_string(), scratch.to_string_lossy().into_owned()),
    ]
}

fn create_runtime_roots(root: &Path) {
    for name in [
        "home",
        "home/Library/Caches",
        "config",
        "xdg-data",
        "cache",
        "state",
        "runtime",
        "daemon",
        "data",
        "scratch",
    ] {
        fs::create_dir_all(root.join(name)).expect("create isolated runtime root");
    }
}

struct ExecRun {
    timed_out: bool,
    stdout: String,
    stderr: String,
}

#[derive(Clone, Copy)]
enum HeadlessMode {
    Exec,
    Run,
}

impl HeadlessMode {
    fn label(self) -> &'static str {
        match self {
            Self::Exec => "exec",
            Self::Run => "run",
        }
    }
}

fn run_headless(
    binary: &Path,
    mode: HeadlessMode,
    workspace: &Path,
    environment: &[(String, String)],
    json_input: &str,
    transcript_root: &Path,
) -> ExecRun {
    let stdout_path = transcript_root.join(format!("{}.stdout", mode.label()));
    let stderr_path = transcript_root.join(format!("{}.stderr", mode.label()));
    let stdout_file = fs::File::create(&stdout_path).expect("create headless stdout capture");
    let stderr_file = fs::File::create(&stderr_path).expect("create headless stderr capture");

    let mut command = Command::new(binary);
    match mode {
        HeadlessMode::Exec => {
            command.arg("exec").arg("--json").arg(json_input);
        }
        HeadlessMode::Run => {
            let input: serde_json::Value =
                serde_json::from_str(json_input).expect("headless input is JSON");
            command
                .arg("--run")
                .arg(input["prompt"].as_str().expect("prompt is text"))
                .arg("--model")
                .arg(input["model"].as_str().expect("model is text"))
                .arg("--agent")
                .arg(input["agent"].as_str().expect("agent is text"));
        }
    }
    command
        .arg("--format")
        .arg("json")
        .arg("--approval-mode")
        .arg("yolo")
        .arg("--sandbox")
        .arg("workspace-write")
        .current_dir(workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file));
    for (key, value) in environment {
        command.env(key, value);
    }
    for key in AMBIENT_PROVIDER_ENV {
        command.env_remove(key);
    }
    command.env_remove("RUST_LOG");

    let mut child = command.spawn().expect("spawn headless codegg");
    let deadline = Instant::now() + EXEC_TIMEOUT;
    let mut timed_out = false;
    loop {
        match child.try_wait().expect("poll codegg exec") {
            Some(_) => break,
            None => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    timed_out = true;
                    break;
                }
                thread::sleep(Duration::from_millis(25));
            }
        }
    }

    ExecRun {
        timed_out,
        stdout: fs::read_to_string(&stdout_path).unwrap_or_default(),
        stderr: fs::read_to_string(&stderr_path).unwrap_or_default(),
    }
}

fn workspace_scoped_tool_call_is_authorized_by_a_workspace_path_policy(mode: HeadlessMode) {
    let mode_name = mode.label();
    let root = tempfile::tempdir().expect("isolated temp root");
    let root_path = root.path().canonicalize().expect("canonical temp root");
    create_runtime_roots(&root_path);

    let workspace = root_path.join("workspace");
    fs::create_dir_all(&workspace).expect("create temp workspace");
    fs::write(workspace.join(MARKER_FILE), format!("{MARKER_BODY}\n")).expect("write marker file");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind provider fixture");
    let provider_base = format!(
        "http://127.0.0.1:{}/v1",
        listener
            .local_addr()
            .expect("provider fixture address")
            .port()
    );
    listener
        .set_nonblocking(true)
        .expect("provider fixture listener is non-blocking");

    let config_path = root_path.join("config").join("codegg.json");
    fs::write(
        &config_path,
        serde_json::json!({
            "provider": {
                PROVIDER_ID: {
                    "api_key": "local-fixture-only",
                    "base_url": provider_base,
                    "models": { MODEL_ID: {"name": MODEL_ID} }
                }
            }
        })
        .to_string(),
    )
    .expect("write isolated provider config");

    let requests = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let server = spawn_scripted_openai(listener, Arc::clone(&requests), Arc::clone(&stop));

    let json_input = serde_json::json!({
        "prompt": "Read the marker file in the workspace and report its contents.",
        "model": format!("{PROVIDER_ID}/{MODEL_ID}"),
        "agent": "build",
    })
    .to_string();

    let environment = isolated_environment(&root_path, &config_path);
    {
        let mut probe = Command::new("/bin/echo")
            .arg("hi")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn /bin/echo");
        let _ = probe.wait();
    }
    {
        let mut probe = Command::new(env!("CARGO_BIN_EXE_codegg"))
            .arg("--version")
            .current_dir(&workspace)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn codegg --version");
        let _ = probe.wait();
    }
    let run = run_headless(
        Path::new(env!("CARGO_BIN_EXE_codegg")),
        mode,
        &workspace,
        &environment,
        &json_input,
        &root_path,
    );

    stop.store(true, Ordering::Relaxed);
    let _ = server.join();

    let transcript = format!("{}\n{}", run.stdout, run.stderr);
    assert!(
        !run.timed_out,
        "the headless `codegg {mode_name}` run did not finish within {EXEC_TIMEOUT:?}.\n\
         stdout:\n{}\nstderr:\n{}",
        bounded(&run.stdout),
        bounded(&run.stderr)
    );

    let observed = requests.lock().expect("provider request log").clone();
    assert!(
        observed.len() >= 2,
        "the scripted provider fixture never saw the follow-up request that carries the tool \
         result (observed {} request(s): {:?}). The headless `codegg {mode_name}` run did not \
         reach the scripted tool call.\nstdout:\n{}\nstderr:\n{}",
        observed.len(),
        observed
            .iter()
            .map(|request| format!("#{} {}", request.sequence, request.request_line))
            .collect::<Vec<_>>(),
        bounded(&run.stdout),
        bounded(&run.stderr)
    );

    let tool_results = observed_tool_results(&observed);
    assert!(
        !tool_results.is_empty(),
        "the scripted model never received a tool result: the headless `codegg {mode_name}` run \
         refused the workspace tool before any result was produced.\nstdout:\n{}\nstderr:\n{}",
        bounded(&run.stdout),
        bounded(&run.stderr)
    );
    let tool_text = tool_results.join("\n---\n");

    assert!(
        tool_text.contains(MARKER_BODY),
        "the relative `read` of `{MARKER_FILE}` did not return the marker body: the headless \
         `codegg {mode_name}` path must install the workspace path policy or the broker refuses \
         every workspace-scoped tool call with `permission denied: \
         verified tool authority is {POLICY_DEFECT}`. Expected the tool result to contain \
         {MARKER_BODY:?}.\ntool result the scripted model received:\n{}",
        bounded(&tool_text)
    );

    assert!(
        !transcript.contains(POLICY_DEFECT),
        "the headless `codegg {mode_name}` transcript still reports `{POLICY_DEFECT}`: the path \
         built its `AgentLoop` without a workspace path policy, so the broker refused the \
         workspace-scoped tool call.\nexec stdout:\n{}\nexec stderr:\n{}",
        bounded(&run.stdout),
        bounded(&run.stderr)
    );
}

#[test]
fn exec_workspace_scoped_tool_call_is_authorized_by_a_workspace_path_policy() {
    workspace_scoped_tool_call_is_authorized_by_a_workspace_path_policy(HeadlessMode::Exec);
}

#[test]
fn run_workspace_scoped_tool_call_is_authorized_by_a_workspace_path_policy() {
    workspace_scoped_tool_call_is_authorized_by_a_workspace_path_policy(HeadlessMode::Run);
}
