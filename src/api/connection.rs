use super::dispatch::Command;
use caros_protocol::{ApiError, ClientMessage, ErrorCode, PROTOCOL_VERSION, ServerMessage, wire};
use std::{io, time::Duration};
use tokio::{
    net::UnixStream,
    sync::{broadcast, mpsc, oneshot},
    time::timeout,
};

const IO_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) async fn serve(
    mut stream: UnixStream,
    commands: mpsc::Sender<Command>,
) -> io::Result<()> {
    // A management connection represents the OS user, not a caller-supplied role.
    if stream.peer_cred()?.uid() != unsafe { libc::geteuid() } {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "management peer UID mismatch",
        ));
    }
    let hello = timeout(IO_TIMEOUT, wire::read::<ClientMessage>(&mut stream)).await??;
    match hello {
        ClientMessage::Hello { version } if version == PROTOCOL_VERSION => {
            send(&mut stream, &ServerMessage::Hello { version }).await?;
        }
        _ => {
            send(
                &mut stream,
                &ServerMessage::Rejected(ApiError::new(
                    ErrorCode::IncompatibleVersion,
                    "expected a compatible version handshake",
                )),
            )
            .await?;
            return Ok(());
        }
    }
    let mut previous_id = 0;
    loop {
        let message = wire::read::<ClientMessage>(&mut stream).await?;
        let ClientMessage::Request { id, request } = message else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "expected request",
            ));
        };
        if id <= previous_id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request IDs must increase",
            ));
        }
        previous_id = id;
        let (reply, response) = oneshot::channel();
        let answer = timeout(IO_TIMEOUT, async {
            commands
                .send(Command { request, reply })
                .await
                .map_err(io::Error::other)?;
            response.await.map_err(io::Error::other)
        })
        .await??;
        send(
            &mut stream,
            &ServerMessage::Response {
                id,
                result: answer.result,
            },
        )
        .await?;
        if let Some(mut events) = answer.events {
            loop {
                tokio::select! {
                    event = events.recv() => {
                        match event {
                            Ok(event) => send(&mut stream, &ServerMessage::Event(event)).await?,
                            Err(broadcast::error::RecvError::Lagged(missed)) => {
                                send(&mut stream, &ServerMessage::Lagged { missed }).await?;
                                return Ok(());
                            }
                            Err(broadcast::error::RecvError::Closed) => return Ok(()),
                        }
                    }
                    // Subscriptions are receive-only after the acknowledgement.
                    _ = stream.readable() => {
                        let mut byte = [0];
                        match stream.try_read(&mut byte) {
                            Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                            _ => return Ok(()),
                        }
                    }
                }
            }
        }
    }
}
async fn send(stream: &mut UnixStream, message: &ServerMessage) -> io::Result<()> {
    timeout(IO_TIMEOUT, wire::write(stream, message)).await?
}
