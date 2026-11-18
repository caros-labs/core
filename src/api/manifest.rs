//! Local administrator-owned configuration, not part of the socket protocol.
use crate::supervisor::{AppSpec, RestartPolicy, Supervisor};
use caros_protocol::AppId;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    apps: Vec<Entry>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    id: AppId,
    name: String,
    executable: PathBuf,
    #[serde(default)]
    args: Vec<String>,
    working_directory: Option<PathBuf>,
    #[serde(default)]
    environment: BTreeMap<String, String>,
    #[serde(default)]
    max_restarts: u32,
    #[serde(default = "restart_delay")]
    restart_delay_ms: u32,
    #[serde(default = "stop_timeout")]
    stop_timeout_ms: u32,
}
fn restart_delay() -> u32 {
    500
}
fn stop_timeout() -> u32 {
    3000
}

pub fn load(path: impl AsRef<Path>) -> io::Result<Supervisor> {
    let mut data = Vec::new();
    File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut data)?;
    if data.len() > 1024 * 1024 {
        return Err(invalid("manifest exceeds 1 MiB"));
    }
    let manifest: Manifest = serde_json::from_slice(&data).map_err(|e| invalid(&e.to_string()))?;
    if manifest.apps.len() > 128 {
        return Err(invalid("at most 128 apps are supported"));
    }
    let mut supervisor = Supervisor::new();
    for entry in manifest.apps {
        if !entry.executable.is_absolute() {
            return Err(invalid("manifest executable must be absolute"));
        }
        if entry.name.len() > 256 {
            return Err(invalid("app name exceeds 256 bytes"));
        }
        let mut spec = AppSpec::new(entry.id, entry.name, entry.executable);
        spec.args = entry.args.into_iter().map(Into::into).collect();
        spec.environment = entry
            .environment
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        spec.working_directory = entry.working_directory;
        spec.stop_timeout = Duration::from_millis(entry.stop_timeout_ms.into());
        if entry.max_restarts > 0 {
            spec.restart = RestartPolicy::OnFailure {
                max_restarts: entry.max_restarts,
                delay: Duration::from_millis(entry.restart_delay_ms.into()),
            };
        }
        supervisor.register(spec)?;
    }
    Ok(supervisor)
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
