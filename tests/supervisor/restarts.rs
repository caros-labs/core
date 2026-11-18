use super::support::*;
use caros_core::supervisor::{AppState, EventKind, RestartPolicy, Supervisor};
use std::{fs, time::Duration};

#[test]
fn failed_process_restarts_exactly_up_to_budget() {
    let mut supervisor = Supervisor::new();
    let mut spec = shell("crash", "exit 7");
    spec.restart = RestartPolicy::OnFailure {
        max_restarts: 2,
        delay: Duration::from_millis(15),
    };
    supervisor.register(spec).unwrap();
    supervisor.start(&app_id("crash")).unwrap();
    until(&mut supervisor, Supervisor::is_idle);
    let status = supervisor.status(&app_id("crash")).unwrap();
    assert_eq!(status.state, AppState::Failed);
    assert_eq!(status.restarts, 2);
    assert_eq!(status.last_exit.unwrap().code, Some(7));
    assert_eq!(
        supervisor
            .drain_events()
            .filter(|e| matches!(e.kind, EventKind::Started { .. }))
            .count(),
        3
    );
    // Only an explicit new start resets the budget.
    supervisor.start(&app_id("crash")).unwrap();
    assert_eq!(supervisor.status(&app_id("crash")).unwrap().restarts, 0);
}

#[test]
fn shutdown_cancels_pending_restart_and_rejects_new_starts() {
    let mut supervisor = Supervisor::new();
    let mut spec = shell("crash", "exit 1");
    spec.restart = RestartPolicy::OnFailure {
        max_restarts: 3,
        delay: Duration::from_secs(30),
    };
    supervisor.register(spec).unwrap();
    supervisor.start(&app_id("crash")).unwrap();
    until(&mut supervisor, |s| {
        s.status(&app_id("crash")).unwrap().state == AppState::RestartPending
    });
    supervisor.shutdown().unwrap();
    supervisor.poll().unwrap();
    assert!(supervisor.is_idle());
    assert!(supervisor.start(&app_id("crash")).is_err());
    assert!(supervisor.register(shell("late", "exit 0")).is_err());
}

#[test]
fn restart_spawn_failure_still_exhausts_the_budget() {
    let dir = TempDir::new();
    let mut spec = shell("vanishes", "exit 1");
    spec.working_directory = Some(dir.0.clone());
    spec.restart = RestartPolicy::OnFailure {
        max_restarts: 2,
        delay: Duration::from_millis(40),
    };
    let mut supervisor = Supervisor::new();
    supervisor.register(spec).unwrap();
    supervisor.start(&app_id("vanishes")).unwrap();
    until(&mut supervisor, |s| {
        s.status(&app_id("vanishes")).unwrap().state == AppState::RestartPending
    });
    fs::remove_dir(&dir.0).unwrap();
    until(&mut supervisor, Supervisor::is_idle);
    let status = supervisor.status(&app_id("vanishes")).unwrap();
    assert_eq!(status.state, AppState::Failed);
    assert_eq!(status.restarts, 2);
    assert_eq!(
        supervisor
            .drain_events()
            .filter(|e| matches!(e.kind, EventKind::Started { .. }))
            .count(),
        1
    );
}
