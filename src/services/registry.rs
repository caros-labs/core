use crate::supervisor::{AppSpec, AppState, Supervisor};
use caros_protocol::{ApiError, AppId, ErrorCode, ServiceStatus};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
};

pub struct ServiceSpec {
    pub name: String,
    pub process: AppSpec,
    pub allowed_apps: BTreeSet<AppId>,
    pub resources: BTreeSet<String>,
}
struct Slot {
    spec: ServiceSpec,
    owner: Option<(AppId, u32)>,
}
#[derive(Default)]
pub struct ServiceRegistry {
    processes: Supervisor,
    slots: BTreeMap<String, Slot>,
}
impl ServiceRegistry {
    pub fn register(&mut self, spec: ServiceSpec) -> io::Result<()> {
        if spec.name.is_empty()
            || spec.name.len() > 128
            || !spec
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || spec.allowed_apps.is_empty()
            || spec.resources.is_empty()
            || spec.resources.iter().any(|r| r.is_empty() || r.len() > 128)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid service name, grants or resources",
            ));
        }
        if self.slots.contains_key(&spec.name) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "duplicate service",
            ));
        }
        self.processes.register(spec.process.clone())?;
        self.slots
            .insert(spec.name.clone(), Slot { spec, owner: None });
        Ok(())
    }
    pub fn status(&self, name: &str) -> Result<ServiceStatus, ApiError> {
        let slot = self.slot(name)?;
        let state = self.processes.status(&slot.spec.process.id).unwrap();
        let state: caros_protocol::AppStatus = state.into();
        Ok(ServiceStatus {
            name: name.into(),
            state: state.state,
            pid: state.pid,
            owner: slot.owner.map(|(app, _)| app),
            resources: slot.spec.resources.iter().cloned().collect(),
        })
    }
    pub fn acquire(&mut self, name: &str, owner: (AppId, u32)) -> Result<ServiceStatus, ApiError> {
        let slot = self.slot(name)?;
        if !slot.spec.allowed_apps.contains(&owner.0) {
            return Err(error(
                ErrorCode::PermissionDenied,
                "app has no grant for this service",
            ));
        }
        if slot.owner == Some(owner) {
            return self.status(name);
        }
        if slot.owner.is_some() || !self.idle(slot) {
            return Err(error(
                ErrorCode::Unavailable,
                "service is already owned or stopping",
            ));
        }
        if self.slots.values().any(|other| {
            !self.idle(other) && !slot.spec.resources.is_disjoint(&other.spec.resources)
        }) {
            return Err(error(
                ErrorCode::Unavailable,
                "hardware resources are reserved by another service",
            ));
        }
        let id = slot.spec.process.id;
        self.processes.start(&id).map_err(internal)?;
        self.slots.get_mut(name).unwrap().owner = Some(owner);
        self.status(name)
    }
    pub fn release(&mut self, name: &str, owner: (AppId, u32)) -> Result<(), ApiError> {
        let slot = self.slot(name)?;
        if slot.owner != Some(owner) {
            return Err(error(
                ErrorCode::PermissionDenied,
                "service is not owned by this app instance",
            ));
        }
        let id = slot.spec.process.id;
        self.processes.stop(&id).map_err(internal)
        // Retain owner and resource reservation until the process actually exits.
    }
    pub fn poll(&mut self, apps: &Supervisor) -> io::Result<()> {
        for slot in self.slots.values() {
            if let Some((id, pid)) = slot.owner {
                let alive = apps
                    .status(&id)
                    .is_some_and(|a| a.state == AppState::Running && a.pid == Some(pid));
                if !alive {
                    self.processes.stop(&slot.spec.process.id)?;
                }
            }
        }
        self.processes.poll()?;
        for slot in self.slots.values_mut() {
            let state = self.processes.status(&slot.spec.process.id).unwrap().state;
            if matches!(state, AppState::Stopped | AppState::Failed) {
                slot.owner = None;
            }
        }
        // Services have their own status API, not launcher entries/events.
        self.processes.drain_events().for_each(drop);
        Ok(())
    }
    pub fn shutdown(&mut self) -> io::Result<()> {
        self.processes.shutdown()
    }
    pub fn is_idle(&self) -> bool {
        self.processes.is_idle()
    }
    fn idle(&self, slot: &Slot) -> bool {
        self.processes
            .status(&slot.spec.process.id)
            .is_some_and(|s| matches!(s.state, AppState::Stopped | AppState::Failed))
    }
    fn slot(&self, name: &str) -> Result<&Slot, ApiError> {
        self.slots
            .get(name)
            .ok_or_else(|| error(ErrorCode::NotFound, "service is not configured"))
    }
}
fn error(code: ErrorCode, message: &str) -> ApiError {
    ApiError::new(code, message)
}
fn internal(e: io::Error) -> ApiError {
    error(ErrorCode::Internal, &e.to_string())
}
