//! PCM mixing, volume policy and output ownership. Supervisor only supplies identity/liveness.
mod backend;
mod mixer;
mod stream;
#[cfg(test)]
mod tests;
use crate::supervisor::{AppState, Supervisor};
pub use backend::OutputBackend;
use caros_protocol::{
    ApiError, AppId, ErrorCode,
    playback::{PlaybackStatus, Volume},
};
use mixer::{Mixer, Track};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};
pub(crate) use stream::PlaybackLease;
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};

#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaybackConfig {
    #[serde(default)]
    pub allowed_apps: BTreeSet<AppId>,
    #[serde(default)]
    pub control_apps: BTreeSet<AppId>,
    #[serde(default)]
    pub backend: OutputBackend,
}
pub struct PlaybackService {
    config: PlaybackConfig,
    state: Arc<Mutex<Mixer>>,
    worker: Option<(watch::Sender<bool>, JoinHandle<()>)>,
}
impl Default for PlaybackService {
    fn default() -> Self {
        Self::new(PlaybackConfig::default())
    }
}
impl PlaybackService {
    pub fn new(config: PlaybackConfig) -> Self {
        let mut mixer = Mixer::default();
        for app in &config.allowed_apps {
            mixer.volumes.insert(*app, Volume::default());
        }
        Self {
            config,
            state: Arc::new(Mutex::new(mixer)),
            worker: None,
        }
    }
    pub fn status(&self) -> PlaybackStatus {
        self.state.lock().unwrap().status()
    }
    pub(crate) fn open(&mut self, (app, pid): (AppId, u32)) -> Result<PlaybackLease, ApiError> {
        if !self.config.allowed_apps.contains(&app) {
            return Err(denied());
        }
        let (frames, receiver) = mpsc::channel(4);
        let (revoked, revoked_rx) = watch::channel(false);
        {
            let mut mixer = self.state.lock().unwrap();
            if mixer.tracks.len() >= 16 || mixer.tracks.iter().filter(|t| t.app == app).count() >= 2
            {
                return Err(ApiError::new(
                    ErrorCode::Unavailable,
                    "playback stream limit reached",
                ));
            }
            mixer.tracks.push(Track {
                app,
                pid,
                frames: receiver,
                revoked,
                gain: 0.0,
            });
        }
        self.start_worker();
        Ok(PlaybackLease {
            frames,
            revoked: revoked_rx,
        })
    }
    pub(crate) fn set_volume(
        &self,
        caller: AppId,
        target: Option<AppId>,
        level: Option<u8>,
        muted: Option<bool>,
    ) -> Result<(), ApiError> {
        let app = target.unwrap_or(caller);
        if app != caller && !self.config.control_apps.contains(&caller) {
            return Err(denied());
        }
        if level.is_some_and(|v| v > 100) {
            return Err(invalid());
        }
        let mut mixer = self.state.lock().unwrap();
        let volume = mixer.volumes.get_mut(&app).ok_or_else(denied)?;
        if let Some(level) = level {
            volume.level = level;
        }
        if let Some(muted) = muted {
            volume.muted = muted;
        }
        Ok(())
    }
    pub(crate) fn set_master(&self, caller: AppId, volume: Volume) -> Result<(), ApiError> {
        if !self.config.control_apps.contains(&caller) {
            return Err(denied());
        }
        if volume.level > 100 {
            return Err(invalid());
        }
        self.state.lock().unwrap().master = volume;
        Ok(())
    }
    pub(crate) fn poll(&mut self, supervisor: &Supervisor) {
        self.state.lock().unwrap().tracks.retain(|track| {
            let running = supervisor
                .status(&track.app)
                .is_some_and(|s| s.pid == Some(track.pid) && s.state == AppState::Running);
            if !running {
                track.revoked.send_replace(true);
            }
            running && track.revoked.receiver_count() > 0
        });
        if self
            .worker
            .as_ref()
            .is_some_and(|(_, task)| task.is_finished())
        {
            self.worker.take();
        }
        self.start_worker();
    }
    fn start_worker(&mut self) {
        if self.worker.is_none() && !self.state.lock().unwrap().tracks.is_empty() {
            let (stop, receiver) = watch::channel(false);
            self.worker = Some((
                stop,
                tokio::spawn(backend::run(
                    self.state.clone(),
                    self.config.backend,
                    receiver,
                )),
            ));
        }
    }
    pub(crate) async fn shutdown(&mut self) {
        for track in self.state.lock().unwrap().tracks.drain(..) {
            track.revoked.send_replace(true);
        }
        if let Some((stop, task)) = self.worker.take() {
            stop.send_replace(true);
            let _ = task.await;
        }
    }
}
impl Drop for PlaybackService {
    fn drop(&mut self) {
        if let Some((stop, _)) = &self.worker {
            stop.send_replace(true);
        }
    }
}
fn denied() -> ApiError {
    ApiError::new(
        ErrorCode::PermissionDenied,
        "playback operation is not granted",
    )
}
fn invalid() -> ApiError {
    ApiError::new(
        ErrorCode::InvalidRequest,
        "volume must be between 0 and 100",
    )
}
