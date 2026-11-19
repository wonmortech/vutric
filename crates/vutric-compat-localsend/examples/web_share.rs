//! Opens a browser link on loopback for manual or scripted checks of the page (C2):
//! `cargo run -p vutric-compat-localsend --example web_share -- <file to share> <staging folder>`.
//! Prints `URL <link>` and accepts every upload, printing `RECEIVED <name> <bytes>`.

use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::time::Duration;

use vutric_compat_localsend::proto::DeviceType;
use vutric_compat_localsend::server::{self, Decision, Event, Receiver};
use vutric_compat_localsend::tls::Identity;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let shared: Vec<PathBuf> = args.next().into_iter().collect();
    let staging = args
        .next()
        .unwrap_or_else(|| std::env::temp_dir().join("vutric-web-share"));
    let (rx, mut events) = Receiver::start(
        server::Config {
            alias: "Vutric demo".into(),
            device_type: DeviceType::Desktop,
            port: 0,
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            staging,
            pin: None,
            decide_timeout: Duration::from_secs(30),
        },
        &Identity::generate()?,
    )
    .await?;
    let token = rx.share(&shared, Duration::from_secs(600))?;
    println!("URL https://localhost:{}/s/{token}/", rx.addr().port());
    while let Some(ev) = events.recv().await {
        match ev {
            Event::Offer { offer, reply } => {
                let _ = reply.send(Decision::Accept(
                    offer.files.iter().map(|f| f.id.clone()).collect(),
                ));
            }
            Event::Received { name, size, .. } => println!("RECEIVED {name} {size}"),
            Event::Downloaded { name, .. } => println!("DOWNLOADED {name}"),
            _ => {}
        }
    }
    Ok(())
}
