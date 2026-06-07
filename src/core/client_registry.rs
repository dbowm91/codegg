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
}
