use crate::error::AppError;
use async_trait::async_trait;
use std::sync::Mutex;

#[derive(Debug, Default)]
pub enum TtsProvider {
    #[default]
    None,
}

#[async_trait]
pub trait TtsEngine: Send + Sync {
    async fn speak(&self, text: &str) -> Result<(), AppError>;
    async fn stop(&self) -> Result<(), AppError>;
    fn is_speaking(&self) -> bool;
}

pub struct Tts {
    speaking: Mutex<std::sync::atomic::AtomicBool>,
}

impl Clone for Tts {
    fn clone(&self) -> Self {
        Self {
            speaking: Mutex::new(std::sync::atomic::AtomicBool::new(
                self.speaking.lock().unwrap().load(std::sync::atomic::Ordering::SeqCst),
            )),
        }
    }
}

impl Default for Tts {
    fn default() -> Self {
        Self::new()
    }
}

impl Tts {
    pub fn new() -> Self {
        Self {
            speaking: Mutex::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub fn init(&mut self, provider: TtsProvider) -> Result<(), AppError> {
        match provider {
            TtsProvider::None => Ok(()),
        }
    }

    pub async fn speak(&self, text: &str) -> Result<(), AppError> {
        if text.is_empty() {
            return Err(AppError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cannot speak empty string",
            )));
        }
        self.speaking
            .lock()
            .unwrap()
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let output = tokio::process::Command::new("say")
            .arg(text)
            .output()
            .await
            .map_err(|e| {
                self.speaking.lock().unwrap().store(false, std::sync::atomic::Ordering::SeqCst);
                AppError::Io(e)
            })?;
        self.speaking
            .lock()
            .unwrap()
            .store(false, std::sync::atomic::Ordering::SeqCst);
self.speaking
            .lock()
            .unwrap()
            .store(false, std::sync::atomic::Ordering::SeqCst);
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("say command failed: {}", stderr);
            return Err(AppError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("say command failed: {}", stderr),
            )));
        }
        Ok(())
    }

    pub async fn stop(&self) -> Result<(), AppError> {
        self.speaking
            .lock()
            .unwrap()
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let output = tokio::process::Command::new("pkill")
            .arg("say")
            .output()
            .await
            .map_err(AppError::Io)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("pkill say failed: {}", stderr);
            return Err(AppError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("pkill say failed: {}", stderr),
            )));
        }
        Ok(())
    }

    pub fn is_speaking(&self) -> bool {
        self.speaking.lock().unwrap().load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait]
impl TtsEngine for Tts {
    async fn speak(&self, text: &str) -> Result<(), AppError> {
        self.speak(text).await
    }

    async fn stop(&self) -> Result<(), AppError> {
        self.stop().await
    }

    fn is_speaking(&self) -> bool {
        self.is_speaking()
    }
}
