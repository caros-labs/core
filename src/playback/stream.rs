use caros_protocol::playback::read_frame;
use std::{io, time::Duration};
use tokio::{
    io::AsyncWriteExt,
    net::UnixStream,
    sync::{mpsc, watch},
    time::timeout,
};
pub(crate) struct PlaybackLease {
    pub frames: mpsc::Sender<Vec<i16>>,
    pub revoked: watch::Receiver<bool>,
}
impl PlaybackLease {
    pub async fn serve(mut self, mut socket: UnixStream) -> io::Result<()> {
        loop {
            if *self.revoked.borrow() {
                return Ok(());
            }
            tokio::select! {
                biased;
                _ = self.revoked.changed() => return Ok(()),
                result = async {
                    let frame = timeout(Duration::from_secs(5), read_frame(&mut socket)).await??;
                    self.frames.send(frame).await.map_err(io::Error::other)?;
                    timeout(Duration::from_millis(200), socket.write_u8(1)).await??;
                    Ok::<_, io::Error>(())
                } => result?,
            }
        }
    }
}
