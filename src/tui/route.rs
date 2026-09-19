#[derive(Debug, Clone, PartialEq, Default)]
pub enum Route {
    #[default]
    Home,
    Session(String),
    /// Team Collaboration M005: non-modal Workspace primary view.
    /// The selected project is an explicit routing locator stored in
    /// `WorkspaceDashboardState`; it never confers authority. The
    /// ordinary Session/Task composer stays editable while this route
    /// is active and the sidebar shows project chat for the selection.
    Workspace,
}

pub struct RouteManager {
    current: Route,
    history: Vec<Route>,
}

impl RouteManager {
    pub fn new() -> Self {
        Self {
            current: Route::Home,
            history: Vec::new(),
        }
    }

    pub fn current(&self) -> &Route {
        &self.current
    }

    pub fn navigate_to(&mut self, route: Route) {
        self.history.push(self.current.clone());
        self.current = route;
    }

    pub fn back(&mut self) -> bool {
        if let Some(route) = self.history.pop() {
            self.current = route;
            true
        } else {
            false
        }
    }
}

impl Default for RouteManager {
    fn default() -> Self {
        Self::new()
    }
}
