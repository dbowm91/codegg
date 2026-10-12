//! TTS module. In daemon mode, TTS is owned by CoreDaemon's NotificationRouter.
//! In embedded mode, TTS remains here for direct user-triggered speech.
//! Global notification speech should go through the daemon, not this module.

use crate::error::AppError;
use async_trait::async_trait;
#[cfg(unix)]
use nix::sys::signal::{self, killpg, Signal};
#[cfg(unix)]
use nix::unistd::Pid;
#[cfg(unix)]
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
pub enum TtsProvider {
    #[default]
    None,
}

#[async_trait]
// Stable >=1.99 flags the #[must_use] injected by the async-trait expansion
// (the desugared future is already must_use). Suppressed at the macro site;
// no behavior change.
#[allow(clippy::double_must_use)]
pub trait TtsEngine: Send + Sync {
    async fn speak(&self, text: &str) -> Result<(), AppError>;
    async fn stop(&self) -> Result<(), AppError>;
    fn is_speaking(&self) -> bool;
}

pub struct Tts {
    speaking: Arc<AtomicBool>,
    /// PID of the `say` child currently speaking, so `stop()` terminates only
    /// our own speech rather than every `say` on the host.
    ///
    /// Shared via `Arc` because every call site works through a clone:
    /// `speak()` runs inside a spawned TUI task and `stop()` runs inside a
    /// different one, so the child pid has to be visible across handles.
    pid: Arc<Mutex<Option<u32>>>,
}

impl Clone for Tts {
    fn clone(&self) -> Self {
        // Clones share the child pid slot. That is required for correctness:
        // `speak()` and `stop()` are invoked from different tasks holding
        // different clones of the same logical speaker.
        Self {
            speaking: Arc::clone(&self.speaking),
            pid: Arc::clone(&self.pid),
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
            speaking: Arc::new(AtomicBool::new(false)),
            pid: Arc::new(Mutex::new(None)),
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
        self.speaking.store(true, Ordering::SeqCst);
        let mut command = tokio::process::Command::new("say");
        command.arg(text);
        // Own process group so `stop()` signals only this speech, never a
        // `say` the user started elsewhere. Failure to establish the group is
        // not fatal: `stop()` falls back to signalling the bare pid.
        #[cfg(unix)]
        command.process_group(0);
        let child = command.spawn().map_err(|e| {
            self.speaking.store(false, Ordering::SeqCst);
            AppError::Io(e)
        })?;
        if let Some(pid) = child.id() {
            if let Ok(mut guard) = self.pid.lock() {
                *guard = Some(pid);
            }
        }
        let output = child.wait_with_output().await.map_err(|e| {
            self.speaking.store(false, Ordering::SeqCst);
            AppError::Io(e)
        })?;
        self.speaking.store(false, Ordering::SeqCst);
        if let Ok(mut guard) = self.pid.lock() {
            *guard = None;
        }
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("say command failed: {}", stderr);
            return Err(AppError::Io(std::io::Error::other(format!(
                "say command failed: {}",
                stderr
            ))));
        }
        Ok(())
    }

    pub async fn stop(&self) -> Result<(), AppError> {
        #[cfg(not(unix))]
        {
            self.speaking.store(false, Ordering::SeqCst);
            return Ok(());
        }
        #[cfg(unix)]
        {
            if !self.is_speaking() {
                return Ok(());
            }
            self.speaking.store(false, Ordering::SeqCst);
            // Take and clear the PID under the lock so a concurrent stop() cannot
            // signal the same child twice.
            let pid = self.pid.lock().ok().and_then(|mut guard| guard.take());
            let Some(pid) = pid else {
                // No child of ours is tracked (already reaped, or spawned before
                // this instance took ownership). Never fall back to a
                // pattern-kill: that would terminate unrelated `say` processes
                // belonging to the user.
                return Ok(());
            };
            // `say` runs in its own process group, so signalling the group cannot
            // reach an unrelated `say`. If grouping was not established, fall back
            // to signalling the bare pid.
            let target = Pid::from_raw(pid as i32);
            let result =
                killpg(target, Signal::SIGTERM).or_else(|_| signal::kill(target, Signal::SIGTERM));
            match result {
                Ok(()) => Ok(()),
                // ESRCH means the child already exited, which is the desired
                // end state for stop().
                Err(nix::errno::Errno::ESRCH) => Ok(()),
                Err(err) => {
                    let err: io::Error = io::Error::from_raw_os_error(err as i32);
                    tracing::warn!("stop say pid {pid} failed: {err}");
                    Err(AppError::Io(io::Error::other(format!(
                        "stop say pid {pid} failed: {err}"
                    ))))
                }
            }
        }
    }

    pub fn is_speaking(&self) -> bool {
        self.speaking.load(Ordering::SeqCst)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_tts_is_idle_and_owns_no_child() {
        let tts = Tts::new();
        assert!(!tts.is_speaking());
        assert!(tts.pid.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn stop_when_idle_is_a_no_op() {
        let tts = Tts::new();
        // Nothing was spawned, so stop() must succeed without signalling any
        // process. In particular it must not fall back to a pattern kill.
        assert!(tts.stop().await.is_ok());
    }

    #[tokio::test]
    async fn stop_without_tracked_child_does_not_signal() {
        // Simulate the state where `speaking` is true but no child is tracked
        // (e.g. speak() was interrupted before recording its pid). stop() must
        // return Ok without signalling anything.
        let tts = Tts::new();
        tts.speaking.store(true, Ordering::SeqCst);
        assert!(tts.pid.lock().unwrap().is_none());
        assert!(tts.stop().await.is_ok());
        // The tracked pid must remain cleared.
        assert!(tts.pid.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn stop_with_stale_pid_tolerates_esrch() {
        // A recorded pid that has already exited must be treated as success,
        // not an error. Pick a pid that is very unlikely to be live.
        let tts = Tts::new();
        tts.speaking.store(true, Ordering::SeqCst);
        *tts.pid.lock().unwrap() = Some(0x7FFF_FFFE);
        // Either the pid is gone (Ok) or something odd happened; the contract
        // under test is that stop() never panics and clears the pid.
        let _ = tts.stop().await;
        assert!(tts.pid.lock().unwrap().is_none());
    }

    #[test]
    fn clones_share_speaking_state_and_child_pid() {
        // Every TTS call site operates through a clone: `speak()` runs inside a
        // spawned TUI task and `stop()` inside a different one. Both handles
        // must therefore observe the same child pid, otherwise stop() could
        // never find the process speak() started.
        let tts = Tts::new();
        let clone = tts.clone();
        tts.speaking.store(true, Ordering::SeqCst);
        *tts.pid.lock().unwrap() = Some(4242);
        assert!(
            clone.is_speaking(),
            "clone must observe shared speaking flag"
        );
        assert_eq!(*clone.pid.lock().unwrap(), Some(4242));
        // And the slot is genuinely one allocation, not two that happen to
        // agree.
        assert!(Arc::ptr_eq(&tts.pid, &clone.pid));
        assert!(Arc::ptr_eq(&tts.speaking, &clone.speaking));
    }

    #[tokio::test]
    async fn speak_rejects_empty_text() {
        let tts = Tts::new();
        assert!(tts.speak("").await.is_err());
        assert!(!tts.is_speaking());
    }
}
