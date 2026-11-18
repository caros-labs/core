use super::ExitInfo;
use caros_protocol::AppId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventKind {
    Started { pid: u32 },
    StopRequested,
    ForceKilled,
    Exited(ExitInfo),
    RestartScheduled { attempt: u32 },
    Stopped,
    Failed { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppEvent {
    pub app_id: AppId,
    pub kind: EventKind,
}
