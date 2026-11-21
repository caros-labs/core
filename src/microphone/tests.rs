use super::*;
use crate::supervisor::AppSpec;
use std::time::Duration;

#[tokio::test]
async fn grants_stream_limits_drop_and_process_revocation() {
    let app = AppId::new();
    let mut spec = AppSpec::new(app, "Microphone consumer", "/bin/sleep");
    spec.args = vec!["30".into()];
    let mut supervisor = Supervisor::new();
    supervisor.register(spec).unwrap();
    supervisor.start(&app).unwrap();
    let pid = supervisor.status(&app).unwrap().pid.unwrap();
    let mut mic = MicrophoneService::new(MicrophoneConfig {
        allowed_apps: [app].into(),
        source: CaptureSource::TestTone,
        ..Default::default()
    });
    assert!(
        matches!(mic.open((AppId::new(), pid)), Err(e) if e.code == ErrorCode::PermissionDenied)
    );
    assert!(matches!(mic.set_muted(app, true), Err(e) if e.code == ErrorCode::PermissionDenied));
    let first = mic.open((app, pid)).unwrap();
    let second = mic.open((app, pid)).unwrap();
    assert!(matches!(mic.open((app, pid)), Err(e) if e.code == ErrorCode::Unavailable));
    drop(first);
    mic.poll(&supervisor);
    let mut third = mic.open((app, pid)).unwrap();
    supervisor.stop(&app).unwrap();
    mic.poll(&supervisor);
    assert!(*second.revoke.borrow());
    assert!(*third.revoke.borrow());
    assert!(mic.status().consumers.is_empty());
    tokio::time::timeout(Duration::from_secs(1), third.revoke.changed())
        .await
        .unwrap()
        .unwrap();
    mic.shutdown().await;
    assert!(mic.worker.is_none());
}

#[tokio::test]
async fn slow_receiver_lags_without_blocking_capture_and_fast_receiver() {
    let app = AppId::new();
    let mut mic = MicrophoneService::new(MicrophoneConfig {
        allowed_apps: [app].into(),
        source: CaptureSource::TestTone,
        ..Default::default()
    });
    let mut slow = mic.open((app, 1)).unwrap();
    let mut fast = mic.open((app, 1)).unwrap();
    let mut count = 0;
    tokio::time::timeout(Duration::from_secs(2), async {
        while count < 20 {
            if matches!(
                fast.frames.recv().await.unwrap(),
                MicrophonePacket::Frame { .. }
            ) {
                count += 1;
            }
        }
    })
    .await
    .unwrap();
    assert!(matches!(
        slow.frames.recv().await,
        Err(broadcast::error::RecvError::Lagged(_))
    ));
    mic.shutdown().await;
}
