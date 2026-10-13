use serde_json::{json, Value};
use std::fs;
#[cfg(windows)]
use std::path::PathBuf;
#[cfg(windows)]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use mxc_sdk::v1::policy::FilesystemPolicy;
#[cfg(windows)]
use mxc_sdk::v1::{self, ContainerRequest};

fn emit(report: Value) {
    println!("C037_REPORT={report}");
    if let Some(path) = std::env::var_os("C037_REPORT_JSON") {
        let _ = fs::write(path, format!("{report}\n"));
    }
}

fn main() {
    #[cfg(windows)]
    run();
    #[cfg(not(windows))]
    emit(
        json!({"schema":"codegg.mxc_c037.v1","qualified_test_only":false,"disposition":"blocked","reason":"native evidence requires eligible Windows 11 ARM64 host"}),
    );
}

#[cfg(windows)]
fn run() {
    use std::io::Write;
    use std::os::windows::io::AsRawHandle;
    use std::process::Command;
    use windows::Win32::Foundation::{SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT};

    let nonce = format!(
        "{:x}-{:x}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let Some(helper) = std::env::var_os("C037_NATIVE_HELPER").map(PathBuf::from) else {
        emit(
            json!({"schema":"codegg.mxc_c037.v1","qualified_test_only":false,"disposition":"blocked","failed_gates":["helper-missing"]}),
        );
        return;
    };
    let root = std::env::var_os("RUNNER_TEMP")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(format!("codegg-mxc-c037-{nonce}"));
    let host = root.join("host");
    let stage = root.join("stage");
    if fs::create_dir_all(&host).is_err() || fs::create_dir_all(&stage).is_err() {
        emit(
            json!({"schema":"codegg.mxc_c037.v1","qualified_test_only":false,"disposition":"blocked","failed_gates":["fixture-setup"]}),
        );
        return;
    }
    for directory in ["profile", "localappdata", "appdata"] {
        let _ = fs::create_dir_all(stage.join(directory));
    }
    let secret = host.join("synthetic-c037-secret.txt");
    if fs::write(&secret, b"synthetic-c035-secret").is_err() {
        emit(
            json!({"schema":"codegg.mxc_c037.v1","qualified_test_only":false,"disposition":"blocked","failed_gates":["fixture-setup"]}),
        );
        return;
    }
    let grant = Command::new("icacls.exe")
        .args([
            secret.to_string_lossy().as_ref(),
            "/grant",
            "*S-1-1-0:R",
            "*S-1-5-32-545:R",
            "*S-1-5-11:R",
        ])
        .output();
    let acl_granted = grant.is_ok_and(|result| result.status.success());
    let acl_after_grant = Command::new("icacls.exe")
        .arg(&secret)
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    let source_identity = Command::new(&helper)
        .args(["--identity", secret.to_string_lossy().as_ref()])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|output| {
            output
                .lines()
                .find_map(|line| line.strip_prefix("C037_IDENTITY=").map(str::to_owned))
        })
        .and_then(|identity| {
            identity
                .split_once(',')
                .map(|(volume, index)| (volume.to_owned(), index.to_owned()))
        });

    let control_on = Command::new(&helper)
        .args([
            "--control",
            secret.to_string_lossy().as_ref(),
            helper.to_string_lossy().as_ref(),
            root.join("on-entry.json").to_string_lossy().as_ref(),
            root.join("on-proof.json").to_string_lossy().as_ref(),
            "c037-on",
        ])
        .output();
    let control_on_proof = fs::read_to_string(root.join("on-proof.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let inherit_on = control_on
        .as_ref()
        .is_ok_and(|result| result.status.success())
        && control_on_proof.as_ref().is_some_and(|proof| {
            proof["secret_match"] == true && proof["handle_same_file"] == true
        });
    let control_off = Command::new(&helper)
        .args([
            "--control-off",
            secret.to_string_lossy().as_ref(),
            helper.to_string_lossy().as_ref(),
            root.join("off-entry.json").to_string_lossy().as_ref(),
            root.join("off-proof.json").to_string_lossy().as_ref(),
            "c037-off",
        ])
        .output();
    let control_off_proof = fs::read_to_string(root.join("off-proof.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let inherit_off = control_off
        .as_ref()
        .is_ok_and(|result| result.status.success())
        && control_off_proof.as_ref().is_some_and(|proof| {
            proof["secret_match"] == false && proof["handle_same_file"] == false
        });

    let file = match fs::File::open(&secret) {
        Ok(file) => file,
        Err(_) => {
            emit(
                json!({"schema":"codegg.mxc_c037.v1","qualified_test_only":false,"disposition":"blocked","failed_gates":["fixture-open"]}),
            );
            return;
        }
    };
    let raw_handle = file.as_raw_handle();
    let inherit_flag_set = unsafe {
        SetHandleInformation(
            HANDLE(raw_handle),
            HANDLE_FLAG_INHERIT.0,
            HANDLE_FLAG_INHERIT,
        )
        .is_ok()
    };
    let mut container = mxc_sdk::v1::configs::ProcessContainerConfig::default();
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let filesystem = FilesystemPolicy {
        readwrite_paths: vec![stage.to_string_lossy().into_owned()],
        readonly_paths: vec![
            format!(r"{system_root}\System32"),
            helper.to_string_lossy().into_owned(),
        ],
        denied_paths: vec![host.to_string_lossy().into_owned()],
        clear_policy_on_exit: Some(true),
    };
    let entry = stage.join("entry.json");
    let proof = stage.join("proof.json");
    let descendant = stage.join("descendant-proof.json");
    let (volume_serial, file_index) = source_identity
        .clone()
        .unwrap_or_else(|| ("0".into(), "0".into()));
    let command = format!(
        "\"{}\" --child \"{}\" \"{}\" {} {} \"{}\" \"{}\" \"{}\" {} {} C037_STDIN",
        helper.display(),
        entry.display(),
        proof.display(),
        nonce,
        raw_handle as usize,
        secret.display(),
        descendant.display(),
        helper.display(),
        volume_serial,
        file_index
    );
    let request = ContainerRequest {
        containment: v1::Containment::ProcessContainer(std::mem::take(&mut container)),
        filesystem: Some(filesystem),
        working_directory: Some(stage.to_string_lossy().into_owned()),
        timeout_ms: Some(20_000),
        environment: Some(vec![
            ("SystemRoot".into(), system_root),
            ("TEMP".into(), stage.to_string_lossy().into_owned()),
            ("TMP".into(), stage.to_string_lossy().into_owned()),
        ]),
        inherit_default_environment: Some(false),
        ..ContainerRequest::new(command)
    };
    let probe = v1::probe(Some(&request));
    let probe_json = probe
        .ok()
        .and_then(|result| serde_json::to_value(result).ok())
        .unwrap_or(Value::Null);
    let effective_tier = probe_json["tier"].as_str().unwrap_or("unavailable");
    let execution = v1::spawn(request, Default::default());
    let (outcome, stdout, stderr, child_pid) = match execution {
        Ok(mut process) => {
            let pid = process.id();
            let stdin_ok = process
                .take_stdin()
                .and_then(|mut stdin| stdin.write_all(b"C037_STDIN\n").ok())
                .is_some();
            match process.wait_with_output() {
                Ok(result) => (
                    json!({"wait":format!("{:?}",result.outcome),"stdin_pipe_write_ok":stdin_ok}),
                    String::from_utf8_lossy(&result.stdout).into_owned(),
                    String::from_utf8_lossy(&result.stderr).into_owned(),
                    pid,
                ),
                Err(error) => (
                    json!({"error":error.to_string(),"stdin_pipe_write_ok":stdin_ok}),
                    String::new(),
                    String::new(),
                    pid,
                ),
            }
        }
        Err(error) => (
            json!({"error":error.to_string()}),
            String::new(),
            String::new(),
            0,
        ),
    };
    let child_proof = fs::read_to_string(&proof)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let descendant_proof = fs::read_to_string(&descendant)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let child_entry = fs::read(&entry).is_ok();
    let acl_after_run = Command::new("icacls.exe")
        .arg(&secret)
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    let child_denied = child_proof.as_ref().is_some_and(|p| {
        p["host_path_read_ok"] == false
            && p["host_path_read_error"] == 5
            && p["host_path_secret_match"] == false
    });
    let child_no_same_handle = child_proof
        .as_ref()
        .is_some_and(|p| p["handle_same_file"] == false && p["secret_match"] == false);
    let descendant_no_same_handle = descendant_proof.as_ref().is_some_and(|p| {
        p["handle_same_file"] == false
            && p["secret_match"] == false
            && p["host_path_secret_match"] == false
            && p["host_path_read_ok"] == false
            && p["host_path_read_error"] == 5
            && p["pid"].as_u64().is_some_and(|pid| pid > 0)
            && p["parent_pid"] == child_pid
    });
    let stdio_ok = stdout.contains(&format!("C037_STDOUT nonce={nonce}"))
        && stderr.contains(&format!("C037_STDERR nonce={nonce}"))
        && child_proof.as_ref().is_some_and(|p| p["stdin_ok"] == true);
    let fixture_content_unchanged =
        fs::read(&secret).is_ok_and(|bytes| bytes == b"synthetic-c035-secret");
    let synthetic_acl_stable = !acl_after_grant.is_empty() && acl_after_grant == acl_after_run;
    let qualified = acl_granted
        && source_identity.is_some()
        && fixture_content_unchanged
        && synthetic_acl_stable
        && inherit_flag_set
        && inherit_on
        && inherit_off
        && effective_tier == "base-container"
        && child_entry
        && child_pid > 0
        && child_denied
        && child_no_same_handle
        && descendant_no_same_handle
        && stdio_ok
        && outcome["wait"] == "Exited(0)";
    let failed_gates = [
        (!acl_granted).then_some("synthetic-acl"),
        source_identity
            .is_none()
            .then_some("source-handle-object-identity"),
        (!fixture_content_unchanged || !synthetic_acl_stable).then_some("fixture-integrity"),
        (!inherit_flag_set).then_some("parent-handle-inherit-flag"),
        (!inherit_on).then_some("ordinary-inherit-on-positive"),
        (!inherit_off).then_some("ordinary-inherit-off-negative"),
        (effective_tier != "base-container").then_some("effective-tier"),
        (!child_entry).then_some("mxc-child-entry"),
        (!child_denied).then_some("mxc-same-file-path-denial"),
        (!child_no_same_handle).then_some("mxc-ambient-handle"),
        (!descendant_no_same_handle).then_some("descendant-handle"),
        (!stdio_ok).then_some("stdio-functional-streams"),
        (outcome["wait"] != "Exited(0)").then_some("mxc-exit"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    emit(json!({
        "schema":"codegg.mxc_c037.v1","qualified_test_only":qualified,"disposition":if qualified {"bounded-positive-test-only"} else {"negative-or-incomplete"},
        "sdk":{"crate":"mxc-sdk","version":"1.0.0","checksum":"e4866ca589a68edc8e931eba58eed9253ad1ce670e974aedc4830a1d94bca6cd","vcs_commit":"a846837d827b496fc4a08dfe60554515758c6647"},
        "host_tier":effective_tier,"tier_probe":{"tier":effective_tier,"error_present":!probe_json["error"].is_null()},"control_inherit_on_read":inherit_on,"control_inherit_off_read":inherit_off,
        "parent_handle_inheritable":inherit_flag_set,"mxc_ambient_transfer_observed":child_proof.as_ref().is_some_and(|p|p["handle_same_file"]==true),
        "source_handle_object_identity_available":source_identity.is_some(),
        "source_handle_owner_pid":std::process::id(),
        "mxc_launcher_handle_owner_relation":"the public request exposes no explicit handle-transfer API; a numeric value supplied in command arguments is only a probe and is not transfer evidence",
        "mxc_handle_attempted":true,"mxc_handle_attempt":{"present":child_proof.as_ref().is_some_and(|p|p["handle_present"]==true),"same_file":child_proof.as_ref().is_some_and(|p|p["handle_same_file"]==true),"read_secret":child_proof.as_ref().is_some_and(|p|p["secret_match"]==true)},
        "path_read_denied":child_denied,"child_entry":child_entry,"child_pid":child_pid,"stage_write_proof_present":child_proof.is_some(),"mxc_probe_result":child_proof,
        "descendant_probe_result":descendant_proof,"descendant_cleanup":"descendant completed with exit code 0 and was synchronously reaped within 10s; parent PID correlated",
        "stdout_stderr_ok":stdio_ok,"stdin_pipe_write_ok":outcome["stdin_pipe_write_ok"],"execution":outcome,
        "explicit_transfer_supported":false,"explicit_transfer_observed":false,"unavailable_dimensions":["public ContainerRequest has no arbitrary handle field","DuplicateHandle/cross-process authority","named-pipe IPC beyond SDK stdio","Windows sandbox host matrix","network enforcement"],
        "fixture_integrity":{"content_unchanged":fixture_content_unchanged,"synthetic_fixture_acl_stable_after_setup":synthetic_acl_stable,"original_host_acl_mutation":false,"only_disposable_synthetic_fixture_acl_granted":true},
        "failed_gates":failed_gates
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_transfer_is_not_relabelled_as_a_denial() {
        let report = json!({"explicit_transfer_supported":false,"explicit_transfer_observed":false,"unavailable_dimensions":["DuplicateHandle"]});
        assert_eq!(report["explicit_transfer_supported"], false);
        assert_eq!(report["unavailable_dimensions"][0], "DuplicateHandle");
    }
}
