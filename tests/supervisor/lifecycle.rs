use super::support::*;
use caros_core::supervisor::{AppSpec, AppState, EventKind, RestartPolicy, Supervisor};
use std::{fs, io, time::Duration};

#[test]
fn successful_exit_is_reaped_and_does_not_restart() {
    let mut supervisor = Supervisor::new();
    let mut spec = shell("one", "exit 0");
    spec.restart = RestartPolicy::OnFailure {
        max_restarts: 2,
        delay: Duration::ZERO,
    };
    supervisor.register(spec).unwrap();
    supervisor.start(&app_id("one")).unwrap();
    until(&mut supervisor, Supervisor::is_idle);
    let status = supervisor.status(&app_id("one")).unwrap();
    assert_eq!(status.state, AppState::Stopped);
    assert_eq!(status.pid, None);
    assert_eq!(status.restarts, 0);
    assert_eq!(status.last_exit.unwrap().code, Some(0));
    let events: Vec<_> = supervisor.drain_events().map(|e| e.kind).collect();
    assert!(matches!(events[0], EventKind::Started { .. }));
    assert!(matches!(events[1], EventKind::Exited(_)));
    assert_eq!(events[2], EventKind::Stopped);
}

#[test]
fn registration_and_spawn_errors_are_reported() {
    let mut supervisor = Supervisor::new();
    assert!(
        supervisor
            .register(AppSpec::new(app_id("empty"), "", "/bin/true"))
            .is_err()
    );
    assert_eq!(
        supervisor.start(&app_id("unknown")).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
    assert_eq!(
        supervisor.stop(&app_id("unknown")).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
    supervisor
        .register(AppSpec::new(
            app_id("missing"),
            "missing",
            "/caros-test/nonexistent/executable",
        ))
        .unwrap();
    assert_eq!(
        supervisor
            .register(shell("missing", "exit 0"))
            .unwrap_err()
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    assert!(supervisor.start(&app_id("missing")).is_err());
    assert_eq!(
        supervisor.status(&app_id("missing")).unwrap().state,
        AppState::Failed
    );
    assert!(supervisor.status(&app_id("missing")).unwrap().pid.is_none());
}

#[test]
fn apps_are_independent_and_drop_reaps_live_children() {
    let mut supervisor = Supervisor::new();
    supervisor
        .register(shell("healthy", "exec sleep 60"))
        .unwrap();
    supervisor.register(shell("failed", "exit 2")).unwrap();
    supervisor.start(&app_id("healthy")).unwrap();
    let pid = supervisor.status(&app_id("healthy")).unwrap().pid.unwrap();
    assert!(supervisor.start(&app_id("healthy")).is_err());
    assert_eq!(
        supervisor.status(&app_id("healthy")).unwrap().pid,
        Some(pid)
    );
    supervisor.start(&app_id("failed")).unwrap();
    until(&mut supervisor, |s| {
        s.status(&app_id("failed")).unwrap().state == AppState::Failed
    });
    assert_eq!(
        supervisor.status(&app_id("healthy")).unwrap().state,
        AppState::Running
    );
    drop(supervisor);
    // waitpid must report ECHILD: Drop already reaped our direct child.
    let result = unsafe { libc::waitpid(pid as i32, std::ptr::null_mut(), libc::WNOHANG) };
    assert_eq!(result, -1);
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}

#[test]
fn launches_with_configured_arguments_environment_and_directory() {
    let dir = TempDir::new();
    let mut spec = shell(
        "configured",
        "printf '%s|%s' \"$1\" \"$CAROS_TEST_VALUE\" > result",
    );
    spec.args
        .extend(["fixture".into(), "literal ; $(false)".into()]);
    spec.environment
        .insert("CAROS_TEST_VALUE".into(), "configured".into());
    spec.working_directory = Some(dir.0.clone());
    let mut supervisor = Supervisor::new();
    supervisor.register(spec).unwrap();
    supervisor.start(&app_id("configured")).unwrap();
    until(&mut supervisor, Supervisor::is_idle);
    assert_eq!(
        fs::read_to_string(dir.0.join("result")).unwrap(),
        "literal ; $(false)|configured"
    );
}
