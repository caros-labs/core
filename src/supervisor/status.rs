use std::{os::unix::process::ExitStatusExt, process::ExitStatus};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppState {
    Stopped,
    /// The process was spawned. This does not mean the application is ready.
    Running,
    Stopping,
    RestartPending,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExitInfo {
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

impl From<ExitStatus> for ExitInfo {
    fn from(status: ExitStatus) -> Self {
        Self {
            code: status.code(),
            signal: status.signal(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppStatus {
    pub id: String,
    pub state: AppState,
    pub pid: Option<u32>,
    pub restarts: u32,
    pub last_exit: Option<ExitInfo>,
}
