use caros_core::{
    AppId,
    services::{ServiceRegistry, ServiceSpec},
    supervisor::{AppSpec, Supervisor},
};
use caros_protocol::{AppState, ErrorCode};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

fn sleep_spec(name: &str) -> AppSpec {
    let mut spec = AppSpec::new(AppId::new(), name, "/bin/sleep");
    spec.args.push("60".into());
    spec.stop_timeout = Duration::from_millis(50);
    spec
}
fn service(name: &str, app: AppId, resource: &str) -> ServiceSpec {
    ServiceSpec {
        name: name.into(),
        process: sleep_spec(name),
        allowed_apps: BTreeSet::from([app]),
        resources: BTreeSet::from([resource.into()]),
    }
}
fn wait(registry: &mut ServiceRegistry, apps: &mut Supervisor, name: &str) {
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        apps.poll().unwrap();
        registry.poll(apps).unwrap();
        if registry.status(name).unwrap().state == AppState::Stopped {
            break;
        }
        assert!(Instant::now() < until, "service did not stop");
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn reservations_survive_release_until_exit_and_are_reclaimed_on_app_exit() {
    let mut apps = Supervisor::new();
    let app = sleep_spec("app");
    let id = app.id;
    apps.register(app).unwrap();
    apps.start(&id).unwrap();
    let owner = (id, apps.status(&id).unwrap().pid.unwrap());
    let mut registry = ServiceRegistry::default();
    registry
        .register(service("receiver", id, "audio.output"))
        .unwrap();
    registry
        .register(service("other", id, "audio.output"))
        .unwrap();
    let first = registry.acquire("receiver", owner).unwrap();
    assert_eq!(registry.acquire("receiver", owner).unwrap().pid, first.pid);
    assert_eq!(
        registry.acquire("other", owner).unwrap_err().code,
        ErrorCode::Unavailable
    );
    assert_eq!(
        registry
            .release("receiver", (id, owner.1 + 1))
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    registry.release("receiver", owner).unwrap();
    assert_eq!(
        registry.acquire("other", owner).unwrap_err().code,
        ErrorCode::Unavailable
    );
    wait(&mut registry, &mut apps, "receiver");
    registry.acquire("other", owner).unwrap();
    apps.stop(&id).unwrap();
    wait(&mut registry, &mut apps, "other");
    assert!(registry.status("other").unwrap().owner.is_none());
}
#[test]
fn failed_spawn_releases_resources_and_grants_are_checked() {
    let id = AppId::new();
    let mut registry = ServiceRegistry::default();
    let mut broken = service("broken", id, "usb.iphone");
    broken.process.executable = "/no/such/provider".into();
    registry.register(broken).unwrap();
    registry
        .register(service("good", id, "usb.iphone"))
        .unwrap();
    assert_eq!(
        registry
            .acquire("good", (AppId::new(), 1))
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    assert_eq!(
        registry.acquire("broken", (id, 1)).unwrap_err().code,
        ErrorCode::Internal
    );
    assert!(registry.acquire("good", (id, 1)).is_ok());
    registry.shutdown().unwrap();
    let mut apps = Supervisor::new();
    wait(&mut registry, &mut apps, "good");
}
