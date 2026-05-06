use notify_rust::{Notification, Urgency};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::schema::NotificationConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationType {
    Info,
    Success,
    Warning,
    Error,
}

impl NotificationType {
    pub fn urgency(self) -> Urgency {
        match self {
            NotificationType::Info => Urgency::Normal,
            NotificationType::Success => Urgency::Normal,
            NotificationType::Warning => Urgency::Critical,
            NotificationType::Error => Urgency::Critical,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            NotificationType::Info => "Codegg",
            NotificationType::Success => "Codegg",
            NotificationType::Warning => "Codegg - Warning",
            NotificationType::Error => "Codegg - Error",
        }
    }
}

pub struct NotificationManager {
    config: Arc<RwLock<NotificationConfig>>,
}

impl NotificationManager {
    pub fn new(config: NotificationConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
        }
    }

    pub async fn update_config(&self, config: NotificationConfig) {
        let mut cfg = self.config.write().await;
        *cfg = config;
    }

    pub async fn is_enabled(&self) -> bool {
        let cfg = self.config.read().await;
        cfg.enabled.unwrap_or(true)
    }

    pub async fn send(
        &self,
        notification_type: NotificationType,
        body: &str,
    ) -> Result<(), notify_rust::error::Error> {
        if !self.is_enabled().await {
            return Ok(());
        }

        let cfg = self.config.read().await;
        match notification_type {
            NotificationType::Error => {
                if !cfg.on_error.unwrap_or(true) {
                    return Ok(());
                }
            }
            NotificationType::Info | NotificationType::Success => {
                if !cfg.on_task_complete.unwrap_or(true) {
                    return Ok(());
                }
            }
            NotificationType::Warning => {}
        }

        Notification::new()
            .urgency(notification_type.urgency())
            .summary(notification_type.title())
            .body(body)
            .show()?;

        Ok(())
    }

    pub fn blocking_send(
        notification_type: NotificationType,
        body: &str,
        enabled: bool,
    ) -> Result<(), notify_rust::error::Error> {
        if !enabled {
            return Ok(());
        }

        Notification::new()
            .urgency(notification_type.urgency())
            .summary(notification_type.title())
            .body(body)
            .show()?;

        Ok(())
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self::new(NotificationConfig::default())
    }
}
