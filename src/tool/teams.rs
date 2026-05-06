use std::path::PathBuf;
use std::sync::Arc;

use crate::agent::teams::{
    IdleNotifier, ListMessagesTool, ListTeamsTool, SendMessageTool, SharedTaskList, TeamCreateTool,
    TeamManager, TeamStatusTool,
};

pub struct TeamTools {
    pub team_create: TeamCreateTool,
    pub send_message: SendMessageTool,
    pub list_messages: ListMessagesTool,
    pub team_status: TeamStatusTool,
    pub list_teams: ListTeamsTool,
}

impl TeamTools {
    pub fn new(manager: Arc<TeamManager>, base_dir: PathBuf) -> Self {
        Self {
            team_create: TeamCreateTool::new(manager.clone(), base_dir),
            send_message: SendMessageTool::new(manager.clone()),
            list_messages: ListMessagesTool::new(manager.clone()),
            team_status: TeamStatusTool::new(manager.clone()),
            list_teams: ListTeamsTool::new(manager),
        }
    }

    pub fn register_all(self, registry: &mut crate::tool::ToolRegistry)
    where
        Self: Sized,
    {
        registry.register(self.team_create);
        registry.register(self.send_message);
        registry.register(self.list_messages);
        registry.register(self.team_status);
        registry.register(self.list_teams);
    }
}

pub type SharedTaskListHandle = Arc<SharedTaskList>;
pub type IdleNotifierHandle = Arc<IdleNotifier>;

pub fn create_team_handles() -> (
    Arc<TeamManager>,
    SharedTaskListHandle,
    IdleNotifierHandle,
) {
    (
        Arc::new(TeamManager::new()),
        Arc::new(SharedTaskList::new()),
        Arc::new(IdleNotifier::new()),
    )
}
