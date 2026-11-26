#[derive(Debug, Clone, PartialEq, Default)]
pub enum Route {
    #[default]
    Home,
    Session(String),
    /// Non-modal Workspace primary view.
    /// The selected project is an explicit routing locator stored in
    /// `WorkspaceDashboardState`; it never confers authority. The
    /// ordinary Session/Task composer stays editable while this route
    /// is active and the sidebar shows project chat for the selection.
    Workspace,
    /// Non-modal document editor primary view.
    ///
    /// The editor is identified by this route plus the open
    /// `EditorState` attachment. With a document open the editor is a
    /// **whole view**: the main composer is not painted, its rows go to the
    /// document, and its mode badge moves into the editor's own bottom row.
    /// Without a document the route is only a shell message, so the ordinary
    /// composer stays. This is stricter than [`Route::Workspace`], whose
    /// filter is not the session composer. The selected project/workspace
    /// comes from the active tab's explicit execution context and never
    /// confers authority by itself.
    Editor,
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
