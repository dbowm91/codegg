use serde_json::{json, Value};
#[cfg(windows)]
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

#[cfg(windows)]
use std::path::PathBuf;
#[cfg(windows)]
use std::process::Command;
#[cfg(windows)]
use std::time::UNIX_EPOCH;

#[cfg(windows)]
use mxc_sdk::v1::configs::{CaptureDenials, CaptureDenialsMode, ProcessContainerConfig};
#[cfg(windows)]
use mxc_sdk::v1::policy::FilesystemPolicy;
#[cfg(windows)]
use mxc_sdk::v1::{self, ContainerRequest};

const MAX_DOCUMENT_BYTES: u64 = 1024 * 1024;
const MAX_DENIALS: usize = 4096;

#[cfg(windows)]
fn capture_temp_entries() -> BTreeSet<String> {
    fs::read_dir(std::env::temp_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with("mxc_capture_denials_"))
        .collect()
}

fn main() {
    #[cfg(windows)]
    run();
    #[cfg(not(windows))]
    emit(json!({
        "schema": "codegg.mxc_c036.v1",
        "qualified_test_only": false,
        "disposition": "blocked",
        "reason": "native evidence requires eligible Windows 11 ARM64 host"
    }));
}

fn emit(report: Value) {
    let line = format!("C036_REPORT={report}");
    println!("{line}");
    if let Some(path) = std::env::var_os("C036_REPORT_JSON") {
        if let Err(error) = fs::write(path, format!("{report}\n")) {
            eprintln!("C036 report file write failed: {error}");
        }
    }
}

fn stamped_capture_name(name: &str) -> bool {
    let Some(stem) = name
        .strip_prefix("denials.")
        .and_then(|s| s.strip_suffix(".json"))
    else {
        return false;
    };
    let Some((pid, nonce)) = stem.split_once('_') else {
        return false;
    };
    pid.parse::<u32>().is_ok_and(|pid| pid > 0)
        && nonce.len() == 32
        && nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn stamped_capture_name_for_process(name: &str, expected_pid: u32) -> bool {
    let Some(stem) = name
        .strip_prefix("denials.")
        .and_then(|s| s.strip_suffix(".json"))
    else {
        return false;
    };
    stem.split_once('_')
        .and_then(|(pid, _)| pid.parse::<u32>().ok())
        == Some(expected_pid)
        && stamped_capture_name(name)
}

fn parse_capture_document(bytes: &[u8]) -> Option<Value> {
    let document: Value = serde_json::from_slice(bytes).ok()?;
    let denials = document.get("denials")?.as_array()?;
    let summary = document.get("summary")?;
    let count = summary.get("totalDenials")?.as_u64()? as usize;
    summary.get("exitCode")?.as_i64()?;
    summary.get("deniedResourcesTruncated")?.as_bool()?;
    (denials.len() <= MAX_DENIALS && denials.len() == count).then_some(document)
}

fn denial_claim_valid(child_entered: bool, path_denied: bool, event_correlated: bool) -> bool {
    child_entered && path_denied && event_correlated
}

fn metadata_path_is_confined(path: &Path, root: &Path) -> bool {
    let (Ok(root), Ok(parent)) = (
        root.canonicalize(),
        path.parent().unwrap_or(Path::new(".")).canonicalize(),
    ) else {
        return false;
    };
    if parent != root || !path.is_absolute() {
        return false;
    }
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_DOCUMENT_BYTES
    {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Storage::FileSystem::{
            GetFileAttributesW, FILE_ATTRIBUTE_REPARSE_POINT,
        };
        let wide = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let attributes = unsafe { GetFileAttributesW(windows::core::PCWSTR(wide.as_ptr())) };
        if attributes == u32::MAX || attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return false;
        }
    }
    true
}

fn validate_capture(
    path: &Path,
    root: &Path,
    started: SystemTime,
    expected_pid: u32,
    expected_exit: i32,
    metadata: &Value,
    expected_resource: Option<&Path>,
    started_filetime: Option<u128>,
) -> Option<Value> {
    if metadata.get("kind")?.as_str()? != "captureDenials"
        || Path::new(metadata.get("outputPath")?.as_str()?) != path
    {
        return None;
    }
    let name = path.file_name()?.to_str()?;
    if !stamped_capture_name_for_process(name, std::process::id())
        || !metadata_path_is_confined(path, root)
    {
        return None;
    }
    let file_metadata = fs::metadata(path).ok()?;
    let created = file_metadata.created().ok()?;
    if created < started {
        return None;
    }
    if file_metadata.len() > MAX_DOCUMENT_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let document = parse_capture_document(&bytes)?;
    let summary = document.get("summary")?;
    let denials = document.get("denials")?.as_array()?;
    let exit = summary.get("exitCode")?.as_i64()?;
    let count = summary.get("totalDenials")?.as_u64()? as usize;
    let truncated = summary.get("deniedResourcesTruncated")?.as_bool()?;
    if exit != expected_exit as i64
        || count != metadata.get("totalDenials")?.as_u64()? as usize
        || exit != metadata.get("exitCode")?.as_i64()?
        || truncated != metadata.get("deniedResourcesTruncated")?.as_bool()?
        || count != denials.len()
        || truncated
    {
        return None;
    }
    let matching = denials
        .iter()
        .filter(|event| {
            let matches_resource = expected_resource.is_none_or(|resource| {
                event
                    .get("resource")
                    .and_then(Value::as_str)
                    .is_some_and(|observed| {
                        observed.eq_ignore_ascii_case(&resource.to_string_lossy())
                    })
            });
            let matches_time = started_filetime.is_none_or(|minimum| {
                event
                    .get("filetime")
                    .and_then(Value::as_str)
                    .and_then(|time| time.parse::<u128>().ok())
                    .is_some_and(|time| time >= minimum)
            });
            matches_resource
                && matches_time
                && event.get("pid").and_then(Value::as_u64) == Some(expected_pid as u64)
                && event.get("resourceType").and_then(Value::as_str) == Some("file")
                && event.get("accessType").and_then(Value::as_str) == Some("read")
        })
        .count();
    Some(json!({
        "document_verified": true,
        "summary_exit_code": exit,
        "summary_total_denials": count,
        "summary_truncated": truncated,
        "expected_child_pid": expected_pid,
        "matching_child_file_read_events": matching,
        "event_pid_correlated": matching > 0
    }))
}

#[cfg(windows)]
fn sha256(path: &Path) -> String {
    Command::new("certutil.exe")
        .args(["-hashfile", path.to_string_lossy().as_ref(), "SHA256"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .nth(1)
                .unwrap_or("unavailable")
                .replace(' ', "")
        })
        .unwrap_or_else(|| "unavailable".into())
}

#[cfg(windows)]
fn child_proof(path: &Path, nonce: &str, marker: &str) -> Option<Value> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() % 2 != 0 {
        return None;
    }
    let words = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    let value: Value = serde_json::from_str(&String::from_utf16(&words).ok()?).ok()?;
    (value.get("nonce")?.as_str()? == nonce && value.get("marker")?.as_str()? == marker)
        .then_some(value)
}

#[cfg(windows)]
fn child_entry(path: &Path, nonce: &str) -> Option<Value> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() % 2 != 0 {
        return None;
    }
    let words = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    let value: Value = serde_json::from_str(&String::from_utf16(&words).ok()?).ok()?;
    (value.get("nonce")?.as_str()? == nonce && value.get("phase")?.as_str()? == "entry")
        .then_some(value)
}

#[cfg(windows)]
fn process_is_gone(pid: u32) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
    else {
        return false;
    };
    let mut exit_code = 0;
    let gone = unsafe { GetExitCodeProcess(process, &mut exit_code) }.is_ok() && exit_code != 259;
    let _ = unsafe { CloseHandle(process) };
    gone
}

#[cfg(windows)]
fn request(
    helper: &Path,
    stage: &Path,
    host: &Path,
    command: String,
    timeout_ms: u32,
    capture: Option<&Path>,
) -> ContainerRequest {
    let mut container = ProcessContainerConfig::default();
    if let Some(output) = capture {
        let mut config = CaptureDenials::default();
        config.mode = CaptureDenialsMode::Block;
        config.output_path = Some(output.to_string_lossy().into_owned());
        config.retain_etl = false;
        container.capture_denials = Some(config);
    }
    let mut readonly =
        vec![std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()) + r"\System32"];
    readonly.push(helper.to_string_lossy().into_owned());
    let filesystem = FilesystemPolicy {
        readwrite_paths: vec![stage.to_string_lossy().into_owned()],
        readonly_paths: readonly,
        denied_paths: vec![host.to_string_lossy().into_owned()],
        clear_policy_on_exit: Some(true),
    };
    ContainerRequest {
        containment: v1::Containment::ProcessContainer(container),
        filesystem: Some(filesystem),
        working_directory: Some(stage.to_string_lossy().into_owned()),
        timeout_ms: Some(timeout_ms),
        environment: Some(vec![
            (
                "SystemRoot".into(),
                std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()),
            ),
            (
                "USERPROFILE".into(),
                stage.join("profile").to_string_lossy().into_owned(),
            ),
            (
                "LOCALAPPDATA".into(),
                stage.join("localappdata").to_string_lossy().into_owned(),
            ),
            (
                "APPDATA".into(),
                stage.join("appdata").to_string_lossy().into_owned(),
            ),
            (
                "SystemDrive".into(),
                std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()),
            ),
            ("TEMP".into(), stage.to_string_lossy().into_owned()),
            ("TMP".into(), stage.to_string_lossy().into_owned()),
        ]),
        inherit_default_environment: Some(false),
        ..ContainerRequest::new(command)
    }
}

#[cfg(windows)]
fn run() {
    let nonce = format!(
        "{:x}-{:x}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let request_id = format!("c036-{nonce}");
    let Some(helper) = std::env::var_os("C036_NATIVE_HELPER").map(PathBuf::from) else {
        emit(
            json!({"schema":"codegg.mxc_c036.v1","qualified_test_only":false,"disposition":"blocked","failed_gates":["helper-missing"]}),
        );
        return;
    };
    let temp = std::env::var_os("RUNNER_TEMP")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let root = temp.join(format!("codegg-mxc-c036-{nonce}"));
    let capture_temp_before = capture_temp_entries();
    let host = root.join("host");
    let block_stage = root.join("stage-block");
    let allowed_stage = root.join("stage-allowed");
    let block_capture = root.join("capture-block");
    let allowed_capture = root.join("capture-allowed");
    let disabled_capture = root.join("capture-disabled");
    for directory in [
        &host,
        &block_stage,
        &allowed_stage,
        &block_capture,
        &allowed_capture,
        &disabled_capture,
    ] {
        if let Err(error) = fs::create_dir_all(directory) {
            emit(
                json!({"schema":"codegg.mxc_c036.v1","qualified_test_only":false,"disposition":"blocked","failed_gates":["fixture-setup"],"reason":error.to_string()}),
            );
            return;
        }
    }
    for stage in [&block_stage, &allowed_stage] {
        for child in ["profile", "localappdata", "appdata"] {
            if let Err(error) = fs::create_dir_all(stage.join(child)) {
                emit(
                    json!({"schema":"codegg.mxc_c036.v1","qualified_test_only":false,"disposition":"blocked","failed_gates":["fixture-setup"],"reason":error.to_string()}),
                );
                return;
            }
        }
    }
    let secret = host.join("synthetic-c036-secret.txt");
    if let Err(error) = fs::write(&secret, b"synthetic-c036-secret") {
        emit(
            json!({"schema":"codegg.mxc_c036.v1","qualified_test_only":false,"disposition":"blocked","failed_gates":["fixture-setup"],"reason":error.to_string()}),
        );
        return;
    }
    let acl_before = Command::new("icacls.exe")
        .arg(&secret)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let grant = Command::new("icacls.exe")
        .args([
            secret.to_string_lossy().as_ref(),
            "/grant",
            "*S-1-1-0:R",
            "*S-1-5-32-545:R",
            "*S-1-5-11:R",
        ])
        .output();
    let acl_granted = grant.is_ok_and(|output| output.status.success());
    let acl_at_start = Command::new("icacls.exe")
        .arg(&secret)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let secret_hash_before = sha256(&secret);
    let control_nonce = format!("control-{nonce}");
    let control_entry = root.join("control-entry.json");
    let control_proof_path = root.join("control-proof.json");
    let control = Command::new(&helper)
        .args([
            "--control",
            secret.to_string_lossy().as_ref(),
            helper.to_string_lossy().as_ref(),
            control_entry.to_string_lossy().as_ref(),
            control_proof_path.to_string_lossy().as_ref(),
            control_nonce.as_str(),
        ])
        .output();
    let control_proof = child_proof(&control_proof_path, &control_nonce, "child-proof");
    let ordinary_read_allowed = control.as_ref().is_ok_and(|o| o.status.success())
        && control_proof
            .as_ref()
            .is_some_and(|p| p["secret_match"] == true && p["host_path_secret_match"] == true);

    let probe_request = request(
        &helper,
        &block_stage,
        &host,
        "cmd.exe /c exit 0".into(),
        20_000,
        None,
    );
    let probe = v1::probe(Some(&probe_request));
    let probe_json = probe
        .as_ref()
        .ok()
        .and_then(|value| serde_json::to_value(value).ok())
        .unwrap_or(Value::Null);
    let probe_summary = probe
        .as_ref()
        .map(|_| probe_json.to_string())
        .unwrap_or_else(|error| format!("error: {error}"));
    let observed_tier = probe_json["tier"].as_str().unwrap_or("unavailable");

    let block_nonce = format!("block-{nonce}");
    let block_entry = block_stage.join("entry.json");
    let block_proof_path = block_stage.join("proof.json");
    let _ = fs::remove_file(&block_entry);
    let _ = fs::remove_file(&block_proof_path);
    let block_command = format!(
        "\"{}\" --child \"{}\" \"{}\" {} 0 \"{}\"",
        helper.display(),
        block_entry.display(),
        block_proof_path.display(),
        block_nonce,
        secret.display()
    );
    let block_request = request(
        &helper,
        &block_stage,
        &host,
        block_command,
        20_000,
        Some(&block_capture.join("denials.json")),
    );
    let started = SystemTime::now();
    let started_filetime = started
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        / 100
        + 116_444_736_000_000_000u128;
    let block_execution = v1::run(block_request, Default::default());
    let (block_exit, block_result, capture_metadata, capture_error) = match block_execution {
        Ok(result) => {
            let metadata = result.output_metadata.as_ref().and_then(|m| m.capture_denials.as_ref()).map(|m| json!({
                "kind":m.kind,"outputPath":m.output_path,"exitCode":m.exit_code,"totalDenials":m.total_denials,
                "deniedResourcesTruncated":m.denied_resources_truncated,"etlPath":m.etl_path
            }));
            let error = result
                .output_metadata
                .as_ref()
                .and_then(|m| m.capture_denials_error.as_ref())
                .map(|error| {
                    error
                        .message
                        .replace(&temp.to_string_lossy().to_string(), "<runner-temp>")
                });
            (
                format!("{:?}", result.outcome),
                Some(result.outcome),
                metadata,
                error,
            )
        }
        Err(error) => (
            format!("error: {error}"),
            None,
            None,
            Some(error.to_string()),
        ),
    };
    let block_proof = child_proof(&block_proof_path, &block_nonce, "child-proof");
    let child_pid = block_proof
        .as_ref()
        .and_then(|p| p["pid"].as_u64())
        .unwrap_or_default() as u32;
    let metadata_output_path = capture_metadata
        .as_ref()
        .and_then(|m| m["outputPath"].as_str())
        .map(PathBuf::from);
    let redacted_path = metadata_output_path
        .as_ref()
        .and_then(|path| path.strip_prefix(&block_capture).ok())
        .and_then(Path::to_str)
        .map(|relative| format!("<capture-block>/{relative}"));
    let validated = metadata_output_path
        .as_ref()
        .zip(capture_metadata.as_ref())
        .and_then(|(path, metadata)| {
            let exit = block_result.and_then(|value| match value {
                v1::WaitResult::Exited(code) => Some(code),
                _ => None,
            })?;
            validate_capture(
                path,
                &block_capture,
                started,
                child_pid,
                exit,
                metadata,
                Some(&secret),
                Some(started_filetime),
            )
        });
    let capture_bytes = metadata_output_path
        .as_ref()
        .and_then(|path| fs::read(path).ok());
    let denied_host_read_event = capture_bytes
        .as_deref()
        .and_then(parse_capture_document)
        .and_then(|doc| doc["denials"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .any(|event| {
            let resource = event["resource"].as_str().unwrap_or_default();
            event["pid"].as_u64() == Some(child_pid as u64)
                && event["resourceType"] == "file"
                && event["accessType"] == "read"
                && resource.eq_ignore_ascii_case(&secret.to_string_lossy())
                && event["filetime"]
                    .as_str()
                    .and_then(|s| s.parse::<u128>().ok())
                    .is_some_and(|time| time >= started_filetime)
        });

    let allowed_nonce = format!("allowed-{nonce}");
    let allowed_proof_path = allowed_stage.join("proof.json");
    let allowed_command = format!(
        "\"{}\" --stage-only \"{}\" {}",
        helper.display(),
        allowed_proof_path.display(),
        allowed_nonce
    );
    let allowed_request = request(
        &helper,
        &allowed_stage,
        &host,
        allowed_command,
        20_000,
        Some(&allowed_capture.join("denials.json")),
    );
    let allowed_started = SystemTime::now();
    let allowed_execution = v1::run(allowed_request, Default::default());
    let (allowed_exit, allowed_metadata) = match allowed_execution {
        Ok(result) => (format!("{:?}", result.outcome), result.output_metadata.and_then(|m| m.capture_denials).map(|m| json!({
            "kind":m.kind,"outputPath":m.output_path,"exitCode":m.exit_code,"totalDenials":m.total_denials,
            "deniedResourcesTruncated":m.denied_resources_truncated
        }))),
        Err(error) => (format!("error: {error}"), None),
    };
    let allowed_proof = child_proof(&allowed_proof_path, &allowed_nonce, "stage-only-proof");
    let allowed_path = allowed_metadata
        .as_ref()
        .and_then(|m| m["outputPath"].as_str())
        .map(PathBuf::from);
    let allowed_doc = allowed_path.as_ref().and_then(|path| {
        validate_capture(
            path,
            &allowed_capture,
            allowed_started,
            allowed_proof
                .as_ref()
                .and_then(|p| p["pid"].as_u64())
                .unwrap_or_default() as u32,
            0,
            allowed_metadata.as_ref().unwrap_or(&Value::Null),
            None,
            None,
        )
    });
    let allowed_fake_secret_event = allowed_path
        .as_ref()
        .and_then(|path| fs::read(path).ok())
        .as_deref()
        .and_then(parse_capture_document)
        .and_then(|doc| doc["denials"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .any(|event| {
            event["resource"]
                .as_str()
                .is_some_and(|resource| resource.eq_ignore_ascii_case(&secret.to_string_lossy()))
        });

    let stale = disabled_capture.join("denials.1_deadbeef.json");
    let _ = fs::write(&stale, br#"{"denials":[],"summary":{"exitCode":0,"totalDenials":0,"deniedResourcesTruncated":false}}"#);
    let disabled_nonce = format!("disabled-{nonce}");
    let disabled_proof_path = block_stage.join("disabled-proof.json");
    let disabled_command = format!(
        "\"{}\" --stage-only \"{}\" {}",
        helper.display(),
        disabled_proof_path.display(),
        disabled_nonce
    );
    let disabled_request = request(&helper, &block_stage, &host, disabled_command, 20_000, None);
    let disabled_execution = v1::run(disabled_request, Default::default());
    let disabled_metadata_absent =
        disabled_execution.is_ok_and(|result| result.output_metadata.is_none()) && stale.is_file();

    let pre_entry = v1::run(
        request(
            &helper,
            &block_stage,
            &host,
            format!("\"{}\" --exit-before-entry", helper.display()),
            5_000,
            None,
        ),
        Default::default(),
    );
    let (pre_entry_outcome, pre_entry_exit) = match pre_entry {
        Ok(result) => (format!("{:?}", result.outcome), Some(result.outcome)),
        Err(error) => (format!("error: {error}"), None),
    };
    let pre_entry_not_run = pre_entry_exit == Some(v1::WaitResult::Exited(23));

    let timeout_nonce = format!("timeout-{nonce}");
    let timeout_entry_path = block_stage.join("timeout-entry.json");
    let timeout_command = format!(
        "\"{}\" --sleep \"{}\" {} 3000",
        helper.display(),
        timeout_entry_path.display(),
        timeout_nonce
    );
    let timeout_execution = v1::run(
        request(&helper, &block_stage, &host, timeout_command, 500, None),
        Default::default(),
    );
    let (timeout_outcome, timeout_result) = match timeout_execution {
        Ok(result) => (format!("{:?}", result.outcome), Some(result.outcome)),
        Err(error) => (format!("error: {error}"), None),
    };
    let timeout_entry_proof = child_entry(&timeout_entry_path, &timeout_nonce);
    let timeout_pid = timeout_entry_proof
        .as_ref()
        .and_then(|proof| proof["pid"].as_u64())
        .unwrap_or_default() as u32;
    let timeout_process_gone = timeout_pid > 0 && process_is_gone(timeout_pid);
    let timeout_confirmed = timeout_result == Some(v1::WaitResult::TimedOut)
        && timeout_entry_proof.is_some()
        && timeout_process_gone;

    let no_capture_temps = [&block_capture, &allowed_capture, &disabled_capture]
        .into_iter()
        .flat_map(|directory| fs::read_dir(directory).into_iter().flatten().flatten())
        .all(|entry| entry.path().extension().and_then(|ext| ext.to_str()) != Some("etl"))
        && capture_temp_entries()
            .difference(&capture_temp_before)
            .next()
            .is_none();

    let acl_after = Command::new("icacls.exe")
        .arg(&secret)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let secret_hash_after = sha256(&secret);
    let fixture_unchanged = secret_hash_before == secret_hash_after && acl_at_start == acl_after;
    let child_entered = block_proof.is_some();
    let child_path_denied = block_proof.as_ref().is_some_and(|proof| {
        proof["host_path_read_ok"] == false
            && proof["host_path_read_error"] == 5
            && proof["host_path_secret_match"] == false
    });
    let selected_backend = observed_tier == "base-container" && probe_json["error"].is_null();
    let capture_pointer_present = capture_metadata.is_some();
    let output_verified = validated.is_some();
    let allowed_no_false_positive =
        allowed_proof.is_some() && allowed_doc.is_some() && !allowed_fake_secret_event;
    let qualified = ordinary_read_allowed
        && child_entered
        && child_path_denied
        && block_exit == "Exited(0)"
        && selected_backend
        && capture_pointer_present
        && output_verified
        && denied_host_read_event
        && allowed_no_false_positive
        && disabled_metadata_absent
        && pre_entry_not_run
        && timeout_confirmed
        && no_capture_temps
        && fixture_unchanged
        && acl_granted;
    let failed_gates = [
        (!ordinary_read_allowed).then_some("ordinary-positive-control"),
        (!child_entered).then_some("child-entry"),
        (!child_path_denied).then_some("same-file-host-path-denial"),
        (!selected_backend).then_some("backend-or-tier-probe"),
        (!capture_pointer_present).then_some("structured-output-metadata-missing"),
        (!output_verified).then_some("capture-document-validation"),
        (!denied_host_read_event).then_some("child-correlated-denial-event"),
        (!allowed_no_false_positive).then_some("allowed-only-negative-control"),
        (!disabled_metadata_absent).then_some("disabled-stale-control"),
        (!pre_entry_not_run).then_some("pre-entry-failure-classification"),
        (!timeout_confirmed).then_some("bounded-timeout-classification"),
        (!no_capture_temps).then_some("capture-temporary-cleanup"),
        (!fixture_unchanged || !acl_granted).then_some("fixture-integrity"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    emit(json!({
        "schema":"codegg.mxc_c036.v1", "qualified_test_only":qualified,"request_id":request_id,
        "disposition":if qualified {"bounded-positive-test-only"} else {"negative-or-incomplete"},
        "sdk":{"crate":"mxc-sdk","version":"1.0.0","checksum":"e4866ca589a68edc8e931eba58eed9253ad1ce670e974aedc4830a1d94bca6cd","vcs_commit":"a846837d827b496fc4a08dfe60554515758c6647","path_in_vcs":"src/mxc-sdk"},
        "capture_contract_kind":"ExecutionResult.output_metadata.capture_denials",
        "capture_error_reported":capture_error.is_some(),"capture_error_redacted":capture_error,
        "configured_path_kind":"per-invocation stamped sibling under owned directory",
        "actual_output_path_redacted":redacted_path,
        "output_metadata_present":capture_metadata.is_some(), "filename_stamped":metadata_output_path.as_ref().and_then(|p|p.file_name()).and_then(|n|n.to_str()).is_some_and(|name|stamped_capture_name_for_process(name,std::process::id())),
        "document_verified":output_verified,"matched_denial_count":validated.as_ref().map(|v|v["matching_child_file_read_events"].as_u64()).flatten().unwrap_or_default(),
        "event_pid_correlated":denied_host_read_event,"capture_mode":"block","host_read_result":if child_path_denied {"ERROR_ACCESS_DENIED"} else {"not_denied_or_unavailable"},
        "child_entry":child_entered,"child_pid":child_pid,"child_nonce":block_nonce,"child_exit":block_exit,
        "selected_backend":"processcontainer","observed_tier":observed_tier,"tier_probe":probe_json,"probe_diagnostic":probe_summary,"requested_backend":"ProcessContainer","ordinary_host_read":"allowed",
        "ordinary_control":{"success":control.as_ref().is_ok_and(|output|output.status.success()),"exit_code":control.as_ref().ok().and_then(|output|output.status.code()),"entry_proof":control_proof.as_ref().map(|proof|json!({"pid":proof["pid"],"secret_match":proof["secret_match"],"same_file":proof["handle_same_file"],"host_path_secret_match":proof["host_path_secret_match"],"host_path_read_error":proof["host_path_read_error"]}))},
        "allowed_stage_only":{"exit":allowed_exit,"proof_present":allowed_proof.is_some(),"capture_verified":allowed_doc.is_some(),"false_host_denial":allowed_fake_secret_event},
        "capture_disabled_stale_control":{"metadata_absent":disabled_metadata_absent,"decoy_present":stale.is_file()},
        "failure_lifecycle":{"pre_entry_outcome":pre_entry_outcome,"pre_entry_classification":if pre_entry_not_run {"not_run"} else {"failed_or_unavailable"},"timeout_outcome":timeout_outcome,"timeout_child_entered":timeout_entry_proof.is_some(),"timeout_process_gone":timeout_process_gone,"timeout_classification":if timeout_confirmed {"failed"} else {"failed_or_unavailable"},"neither_is_denial":!denial_claim_valid(pre_entry_exit.is_some(), false, false) && !denial_claim_valid(timeout_entry_proof.is_some(), false, false)},
        "capture_temporary_cleanup":{"etl_files_absent":no_capture_temps,"retain_etl":false},
        "fixture_integrity":{"acl_grant_succeeded":acl_granted,"acl_before_grant":acl_before,"acl_unchanged":acl_at_start==acl_after,"content_unchanged":secret_hash_before==secret_hash_after,"sha256_before":secret_hash_before,"sha256_after":secret_hash_after},
        "failed_gates":failed_gates,
        "limitations":["diagnostic only; no production admission", "no network enforcement claim", "host edition/build/UBR recorded by the workflow"]
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_v1_output_metadata_contract_deserializes() {
        let metadata: mxc_sdk::v1::ExecutionMetadata = serde_json::from_value(json!({
            "captureDenials":{"type":"captureDenials","outputPath":"C:/owned/denials.123_0123456789abcdef0123456789abcdef.json","exitCode":0,"totalDenials":1,"deniedResourcesTruncated":false}
        })).expect("pinned v1 metadata shape");
        let output = metadata.capture_denials.expect("capture output");
        assert_eq!(output.kind, "captureDenials");
        assert_eq!(output.total_denials, 1);
        assert!(stamped_capture_name(
            Path::new(&output.output_path)
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
        ));
    }

    #[test]
    fn stamped_name_rejects_literal_stale_and_malformed_names() {
        assert!(stamped_capture_name(
            "denials.123_0123456789abcdef0123456789abcdef.json"
        ));
        assert!(!stamped_capture_name("denials.json"));
        assert!(!stamped_capture_name("denials.123_deadbeef.json"));
        assert!(!stamped_capture_name_for_process(
            "denials.123_0123456789abcdef0123456789abcdef.json",
            456
        ));
        assert!(!stamped_capture_name(
            "elsewhere.123_0123456789abcdef0123456789abcdef.json"
        ));
    }

    #[test]
    fn document_schema_requires_bounded_denials_and_consistent_summary() {
        let valid = br#"{"denials":[],"summary":{"exitCode":0,"totalDenials":0,"deniedResourcesTruncated":false}}"#;
        assert!(parse_capture_document(valid).is_some());
        let mismatch = br#"{"denials":[],"summary":{"exitCode":0,"totalDenials":1,"deniedResourcesTruncated":false}}"#;
        assert!(parse_capture_document(mismatch).is_none());
        assert!(parse_capture_document(br#"{"denials":[],"summary":{}}"#).is_none());
    }

    #[test]
    fn child_entry_and_path_denial_without_a_correlated_event_is_not_a_denial_claim() {
        assert!(!denial_claim_valid(false, false, false));
        assert!(!denial_claim_valid(true, false, false));
        assert!(!denial_claim_valid(true, true, false));
        assert!(denial_claim_valid(true, true, true));
    }

    #[test]
    fn metadata_path_cannot_escape_owned_capture_directory() {
        let root = std::env::temp_dir().join(format!("c036-path-test-{}", std::process::id()));
        let outside = root.with_extension("outside.json");
        let _ = fs::create_dir_all(&root);
        let _ = fs::write(&outside, b"{}");
        assert!(!metadata_path_is_confined(&outside, &root));
        let _ = fs::remove_file(outside);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn stale_stamped_document_is_rejected_even_when_well_formed() {
        let root = std::env::temp_dir().join(format!("c036-stale-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let name = format!(
            "denials.{}_0123456789abcdef0123456789abcdef.json",
            std::process::id()
        );
        let path = root.join(name);
        fs::write(&path, br#"{"denials":[],"summary":{"exitCode":0,"totalDenials":0,"deniedResourcesTruncated":false}}"#).unwrap();
        let metadata = json!({"kind":"captureDenials","outputPath":path,"exitCode":0,"totalDenials":0,"deniedResourcesTruncated":false});
        let future = SystemTime::now() + std::time::Duration::from_secs(10);
        assert!(validate_capture(
            &path,
            &root,
            future,
            std::process::id(),
            0,
            &metadata,
            None,
            None
        )
        .is_none());
        let _ = fs::remove_dir_all(root);
    }
}
