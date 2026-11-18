use std::{
    collections::{BTreeMap, VecDeque},
    io,
    time::Instant,
};

use super::{AppEvent, AppSpec, AppStatus, app::App};

/// Owns the app registry. Call `poll` regularly and drain events each iteration.
/// Process I/O is inherited; this is supervision, not an application sandbox.
#[derive(Default)]
pub struct Supervisor {
    apps: BTreeMap<String, App>,
    events: VecDeque<AppEvent>,
    shutting_down: bool,
}

impl Supervisor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, spec: AppSpec) -> io::Result<()> {
        self.ensure_active()?;
        if spec.id.trim().is_empty() || spec.executable.as_os_str().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "app ID and executable are required",
            ));
        }
        if self.apps.contains_key(&spec.id) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "app ID is already registered",
            ));
        }
        self.apps.insert(spec.id.clone(), App::new(spec));
        Ok(())
    }

    pub fn start(&mut self, id: &str) -> io::Result<()> {
        self.ensure_active()?;
        self.apps
            .get_mut(id)
            .ok_or_else(unknown_app)?
            .start(&mut self.events)
    }

    pub fn stop(&mut self, id: &str) -> io::Result<()> {
        self.apps
            .get_mut(id)
            .ok_or_else(unknown_app)?
            .stop(&mut self.events)
    }

    /// Prevent new starts and cancel pending restarts before requesting termination.
    pub fn shutdown(&mut self) -> io::Result<()> {
        self.shutting_down = true;
        let mut first_error = None;
        for app in self.apps.values_mut() {
            if let Err(error) = app.stop(&mut self.events) {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    pub fn status(&self, id: &str) -> Option<AppStatus> {
        self.apps.get(id).map(App::status)
    }
    pub fn is_idle(&self) -> bool {
        self.apps.values().all(App::is_idle)
    }
    pub fn drain_events(&mut self) -> impl Iterator<Item = AppEvent> + '_ {
        self.events.drain(..)
    }

    pub fn poll(&mut self) -> io::Result<()> {
        let now = Instant::now();
        for app in self.apps.values_mut() {
            app.poll(now, self.shutting_down, &mut self.events)?;
        }
        Ok(())
    }

    fn ensure_active(&self) -> io::Result<()> {
        if self.shutting_down {
            Err(io::Error::other("supervisor is shutting down"))
        } else {
            Ok(())
        }
    }
}

fn unknown_app() -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, "app is not registered")
}
