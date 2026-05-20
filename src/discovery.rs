//! mDNS service discovery — advertises the grod daemon on the LAN.
//!
//! Service: `_grod._tcp.local.`
//! TXT records:
//!   - `version`: grod crate version
//!   - `pin`: "1" if API requires PIN, "0" otherwise
//!
//! Clients (Flutter app) can browse for this service to auto-discover
//! the server's IP + port instead of requiring manual entry.

use anyhow::{Context, Result};
use mdns_sd::{ServiceDaemon, ServiceInfo};

const SERVICE_TYPE: &str = "_grod._tcp.local.";

/// Publish the grod daemon on mDNS. Returns the running daemon so the caller
/// can keep it alive; dropping it unregisters the service.
///
/// `lan_host` should be the LAN-reachable IP (same /24 as the Chromecast),
/// not a loopback or hotspot address.
pub fn publish(lan_host: &str, api_port: u16, pin_required: bool) -> Result<ServiceDaemon> {
    let mdns = ServiceDaemon::new().context("failed to start mDNS daemon")?;

    let hostname = format!("grod-{}.local.", sanitize_for_hostname(lan_host));
    let instance_name = "grod"; // Service instance name shown to discoverers

    let mut props = std::collections::HashMap::new();
    props.insert("version".to_string(), env!("CARGO_PKG_VERSION").to_string());
    props.insert("pin".to_string(), if pin_required { "1" } else { "0" }.to_string());

    let info = ServiceInfo::new(
        SERVICE_TYPE,
        instance_name,
        &hostname,
        lan_host,
        api_port,
        Some(props),
    )
    .context("building mDNS ServiceInfo")?;

    mdns.register(info)
        .context("registering mDNS service")?;

    eprintln!(
        "[daemon] mDNS service registered: {instance_name}.{SERVICE_TYPE} on {lan_host}:{api_port}"
    );
    Ok(mdns)
}

/// mDNS hostnames can't contain dots or colons in some clients — convert IP
/// to dash-separated for the hostname component.
fn sanitize_for_hostname(ip: &str) -> String {
    ip.replace('.', "-").replace(':', "-")
}
