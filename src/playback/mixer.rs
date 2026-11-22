use caros_protocol::{
    AppId,
    playback::{AppPlaybackStatus, PLAYBACK_SAMPLES, PlaybackStatus, Volume},
};
use std::collections::BTreeMap;
use tokio::sync::{mpsc, watch};
pub(super) struct Track {
    pub app: AppId,
    pub pid: u32,
    pub frames: mpsc::Receiver<Vec<i16>>,
    pub revoked: watch::Sender<bool>,
    pub gain: f32,
}
#[derive(Default)]
pub(super) struct Mixer {
    pub tracks: Vec<Track>,
    pub volumes: BTreeMap<AppId, Volume>,
    pub master: Volume,
    pub available: bool,
    pub peak: u16,
    pub mixed_frames: u64,
}
impl Mixer {
    pub fn mix(&mut self) -> Vec<i16> {
        // Disconnects discard queued audio instead of playing an orphan's backlog.
        self.tracks.retain(|t| t.revoked.receiver_count() > 0);
        let mut mixed = vec![0.0f32; PLAYBACK_SAMPLES];
        for track in &mut self.tracks {
            let volume = self.volumes[&track.app];
            let target = volume.gain() * self.master.gain();
            let silent = volume.muted || self.master.muted;
            let start = if silent { 0.0 } else { track.gain };
            if let Ok(samples) = track.frames.try_recv() {
                for (index, (out, sample)) in mixed.iter_mut().zip(samples).enumerate() {
                    // Same gain for left/right; ramp changes over a 20 ms block.
                    let fraction = (index / 2 + 1) as f32 / 960.0;
                    *out += sample as f32 * (start + (target - start) * fraction);
                }
            }
            track.gain = target;
        }
        let samples: Vec<i16> = mixed
            .into_iter()
            .map(|v| v.round().clamp(i16::MIN as f32, i16::MAX as f32) as i16)
            .collect();
        self.peak = samples
            .iter()
            .map(|s| (*s as i32).unsigned_abs() as u16)
            .max()
            .unwrap_or(0);
        self.mixed_frames += 1;
        samples
    }
    pub fn status(&self) -> PlaybackStatus {
        PlaybackStatus {
            master: self.master,
            available: self.available,
            mixed_peak: self.peak,
            mixed_frames: self.mixed_frames,
            apps: self
                .volumes
                .iter()
                .map(|(app, volume)| AppPlaybackStatus {
                    app: *app,
                    volume: *volume,
                    streams: self.tracks.iter().filter(|t| t.app == *app).count(),
                })
                .collect(),
        }
    }
}
