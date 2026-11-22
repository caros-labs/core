use crate::supervisor::Supervisor;
use caros_protocol::{ApiError, ErrorCode, LifecycleEvent, Reply, Request};
use std::io;
use tokio::sync::{broadcast, oneshot};

pub(super) struct Command {
    pub request: Request,
    pub peer_pid: Option<u32>,
    pub reply: oneshot::Sender<Answer>,
}
pub(super) struct Answer {
    pub result: Result<Reply, ApiError>,
    pub events: Option<broadcast::Receiver<LifecycleEvent>>,
    pub microphone: Option<crate::microphone::MicrophoneLease>,
    pub playback: Option<crate::playback::PlaybackLease>,
}

pub(super) struct Dispatcher {
    pub supervisor: Supervisor,
    pub services: crate::services::ServiceRegistry,
    pub microphone: crate::microphone::MicrophoneService,
    pub playback: crate::playback::PlaybackService,
    sequence: u64,
    events: broadcast::Sender<LifecycleEvent>,
}
impl Dispatcher {
    pub fn new(supervisor: Supervisor) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            supervisor,
            services: Default::default(),
            microphone: Default::default(),
            playback: Default::default(),
            sequence: 0,
            events,
        }
    }
    pub fn publish(&mut self) {
        for event in self.supervisor.drain_events() {
            self.sequence += 1;
            let _ = self.events.send(LifecycleEvent {
                sequence: self.sequence,
                event: event.into(),
            });
        }
    }
    pub fn handle(&mut self, request: Request, peer_pid: Option<u32>) -> Answer {
        let mut events = None;
        let mut microphone = None;
        let mut playback = None;
        let result = match request {
            Request::PlaybackStatus => Ok(Reply::PlaybackStatus(self.playback.status())),
            Request::OpenPlayback => self
                .caller(peer_pid, true)
                .and_then(|owner| self.playback.open(owner))
                .map(|lease| {
                    playback = Some(lease);
                    Reply::PlaybackOpened
                }),
            Request::SetPlaybackVolume { app, level, muted } => self
                .caller(peer_pid, true)
                .and_then(|(caller, _)| self.playback.set_volume(caller, app, level, muted))
                .map(|()| Reply::Accepted),
            Request::SetMasterVolume { volume } => self
                .caller(peer_pid, true)
                .and_then(|(caller, _)| self.playback.set_master(caller, volume))
                .map(|()| Reply::Accepted),
            Request::MicrophoneStatus => Ok(Reply::MicrophoneStatus(self.microphone.status())),
            Request::OpenMicrophone => self
                .caller(peer_pid, true)
                .and_then(|owner| self.microphone.open(owner))
                .map(|lease| {
                    microphone = Some(lease);
                    Reply::MicrophoneOpened
                }),
            Request::SetMicrophoneMuted { muted } => self
                .caller(peer_pid, true)
                .and_then(|(app, _)| self.microphone.set_muted(app, muted))
                .map(|()| Reply::Accepted),
            Request::AcquireService { name } => self
                .caller(peer_pid, true)
                .and_then(|owner| self.services.acquire(&name, owner))
                .map(Reply::Service),
            Request::ReleaseService { name } => self
                .caller(peer_pid, false)
                .and_then(|owner| self.services.release(&name, owner))
                .map(|()| Reply::Accepted),
            Request::ServiceStatus { name } => self.services.status(&name).map(Reply::Service),
            Request::ListApps => Ok(Reply::Apps(self.snapshot())),
            Request::AppStatus { id } => self
                .supervisor
                .status(&id)
                .map(|status| Reply::Status(status.into()))
                .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "app is not registered")),
            Request::StartApp { id } => self
                .supervisor
                .start(&id)
                .map(|()| Reply::Accepted)
                .map_err(map_error),
            Request::StopApp { id } => self
                .supervisor
                .stop(&id)
                .map(|()| Reply::Accepted)
                .map_err(map_error),
            Request::Subscribe => {
                // Subscribe and snapshot on the same owner, with no await in between.
                events = Some(self.events.subscribe());
                Ok(Reply::Subscribed {
                    apps: self.snapshot(),
                    sequence: self.sequence,
                })
            }
        };
        self.publish();
        Answer {
            result,
            events,
            microphone,
            playback,
        }
    }
    fn caller(
        &self,
        pid: Option<u32>,
        starting: bool,
    ) -> Result<(caros_protocol::AppId, u32), ApiError> {
        let pid =
            pid.ok_or_else(|| ApiError::new(ErrorCode::PermissionDenied, "peer PID unavailable"))?;
        self.supervisor
            .list()
            .into_iter()
            .find(|app| {
                app.pid == Some(pid)
                    && (!starting || app.state == crate::supervisor::AppState::Running)
            })
            .map(|app| (app.id, pid))
            .ok_or_else(|| {
                ApiError::new(
                    ErrorCode::PermissionDenied,
                    "caller is not a registered running app process",
                )
            })
    }
    fn snapshot(&self) -> Vec<caros_protocol::AppStatus> {
        self.supervisor.list().into_iter().map(Into::into).collect()
    }
}
fn map_error(error: io::Error) -> ApiError {
    let code = match error.kind() {
        io::ErrorKind::NotFound => ErrorCode::NotFound,
        io::ErrorKind::AlreadyExists => ErrorCode::AlreadyActive,
        io::ErrorKind::InvalidInput => ErrorCode::InvalidRequest,
        _ => ErrorCode::Internal,
    };
    ApiError::new(code, error.to_string())
}
