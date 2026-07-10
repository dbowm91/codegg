use crate::command_planner::{CommandPlan, ExecutionBackend, PythonModeGuess};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutingDecision {
    RouteToTestRunner {
        argv: Vec<String>,
        scope_label: String,
        validated_command: Option<String>,
    },
    RouteToShell {
        command: String,
        timeout_secs: Option<u64>,
    },
    RouteToPythonScripting {
        script: String,
        mode: PythonModeGuess,
        timeout_secs: Option<u64>,
    },
    RouteToNativeTool {
        tool_name: String,
        command: String,
    },
    RouteToManagedProcess {
        argv: Vec<String>,
        cwd: std::path::PathBuf,
        timeout_secs: Option<u64>,
    },
    Rejected {
        reason: String,
    },
}

pub fn resolve_routing(plan: &CommandPlan) -> RoutingDecision {
    match &plan.backend {
        ExecutionBackend::TestRunner { validated_command } => {
            let argv: Vec<String> = plan.intent.parsed_argv.clone().unwrap_or_else(|| {
                plan.intent
                    .command
                    .split_whitespace()
                    .map(String::from)
                    .collect()
            });
            let scope_label = format!("command-intent:{}", plan.intent.kind.label());
            RoutingDecision::RouteToTestRunner {
                argv,
                scope_label,
                validated_command: validated_command.clone(),
            }
        }
        ExecutionBackend::PythonScript { script, mode_guess } => {
            RoutingDecision::RouteToPythonScripting {
                script: script.clone(),
                mode: *mode_guess,
                timeout_secs: plan.timeout_secs,
            }
        }
        ExecutionBackend::NativeTool { tool_name } => RoutingDecision::RouteToNativeTool {
            tool_name: tool_name.clone(),
            command: plan.intent.command.clone(),
        },
        ExecutionBackend::ManagedArgv { argv, cwd } => RoutingDecision::RouteToManagedProcess {
            argv: argv.clone(),
            cwd: cwd.clone(),
            timeout_secs: plan.timeout_secs,
        },
        ExecutionBackend::GitMutating { tool_name: _, argv } => {
            RoutingDecision::RouteToManagedProcess {
                argv: argv.clone(),
                cwd: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
                timeout_secs: plan.timeout_secs,
            }
        }
        ExecutionBackend::RawShell { command } => RoutingDecision::RouteToShell {
            command: command.clone(),
            timeout_secs: plan.timeout_secs,
        },
        ExecutionBackend::Reject { reason } => RoutingDecision::Rejected {
            reason: reason.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_intent::classify_command;
    use crate::command_planner::plan_execution;

    #[test]
    fn test_command_routes_to_test_runner() {
        let intent = classify_command("cargo test --lib");
        let plan = plan_execution(&intent);
        let decision = resolve_routing(&plan);
        match decision {
            RoutingDecision::RouteToTestRunner { argv, .. } => {
                assert_eq!(argv[0], "cargo");
                assert_eq!(argv[1], "test");
            }
            _ => panic!("expected RouteToTestRunner"),
        }
    }

    #[test]
    fn git_status_routes_to_native() {
        let intent = classify_command("git status");
        let plan = plan_execution(&intent);
        let decision = resolve_routing(&plan);
        match decision {
            RoutingDecision::RouteToNativeTool { tool_name, .. } => {
                assert_eq!(tool_name, "egggit");
            }
            _ => panic!("expected RouteToNativeTool"),
        }
    }

    #[test]
    fn python_script_routes_to_python() {
        let intent = classify_command("python3 script.py");
        let plan = plan_execution(&intent);
        let decision = resolve_routing(&plan);
        assert!(matches!(
            decision,
            RoutingDecision::RouteToPythonScripting { .. }
        ));
    }

    #[test]
    fn raw_shell_routes_to_shell() {
        let intent = classify_command("echo hello");
        let plan = plan_execution(&intent);
        let decision = resolve_routing(&plan);
        assert!(matches!(decision, RoutingDecision::RouteToShell { .. }));
    }

    #[test]
    fn search_routes_to_managed() {
        let intent = classify_command("rg 'pattern'");
        let plan = plan_execution(&intent);
        let decision = resolve_routing(&plan);
        assert!(matches!(
            decision,
            RoutingDecision::RouteToManagedProcess { .. }
        ));
    }

    #[test]
    fn rejected_command_is_rejected() {
        let intent = classify_command("");
        let plan = plan_execution(&intent);
        let decision = resolve_routing(&plan);
        assert!(matches!(decision, RoutingDecision::Rejected { .. }));
    }

    #[test]
    fn test_runner_includes_validated_command() {
        let intent = classify_command("cargo test");
        let plan = plan_execution(&intent);
        let decision = resolve_routing(&plan);
        match decision {
            RoutingDecision::RouteToTestRunner {
                validated_command, ..
            } => {
                assert!(validated_command.is_some());
            }
            _ => panic!("expected RouteToTestRunner"),
        }
    }

    #[test]
    fn managed_process_includes_cwd() {
        let intent = classify_command("rg 'pattern'");
        let plan = plan_execution(&intent);
        let decision = resolve_routing(&plan);
        match decision {
            RoutingDecision::RouteToManagedProcess { cwd, .. } => {
                assert!(cwd.exists());
            }
            _ => panic!("expected RouteToManagedProcess"),
        }
    }

    #[test]
    fn test_runner_uses_parsed_argv() {
        let intent = classify_command("cargo test --lib -p foo");
        let plan = plan_execution(&intent);
        let decision = resolve_routing(&plan);
        match decision {
            RoutingDecision::RouteToTestRunner { argv, .. } => {
                assert_eq!(argv, vec!["cargo", "test", "--lib", "-p", "foo"]);
            }
            _ => panic!("expected RouteToTestRunner"),
        }
    }

    #[test]
    fn managed_process_uses_parsed_argv() {
        let intent = classify_command("rg 'fn main' src/");
        let plan = plan_execution(&intent);
        let decision = resolve_routing(&plan);
        match decision {
            RoutingDecision::RouteToManagedProcess { argv, .. } => {
                assert_eq!(argv, vec!["rg", "fn main", "src/"]);
            }
            _ => panic!("expected RouteToManagedProcess"),
        }
    }

    // ── Git mutation routing tests (Workstream E) ────────────────────

    #[test]
    fn git_add_routes_to_managed_process() {
        let intent = classify_command("git add src/main.rs");
        let plan = plan_execution(&intent);
        let decision = resolve_routing(&plan);
        match decision {
            RoutingDecision::RouteToManagedProcess { argv, .. } => {
                assert_eq!(argv[0], "git");
                assert_eq!(argv[1], "add");
                assert_eq!(argv[2], "src/main.rs");
            }
            _ => panic!("expected RouteToManagedProcess, got {:?}", decision),
        }
    }

    #[test]
    fn git_commit_routes_to_managed_process() {
        let intent = classify_command("git commit -m 'fix'");
        let plan = plan_execution(&intent);
        let decision = resolve_routing(&plan);
        match decision {
            RoutingDecision::RouteToManagedProcess { argv, .. } => {
                assert_eq!(argv[0], "git");
                assert_eq!(argv[1], "commit");
            }
            _ => panic!("expected RouteToManagedProcess, got {:?}", decision),
        }
    }

    #[test]
    fn git_push_routes_to_shell() {
        let intent = classify_command("git push origin main");
        let plan = plan_execution(&intent);
        let decision = resolve_routing(&plan);
        assert!(matches!(decision, RoutingDecision::RouteToShell { .. }));
    }

    #[test]
    fn git_reset_hard_routes_to_shell() {
        let intent = classify_command("git reset --hard HEAD~1");
        let plan = plan_execution(&intent);
        let decision = resolve_routing(&plan);
        assert!(matches!(decision, RoutingDecision::RouteToShell { .. }));
    }

    #[test]
    fn git_clean_f_routes_to_shell() {
        let intent = classify_command("git clean -f");
        let plan = plan_execution(&intent);
        let decision = resolve_routing(&plan);
        assert!(matches!(decision, RoutingDecision::RouteToShell { .. }));
    }

    #[test]
    fn git_checkout_routes_to_managed_process() {
        let intent = classify_command("git checkout main");
        let plan = plan_execution(&intent);
        let decision = resolve_routing(&plan);
        match decision {
            RoutingDecision::RouteToManagedProcess { argv, .. } => {
                assert_eq!(argv[0], "git");
                assert_eq!(argv[1], "checkout");
                assert_eq!(argv[2], "main");
            }
            _ => panic!("expected RouteToManagedProcess, got {:?}", decision),
        }
    }

    #[test]
    fn git_stash_routes_to_managed_process() {
        let intent = classify_command("git stash");
        let plan = plan_execution(&intent);
        let decision = resolve_routing(&plan);
        match decision {
            RoutingDecision::RouteToManagedProcess { argv, .. } => {
                assert_eq!(argv[0], "git");
                assert_eq!(argv[1], "stash");
            }
            _ => panic!("expected RouteToManagedProcess, got {:?}", decision),
        }
    }
}
