use codegg_core::identity::{
    AgentRunId, AgentTaskId, AuditEventId, ChannelId, NodeId, PrincipalId, ProjectBinding,
    ProjectId, ProviderConnectionId, RepositoryId, SessionBinding, WorkspaceId, WorktreeId,
};

#[test]
fn public_core_boundary_exposes_typed_identity_and_relations() {
    let project = ProjectId::parse("project-integration").unwrap();
    let workspace = WorkspaceId::parse("workspace-integration").unwrap();
    let repository = RepositoryId::parse("repository-integration").unwrap();
    let binding = ProjectBinding::new(project.clone(), workspace.clone())
        .with_repository(repository)
        .with_worktree(WorktreeId::parse("worktree-integration").unwrap())
        .with_node(NodeId::parse("node-integration").unwrap());
    let session = SessionBinding::new(project, workspace);

    let json = serde_json::to_string(&(binding.clone(), session.clone())).unwrap();
    let decoded: (ProjectBinding, SessionBinding) = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, (binding, session));
}

#[test]
fn all_identity_kinds_are_public_core_types() {
    assert!(AgentRunId::parse("agent-run-integration").is_ok());
    assert!(AgentTaskId::parse("agent-task-integration").is_ok());
    assert!(PrincipalId::parse("principal-integration").is_ok());
    assert!(ProviderConnectionId::parse("provider-integration").is_ok());
    assert!(ChannelId::parse("channel-integration").is_ok());
    assert!(AuditEventId::parse("audit-integration").is_ok());
}
