use super::support::TestCore;
use caros_protocol::{ClientMessage, ErrorCode, PROTOCOL_VERSION, Request, ServerMessage, wire};
use caros_sdk::ManagementClient;
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    time::timeout,
};

async fn handshake(path: &std::path::Path) -> UnixStream {
    let mut stream = UnixStream::connect(path).await.unwrap();
    wire::write(
        &mut stream,
        &ClientMessage::Hello {
            version: PROTOCOL_VERSION,
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        wire::read::<ServerMessage>(&mut stream).await.unwrap(),
        ServerMessage::Hello { .. }
    ));
    stream
}
async fn closed(mut stream: UnixStream) {
    let mut byte = [0];
    let result = timeout(Duration::from_secs(2), stream.read(&mut byte))
        .await
        .unwrap();
    assert!(matches!(result, Ok(0)) || result.is_err());
}

#[tokio::test]
async fn refuses_wrong_version_and_requests_before_handshake() {
    let mut core = TestCore::start();
    for message in [
        ClientMessage::Hello { version: 999 },
        ClientMessage::Request {
            id: 1,
            request: Request::ListApps,
        },
    ] {
        let mut stream = UnixStream::connect(&core.path).await.unwrap();
        wire::write(&mut stream, &message).await.unwrap();
        assert!(
            matches!(wire::read::<ServerMessage>(&mut stream).await.unwrap(), ServerMessage::Rejected(e) if e.code == ErrorCode::IncompatibleVersion)
        );
        closed(stream).await;
    }
    core.finish().await;
}

#[tokio::test]
async fn malformed_and_oversized_frames_only_disconnect_offending_client() {
    let mut core = TestCore::start();
    for bytes in [
        vec![0, 0, 0, 1, b'{'],
        (wire::MAX_FRAME_BYTES as u32 + 1).to_be_bytes().to_vec(),
        vec![0, 0, 0, 0],
    ] {
        let mut stream = handshake(&core.path).await;
        stream.write_all(&bytes).await.unwrap();
        closed(stream).await;
    }
    let mut truncated = handshake(&core.path).await;
    truncated.write_all(&[0, 0, 0, 100, b'{']).await.unwrap();
    truncated.shutdown().await.unwrap();
    closed(truncated).await;
    let client = ManagementClient::connect(&core.path).await.unwrap();
    assert_eq!(client.apps().list().await.unwrap().len(), 1);
    core.finish().await;
}

#[tokio::test]
async fn duplicate_request_ids_are_not_replayed() {
    let mut core = TestCore::start();
    let mut stream = handshake(&core.path).await;
    let message = ClientMessage::Request {
        id: 1,
        request: Request::StartApp { id: core.id },
    };
    wire::write(&mut stream, &message).await.unwrap();
    assert!(matches!(
        wire::read::<ServerMessage>(&mut stream).await.unwrap(),
        ServerMessage::Response { result: Ok(_), .. }
    ));
    wire::write(&mut stream, &message).await.unwrap();
    closed(stream).await;
    core.finish().await;
}
