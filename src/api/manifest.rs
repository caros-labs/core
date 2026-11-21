//! Administrator-owned process and service configuration, never accepted over IPC.
use crate::services::{ServiceRegistry, ServiceSpec};
use crate::supervisor::{AppSpec, RestartPolicy, Supervisor};
use caros_protocol::AppId;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    apps: Vec<Entry>,
    #[serde(default)]
    services: Vec<ServiceEntry>,
    #[serde(default)]
    microphone: crate::microphone::MicrophoneConfig,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceEntry {
    name: String,
    allowed_apps: BTreeSet<AppId>,
    resources: BTreeSet<String>,
    process: Entry,
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

pub struct Configuration {
    pub apps: Supervisor,
    pub services: ServiceRegistry,
    pub microphone: crate::microphone::MicrophoneConfig,
}
pub fn load(path: impl AsRef<Path>) -> io::Result<Supervisor> {
    let config = load_configuration(path)?;
    Ok(config.apps)
}
pub fn load_configuration(path: impl AsRef<Path>) -> io::Result<Configuration> {
    let mut data = Vec::new();
    File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut data)?;
    if data.len() > 1024 * 1024 {
        return Err(invalid("manifest exceeds 1 MiB"));
    }
    let manifest: Manifest = serde_json::from_slice(&data).map_err(|e| invalid(&e.to_string()))?;
    if manifest.apps.len() > 128 || manifest.services.len() > 64 {
        return Err(invalid("at most 128 apps and 64 services are supported"));
    }
    let mut apps = Supervisor::new();
    for entry in manifest.apps {
        apps.register(entry.spec()?)?;
    }
    let mut services = ServiceRegistry::default();
    for entry in manifest.services {
        if entry
            .allowed_apps
            .iter()
            .any(|id| apps.status(id).is_none())
        {
            return Err(invalid("service grant refers to an unregistered app"));
        }
        services.register(ServiceSpec {
            name: entry.name,
            allowed_apps: entry.allowed_apps,
            resources: entry.resources,
            process: entry.process.spec()?,
        })?;
    }
    if manifest
        .microphone
        .allowed_apps
        .iter()
        .chain(&manifest.microphone.control_apps)
        .any(|id| apps.status(id).is_none())
    {
        return Err(invalid("microphone grant refers to an unregistered app"));
    }
    Ok(Configuration {
        apps,
        services,
        microphone: manifest.microphone,
    })
}
impl Entry {
    fn spec(self) -> io::Result<AppSpec> {
        if !self.executable.is_absolute() {
            return Err(invalid("manifest executable must be absolute"));
        }
        if self.name.len() > 256 {
            return Err(invalid("app name exceeds 256 bytes"));
        }
        let mut spec = AppSpec::new(self.id, self.name, self.executable);
        spec.args = self.args.into_iter().map(Into::into).collect();
        spec.environment = self
            .environment
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        spec.working_directory = self.working_directory;
        spec.stop_timeout = Duration::from_millis(self.stop_timeout_ms.into());
        if self.max_restarts > 0 {
            spec.restart = RestartPolicy::OnFailure {
                max_restarts: self.max_restarts,
                delay: Duration::from_millis(self.restart_delay_ms.into()),
            };
        }
        Ok(spec)
    }
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
