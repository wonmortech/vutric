//! Advertising the receiver over mDNS, so Android lists it in the Quick Share sheet (ADR-0056).
//! Only while receive mode is on (ADR-0010 §3.3): dropping [`Advert`] withdraws it.

use anyhow::{Context as _, Result};
use mdns_sd::{ServiceDaemon, ServiceInfo};

use crate::endpoint;

pub struct Advert {
    daemon: ServiceDaemon,
    fullname: String,
}

impl std::fmt::Debug for Advert {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Advert({})", self.fullname)
    }
}

impl Advert {
    /// Announces `_FC9F5ED42C8A._tcp` with our name and endpoint info on every interface.
    pub fn start(endpoint_id: &str, info: &[u8], port: u16) -> Result<Self> {
        let daemon = ServiceDaemon::new().context("mDNS daemon")?;
        let host = format!("vutric-{}.local.", endpoint_id.to_ascii_lowercase());
        let txt = endpoint::txt(info);
        let props = [("n", txt.as_str())];
        let svc = ServiceInfo::new(
            endpoint::SERVICE_TYPE,
            &endpoint::service_name(endpoint_id),
            &host,
            "",
            port,
            &props[..],
        )
        .context("mDNS service")?
        .enable_addr_auto();
        let fullname = svc.get_fullname().to_owned();
        daemon.register(svc).context("mDNS register")?;
        Ok(Self { daemon, fullname })
    }

    #[must_use]
    pub fn fullname(&self) -> &str {
        &self.fullname
    }
}

impl Drop for Advert {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}
