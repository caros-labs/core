use super::support::TestCore;
use caros_sdk::{AppId, AppState, Error, ErrorCode, EventKind, ManagementClient};
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn sdk_controls_real_app_and_observes_ordered_events() {
    let mut core = TestCore::start();
    let client = ManagementClient::connect(&core.path).await.unwrap();
    let apps = client.apps().list().await.unwrap();
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].id, core.id);
    assert_eq!(apps[0].name, "Test application");
    assert_eq!(apps[0].state, AppState::Stopped);
    let mut subscription = client.subscribe().await.unwrap();
    assert_eq!(subscription.snapshot(), apps);
    client.apps().start(core.id).await.unwrap();
    let started = timeout(Duration::from_secs(2), subscription.next())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(started.event.app_id, core.id);
    assert!(matches!(started.event.kind, EventKind::Started { .. }));
    assert_eq!(
        client.apps().status(core.id).await.unwrap().state,
        AppState::Running
    );
    assert!(
        matches!(client.apps().start(core.id).await, Err(Error::Remote(error)) if error.code == ErrorCode::AlreadyActive)
    );
    client.apps().stop(core.id).await.unwrap();
    let mut kinds = Vec::new();
    timeout(Duration::from_secs(2), async {
        loop {
            let event = subscription.next().await.unwrap();
            let stopped = event.event.kind == EventKind::Stopped;
            kinds.push(event.event.kind);
            if stopped {
                break;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(kinds[0], EventKind::StopRequested);
    assert!(
        kinds
            .iter()
            .any(|kind| matches!(kind, EventKind::Exited(_)))
    );
    assert_eq!(
        client.apps().status(core.id).await.unwrap().state,
        AppState::Stopped
    );
    core.finish().await;
    assert!(client.apps().list().await.is_err());
}

#[tokio::test]
async fn unknown_id_does_not_poison_connection_and_snapshot_includes_existing_state() {
    let mut core = TestCore::start();
    let client = ManagementClient::connect(&core.path).await.unwrap();
    let unknown = AppId::new();
    assert!(
        matches!(client.apps().start(unknown).await, Err(Error::Remote(e)) if e.code == ErrorCode::NotFound)
    );
    client.apps().start(core.id).await.unwrap();
    let subscription = client.subscribe().await.unwrap();
    assert_eq!(subscription.snapshot()[0].state, AppState::Running);
    assert!(subscription.sequence() > 0);
    // Server shutdown terminates the real child and closes management sessions.
    let pid = client.apps().status(core.id).await.unwrap().pid.unwrap();
    core.finish().await;
    assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}

#[tokio::test]
async fn simultaneous_clients_receive_independent_subscriptions() {
    let mut core = TestCore::start();
    let one = ManagementClient::connect(&core.path).await.unwrap();
    let two = ManagementClient::connect(&core.path).await.unwrap();
    let mut a = one.subscribe().await.unwrap();
    let mut b = two.subscribe().await.unwrap();
    let (left, right) = tokio::join!(one.apps().list(), two.apps().list());
    assert_eq!(left.unwrap(), right.unwrap());
    one.apps().start(core.id).await.unwrap();
    let left = timeout(Duration::from_secs(2), a.next())
        .await
        .unwrap()
        .unwrap();
    let right = timeout(Duration::from_secs(2), b.next())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(left, right);
    core.finish().await;
}
