use caros_core::supervisor::{AppSpec, AppState, Supervisor};
use std::{io, process::ExitCode, time::Duration};
use tokio::signal::unix::{SignalKind, signal};

pub(crate) async fn run(spec: AppSpec) -> io::Result<ExitCode> {
    // Install handlers before starting children so Ctrl-C cannot orphan them.
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;
    let id = spec.id;
    let mut supervisor = Supervisor::new();
    supervisor.register(spec)?;
    supervisor.start(&id)?;
    let mut ticks = tokio::time::interval(Duration::from_millis(20));
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut stopping = false;
    loop {
        supervisor.poll()?;
        for event in supervisor.drain_events() {
            eprintln!("[{}] {:?}", event.app_id, event.kind);
        }
        if supervisor.is_idle() {
            return Ok(
                if supervisor.status(&id).unwrap().state == AppState::Failed {
                    ExitCode::FAILURE
                } else {
                    ExitCode::SUCCESS
                },
            );
        }
        tokio::select! {
            _ = ticks.tick() => {}
            _ = interrupt.recv(), if !stopping => {
                stopping = true;
                supervisor.shutdown()?;
            }
            _ = terminate.recv(), if !stopping => {
                stopping = true;
                supervisor.shutdown()?;
            }
        }
    }
}
