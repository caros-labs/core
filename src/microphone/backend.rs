use caros_protocol::microphone::{MICROPHONE_SAMPLES, MicrophonePacket};
use std::{
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::AsyncReadExt,
    process::Command,
    sync::{broadcast, watch},
};

/// A deterministic source is available only through trusted configuration for tests.
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureSource {
    #[default]
    Default,
    TestTone,
}

pub(super) async fn run(
    source: CaptureSource,
    output: broadcast::Sender<MicrophonePacket>,
    mut stop: watch::Receiver<bool>,
    muted: watch::Receiver<bool>,
) {
    let started = Instant::now();
    let mut sequence = 0;
    if matches!(source, CaptureSource::TestTone) {
        let mut tick = tokio::time::interval(Duration::from_millis(20));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let _ = output.send(MicrophonePacket::Ready);
        loop {
            tokio::select! {
                biased;
                _ = stop.changed() => return,
                _ = tick.tick() => {
                    // 1 kHz square wave, exact RMS/peak 4096 for test assertions.
                    let mut samples: Vec<i16> = (0..MICROPHONE_SAMPLES).map(|i| if i % 48 < 24 { 4096 } else { -4096 }).collect();
                    if *muted.borrow() { samples.fill(0); }
                    sequence += 1;
                    let _ = output.send(MicrophonePacket::Frame { sequence, timestamp_us: started.elapsed().as_micros() as u64, samples });
                }
            }
        }
    }
    while !*stop.borrow() {
        // Core alone starts the platform capture client. PipeWire's PulseAudio
        // compatibility server selects the system default source.
        let child = Command::new("parec")
            .args([
                "--raw",
                "--format=s16le",
                "--rate=48000",
                "--channels=1",
                "--latency-msec=20",
                "--client-name=CarOS Core microphone",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn();
        if let Ok(mut child) = child {
            let mut stdout = child.stdout.take().expect("piped capture stdout");
            let mut ready = false;
            loop {
                let mut pcm = [0u8; MICROPHONE_SAMPLES * 2];
                let result = tokio::select! {
                    biased;
                    _ = stop.changed() => break,
                    result = tokio::time::timeout(Duration::from_secs(3), stdout.read_exact(&mut pcm)) => result,
                };
                if !matches!(result, Ok(Ok(_))) {
                    break;
                }
                if !ready {
                    let _ = output.send(MicrophonePacket::Ready);
                    ready = true;
                }
                sequence += 1;
                let mut samples: Vec<i16> = pcm
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|s| i16::from_le_bytes([s[0], s[1]]))
                    .collect();
                if *muted.borrow() {
                    samples.fill(0);
                }
                let _ = output.send(MicrophonePacket::Frame {
                    sequence,
                    timestamp_us: started.elapsed().as_micros() as u64,
                    samples,
                });
            }
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        if *stop.borrow() {
            return;
        }
        let _ = output.send(MicrophonePacket::Unavailable);
        tokio::select! {
            _ = stop.changed() => return,
            _ = tokio::time::sleep(Duration::from_secs(1)) => {}
        }
    }
}
