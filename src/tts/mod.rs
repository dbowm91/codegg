use crate::error::AppError;
use async_trait::async_trait;

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
    speaking: std::sync::atomic::AtomicBool,
}

impl Clone for Tts {
    fn clone(&self) -> Self {
        Self {
            speaking: std::sync::atomic::AtomicBool::new(
                self.speaking.load(std::sync::atomic::Ordering::SeqCst),
            ),
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
            speaking: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn init(&mut self, provider: TtsProvider) -> Result<(), AppError> {
        match provider {
            TtsProvider::None => Ok(()),
        }
    }

    pub async fn speak(&self, text: &str) -> Result<(), AppError> {
        self.speaking
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let output = tokio::process::Command::new("say")
            .arg(text)
            .output()
            .await
            .map_err(AppError::Io)?;
        self.speaking
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
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let _ = tokio::process::Command::new("pkill")
            .arg("say")
            .output()
            .await;
        Ok(())
    }

    pub fn is_speaking(&self) -> bool {
        self.speaking.load(std::sync::atomic::Ordering::SeqCst)
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
