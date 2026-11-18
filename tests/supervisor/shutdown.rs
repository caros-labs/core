use super::support::*;
use caros_core::supervisor::{AppState, EventKind, RestartPolicy, Supervisor};
use std::{
    fs, io, thread,
    time::{Duration, Instant},
};

#[cfg(target_os = "linux")]
fn process_is_terminated(pid: u32) -> bool {
    match fs::read_to_string(format!("/proc/{pid}/stat")) {
        // Orphaned descendants may remain zombies until the system reaps them.
        Ok(stat) => stat
            .rsplit_once(") ")
            .is_some_and(|(_, tail)| tail.starts_with('Z')),
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(error) => panic!("cannot inspect child process: {error}"),
    }
}

#[test]
fn graceful_stop_waits_for_term_handler_and_never_restarts() {
    let dir = TempDir::new();
    let mut spec = shell(
        "graceful",
        "trap 'echo stopped > stopped; exit 0' TERM; echo ready > ready; while :; do sleep 0.02; done",
    );
    spec.working_directory = Some(dir.0.clone());
    spec.stop_timeout = Duration::from_secs(2);
    spec.restart = RestartPolicy::OnFailure {
        max_restarts: 3,
        delay: Duration::ZERO,
    };
    let mut supervisor = Supervisor::new();
    supervisor.register(spec).unwrap();
    supervisor.start(&app_id("graceful")).unwrap();
    until(&mut supervisor, |_| dir.0.join("ready").exists());
    supervisor.stop(&app_id("graceful")).unwrap();
    supervisor.stop(&app_id("graceful")).unwrap();
    assert_eq!(
        supervisor.status(&app_id("graceful")).unwrap().state,
        AppState::Stopping
    );
    until(&mut supervisor, Supervisor::is_idle);
    assert!(dir.0.join("stopped").exists());
    assert_eq!(supervisor.status(&app_id("graceful")).unwrap().restarts, 0);
    assert!(
        !supervisor
            .drain_events()
            .any(|e| e.kind == EventKind::ForceKilled)
    );
}

#[test]
fn unresponsive_app_is_killed_after_grace_period() {
    let dir = TempDir::new();
    let mut spec = shell(
        "stubborn",
        "trap '' TERM; echo ready > ready; exec sleep 60",
    );
    spec.working_directory = Some(dir.0.clone());
    let grace = spec.stop_timeout;
    let mut supervisor = Supervisor::new();
    supervisor.register(spec).unwrap();
    supervisor.start(&app_id("stubborn")).unwrap();
    until(&mut supervisor, |_| dir.0.join("ready").exists());
    let start = Instant::now();
    supervisor.stop(&app_id("stubborn")).unwrap();
    until(&mut supervisor, Supervisor::is_idle);
    assert!(start.elapsed() >= grace);
    assert_eq!(
        supervisor
            .status(&app_id("stubborn"))
            .unwrap()
            .last_exit
            .unwrap()
            .signal,
        Some(libc::SIGKILL)
    );
    assert!(
        supervisor
            .drain_events()
            .any(|e| e.kind == EventKind::ForceKilled)
    );
}

#[test]
#[cfg(target_os = "linux")]
fn parent_exit_cleans_up_background_workers() {
    let dir = TempDir::new();
    let mut spec = shell("workers", "sleep 60 & echo $! > worker; exit 0");
    spec.working_directory = Some(dir.0.clone());
    let mut supervisor = Supervisor::new();
    supervisor.register(spec).unwrap();
    supervisor.start(&app_id("workers")).unwrap();
    until(&mut supervisor, Supervisor::is_idle);
    let pid = fs::read_to_string(dir.0.join("worker"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    until(&mut supervisor, |_| process_is_terminated(pid));
}

#[test]
#[cfg(target_os = "linux")]
fn executable_handles_interrupt_and_termination_without_orphaning_app() {
    use std::process::{Command, Stdio};

    struct CoreProcess(std::process::Child);
    impl Drop for CoreProcess {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    for signal in [libc::SIGINT, libc::SIGTERM] {
        let dir = TempDir::new();
        let mut core = CoreProcess(
            Command::new(env!("CARGO_BIN_EXE_caros-core"))
                .args([
                    "--stop-timeout-ms",
                    "100",
                    "--",
                    "/bin/sh",
                    "-c",
                    "trap 'exit 0' TERM; echo $$ > app; while :; do sleep 0.02; done",
                ])
                .current_dir(&dir.0)
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        let app_pid = loop {
            if let Ok(text) = fs::read_to_string(dir.0.join("app"))
                && let Ok(pid) = text.trim().parse::<u32>()
            {
                break pid;
            }
            assert!(
                core.0.try_wait().unwrap().is_none(),
                "Core exited before starting app"
            );
            assert!(Instant::now() < deadline, "Core did not start app");
            thread::sleep(Duration::from_millis(5));
        };
        // Signal the actual binary, not the supervisor library under test.
        assert_eq!(unsafe { libc::kill(core.0.id() as i32, signal) }, 0);
        loop {
            if let Some(status) = core.0.try_wait().unwrap() {
                assert!(status.success(), "Core failed during shutdown: {status}");
                break;
            }
            if Instant::now() >= deadline {
                // Test failure cleanup, including the app's own process group.
                unsafe {
                    libc::kill(-(app_pid as i32), libc::SIGKILL);
                }
                panic!("Core did not shut down");
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert!(process_is_terminated(app_pid), "app outlived Core");
    }
}
