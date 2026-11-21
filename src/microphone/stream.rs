use caros_protocol::microphone::MicrophonePacket;
use std::{io, os::fd::AsRawFd, time::Duration};
use tokio::{
    net::UnixStream,
    sync::{broadcast, watch},
    time::timeout,
};

pub(crate) struct MicrophoneLease {
    pub frames: broadcast::Receiver<MicrophonePacket>,
    pub revoke: watch::Receiver<bool>,
    pub muted: watch::Receiver<bool>,
}
impl MicrophoneLease {
    pub async fn serve(mut self, mut socket: UnixStream) -> io::Result<()> {
        // Bound kernel buffering too, so the socket cannot accumulate seconds of PCM.
        let size: libc::c_int = 4096;
        let result = unsafe {
            libc::setsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_SNDBUF,
                (&size as *const libc::c_int).cast(),
                std::mem::size_of_val(&size) as libc::socklen_t,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        loop {
            if *self.revoke.borrow() {
                return Ok(());
            }
            tokio::select! {
                biased;
                _ = self.revoke.changed() => return Ok(()),
                _ = socket.readable() => {
                    let mut byte = [0];
                    match socket.try_read(&mut byte) {
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {},
                        _ => return Ok(()),
                    }
                }
                packet = self.frames.recv() => {
                    let mut packet = match packet {
                        Ok(packet) => packet,
                        Err(broadcast::error::RecvError::Lagged(missed)) => MicrophonePacket::Gap { missed },
                        Err(broadcast::error::RecvError::Closed) => return Ok(()),
                    };
                    if *self.muted.borrow() && let MicrophonePacket::Frame { samples, .. } = &mut packet { samples.fill(0); }
                    // A slow app cannot stall capture or another subscriber.
                    tokio::select! {
                        biased;
                        _ = self.revoke.changed() => return Ok(()),
                        result = timeout(Duration::from_millis(200), packet.write(&mut socket)) => result??,
                    }
                }
            }
        }
    }
}
