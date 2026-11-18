use caros_protocol::AppId;
use std::{collections::BTreeMap, ffi::OsString, path::PathBuf, time::Duration};

/// Restarts are bounded across one explicitly started run, not reset by uptime.
#[derive(Clone, Debug, Default)]
pub enum RestartPolicy {
    #[default]
    Never,
    OnFailure {
        max_restarts: u32,
        delay: Duration,
    },
}

#[derive(Clone, Debug)]
pub struct AppSpec {
    pub id: AppId,
    pub name: String,
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub working_directory: Option<PathBuf>,
    /// Overrides inherited environment entries; Core never logs these values.
    pub environment: BTreeMap<OsString, OsString>,
    pub restart: RestartPolicy,
    pub stop_timeout: Duration,
}

impl AppSpec {
    pub fn new(id: AppId, name: impl Into<String>, executable: impl Into<PathBuf>) -> Self {
        Self {
            id,
            name: name.into(),
            executable: executable.into(),
            args: Vec::new(),
            working_directory: None,
            environment: BTreeMap::new(),
            restart: RestartPolicy::Never,
            stop_timeout: Duration::from_secs(3),
        }
    }
}
