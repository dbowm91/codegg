use notify::{Config as NotifyConfig, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;

use crate::config::paths::resolve_config_paths;
use crate::config::schema::{Config, WatcherConfig};
use crate::error::{AppError, ConfigError};

pub struct ConfigWatcher {
    watcher: Option<RecommendedWatcher>,
    rx: mpsc::Receiver<()>,
    tx: mpsc::Sender<()>,
    watched_paths: Vec<PathBuf>,
    started: bool,
    debounce_duration: Duration,
    last_hash: Option<u64>,
    ignore_patterns: Vec<String>,
}

impl ConfigWatcher {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(32);
        Self {
            watcher: None,
            rx,
            tx,
            watched_paths: Vec::new(),
            started: false,
            debounce_duration: Duration::from_millis(500),
            last_hash: None,
            ignore_patterns: Vec::new(),
        }
    }

    pub fn with_config(mut self, config: &WatcherConfig) -> Self {
        if let Some(ms) = config.debounce_duration_ms {
            self.debounce_duration = Duration::from_millis(ms);
        }
        if let Some(ignore) = &config.ignore {
            self.ignore_patterns = ignore.clone();
        }
        self
    }

    pub async fn start(&mut self) -> Result<(), AppError> {
        if self.started {
            return Ok(());
        }
        let paths = Self::collect_config_paths();
        if paths.is_empty() {
            return Err(AppError::Config(ConfigError::Watch(
                "no config files found to watch".to_string(),
            )));
        }

        let tx = self.tx.clone();
        let ignore_patterns = self.ignore_patterns.clone();
        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    if event.kind.is_modify() || event.kind.is_create() {
                        let ignored = event.paths.iter().any(|path| {
                            let p = path.to_string_lossy();
                            ignore_patterns.iter().any(|pattern| p.contains(pattern))
                        });
                        if !ignored {
                            let _ = tx.blocking_send(());
                        }
                    }
                }
            },
            NotifyConfig::default(),
        )
        .map_err(|e| AppError::Config(ConfigError::Watch(e.to_string())))?;

        for path in &paths {
            let parent = path.parent().unwrap_or_else(|| Path::new("."));
            if parent.exists() {
                let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
            }
        }

        self.watched_paths = paths;
        self.watcher = Some(watcher);
        self.started = true;
        Ok(())
    }

    pub async fn recv(&mut self) -> Option<Result<Config, AppError>> {
        while let Some(()) = self.rx.recv().await {
            sleep(self.debounce_duration).await;

            while let Ok(()) = self.rx.try_recv() {
            }

            let first_hash = Self::compute_config_hash();
            sleep(Duration::from_millis(100)).await;
            let second_hash = Self::compute_config_hash();

            if first_hash != second_hash {
                continue;
            }

            if let Some(new_hash) = first_hash {
                if self.last_hash != Some(new_hash) {
                    self.last_hash = Some(new_hash);
                    return Some(Self::reload_config());
                }
            }
        }
        None
    }

    pub fn reload_now(&self) -> Result<Config, AppError> {
        Self::reload_config()
    }

    fn collect_config_paths() -> Vec<PathBuf> {
        resolve_config_paths()
    }

    fn compute_config_hash() -> Option<u64> {
        let paths = Self::collect_config_paths();
        if paths.is_empty() {
            return None;
        }

        let mut hasher = std::hash::DefaultHasher::new();
        for path in &paths {
            if let Ok(content) = std::fs::read(path) {
                content.hash(&mut hasher);
            }
        }
        Some(hasher.finish())
    }

    fn reload_config() -> Result<Config, AppError> {
        let paths = Self::collect_config_paths();
        if paths.is_empty() {
            return Ok(Config::default());
        }

        let configs: Result<Vec<_>, _> = paths
            .iter()
            .map(|p| {
                crate::config::paths::load_config(p)
                    .map_err(|e| AppError::Config(ConfigError::Watch(e.to_string())))
            })
            .collect();

        let configs = configs?;
        let mut config = crate::config::paths::merge_configs(&configs);

        config.migrate();

        if let Err(errors) = config.validate() {
            tracing::warn!("config validation errors: {:?}", errors);
        }

        crate::config::encryption::decrypt_provider_keys(&mut config)
            .map_err(|e| AppError::Config(ConfigError::Watch(e.to_string())))?;

        Ok(config)
    }
}

impl Default for ConfigWatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_config_watcher() {
        let watcher = ConfigWatcher::new();
        assert!(!watcher.started);
        assert_eq!(watcher.debounce_duration, Duration::from_millis(500));
        assert!(watcher.ignore_patterns.is_empty());
    }

    #[test]
    fn test_with_config_debounce() {
        let watcher_config = WatcherConfig {
            ignore: Some(vec!["node_modules".to_string()]),
            debounce_duration_ms: Some(1000),
        };
        let watcher = ConfigWatcher::new().with_config(&watcher_config);
        assert_eq!(watcher.debounce_duration, Duration::from_millis(1000));
        assert_eq!(watcher.ignore_patterns, vec!["node_modules".to_string()]);
    }

    #[test]
    fn test_with_config_ignore_patterns() {
        let watcher_config = WatcherConfig {
            ignore: Some(vec![".git".to_string(), "target".to_string()]),
            debounce_duration_ms: None,
        };
        let watcher = ConfigWatcher::new().with_config(&watcher_config);
        assert_eq!(watcher.ignore_patterns, vec![".git".to_string(), "target".to_string()]);
    }

    #[test]
    fn test_default_debounce_is_500ms() {
        let watcher = ConfigWatcher::new();
        assert_eq!(watcher.debounce_duration, Duration::from_millis(500));
    }

    #[test]
    fn test_started_initially_false() {
        let watcher = ConfigWatcher::new();
        assert!(!watcher.started);
    }

    #[test]
    fn test_watched_paths_initially_empty() {
        let watcher = ConfigWatcher::new();
        assert!(watcher.watched_paths.is_empty());
    }
}
