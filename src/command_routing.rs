use crate::command_intent::{CommandIntent, CommandIntentKind};
use crate::command_planner::{CommandPlan, ExecutionBackend};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutingDecision {
    RouteToTestRunner {
        argv: Vec<String>,
        scope_label: String,
    },
    RouteToShell {
        command: String,
        timeout_secs: Option<u64>,
    },
    RouteToPythonScripting {
        script: String,
        mode: PythonScriptMode,
        timeout_secs: Option<u64>,
    },
    RouteToNativeTool {
        tool_name: String,
        command: String,
    },
    RouteToManagedProcess {
        argv: Vec<String>,
        timeout_secs: Option<u64>,
    },
    Rejected {
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PythonScriptMode {
    Analyze,
    Transform,
    Verify,
}

pub fn resolve_routing(plan: &CommandPlan) -> RoutingDecision {
    match plan.backend {
        ExecutionBackend::TestRunner => resolve_test_runner(&plan.intent),
        ExecutionBackend::PythonScripting => resolve_python_scripting(&plan.intent),
        ExecutionBackend::NativeTool(ref name) => RoutingDecision::RouteToNativeTool {
            tool_name: name.clone(),
            command: plan.intent.command.clone(),
        },
        ExecutionBackend::ManagedProcess => resolve_managed_process(&plan.intent),
        ExecutionBackend::RawShell => RoutingDecision::RouteToShell {
            command: plan.intent.command.clone(),
            timeout_secs: plan.timeout_secs,
        },
        ExecutionBackend::Rejected => RoutingDecision::Rejected {
            reason: "command was rejected by classifier".to_string(),
        },
    }
}

fn resolve_test_runner(intent: &CommandIntent) -> RoutingDecision {
    let argv: Vec<String> = intent
        .command
        .split_whitespace()
        .map(String::from)
        .collect();
    let scope_label = format!("command-intent:{}", intent.kind.label());
    RoutingDecision::RouteToTestRunner { argv, scope_label }
}

fn resolve_python_scripting(intent: &CommandIntent) -> RoutingDecision {
    let mode = match intent.kind {
        CommandIntentKind::PythonVerify => PythonScriptMode::Verify,
        CommandIntentKind::PythonAnalyze => PythonScriptMode::Analyze,
        _ => PythonScriptMode::Transform,
    };
    RoutingDecision::RouteToPythonScripting {
        script: intent.command.clone(),
        mode,
        timeout_secs: Some(60),
    }
}

fn resolve_managed_process(intent: &CommandIntent) -> RoutingDecision {
    let argv: Vec<String> = intent
        .command
        .split_whitespace()
        .map(String::from)
        .collect();
    RoutingDecision::RouteToManagedProcess {
        argv,
        timeout_secs: Some(30),
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
}
