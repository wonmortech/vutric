//! Vutric's own relay (backlog R2, ADR-0053): the iroh relay server with our defaults.
//!
//! The relay only forwards packets that are already end-to-end encrypted by QUIC between the two devices. What
//! its operator can see is listed in `docs/relay.md`: endpoint keys, client IP addresses, when and how much,
//! never names or content.
//!
//! On top of upstream it adds what a public relay needs and upstream leaves to the operator:
//!
//! - a small TOML config ([`Config`]) with safe defaults: ACME (Let's Encrypt) TLS, a per-client receive rate
//!   limit, at most [`Limits::max_connections_per_key`] connections per key and [`Limits::max_connections`] in
//!   total ([`Caps`]);
//! - QUIC address discovery on 7842, so devices learn their public address without STUN.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context as _, Result, bail};
use iroh_base::EndpointId;
use iroh_relay::server::{
    Access, AccessControl, AcmeConfig, CertConfig, ClientRateLimit, ClientRequest, ConnectionId,
    Limits as UpstreamLimits, QuicConfig, RelayConfig, ServerConfig, TlsConfig,
};
use serde::Deserialize;

/// The whole config file.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub http: Http,
    /// Absent: plain HTTP only (tests, or behind a TLS-terminating proxy).
    pub tls: Option<Tls>,
    /// QUIC address discovery. Needs `tls`.
    pub quic: Option<Quic>,
    #[serde(default)]
    pub limits: Limits,
    /// Prometheus metrics; keep it on localhost.
    pub metrics_bind: Option<SocketAddr>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Http {
    /// Port 80 in production: the captive-portal probe and the ACME HTTP challenge live here.
    pub bind: SocketAddr,
}

impl Default for Http {
    fn default() -> Self {
        Self {
            bind: "[::]:80".parse().expect("valid"),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TlsMode {
    /// Let's Encrypt, renewed automatically.
    Acme,
    /// PEM files you renew yourself.
    Manual,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Tls {
    #[serde(default = "https_default")]
    pub bind: SocketAddr,
    pub mode: TlsMode,
    /// The relay's DNS name, `relay.example.org`.
    pub domain: String,
    /// ACME account contact, `mailto:...`.
    pub contact: Option<String>,
    /// Where ACME keeps the account and certificates. Without it every restart orders a new certificate and
    /// soon hits the Let's Encrypt rate limit.
    pub cache_dir: Option<PathBuf>,
    /// Let's Encrypt staging (for trying the setup).
    #[serde(default)]
    pub staging: bool,
    /// `mode = "manual"`: certificate chain and private key, PEM.
    pub cert: Option<PathBuf>,
    pub key: Option<PathBuf>,
}

fn https_default() -> SocketAddr {
    "[::]:443".parse().expect("valid")
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Quic {
    #[serde(default = "quic_default")]
    pub bind: SocketAddr,
}

fn quic_default() -> SocketAddr {
    SocketAddr::from(([0, 0, 0, 0], iroh_relay::defaults::DEFAULT_RELAY_QUIC_PORT))
}

/// Abuse limits. The defaults let a phone send a 4K video at full Wi-Fi speed when no direct path exists, and
/// keep one key from taking the server.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Limits {
    /// Bytes per second one client may send into the relay.
    pub client_bytes_per_second: u32,
    /// Burst above that rate.
    pub client_burst_bytes: u32,
    /// Concurrent connections of one endpoint key (a device reconnecting, a few app instances).
    pub max_connections_per_key: usize,
    /// Concurrent connections in total.
    pub max_connections: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            client_bytes_per_second: 12 * 1024 * 1024,
            client_burst_bytes: 32 * 1024 * 1024,
            max_connections_per_key: 8,
            max_connections: 20_000,
        }
    }
}

impl Config {
    /// Reads and checks a TOML config.
    pub fn parse(text: &str) -> Result<Self> {
        let c: Self = toml::from_str(text).context("relay config")?;
        c.check()?;
        Ok(c)
    }

    pub fn load(path: &Path) -> Result<Self> {
        Self::parse(
            &std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?,
        )
    }

    fn check(&self) -> Result<()> {
        let l = &self.limits;
        if l.client_bytes_per_second == 0
            || l.max_connections_per_key == 0
            || l.max_connections == 0
        {
            bail!("limits must be above 0");
        }
        if l.client_burst_bytes < l.client_bytes_per_second {
            bail!("client_burst_bytes must be at least client_bytes_per_second");
        }
        if self.quic.is_some() && self.tls.is_none() {
            bail!("[quic] needs [tls]");
        }
        if let Some(t) = &self.tls {
            if t.domain.is_empty() || t.domain.contains(['/', ':', ' ']) {
                bail!("tls.domain must be a bare DNS name");
            }
            match t.mode {
                TlsMode::Acme => {
                    if t.contact
                        .as_deref()
                        .is_none_or(|c| !c.starts_with("mailto:"))
                    {
                        bail!("tls.contact must be a mailto: address for ACME");
                    }
                }
                TlsMode::Manual => {
                    if t.cert.is_none() || t.key.is_none() {
                        bail!("tls.mode = \"manual\" needs tls.cert and tls.key");
                    }
                }
            }
        }
        Ok(())
    }

    /// The relay URL clients put in their config: `https://domain/` with TLS, else `http://bind/`.
    #[must_use]
    pub fn url(&self) -> String {
        match &self.tls {
            Some(t) => format!("https://{}/", t.domain),
            None => format!("http://{}/", self.http.bind),
        }
    }
}

/// Connection caps: [`Limits::max_connections_per_key`] per key, [`Limits::max_connections`] in total.
#[derive(Debug)]
pub struct Caps {
    per_key: usize,
    total: usize,
    open: Mutex<HashMap<[u8; 32], usize>>,
}

impl Caps {
    #[must_use]
    pub fn new(per_key: usize, total: usize) -> Self {
        Self {
            per_key,
            total,
            open: Mutex::new(HashMap::new()),
        }
    }

    /// Admits one more connection of `key`, or says why not.
    pub fn admit(&self, key: [u8; 32]) -> Access {
        let mut open = self.open.lock().expect("caps lock");
        let total: usize = open.values().sum();
        if total >= self.total {
            return Access::Deny {
                reason: Some("relay full".into()),
            };
        }
        let n = open.entry(key).or_insert(0);
        if *n >= self.per_key {
            return Access::Deny {
                reason: Some("too many connections for this key".into()),
            };
        }
        *n += 1;
        Access::Allow
    }

    pub fn release(&self, key: [u8; 32]) {
        let mut open = self.open.lock().expect("caps lock");
        if let Some(n) = open.get_mut(&key) {
            *n -= 1;
            if *n == 0 {
                open.remove(&key);
            }
        }
    }

    /// Connections open now.
    #[must_use]
    pub fn open(&self) -> usize {
        self.open.lock().expect("caps lock").values().sum()
    }
}

impl AccessControl for Caps {
    async fn on_connect(&self, request: &ClientRequest) -> Access {
        self.admit(*request.endpoint_id().as_bytes())
    }

    fn on_disconnect(&self, endpoint_id: EndpointId, _: ConnectionId) {
        self.release(*endpoint_id.as_bytes());
    }
}

fn rustls_builder() -> rustls::ConfigBuilder<rustls::ServerConfig, rustls::server::WantsServerCert>
{
    rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("ring supports TLS 1.2 and 1.3")
        .with_no_client_auth()
}

fn manual_tls(cert: &Path, key: &Path) -> Result<rustls::ServerConfig> {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let certs = CertificateDer::pem_file_iter(cert)
        .with_context(|| format!("read {}", cert.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("certificate PEM")?;
    if certs.is_empty() {
        bail!("no certificate in {}", cert.display());
    }
    let key =
        PrivateKeyDer::from_pem_file(key).with_context(|| format!("key {}", key.display()))?;
    rustls_builder()
        .with_single_cert(certs, key)
        .context("certificate and key do not match")
}

/// The upstream server config for `c`, sharing `caps` (kept by the caller for metrics and tests).
pub fn server_config(c: &Config, caps: Arc<Caps>) -> Result<ServerConfig> {
    let mut limits = UpstreamLimits::default();
    let mut rx = ClientRateLimit::new(
        NonZeroU32::new(c.limits.client_bytes_per_second).context("rate above 0")?,
    );
    rx.max_burst_bytes = NonZeroU32::new(c.limits.client_burst_bytes);
    limits.client_rx = Some(rx);

    let mut relay = RelayConfig::new(c.http.bind);
    relay.limits = limits;
    relay.access = caps;
    let mut quic_tls = None;
    if let Some(t) = &c.tls {
        let cert = match t.mode {
            TlsMode::Acme => {
                let mut acme = AcmeConfig::letsencrypt(!t.staging)
                    .domains(vec![t.domain.clone()])
                    .contact(t.contact.iter().cloned().collect());
                if let Some(dir) = &t.cache_dir {
                    acme = acme.cache_path(dir.clone());
                }
                CertConfig::LetsEncrypt {
                    acme_config: acme,
                    server_config_builder: rustls_builder(),
                }
            }
            TlsMode::Manual => {
                let sc = manual_tls(
                    t.cert.as_deref().expect("checked"),
                    t.key.as_deref().expect("checked"),
                )?;
                quic_tls = Some(sc.clone());
                CertConfig::Manual { server_config: sc }
            }
        };
        relay.tls = Some(TlsConfig::new(t.bind, cert));
    }
    let mut out = ServerConfig::default();
    out.relay = Some(relay);
    if let Some(q) = &c.quic {
        let mut qc = QuicConfig::new(q.bind);
        qc.server_config = quic_tls;
        out.quic = Some(qc);
    }
    out.metrics_addr = c.metrics_bind;
    Ok(out)
}

/// A running relay.
#[derive(Debug)]
pub struct Relay {
    server: iroh_relay::server::Server,
    pub caps: Arc<Caps>,
}

impl Relay {
    pub async fn spawn(c: &Config) -> Result<Self> {
        let caps = Arc::new(Caps::new(
            c.limits.max_connections_per_key,
            c.limits.max_connections,
        ));
        let server = iroh_relay::server::Server::spawn(server_config(c, Arc::clone(&caps))?)
            .await
            .context("start relay")?;
        Ok(Self { server, caps })
    }

    /// The plain-HTTP address (captive-portal probe, ACME, or the whole relay without TLS).
    #[must_use]
    pub fn http_addr(&self) -> Option<SocketAddr> {
        self.server.http_addr()
    }

    #[must_use]
    pub fn https_addr(&self) -> Option<SocketAddr> {
        self.server.https_addr()
    }

    #[must_use]
    pub fn quic_addr(&self) -> Option<SocketAddr> {
        self.server.quic_addr()
    }

    pub async fn shutdown(self) -> Result<()> {
        self.server
            .shutdown()
            .await
            .map_err(|e| anyhow::anyhow!("relay shutdown: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_checks() {
        let c = Config::parse("").unwrap();
        assert_eq!(c.limits, Limits::default());
        assert!(c.tls.is_none());
        assert_eq!(c.url(), "http://[::]:80/");

        let acme = r#"
            [tls]
            mode = "acme"
            domain = "relay.example.org"
            contact = "mailto:ops@example.org"
            cache_dir = "/var/lib/vutric-relay"
            [quic]
        "#;
        let c = Config::parse(acme).unwrap();
        assert_eq!(c.url(), "https://relay.example.org/");
        assert_eq!(c.quic.unwrap().bind.port(), 7842);

        for bad in [
            "[quic]",
            "[limits]\nclient_bytes_per_second = 0",
            "[limits]\nclient_bytes_per_second = 10\nclient_burst_bytes = 5",
            "[tls]\nmode = \"acme\"\ndomain = \"r.example.org\"",
            "[tls]\nmode = \"acme\"\ndomain = \"https://r.example.org\"\ncontact = \"mailto:a@b.c\"",
            "[tls]\nmode = \"manual\"\ndomain = \"r.example.org\"",
            "unknown = 1",
        ] {
            assert!(Config::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_example_config_is_valid() {
        let c = Config::parse(include_str!("../vutric-relay.example.toml")).unwrap();
        assert_eq!(c.limits, Limits::default());
        assert_eq!(c.tls.unwrap().mode, TlsMode::Acme);
    }

    #[test]
    fn caps_per_key_and_total() {
        let caps = Caps::new(2, 3);
        let (a, b) = ([1; 32], [2; 32]);
        assert_eq!(caps.admit(a), Access::Allow);
        assert_eq!(caps.admit(a), Access::Allow);
        assert!(matches!(caps.admit(a), Access::Deny { .. }), "per key");
        assert_eq!(caps.admit(b), Access::Allow);
        assert!(matches!(caps.admit([3; 32]), Access::Deny { .. }), "total");
        caps.release(a);
        assert_eq!(caps.open(), 2);
        assert_eq!(caps.admit([3; 32]), Access::Allow);
        caps.release([9; 32]);
        assert_eq!(caps.open(), 3);
    }
}
