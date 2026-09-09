use chrono::{DateTime, Utc};
use dashmap::DashMap;

pub use codegg_core::transport_auth::AuthenticatedPrincipal;

#[derive(Debug, Clone)]
pub struct ConnectedClient {
    pub client_id: String,
    pub client_name: String,
    pub connected_at: DateTime<Utc>,
    pub attached_sessions: Vec<String>,
    pub capabilities: Option<crate::protocol::frames::ClientCapabilities>,
    /// Transport-bound canonical principal. Set once at handshake from
    /// trusted transport evidence; never derived from a request payload.
    /// `None` only for legacy registration paths that have not yet bound.
    pub principal: Option<AuthenticatedPrincipal>,
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
                principal: None,
            },
        );
    }

    /// Register a connection with its transport-bound canonical principal.
    /// The principal is immutable for the connection; see [`Self::set_principal`].
    pub fn register_with_principal(
        &self,
        client_id: String,
        client_name: String,
        capabilities: Option<crate::protocol::frames::ClientCapabilities>,
        principal: AuthenticatedPrincipal,
    ) {
        self.clients.insert(
            client_id.clone(),
            ConnectedClient {
                client_id,
                client_name,
                connected_at: Utc::now(),
                attached_sessions: Vec::new(),
                capabilities,
                principal: Some(principal),
            },
        );
    }

    /// Bind a principal to an already-registered connection. The binding is
    /// immutable: when a different principal is already bound the call fails
    /// closed (`false`) and keeps the original. Binding the same principal
    /// id is idempotent (`true`). Binding when no principal is bound yet
    /// installs it and returns `true`. Returns `false` when the client is
    /// unknown.
    pub fn set_principal(&self, client_id: &str, principal: AuthenticatedPrincipal) -> bool {
        if let Some(mut client) = self.clients.get_mut(client_id) {
            match &client.principal {
                None => {
                    client.principal = Some(principal);
                    true
                }
                Some(existing)
                    if existing.principal_id() == principal.principal_id()
                        && existing.auth_method() == principal.auth_method() =>
                {
                    true
                }
                Some(_) => false,
            }
        } else {
            false
        }
    }

    /// Clone the bound principal for a connection, if any.
    pub fn principal_for(&self, client_id: &str) -> Option<AuthenticatedPrincipal> {
        self.clients
            .get(client_id)
            .and_then(|client| client.principal.clone())
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

    #[test]
    fn principal_binding_is_immutable_for_connection() {
        let reg = ClientRegistry::new();
        reg.register("c1".to_string(), "test".to_string(), None);
        assert!(reg.principal_for("c1").is_none());

        let local = AuthenticatedPrincipal::local_owner("c1");
        assert!(reg.set_principal("c1", local.clone()));
        assert_eq!(reg.principal_for("c1").as_ref(), Some(&local));

        // Rebinding the same principal is idempotent.
        assert!(reg.set_principal("c1", local.clone()));

        // A different principal cannot hijack the connection.
        let other = AuthenticatedPrincipal::bootstrap_global_bearer("c1");
        // Same LocalOwner id but different auth method is still a distinct
        // binding attempt; it must fail closed.
        assert!(!reg.set_principal("c1", other));
        assert_eq!(reg.principal_for("c1").as_ref(), Some(&local));

        // Unknown clients cannot be bound.
        assert!(!reg.set_principal("missing", AuthenticatedPrincipal::local_owner("missing")));
    }

    #[test]
    fn register_with_principal_carries_transport_identity() {
        let reg = ClientRegistry::new();
        let principal = AuthenticatedPrincipal::local_owner("c2");
        reg.register_with_principal(
            "c2".to_string(),
            "local-tui".to_string(),
            None,
            principal.clone(),
        );
        assert_eq!(reg.principal_for("c2").as_ref(), Some(&principal));
    }
}
