//! Compatibility facade for the pre-M006 routing API.

use crate::command_intent::plan::CommandPlan;

pub use crate::command_intent::plan::CommandDispatchTarget as RoutingDecision;

pub fn resolve_routing(plan: &CommandPlan) -> RoutingDecision {
    plan.dispatch_target()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_intent::classify_command;
    use crate::command_intent::plan::plan_execution;

    #[test]
    fn compatibility_facade_returns_canonical_target() {
        let plan = plan_execution(&classify_command("git status"));
        assert!(matches!(
            resolve_routing(&plan),
            RoutingDecision::RouteToGit { .. }
        ));
        assert_eq!(resolve_routing(&plan), plan.dispatch_target());
    }
}
