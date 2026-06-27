use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HumanShellPolicyDecision {
    Allow,
    Warn { reason: String },
    Block { reason: String },
}

pub fn evaluate_command(command: &str) -> HumanShellPolicyDecision {
    let normalized = command.trim().to_lowercase();

    if let Some(reason) = check_block_patterns(&normalized) {
        return HumanShellPolicyDecision::Block { reason };
    }

    if let Some(reason) = check_warn_patterns(&normalized) {
        return HumanShellPolicyDecision::Warn { reason };
    }

    HumanShellPolicyDecision::Allow
}

fn check_block_patterns(cmd: &str) -> Option<String> {
    let block_patterns: &[(&str, &str)] = &[
        (
            r"rm\s+-[a-zA-Z]*r\s*-?[a-zA-Z]*f\s*-?[a-zA-Z]*\s+/",
            "rm -rf / is catastrophic",
        ),
        (
            r"rm\s+-[a-zA-Z]*f\s*-?[a-zA-Z]*r\s*-?[a-zA-Z]*\s+/",
            "rm -rf / is catastrophic",
        ),
        (r"mkfs\.", "mkfs destroys filesystems"),
        (r"mkfs\s", "mkfs destroys filesystems"),
        (
            r"dd\s+if=/dev/zero\s+of=/dev/",
            "dd overwriting device nodes",
        ),
        (r"dd\s+if=/dev/", "dd reading/writing device nodes"),
        (r":\(\)\s*\{.*\|.*&\s*\}", "fork bomb"),
        (r"shutdown\s", "shutdown halts the system"),
        (r"reboot\s?", "reboot restarts the system"),
        (r"poweroff\s?", "poweroff halts the system"),
        (r"halt\s?", "halt halts the system"),
    ];

    for (pattern, reason) in block_patterns {
        let re = Regex::new(pattern).unwrap();
        if re.is_match(cmd) {
            return Some(reason.to_string());
        }
    }
    None
}

fn check_warn_patterns(cmd: &str) -> Option<String> {
    let warn_patterns: &[(&str, &str)] = &[
        (
            r"rm\s+-[a-zA-Z]*r\s*-?[a-zA-Z]*f\s*-?[a-zA-Z]*\s+\.",
            "rm -rf in current directory",
        ),
        (
            r"rm\s+-[a-zA-Z]*f\s*-?[a-zA-Z]*r\s*-?[a-zA-Z]*\s+\.",
            "rm -rf in current directory",
        ),
        (
            r"git\s+clean\s+-[a-zA-Z]*f[a-zA-Z]*d?[a-zA-Z]*",
            "git clean removes untracked files",
        ),
        (r"sudo\s", "sudo runs with elevated privileges"),
        (r"curl\s.*\|\s*sh", "piping curl to shell"),
        (r"curl\s.*\|\s*bash", "piping curl to bash"),
        (r"wget\s.*\|\s*sh", "piping wget to shell"),
        (r"wget\s.*\|\s*bash", "piping wget to bash"),
        (
            r"chmod\s+-[a-zA-Z]*r\s+777\b",
            "chmod -R 777 is overly permissive",
        ),
        (
            r"chmod\s+-[a-zA-Z]*r\s+a\+rwx",
            "chmod -R a+rwx is overly permissive",
        ),
        (
            r"chown\s+-[a-zA-Z]*r\b",
            "recursive chown changes ownership widely",
        ),
    ];

    for (pattern, reason) in warn_patterns {
        let re = Regex::new(pattern).unwrap();
        if re.is_match(cmd) {
            return Some(reason.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_command_allowed() {
        assert_eq!(
            evaluate_command("cargo test"),
            HumanShellPolicyDecision::Allow
        );
    }

    #[test]
    fn ls_allowed() {
        assert_eq!(evaluate_command("ls -la"), HumanShellPolicyDecision::Allow);
    }

    #[test]
    fn git_status_allowed() {
        assert_eq!(
            evaluate_command("git status"),
            HumanShellPolicyDecision::Allow
        );
    }

    #[test]
    fn rm_rf_root_blocked() {
        match evaluate_command("rm -rf /") {
            HumanShellPolicyDecision::Block { .. } => {}
            _ => panic!("expected block"),
        }
    }

    #[test]
    fn rm_rf_root_variants_blocked() {
        let blocked = ["rm -rf /", "rm -r -f /", "rm -f -r /"];
        for cmd in &blocked {
            match evaluate_command(cmd) {
                HumanShellPolicyDecision::Block { .. } => {}
                _ => panic!("expected block for: {}", cmd),
            }
        }
    }

    #[test]
    fn rm_rf_glob_blocked() {
        match evaluate_command("rm -rf /*") {
            HumanShellPolicyDecision::Block { .. } => {}
            _ => panic!("expected block"),
        }
    }

    #[test]
    fn mkfs_blocked() {
        match evaluate_command("mkfs.ext4 /dev/sda1") {
            HumanShellPolicyDecision::Block { .. } => {}
            _ => panic!("expected block"),
        }
    }

    #[test]
    fn dd_device_blocked() {
        match evaluate_command("dd if=/dev/zero of=/dev/sda") {
            HumanShellPolicyDecision::Block { .. } => {}
            _ => panic!("expected block"),
        }
    }

    #[test]
    fn shutdown_blocked() {
        match evaluate_command("shutdown -h now") {
            HumanShellPolicyDecision::Block { .. } => {}
            _ => panic!("expected block"),
        }
    }

    #[test]
    fn reboot_blocked() {
        match evaluate_command("reboot") {
            HumanShellPolicyDecision::Block { .. } => {}
            _ => panic!("expected block"),
        }
    }

    #[test]
    fn poweroff_blocked() {
        match evaluate_command("poweroff") {
            HumanShellPolicyDecision::Block { .. } => {}
            _ => panic!("expected block"),
        }
    }

    #[test]
    fn halt_blocked() {
        match evaluate_command("halt") {
            HumanShellPolicyDecision::Block { .. } => {}
            _ => panic!("expected block"),
        }
    }

    #[test]
    fn rm_rf_dot_warned() {
        match evaluate_command("rm -rf .") {
            HumanShellPolicyDecision::Warn { reason } => {
                assert!(reason.contains("current directory"));
            }
            _ => panic!("expected warn"),
        }
    }

    #[test]
    fn git_clean_warned() {
        match evaluate_command("git clean -xfd") {
            HumanShellPolicyDecision::Warn { .. } => {}
            _ => panic!("expected warn"),
        }
    }

    #[test]
    fn sudo_warned() {
        match evaluate_command("sudo apt update") {
            HumanShellPolicyDecision::Warn { reason } => {
                assert!(reason.contains("elevated"));
            }
            _ => panic!("expected warn"),
        }
    }

    #[test]
    fn curl_pipe_sh_warned() {
        match evaluate_command("curl https://example.com/script.sh | sh") {
            HumanShellPolicyDecision::Warn { .. } => {}
            _ => panic!("expected warn"),
        }
    }

    #[test]
    fn curl_pipe_bash_warned() {
        match evaluate_command("curl https://example.com/install | bash") {
            HumanShellPolicyDecision::Warn { .. } => {}
            _ => panic!("expected warn"),
        }
    }

    #[test]
    fn wget_pipe_sh_warned() {
        match evaluate_command("wget -qO- https://example.com/x | sh") {
            HumanShellPolicyDecision::Warn { .. } => {}
            _ => panic!("expected warn"),
        }
    }

    #[test]
    fn chmod_777_warned() {
        match evaluate_command("chmod -R 777 /var/www") {
            HumanShellPolicyDecision::Warn { .. } => {}
            _ => panic!("expected warn"),
        }
    }

    #[test]
    fn chown_recursive_warned() {
        match evaluate_command("chown -R user:group /opt") {
            HumanShellPolicyDecision::Warn { .. } => {}
            _ => panic!("expected warn"),
        }
    }

    #[test]
    fn empty_command_allowed() {
        assert_eq!(evaluate_command(""), HumanShellPolicyDecision::Allow);
    }

    #[test]
    fn whitespace_only_allowed() {
        assert_eq!(evaluate_command("   "), HumanShellPolicyDecision::Allow);
    }

    #[test]
    fn cargo_check_allowed() {
        assert_eq!(
            evaluate_command("cargo check"),
            HumanShellPolicyDecision::Allow
        );
    }

    #[test]
    fn cargo_clippy_allowed() {
        assert_eq!(
            evaluate_command("cargo clippy -- -D warnings"),
            HumanShellPolicyDecision::Allow
        );
    }
}
