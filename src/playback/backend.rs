use super::mixer::Mixer;
use std::{
    process::Stdio,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    io::AsyncWriteExt,
    process::{Child, Command},
    sync::watch,
};

#[derive(Clone, Copy, Default, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputBackend {
    #[default]
    Default,
    Discard,
}

pub(super) async fn run(
    state: Arc<Mutex<Mixer>>,
    backend: OutputBackend,
    mut stop: watch::Receiver<bool>,
) {
    let mut tick = tokio::time::interval(Duration::from_millis(20));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut child: Option<Child> = None;
    let mut retry = Instant::now();
    loop {
        tokio::select! { biased; _ = stop.changed() => break, _ = tick.tick() => {} }
        let samples = {
            let mut mixer = state.lock().unwrap();
            let samples = mixer.mix();
            if mixer.tracks.is_empty() {
                break;
            }
            samples
        };
        if matches!(backend, OutputBackend::Discard) {
            state.lock().unwrap().available = true;
            continue;
        }
        if child.is_none() && Instant::now() >= retry {
            child = Command::new("pacat")
                .args([
                    "--playback",
                    "--raw",
                    "--format=s16le",
                    "--rate=48000",
                    "--channels=2",
                    "--latency-msec=40",
                    "--client-name=CarOS Core playback",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .ok();
            retry = Instant::now() + Duration::from_secs(1);
        }
        let Some(output) = &mut child else {
            continue;
        };
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let result = tokio::select! {
            biased;
            _ = stop.changed() => break,
            result = tokio::time::timeout(Duration::from_millis(100), output.stdin.as_mut().unwrap().write_all(&bytes)) => result,
        };
        if matches!(result, Ok(Ok(()))) {
            state.lock().unwrap().available = true;
        } else {
            state.lock().unwrap().available = false;
            if let Some(mut failed) = child.take() {
                let _ = failed.kill().await;
                let _ = failed.wait().await;
            }
            retry = Instant::now() + Duration::from_secs(1);
        }
    }
    if let Some(mut child) = child {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    let mut mixer = state.lock().unwrap();
    mixer.available = false;
    mixer.peak = 0;
}
