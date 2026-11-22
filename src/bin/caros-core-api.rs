use caros_core::api::{CoreServer, manifest};
use std::{fs::DirBuilder, os::unix::fs::DirBuilderExt, path::PathBuf};
use tokio::signal::unix::{SignalKind, signal};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let mut socket = None;
    let mut config = None;
    while let Some(arg) = args.next() {
        if arg == "--help" {
            println!("Usage: caros-core-api --socket PRIVATE_DIR/core.sock --manifest apps.json");
            return Ok(());
        }
        if arg != "--socket" && arg != "--manifest" {
            return Err("unknown option".into());
        }
        let value = PathBuf::from(args.next().ok_or("missing option value")?);
        let slot = if arg == "--socket" {
            &mut socket
        } else {
            &mut config
        };
        if slot.replace(value).is_some() {
            return Err("duplicate option".into());
        }
    }
    let socket = socket.ok_or("--socket is required")?;
    let config = config.ok_or("--manifest is required")?;
    let configuration = manifest::load_configuration(config)?;
    let parent = socket
        .parent()
        .ok_or("socket requires a parent directory")?;
    match DirBuilder::new().mode(0o700).create(parent) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;
    let server = CoreServer::bind(&socket, configuration.apps)?
        .with_services(configuration.services)
        .with_microphone(configuration.microphone)
        .with_playback(configuration.playback);
    eprintln!("Core management socket: {}", socket.display());
    server
        .run(async move {
            tokio::select! { _ = interrupt.recv() => {}, _ = terminate.recv() => {} }
        })
        .await?;
    Ok(())
}
