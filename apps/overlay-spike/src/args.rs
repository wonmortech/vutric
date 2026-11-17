use std::path::PathBuf;

use anyhow::{Context as _, Result, bail};
use vutric_core::SendZone;

#[derive(Debug)]
pub struct Args {
    pub runs: u32,
    pub interval: f32,
    pub zone: SendZone,
    pub image: Option<PathBuf>,
    pub speed: f32,
    pub angle: f32,
    pub frame_latency: u32,
}

impl Args {
    pub fn parse(mut it: impl Iterator<Item = String>) -> Result<Self> {
        let mut args = Self {
            runs: 5,
            interval: 1.2,
            zone: SendZone::Right,
            image: None,
            speed: 2600.0,
            angle: 0.25,
            frame_latency: 2,
        };
        while let Some(flag) = it.next() {
            let mut value = || it.next().with_context(|| format!("{flag} needs a value"));
            match flag.as_str() {
                "--runs" => args.runs = value()?.parse()?,
                "--interval" => args.interval = value()?.parse()?,
                "--speed" => args.speed = value()?.parse()?,
                "--angle" => args.angle = value()?.parse()?,
                "--latency" => args.frame_latency = value()?.parse()?,
                "--image" => args.image = Some(value()?.into()),
                "--zone" => {
                    args.zone = match value()?.as_str() {
                        "left" => SendZone::Left,
                        "right" => SendZone::Right,
                        "top" => SendZone::Top,
                        other => bail!("unknown zone {other} (left|right|top)"),
                    }
                }
                "-h" | "--help" => {
                    println!(
                        "vutric-overlay-spike [--runs N] [--interval SECS] [--zone left|right|top] \
                         [--image PATH] [--speed PX_PER_S] [--angle RAD] [--latency FRAMES]"
                    );
                    std::process::exit(0);
                }
                other => bail!("unknown flag {other}"),
            }
        }
        if args.runs == 0 {
            bail!("--runs must be at least 1");
        }
        Ok(args)
    }
}
