use crate::supervisor;
use caros_protocol as protocol;

impl From<supervisor::AppState> for protocol::AppState {
    fn from(value: supervisor::AppState) -> Self {
        match value {
            supervisor::AppState::Stopped => Self::Stopped,
            supervisor::AppState::Running => Self::Running,
            supervisor::AppState::Stopping => Self::Stopping,
            supervisor::AppState::RestartPending => Self::RestartPending,
            supervisor::AppState::Failed => Self::Failed,
        }
    }
}
impl From<supervisor::ExitInfo> for protocol::ExitInfo {
    fn from(value: supervisor::ExitInfo) -> Self {
        Self {
            code: value.code,
            signal: value.signal,
        }
    }
}
impl From<supervisor::AppStatus> for protocol::AppStatus {
    fn from(value: supervisor::AppStatus) -> Self {
        Self {
            id: value.id,
            name: value.name,
            state: value.state.into(),
            pid: value.pid,
            restarts: value.restarts,
            last_exit: value.last_exit.map(Into::into),
        }
    }
}
impl From<supervisor::AppEvent> for protocol::AppEvent {
    fn from(value: supervisor::AppEvent) -> Self {
        use protocol::EventKind as P;
        use supervisor::EventKind as S;
        let kind = match value.kind {
            S::Started { pid } => P::Started { pid },
            S::StopRequested => P::StopRequested,
            S::ForceKilled => P::ForceKilled,
            S::Exited(exit) => P::Exited(exit.into()),
            S::RestartScheduled { attempt } => P::RestartScheduled { attempt },
            S::Stopped => P::Stopped,
            S::Failed { reason } => P::Failed { reason },
        };
        Self {
            app_id: value.app_id,
            kind,
        }
    }
}
