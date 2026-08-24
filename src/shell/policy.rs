use std::sync::LazyLock;

use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HumanShellPolicyDecision {
    Allow,
    Warn { reason: String },
    Block { reason: String },
}

static BLOCK_RM_RF_ROOT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"rm\s+-[a-zA-Z]*r\s*-?[a-zA-Z]*f\s*-?[a-zA-Z]*\s+/").unwrap());
static BLOCK_RM_FR_ROOT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"rm\s+-[a-zA-Z]*f\s*-?[a-zA-Z]*r\s*-?[a-zA-Z]*\s+/").unwrap());
static BLOCK_MKFS_DOT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"mkfs\.").unwrap());
static BLOCK_MKFS_SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"mkfs\s").unwrap());
static BLOCK_DD_ZERO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"dd\s+if=/dev/zero\s+of=/dev/").unwrap());
static BLOCK_DD_DEV: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"dd\s+if=/dev/").unwrap());
static BLOCK_FORK_BOMB: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[a-zA-Z_:][a-zA-Z0-9_]*\s*\(\)\s*\{[^}]*\|[^}]*&[^}]*\}").unwrap()
});
static BLOCK_SHUTDOWN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"shutdown\s").unwrap());
static BLOCK_REBOOT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"reboot\s?").unwrap());
static BLOCK_POWEROFF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"poweroff\s?").unwrap());
static BLOCK_HALT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"halt\s?").unwrap());

static WARN_RM_RF_DOT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"rm\s+-[a-zA-Z]*r\s*-?[a-zA-Z]*f\s*-?[a-zA-Z]*\s+\.").unwrap());
static WARN_RM_FR_DOT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"rm\s+-[a-zA-Z]*f\s*-?[a-zA-Z]*r\s*-?[a-zA-Z]*\s+\.").unwrap());
static WARN_GIT_CLEAN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"git\s+clean\s+-[a-zA-Z]*f[a-zA-Z]*d?[a-zA-Z]*").unwrap());
static WARN_SUDO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"sudo\s").unwrap());
static WARN_CURL_SH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"curl\s.*\|\s*sh").unwrap());
static WARN_CURL_BASH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"curl\s.*\|\s*bash").unwrap());
static WARN_WGET_SH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"wget\s.*\|\s*sh").unwrap());
static WARN_WGET_BASH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"wget\s.*\|\s*bash").unwrap());
static WARN_CHMOD_777: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"chmod\s+-[a-zA-Z]*r\s+777\b").unwrap());
static WARN_CHMOD_A: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"chmod\s+-[a-zA-Z]*r\s+a\+rwx").unwrap());
static WARN_CHOWN_R: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"chown\s+-[a-zA-Z]*r\b").unwrap());

pub fn evaluate_command(command: &str) -> HumanShellPolicyDecision {
    let normalized = normalize_command(command);

    if let Some(reason) = check_block_patterns(&normalized) {
        return HumanShellPolicyDecision::Block { reason };
    }

    if let Some(reason) = check_warn_patterns(&normalized) {
        return HumanShellPolicyDecision::Warn { reason };
    }

    HumanShellPolicyDecision::Allow
}

fn normalize_command(command: &str) -> String {
    command
        .trim()
        .to_lowercase()
        .replace("--recursive", "-r")
        .replace("--force", "-f")
        .replace(['\'', '"'], "")
}

fn check_block_patterns(cmd: &str) -> Option<String> {
    if BLOCK_RM_RF_ROOT.is_match(cmd) || BLOCK_RM_FR_ROOT.is_match(cmd) {
        return Some("rm -rf / is catastrophic".to_string());
    }
    if BLOCK_MKFS_DOT.is_match(cmd) || BLOCK_MKFS_SPACE.is_match(cmd) {
        return Some("mkfs destroys filesystems".to_string());
    }
    if BLOCK_DD_ZERO.is_match(cmd) || BLOCK_DD_DEV.is_match(cmd) {
        return Some("dd reading/writing device nodes".to_string());
    }
    if BLOCK_FORK_BOMB.is_match(cmd) {
        return Some("fork bomb".to_string());
    }
    if BLOCK_SHUTDOWN.is_match(cmd) {
        return Some("shutdown halts the system".to_string());
    }
    if BLOCK_REBOOT.is_match(cmd) {
        return Some("reboot restarts the system".to_string());
    }
    if BLOCK_POWEROFF.is_match(cmd) {
        return Some("poweroff halts the system".to_string());
    }
    if BLOCK_HALT.is_match(cmd) {
        return Some("halt halts the system".to_string());
    }
    None
}

fn check_warn_patterns(cmd: &str) -> Option<String> {
    if WARN_RM_RF_DOT.is_match(cmd) || WARN_RM_FR_DOT.is_match(cmd) {
        return Some("rm -rf in current directory".to_string());
    }
    if WARN_GIT_CLEAN.is_match(cmd) {
        return Some("git clean removes untracked files".to_string());
    }
    if WARN_SUDO.is_match(cmd) {
        return Some("sudo runs with elevated privileges".to_string());
    }
    if WARN_CURL_SH.is_match(cmd) {
        return Some("piping curl to shell".to_string());
    }
    if WARN_CURL_BASH.is_match(cmd) {
        return Some("piping curl to bash".to_string());
    }
    if WARN_WGET_SH.is_match(cmd) {
        return Some("piping wget to shell".to_string());
    }
    if WARN_WGET_BASH.is_match(cmd) {
        return Some("piping wget to bash".to_string());
    }
    if WARN_CHMOD_777.is_match(cmd) {
        return Some("chmod -R 777 is overly permissive".to_string());
    }
    if WARN_CHMOD_A.is_match(cmd) {
        return Some("chmod -R a+rwx is overly permissive".to_string());
    }
    if WARN_CHOWN_R.is_match(cmd) {
        return Some("recursive chown changes ownership widely".to_string());
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
        let blocked = [
            "rm -rf /",
            "rm -r -f /",
            "rm -f -r /",
            "rm --recursive --force /",
            "rm -rf '/'",
        ];
        for cmd in &blocked {
            match evaluate_command(cmd) {
                HumanShellPolicyDecision::Block { .. } => {}
                _ => panic!("expected block for: {}", cmd),
            }
        }
    }

    #[test]
    fn named_fork_bomb_is_blocked() {
        assert!(matches!(
            evaluate_command("a(){ a|a& };a"),
            HumanShellPolicyDecision::Block { .. }
        ));
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
