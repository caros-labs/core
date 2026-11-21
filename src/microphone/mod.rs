//! Core-owned microphone capture and application leases, separate from supervision.
mod backend;
mod stream;
#[cfg(test)]
mod tests;
use crate::supervisor::{AppState, Supervisor};
pub use backend::CaptureSource;
use caros_protocol::{
    ApiError, AppId, ErrorCode,
    microphone::{MICROPHONE_RATE, MicrophonePacket, MicrophoneStatus},
};
use std::collections::BTreeSet;
pub(crate) use stream::MicrophoneLease;
use tokio::{
    sync::{broadcast, watch},
    task::JoinHandle,
};

#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MicrophoneConfig {
    #[serde(default)]
    pub allowed_apps: BTreeSet<AppId>,
    #[serde(default)]
    pub control_apps: BTreeSet<AppId>,
    #[serde(default)]
    pub source: CaptureSource,
}
struct Consumer {
    app: AppId,
    pid: u32,
    revoked: watch::Sender<bool>,
}
struct Worker {
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}
pub struct MicrophoneService {
    config: MicrophoneConfig,
    consumers: Vec<Consumer>,
    output: broadcast::Sender<MicrophonePacket>,
    muted: watch::Sender<bool>,
    worker: Option<Worker>,
}
impl Default for MicrophoneService {
    fn default() -> Self {
        Self::new(MicrophoneConfig::default())
    }
}
impl MicrophoneService {
    pub fn new(config: MicrophoneConfig) -> Self {
        Self {
            config,
            consumers: Vec::new(),
            output: broadcast::channel(8).0,
            muted: watch::channel(false).0,
            worker: None,
        }
    }
    pub fn status(&self) -> MicrophoneStatus {
        MicrophoneStatus {
            sample_rate: MICROPHONE_RATE,
            channels: 1,
            muted: *self.muted.borrow(),
            consumers: self
                .consumers
                .iter()
                .map(|c| c.app)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
        }
    }
    pub(crate) fn open(&mut self, owner: (AppId, u32)) -> Result<MicrophoneLease, ApiError> {
        if !self.config.allowed_apps.contains(&owner.0) {
            return Err(ApiError::new(
                ErrorCode::PermissionDenied,
                "microphone capture is not granted",
            ));
        }
        if self.consumers.len() >= 16
            || self.consumers.iter().filter(|c| c.app == owner.0).count() >= 2
        {
            return Err(ApiError::new(
                ErrorCode::Unavailable,
                "microphone stream limit reached",
            ));
        }
        let (revoked, revoke) = watch::channel(false);
        self.consumers.push(Consumer {
            app: owner.0,
            pid: owner.1,
            revoked,
        });
        let lease = MicrophoneLease {
            frames: self.output.subscribe(),
            revoke,
            muted: self.muted.subscribe(),
        };
        self.manage_worker();
        Ok(lease)
    }
    pub(crate) fn set_muted(&mut self, app: AppId, muted: bool) -> Result<(), ApiError> {
        if !self.config.control_apps.contains(&app) {
            return Err(ApiError::new(
                ErrorCode::PermissionDenied,
                "microphone control is not granted",
            ));
        }
        self.muted.send_replace(muted);
        Ok(())
    }
    pub(crate) fn poll(&mut self, supervisor: &Supervisor) {
        self.consumers.retain(|c| {
            let running = supervisor
                .status(&c.app)
                .is_some_and(|s| s.pid == Some(c.pid) && s.state == AppState::Running);
            if !running {
                c.revoked.send_replace(true);
            }
            running && c.revoked.receiver_count() > 0
        });
        self.manage_worker();
    }
    fn manage_worker(&mut self) {
        if self.worker.as_ref().is_some_and(|w| w.task.is_finished()) {
            self.worker.take();
        }
        if self.consumers.is_empty() {
            if let Some(worker) = &self.worker {
                worker.stop.send_replace(true);
            }
        } else if self.worker.is_none() {
            let (stop, stopped) = watch::channel(false);
            let task = tokio::spawn(backend::run(
                self.config.source.clone(),
                self.output.clone(),
                stopped,
                self.muted.subscribe(),
            ));
            self.worker = Some(Worker { stop, task });
        }
    }
    pub(crate) async fn shutdown(&mut self) {
        for consumer in self.consumers.drain(..) {
            consumer.revoked.send_replace(true);
        }
        if let Some(worker) = self.worker.take() {
            worker.stop.send_replace(true);
            let _ = worker.task.await;
        }
    }
}
impl Drop for MicrophoneService {
    fn drop(&mut self) {
        // Worker owns a kill-on-drop child; waking it permits explicit reaping.
        if let Some(worker) = &self.worker {
            worker.stop.send_replace(true);
        }
    }
}
