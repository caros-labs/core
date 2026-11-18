use std::{collections::VecDeque, io, time::Instant};

use super::process::AppProcess;
use super::{AppEvent, AppSpec, AppState, AppStatus, EventKind, ExitInfo, RestartPolicy};

/// Lifecycle and retry state for one app. Fields stay private to this owner.
pub(super) struct App {
    spec: AppSpec,
    state: AppState,
    process: Option<AppProcess>,
    restarts: u32,
    restart_at: Option<Instant>,
    stop_at: Option<Instant>,
    last_exit: Option<ExitInfo>,
}

impl App {
    pub fn new(spec: AppSpec) -> Self {
        Self {
            spec,
            state: AppState::Stopped,
            process: None,
            restarts: 0,
            restart_at: None,
            stop_at: None,
            last_exit: None,
        }
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.state, AppState::Stopped | AppState::Failed)
    }

    pub fn status(&self) -> AppStatus {
        AppStatus {
            id: self.spec.id.clone(),
            state: self.state,
            pid: self.process.as_ref().map(AppProcess::id),
            restarts: self.restarts,
            last_exit: self.last_exit.clone(),
        }
    }

    pub fn start(&mut self, events: &mut VecDeque<AppEvent>) -> io::Result<()> {
        if !self.is_idle() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "app is already active",
            ));
        }
        self.restarts = 0;
        self.last_exit = None;
        self.restart_at = None;
        self.stop_at = None;
        // Initial launch failure is returned immediately, without retrying.
        self.spawn(events)
    }

    pub fn stop(&mut self, events: &mut VecDeque<AppEvent>) -> io::Result<()> {
        if self.state == AppState::Stopping {
            return Ok(());
        }
        if let Some(process) = &self.process {
            process.terminate()?;
            self.stop_at = Some(Instant::now() + self.spec.stop_timeout);
            self.state = AppState::Stopping;
            self.emit(events, EventKind::StopRequested);
        } else if self.state != AppState::Stopped {
            self.restart_at = None;
            self.state = AppState::Stopped;
            self.emit(events, EventKind::Stopped);
        }
        Ok(())
    }

    pub fn poll(
        &mut self,
        now: Instant,
        shutting_down: bool,
        events: &mut VecDeque<AppEvent>,
    ) -> io::Result<()> {
        if let Some(process) = &mut self.process {
            if let Some(status) = process.try_wait()? {
                // Also clean up any workers left behind by the main process.
                process.kill()?;
                self.process = None;
                let exit = ExitInfo::from(status);
                self.last_exit = Some(exit.clone());
                self.emit(events, EventKind::Exited(exit));
                if self.state == AppState::Stopping || shutting_down || status.success() {
                    self.state = AppState::Stopped;
                    self.stop_at = None;
                    self.emit(events, EventKind::Stopped);
                } else {
                    self.schedule_restart(events, now, "process exited unsuccessfully");
                }
            } else if self.stop_at.is_some_and(|deadline| now >= deadline) {
                process.kill()?;
                self.stop_at = None;
                self.emit(events, EventKind::ForceKilled);
            }
        }
        if self.restart_at.is_some_and(|deadline| now >= deadline) {
            self.restart_at = None;
            if let Err(error) = self.spawn(events) {
                self.schedule_restart(events, now, &format!("restart failed: {error}"));
            }
        }
        Ok(())
    }

    fn spawn(&mut self, events: &mut VecDeque<AppEvent>) -> io::Result<()> {
        match AppProcess::spawn(&self.spec) {
            Ok(process) => {
                let pid = process.id();
                self.process = Some(process);
                self.state = AppState::Running;
                self.emit(events, EventKind::Started { pid });
                Ok(())
            }
            Err(error) => {
                self.state = AppState::Failed;
                self.emit(
                    events,
                    EventKind::Failed {
                        reason: format!("could not spawn process: {error}"),
                    },
                );
                Err(error)
            }
        }
    }

    fn schedule_restart(&mut self, events: &mut VecDeque<AppEvent>, now: Instant, reason: &str) {
        if let RestartPolicy::OnFailure {
            max_restarts,
            delay,
        } = self.spec.restart
            && self.restarts < max_restarts
        {
            self.restarts += 1;
            self.restart_at = Some(now + delay);
            self.state = AppState::RestartPending;
            self.emit(
                events,
                EventKind::RestartScheduled {
                    attempt: self.restarts,
                },
            );
        } else {
            self.state = AppState::Failed;
            self.emit(
                events,
                EventKind::Failed {
                    reason: reason.into(),
                },
            );
        }
    }

    fn emit(&self, events: &mut VecDeque<AppEvent>, kind: EventKind) {
        events.push_back(AppEvent {
            app_id: self.spec.id.clone(),
            kind,
        });
    }
}
