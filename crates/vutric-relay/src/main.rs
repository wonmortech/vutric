//! `vutric-relay [CONFIG]`: runs the relay until Ctrl-C / SIGTERM. Default config path `vutric-relay.toml`;
//! an example with every key is in `crates/vutric-relay/vutric-relay.example.toml`.

use std::path::PathBuf;

use anyhow::Result;
use vutric_relay::{Config, Relay};

#[tokio::main]
async fn main() -> Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .map_or_else(|| PathBuf::from("vutric-relay.toml"), PathBuf::from);
    let config = Config::load(&path)?;
    let relay = Relay::spawn(&config).await?;
    println!(
        "vutric-relay {} up: http {:?} https {:?} quic {:?}",
        config.url(),
        relay.http_addr(),
        relay.https_addr(),
        relay.quic_addr()
    );
    shutdown_signal().await;
    println!("vutric-relay: stopping");
    relay.shutdown().await
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
