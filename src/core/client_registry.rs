use chrono::{DateTime, Utc};
use dashmap::DashMap;

#[derive(Debug, Clone)]
pub struct ConnectedClient {
    pub client_id: String,
    pub client_name: String,
    pub connected_at: DateTime<Utc>,
    pub attached_sessions: Vec<String>,
    pub capabilities: Option<crate::protocol::frames::ClientCapabilities>,
}

pub struct ClientRegistry {
    clients: DashMap<String, ConnectedClient>,
}

impl Default for ClientRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientRegistry {
    pub fn new() -> Self {
        Self {
            clients: DashMap::new(),
        }
    }

    pub fn register(
        &self,
        client_id: String,
        client_name: String,
        capabilities: Option<crate::protocol::frames::ClientCapabilities>,
    ) {
        self.clients.insert(
            client_id.clone(),
            ConnectedClient {
                client_id,
                client_name,
                connected_at: Utc::now(),
                attached_sessions: Vec::new(),
                capabilities,
            },
        );
    }

    pub fn unregister(&self, client_id: &str) {
        self.clients.remove(client_id);
    }

    /// Update the display name (and optionally capabilities) for an already
    /// registered client. Returns true if a record was found and updated.
    pub fn set_name(
        &self,
        client_id: &str,
        new_name: String,
        capabilities: Option<crate::protocol::frames::ClientCapabilities>,
    ) -> bool {
        if let Some(mut client) = self.clients.get_mut(client_id) {
            client.client_name = new_name;
            if capabilities.is_some() {
                client.capabilities = capabilities;
            }
            true
        } else {
            false
        }
    }

    pub fn attach_session(&self, client_id: &str, session_id: &str) {
        if let Some(mut client) = self.clients.get_mut(client_id) {
            if !client.attached_sessions.contains(&session_id.to_string()) {
                client.attached_sessions.push(session_id.to_string());
            }
        }
    }

    pub fn detach_session(&self, client_id: &str, session_id: &str) {
        if let Some(mut client) = self.clients.get_mut(client_id) {
            client.attached_sessions.retain(|s| s != session_id);
        }
    }

    pub fn count(&self) -> usize {
        self.clients.len()
    }

    #[allow(dead_code)]
    pub fn list(&self) -> Vec<ConnectedClient> {
        self.clients.iter().map(|r| r.value().clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_count() {
        let reg = ClientRegistry::new();
        assert_eq!(reg.count(), 0);

        reg.register("c1".to_string(), "test-client".to_string(), None);
        assert_eq!(reg.count(), 1);

        reg.register("c2".to_string(), "another-client".to_string(), None);
        assert_eq!(reg.count(), 2);
    }

    #[test]
    fn unregister() {
        let reg = ClientRegistry::new();
        reg.register("c1".to_string(), "test".to_string(), None);
        assert_eq!(reg.count(), 1);

        reg.unregister("c1");
        assert_eq!(reg.count(), 0);
    }

    #[test]
    fn attach_detach_session() {
        let reg = ClientRegistry::new();
        reg.register("c1".to_string(), "test".to_string(), None);
        reg.attach_session("c1", "s1");
        reg.attach_session("c1", "s2");

        let clients = reg.list();
        assert_eq!(clients[0].attached_sessions.len(), 2);

        reg.detach_session("c1", "s1");
        let clients = reg.list();
        assert_eq!(clients[0].attached_sessions.len(), 1);
    }

    #[test]
    fn set_name_updates_existing_client() {
        let reg = ClientRegistry::new();
        reg.register("c1".to_string(), "placeholder".to_string(), None);

        let updated = reg.set_name("c1", "real-name".to_string(), None);
        assert!(updated);
        let clients = reg.list();
        assert_eq!(clients[0].client_name, "real-name");
    }

    #[test]
    fn set_name_missing_client_returns_false() {
        let reg = ClientRegistry::new();
        let updated = reg.set_name("nonexistent", "x".to_string(), None);
        assert!(!updated);
    }

    #[test]
    fn register_preserves_codegg_tui_client_name() {
        // The `SocketCoreClient::connect` flow registers the client with
        // `client_name = "codegg-tui"`. Verify that registration round-trips
        // the name through `list()` so the daemon's snapshot of connected
        // clients reports the right identity.
        let reg = ClientRegistry::new();
        reg.register(
            "client-codegg-1".to_string(),
            "codegg-tui".to_string(),
            None,
        );

        let clients = reg.list();
        assert_eq!(clients.len(), 1);
        assert_eq!(clients[0].client_id, "client-codegg-1");
        assert_eq!(clients[0].client_name, "codegg-tui");
    }
}
