use super::ExitInfo;

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
    pub app_id: String,
    pub kind: EventKind,
}
