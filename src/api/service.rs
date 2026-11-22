use super::{connection, dispatch::Dispatcher, socket::Socket};
use crate::supervisor::Supervisor;
use std::{future::Future, io, path::Path, time::Duration};
use tokio::{
    sync::mpsc,
    task::JoinSet,
    time::{MissedTickBehavior, interval},
};

/// Bound management endpoint. The caller supplies trusted registered app specs
/// and a shutdown signal; no executable paths are accepted over the socket.
pub struct CoreServer {
    socket: Socket,
    dispatcher: Dispatcher,
}
impl CoreServer {
    pub fn bind(path: impl AsRef<Path>, supervisor: Supervisor) -> io::Result<Self> {
        Ok(Self {
            socket: Socket::bind(path.as_ref())?,
            dispatcher: Dispatcher::new(supervisor),
        })
    }
    pub fn with_services(mut self, services: crate::services::ServiceRegistry) -> Self {
        self.dispatcher.services = services;
        self
    }
    pub fn with_microphone(mut self, config: crate::microphone::MicrophoneConfig) -> Self {
        self.dispatcher.microphone = crate::microphone::MicrophoneService::new(config);
        self
    }
    pub fn with_playback(mut self, config: crate::playback::PlaybackConfig) -> Self {
        self.dispatcher.playback = crate::playback::PlaybackService::new(config);
        self
    }
    pub async fn run(mut self, shutdown: impl Future<Output = ()>) -> io::Result<()> {
        tokio::pin!(shutdown);
        let (sender, mut commands) = mpsc::channel::<super::dispatch::Command>(64);
        let mut clients = JoinSet::new();
        let mut ticks = interval(Duration::from_millis(20));
        ticks.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut stopping = false;
        loop {
            tokio::select! {
                _ = &mut shutdown, if !stopping => {
                    stopping = true;
                    // No new commands once shutdown starts; no orphan sessions.
                    clients.abort_all();
                    self.dispatcher.microphone.shutdown().await;
                    self.dispatcher.playback.shutdown().await;
                    self.dispatcher.supervisor.shutdown()?;
                    self.dispatcher.services.shutdown()?;
                    self.dispatcher.publish();
                }
                _ = ticks.tick() => {
                    self.dispatcher.supervisor.poll()?;
                    self.dispatcher.services.poll(&self.dispatcher.supervisor)?;
                    self.dispatcher.microphone.poll(&self.dispatcher.supervisor);
                    self.dispatcher.playback.poll(&self.dispatcher.supervisor);
                    self.dispatcher.publish();
                }
                accepted = self.socket.listener.accept(), if !stopping && clients.len() < 64 => {
                    let (stream, _) = accepted?;
                    let sender = sender.clone();
                    clients.spawn(async move { let _ = connection::serve(stream, sender).await; });
                }
                Some(command) = commands.recv(), if !stopping => {
                    self.dispatcher.supervisor.poll()?;
                    self.dispatcher.services.poll(&self.dispatcher.supervisor)?;
                    self.dispatcher.microphone.poll(&self.dispatcher.supervisor);
                    self.dispatcher.playback.poll(&self.dispatcher.supervisor);
                    self.dispatcher.publish();
                    let answer = self.dispatcher.handle(command.request, command.peer_pid);
                    let _ = command.reply.send(answer);
                }
                _ = clients.join_next(), if !clients.is_empty() => {}
            }
            if stopping
                && self.dispatcher.supervisor.is_idle()
                && self.dispatcher.services.is_idle()
            {
                return Ok(());
            }
        }
    }
}
