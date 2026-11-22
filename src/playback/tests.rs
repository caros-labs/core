use super::*;
use caros_protocol::playback::PLAYBACK_SAMPLES;
fn service() -> (PlaybackService, AppId, AppId) {
    let a = AppId::new();
    let b = AppId::new();
    (
        PlaybackService::new(PlaybackConfig {
            allowed_apps: [a, b].into(),
            control_apps: [b].into(),
            backend: OutputBackend::Discard,
        }),
        a,
        b,
    )
}
fn frame(value: i16) -> Vec<i16> {
    vec![value; PLAYBACK_SAMPLES]
}
#[tokio::test]
async fn mixes_app_and_master_gain_and_mute_without_changing_saved_level() {
    let (mut service, a, b) = service();
    let left = service.open((a, 1)).unwrap();
    let right = service.open((b, 2)).unwrap();
    service.set_volume(a, None, Some(50), None).unwrap();
    service.set_volume(b, None, Some(25), None).unwrap();
    service
        .set_master(
            b,
            Volume {
                level: 50,
                muted: false,
            },
        )
        .unwrap();
    service.state.lock().unwrap().mix(); // settle the gain ramp
    left.frames.try_send(frame(8000)).unwrap();
    right.frames.try_send(frame(8000)).unwrap();
    assert!(
        service
            .state
            .lock()
            .unwrap()
            .mix()
            .iter()
            .all(|s| *s == 3000)
    );
    service.set_volume(a, None, None, Some(true)).unwrap();
    left.frames.try_send(frame(8000)).unwrap();
    right.frames.try_send(frame(8000)).unwrap();
    assert!(
        service
            .state
            .lock()
            .unwrap()
            .mix()
            .iter()
            .all(|s| *s == 1000)
    );
    assert_eq!(service.state.lock().unwrap().volumes[&a].level, 50);
    service
        .set_master(
            b,
            Volume {
                level: 50,
                muted: true,
            },
        )
        .unwrap();
    right.frames.try_send(frame(8000)).unwrap();
    assert!(service.state.lock().unwrap().mix().iter().all(|s| *s == 0));
    service.shutdown().await;
}
#[tokio::test]
async fn bounds_queues_clips_sums_and_discards_disconnected_streams() {
    let (mut service, a, b) = service();
    let left = service.open((a, 1)).unwrap();
    let right = service.open((b, 2)).unwrap();
    for _ in 0..4 {
        left.frames.try_send(frame(30000)).unwrap();
    }
    assert!(left.frames.try_send(frame(30000)).is_err());
    service.state.lock().unwrap().mix();
    right.frames.try_send(frame(30000)).unwrap();
    assert!(
        service
            .state
            .lock()
            .unwrap()
            .mix()
            .iter()
            .all(|s| *s == i16::MAX)
    );
    drop(left);
    right.frames.try_send(frame(-30000)).unwrap();
    assert!(
        service
            .state
            .lock()
            .unwrap()
            .mix()
            .iter()
            .all(|s| *s == -30000)
    );
    assert_eq!(service.state.lock().unwrap().tracks.len(), 1);
    service.shutdown().await;
}
#[tokio::test]
async fn enforces_playback_and_controller_grants_and_volume_range() {
    let (mut service, a, b) = service();
    assert!(service.open((AppId::new(), 3)).is_err());
    assert!(service.set_volume(a, Some(b), Some(1), None).is_err());
    assert!(service.set_master(a, Volume::default()).is_err());
    assert!(service.set_volume(a, None, Some(101), None).is_err());
    assert!(
        service
            .set_master(
                b,
                Volume {
                    level: 255,
                    muted: false
                }
            )
            .is_err()
    );
    service
        .set_volume(b, Some(a), Some(40), Some(true))
        .unwrap();
    assert_eq!(
        service.state.lock().unwrap().volumes[&a],
        Volume {
            level: 40,
            muted: true
        }
    );
    let first = service.open((a, 1)).unwrap();
    let second = service.open((a, 1)).unwrap();
    assert!(service.open((a, 1)).is_err());
    drop((first, second));
    service.shutdown().await;
}
#[tokio::test]
async fn supervisor_stop_revokes_audio_but_preserves_app_level() {
    use crate::supervisor::AppSpec;
    let (mut service, a, _) = service();
    let mut supervisor = Supervisor::new();
    let mut spec = AppSpec::new(a, "Playback test", "/bin/sleep");
    spec.args = vec!["30".into()];
    supervisor.register(spec).unwrap();
    supervisor.start(&a).unwrap();
    let lease = service
        .open((a, supervisor.status(&a).unwrap().pid.unwrap()))
        .unwrap();
    service.set_volume(a, None, Some(30), None).unwrap();
    supervisor.stop(&a).unwrap();
    service.poll(&supervisor);
    assert!(*lease.revoked.borrow());
    assert_eq!(
        service
            .status()
            .apps
            .iter()
            .find(|s| s.app == a)
            .unwrap()
            .volume
            .level,
        30
    );
    assert!(service.state.lock().unwrap().tracks.is_empty());
    service.shutdown().await;
}
