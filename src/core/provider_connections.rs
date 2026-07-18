//! Daemon-owned lazy provider-connection runtime resolution.
//!
//! Durable metadata is owned by `codegg_core::provider_connections`, while
//! credential lookup and provider construction are owned by the public
//! `codegg_providers::connection` compatibility APIs. This module supplies
//! the daemon lifecycle, cache, and invalidation boundary around them.

use async_trait::async_trait;
use codegg_core::identity::ProviderConnectionId;
use codegg_core::provider_connections::{
    ProviderConnection, ProviderConnectionRuntimeLease, ProviderConnectionState,
    ProviderKind as CoreProviderKind,
};
use codegg_providers::{
    ConnectionError as ProviderConnectionError, ProviderConnectionDescriptor, ProviderFactory,
    ProviderKind, SecretRef as ProviderSecretRef, SecretResolutionError,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tokio::sync::OnceCell;

#[derive(Debug, Error, Clone)]
pub enum ConnectionStoreError {
    #[error("provider connection store unavailable: {0}")]
    Unavailable(String),
}

/// Errors returned by daemon-owned connection resolution.
#[derive(Debug, Error, Clone)]
pub enum ConnectionError {
    #[error("provider connection '{connection_id}' unavailable: {reason}")]
    Unavailable {
        connection_id: ProviderConnectionId,
        reason: String,
    },
    #[error("provider connection '{0}' has no usable credential")]
    CredentialMissing(ProviderConnectionId),
    #[error("provider connection '{0}' is disabled")]
    Disabled(ProviderConnectionId),
    #[error("provider connection '{connection_id}' credential resolution failed: {reason}")]
    CredentialResolution {
        connection_id: ProviderConnectionId,
        reason: String,
    },
    #[error("provider connection '{connection_id}' could not be constructed: {reason}")]
    Construction {
        connection_id: ProviderConnectionId,
        reason: String,
    },
}

/// Narrow daemon-facing view of the durable core store.
///
/// The implementation for the public SQLite store is supplied below. Keeping
/// this small trait also permits daemon tests to use an in-memory store
/// without creating a second persistence model.
#[async_trait]
pub trait ConnectionStore: Send + Sync {
    async fn get(
        &self,
        connection_id: &ProviderConnectionId,
    ) -> Result<Option<ProviderConnection>, ConnectionStoreError>;

    async fn acquire_active_runtime_reference(
        &self,
        _connection_id: &ProviderConnectionId,
        _expected_revision: u64,
        _reference_id: &str,
    ) -> Result<ProviderConnectionRuntimeLease, ConnectionStoreError> {
        Err(ConnectionStoreError::Unavailable(
            "runtime reference leases are unavailable for this store".to_owned(),
        ))
    }
}

#[async_trait]
impl ConnectionStore for codegg_core::provider_connections::ProviderConnectionStore {
    async fn get(
        &self,
        connection_id: &ProviderConnectionId,
    ) -> Result<Option<ProviderConnection>, ConnectionStoreError> {
        codegg_core::provider_connections::ProviderConnectionStore::get(self, connection_id)
            .await
            .map_err(|error| ConnectionStoreError::Unavailable(error.to_string()))
    }

    async fn acquire_active_runtime_reference(
        &self,
        connection_id: &ProviderConnectionId,
        expected_revision: u64,
        reference_id: &str,
    ) -> Result<ProviderConnectionRuntimeLease, ConnectionStoreError> {
        codegg_core::provider_connections::ProviderConnectionStore::acquire_active_runtime_reference(
            self,
            connection_id,
            expected_revision,
            reference_id,
        )
        .await
        .map_err(|error| ConnectionStoreError::Unavailable(error.to_string()))
    }
}

pub type ProviderInstance = Arc<dyn codegg_providers::Provider>;
type Resolution = Result<ProviderInstance, ConnectionError>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CacheKey {
    connection_id: ProviderConnectionId,
    revision: u64,
}

/// Daemon-owned lazy provider runtime manager.
///
/// Metadata is read on demand. A provider is constructed only on the first
/// resolution for a `(connection_id, revision)` pair. `OnceCell` coalesces
/// concurrent construction for that pair, while invalidation removes the
/// pair from the cache so a later caller observes fresh metadata/credentials.
/// Existing callers retain the old instance if invalidation races with an
/// in-flight request.
pub struct ConnectionManager {
    store: Arc<dyn ConnectionStore>,
    factory: Arc<dyn ProviderFactory>,
    cache: Mutex<HashMap<CacheKey, Arc<OnceCell<Resolution>>>>,
}

impl ConnectionManager {
    /// Construct a manager without reading metadata, resolving credentials,
    /// constructing providers, or probing any endpoint.
    pub fn new<S, F>(store: Arc<S>, factory: Arc<F>) -> Self
    where
        S: ConnectionStore + 'static,
        F: ProviderFactory + 'static,
    {
        Self {
            store: store as Arc<dyn ConnectionStore>,
            factory: factory as Arc<dyn ProviderFactory>,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub async fn resolve(
        &self,
        connection_id: &ProviderConnectionId,
    ) -> Result<ProviderInstance, ConnectionError> {
        self.resolve_at(connection_id, None).await
    }

    /// Resolve a connection at an optional optimistic revision. A caller that
    /// supplies a revision pins the provider instance to that committed
    /// generation for the lifetime of the returned `Arc`.
    pub async fn resolve_at(
        &self,
        connection_id: &ProviderConnectionId,
        expected_revision: Option<u64>,
    ) -> Result<ProviderInstance, ConnectionError> {
        let connection = self
            .store
            .get(connection_id)
            .await
            .map_err(|error| ConnectionError::Unavailable {
                connection_id: connection_id.clone(),
                reason: error.to_string(),
            })?
            .ok_or_else(|| ConnectionError::Unavailable {
                connection_id: connection_id.clone(),
                reason: "metadata not found".to_string(),
            })?;

        if connection.id != *connection_id {
            return Err(ConnectionError::Unavailable {
                connection_id: connection_id.clone(),
                reason: "store returned metadata for a different connection".to_string(),
            });
        }
        if let Some(expected_revision) = expected_revision {
            if connection.revision != expected_revision {
                return Err(ConnectionError::Unavailable {
                    connection_id: connection_id.clone(),
                    reason: format!(
                        "revision conflict: expected {expected_revision}, current {}",
                        connection.revision
                    ),
                });
            }
        }
        match connection.state {
            ProviderConnectionState::Disabled => {
                return Err(ConnectionError::Disabled(connection_id.clone()))
            }
            ProviderConnectionState::CredentialMissing => {
                return Err(ConnectionError::CredentialMissing(connection_id.clone()))
            }
            ProviderConnectionState::Active => {}
            state => {
                return Err(ConnectionError::Unavailable {
                    connection_id: connection_id.clone(),
                    reason: format!("connection lifecycle state is {}", state.storage_key()),
                })
            }
        }
        if connection.secret_binding.is_none() {
            return Err(ConnectionError::CredentialMissing(connection_id.clone()));
        }

        let key = CacheKey {
            connection_id: connection_id.clone(),
            revision: connection.revision,
        };
        let cell = {
            let mut cache = self.cache.lock().expect("connection cache poisoned");
            cache
                .entry(key)
                .or_insert_with(|| Arc::new(OnceCell::new()))
                .clone()
        };

        let result = cell
            .get_or_init(|| async { self.construct(&connection) })
            .await;
        result.clone()
    }

    /// Resolve and acquire the purge-blocking runtime lease as one daemon
    /// operation. The returned lease is released asynchronously on `Drop`,
    /// while callers may release it explicitly at a known request boundary.
    pub async fn resolve_with_runtime_reference(
        &self,
        connection_id: &ProviderConnectionId,
        expected_revision: Option<u64>,
    ) -> Result<(ProviderInstance, ProviderConnectionRuntimeLease), ConnectionError> {
        let provider = self.resolve_at(connection_id, expected_revision).await?;
        let connection = self
            .store
            .get(connection_id)
            .await
            .map_err(|error| ConnectionError::Unavailable {
                connection_id: connection_id.clone(),
                reason: error.to_string(),
            })?
            .ok_or_else(|| ConnectionError::Unavailable {
                connection_id: connection_id.clone(),
                reason: "metadata not found after provider resolution".to_owned(),
            })?;
        let lease = self
            .store
            .acquire_active_runtime_reference(
                connection_id,
                connection.revision,
                &format!("runtime-{}", uuid::Uuid::new_v4()),
            )
            .await
            .map_err(|error| ConnectionError::Unavailable {
                connection_id: connection_id.clone(),
                reason: error.to_string(),
            })?;
        Ok((provider, lease))
    }

    fn construct(&self, connection: &ProviderConnection) -> Resolution {
        let descriptor =
            descriptor_for(connection).map_err(|error| ConnectionError::Construction {
                connection_id: connection.id.clone(),
                reason: error,
            })?;

        self.factory
            .build(&descriptor)
            .map(Arc::from)
            .map_err(|error| map_provider_error(&connection.id, error))
    }

    /// Drop all cached revisions for a connection.
    pub fn invalidate(&self, connection_id: &ProviderConnectionId) {
        let mut cache = self.cache.lock().expect("connection cache poisoned");
        cache.retain(|key, _| &key.connection_id != connection_id);
    }

    /// Drop one exact cached revision, leaving other revisions untouched.
    pub fn invalidate_revision(&self, connection_id: &ProviderConnectionId, revision: u64) {
        let mut cache = self.cache.lock().expect("connection cache poisoned");
        cache.remove(&CacheKey {
            connection_id: connection_id.clone(),
            revision,
        });
    }

    pub fn clear(&self) {
        self.cache
            .lock()
            .expect("connection cache poisoned")
            .clear();
    }

    /// Lifecycle seams used by the daemon transaction coordinator. Storage
    /// commits happen in the owning provider service; these methods make the
    /// cache invalidation boundary explicit and keep old `Arc` instances
    /// valid for in-flight requests.
    pub fn rotate(&self, connection_id: &ProviderConnectionId, old_revision: u64) {
        self.invalidate_revision(connection_id, old_revision);
    }

    pub fn refresh(&self, connection_id: &ProviderConnectionId) {
        self.invalidate(connection_id);
    }

    pub fn disable(&self, connection_id: &ProviderConnectionId) {
        self.invalidate(connection_id);
    }

    pub fn enable(&self, connection_id: &ProviderConnectionId) {
        self.invalidate(connection_id);
    }

    pub fn delete(&self, connection_id: &ProviderConnectionId) {
        self.invalidate(connection_id);
    }

    pub fn restore(&self, connection_id: &ProviderConnectionId) {
        self.invalidate(connection_id);
    }

    #[cfg(test)]
    fn cache_len(&self) -> usize {
        self.cache.lock().expect("connection cache poisoned").len()
    }
}

fn descriptor_for(connection: &ProviderConnection) -> Result<ProviderConnectionDescriptor, String> {
    let provider = match &connection.provider_kind {
        CoreProviderKind::Eggpool => ProviderKind::OpenAiCompatible {
            provider_id: "eggpool".to_string(),
        },
        CoreProviderKind::OpenAiCompatible => ProviderKind::OpenAiCompatible {
            provider_id: "openai_compatible".to_string(),
        },
        CoreProviderKind::OpenAi => ProviderKind::OpenAi,
        CoreProviderKind::Anthropic => ProviderKind::Anthropic,
        CoreProviderKind::Google => ProviderKind::Google,
        CoreProviderKind::AzureOpenAi => ProviderKind::AzureOpenAi,
        unsupported => {
            return Err(format!(
                "provider kind '{}' has no compatibility factory",
                unsupported.as_str()
            ))
        }
    };

    let binding = connection
        .secret_binding
        .as_ref()
        .ok_or_else(|| "connection has no secret binding".to_string())?;
    let secret_ref = ProviderSecretRef::new(
        binding.provider_ref.clone(),
        Some(binding.account_ref.clone()),
    );
    Ok(
        ProviderConnectionDescriptor::new(connection.id.as_str(), provider, secret_ref)
            .with_base_url(connection.endpoint.as_str())
            .with_display_name(connection.display_name.clone()),
    )
}

fn map_provider_error(
    connection_id: &ProviderConnectionId,
    error: ProviderConnectionError,
) -> ConnectionError {
    match error {
        ProviderConnectionError::Credential(SecretResolutionError::Missing { .. }) => {
            ConnectionError::CredentialMissing(connection_id.clone())
        }
        ProviderConnectionError::Credential(error) => ConnectionError::CredentialResolution {
            connection_id: connection_id.clone(),
            reason: error.to_string(),
        },
        other => ConnectionError::Construction {
            connection_id: connection_id.clone(),
            reason: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::provider_connections::{
        Endpoint, ProviderScope, SecretBindingLocator, SecretRef as CoreSecretRef, TlsPolicy,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TestStore {
        connection: Mutex<Option<ProviderConnection>>,
    }

    #[async_trait]
    impl ConnectionStore for TestStore {
        async fn get(
            &self,
            _connection_id: &ProviderConnectionId,
        ) -> Result<Option<ProviderConnection>, ConnectionStoreError> {
            Ok(self.connection.lock().unwrap().clone())
        }
    }

    struct CountingFactory {
        calls: AtomicUsize,
    }

    impl ProviderFactory for CountingFactory {
        fn build(
            &self,
            descriptor: &ProviderConnectionDescriptor,
        ) -> Result<Box<dyn codegg_providers::Provider>, ProviderConnectionError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let base_url = descriptor.base_url.as_deref().unwrap();
            Ok(Box::new(
                codegg_providers::openai_compatible::OpenAiCompatibleProvider::simple(
                    "test-provider",
                    "Test provider",
                    "test-secret",
                    base_url,
                ),
            ))
        }
    }

    fn connection(id: &ProviderConnectionId, revision: u64) -> ProviderConnection {
        ProviderConnection {
            id: id.clone(),
            provider_kind: CoreProviderKind::Eggpool,
            display_name: "Eggpool".to_string(),
            endpoint: Endpoint::new("https://eggpool.example/v1", TlsPolicy::Required).unwrap(),
            tls_policy: TlsPolicy::Required,
            scope: ProviderScope::deployment("deployment").unwrap(),
            secret_binding: Some(
                SecretBindingLocator::new(CoreSecretRef::new(), "eggpool", "account").unwrap(),
            ),
            state: ProviderConnectionState::Active,
            revision,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_resolution_coalesces_by_connection_and_revision() {
        let id = ProviderConnectionId::new();
        let store = Arc::new(TestStore {
            connection: Mutex::new(Some(connection(&id, 1))),
        });
        let factory = Arc::new(CountingFactory {
            calls: AtomicUsize::new(0),
        });
        let manager = Arc::new(ConnectionManager::new(store, factory.clone()));

        let futures = (0..32).map(|_| {
            let manager = Arc::clone(&manager);
            let id = id.clone();
            async move { manager.resolve(&id).await.unwrap() }
        });
        let providers = futures::future::join_all(futures).await;

        assert_eq!(providers.len(), 32);
        assert_eq!(factory.calls.load(Ordering::SeqCst), 1);
        assert_eq!(manager.cache_len(), 1);
    }

    #[tokio::test]
    async fn disabled_and_missing_credentials_are_typed_and_invalidation_rebuilds() {
        let id = ProviderConnectionId::new();
        let store = Arc::new(TestStore {
            connection: Mutex::new(Some(connection(&id, 1))),
        });
        let factory = Arc::new(CountingFactory {
            calls: AtomicUsize::new(0),
        });
        let manager = ConnectionManager::new(store.clone(), factory.clone());

        store.connection.lock().unwrap().as_mut().unwrap().state =
            ProviderConnectionState::Disabled;
        assert!(matches!(
            manager.resolve(&id).await,
            Err(ConnectionError::Disabled(_))
        ));

        let mut active_without_secret = connection(&id, 1);
        active_without_secret.secret_binding = None;
        *store.connection.lock().unwrap() = Some(active_without_secret);
        assert!(matches!(
            manager.resolve(&id).await,
            Err(ConnectionError::CredentialMissing(_))
        ));

        let mut active = connection(&id, 1);
        active.state = ProviderConnectionState::CredentialMissing;
        *store.connection.lock().unwrap() = Some(active);
        assert!(matches!(
            manager.resolve(&id).await,
            Err(ConnectionError::CredentialMissing(_))
        ));

        *store.connection.lock().unwrap() = Some(connection(&id, 2));
        manager.invalidate(&id);
        manager.resolve(&id).await.unwrap();
        assert_eq!(factory.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn construction_and_invalidation_do_not_probe_on_manager_creation() {
        let id = ProviderConnectionId::new();
        let store = Arc::new(TestStore {
            connection: Mutex::new(Some(connection(&id, 1))),
        });
        let factory = Arc::new(CountingFactory {
            calls: AtomicUsize::new(0),
        });
        let manager = ConnectionManager::new(store, factory.clone());

        assert_eq!(factory.calls.load(Ordering::SeqCst), 0);
        manager.resolve(&id).await.unwrap();
        assert_eq!(factory.calls.load(Ordering::SeqCst), 1);
        manager.invalidate_revision(&id, 1);
        assert_eq!(factory.calls.load(Ordering::SeqCst), 1);
    }
}
