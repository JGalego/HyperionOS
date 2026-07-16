//! Real mDNS/DNS-SD advertise+discover for this crate's own `/mcp-server`/`/a2a-server` --
//! docs/998-roadmap.md's Social pillar named this the natural next slice after real MCP/A2A
//! request/reply: publishing a real `_hyperion-mcp._tcp.local.`/`_hyperion-a2a._tcp.local.`
//! service record on the real port a server bound, and a way to browse for the same service
//! types on the real LAN. Feature-gated (`mdns`) the same way `real-http`/`candle` already are
//! -- a real release image opts in explicitly; every host-side dev/test build of this console
//! stays network-free by default and gets an honest [`DiscoveryError::NotCompiledIn`] instead of
//! a silently faked result.
//!
//! **Discovery only, not identity/trust**: a discovered peer is just a `(name, host, addr)`
//! triple resolved off the real LAN -- there is no verification anywhere in this workspace yet
//! that a discovered peer really is who its own advertised name claims. `/mcp-call`/`/a2a-call`
//! (this crate's own real outbound half) still require the caller to decide whether to actually
//! connect; this module only makes "what's out there" answerable without already knowing an
//! exact host/port.

use std::net::SocketAddr;
#[cfg(not(feature = "mdns"))]
use std::time::Duration;

/// docs/998-roadmap.md's own literal service type names for this crate's two real servers.
pub const MCP_SERVICE_TYPE: &str = "_hyperion-mcp._tcp.local.";
pub const A2A_SERVICE_TYPE: &str = "_hyperion-a2a._tcp.local.";

#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[cfg(feature = "mdns")]
    #[error("real mDNS daemon error: {0}")]
    Daemon(#[from] mdns_sd::Error),
    #[error(
        "this binary wasn't built with the \"mdns\" feature -- real advertise/discover isn't \
         available"
    )]
    // Only ever constructed in the `not(feature = "mdns")` fallback below -- real, not dead code
    // in that configuration.
    #[cfg_attr(feature = "mdns", allow(dead_code))]
    NotCompiledIn,
}

/// A real peer resolved off the real LAN while browsing for a service type -- see this module's
/// own doc comment on what this does and doesn't prove about who's actually on the other end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPeer {
    pub instance_name: String,
    pub host: String,
    pub addr: SocketAddr,
}

#[cfg(feature = "mdns")]
mod real {
    use std::net::SocketAddr;
    use std::time::Duration;

    use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

    use super::{DiscoveredPeer, DiscoveryError};

    /// A real, running mDNS advertisement. Can be dropped immediately once returned -- like
    /// this crate's own `RunningServer` (see `main.rs`'s `start_mcp_server`/`start_a2a_server`
    /// doc comments), the real background thread this wraps and the service record it holds
    /// keep running/published independent of any particular handle's lifetime, for the rest of
    /// this process's life.
    pub struct Advertisement(#[allow(dead_code)] ServiceDaemon);

    pub fn advertise(
        service_type: &str,
        instance_name: &str,
        port: u16,
    ) -> Result<Advertisement, DiscoveryError> {
        let daemon = ServiceDaemon::new()?;
        let host_name = format!("{instance_name}.local.");
        let service = ServiceInfo::new(
            service_type,
            instance_name,
            &host_name,
            "",
            port,
            None::<std::collections::HashMap<String, String>>,
        )?
        .enable_addr_auto();
        daemon.register(service)?;
        Ok(Advertisement(daemon))
    }

    /// Browses for every real, currently-resolvable peer advertising `service_type` on the LAN,
    /// collecting `ServiceEvent::ServiceResolved` events for up to `timeout` before returning --
    /// a real, bounded scan, not an open-ended subscription.
    pub fn discover(
        service_type: &str,
        timeout: Duration,
    ) -> Result<Vec<DiscoveredPeer>, DiscoveryError> {
        let daemon = ServiceDaemon::new()?;
        let receiver = daemon.browse(service_type)?;
        let deadline = std::time::Instant::now() + timeout;
        let mut peers = Vec::new();
        while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
            match receiver.recv_timeout(remaining) {
                Ok(ServiceEvent::ServiceResolved(info)) => {
                    for scoped_addr in &info.addresses {
                        peers.push(DiscoveredPeer {
                            instance_name: info.fullname.clone(),
                            host: info.host.clone(),
                            addr: SocketAddr::new(scoped_addr.to_ip_addr(), info.port),
                        });
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        let _ = daemon.shutdown();
        Ok(peers)
    }
}

#[cfg(feature = "mdns")]
pub use real::{advertise, discover};

#[cfg(not(feature = "mdns"))]
pub struct Advertisement;

#[cfg(not(feature = "mdns"))]
pub fn advertise(
    _service_type: &str,
    _instance_name: &str,
    _port: u16,
) -> Result<Advertisement, DiscoveryError> {
    Err(DiscoveryError::NotCompiledIn)
}

#[cfg(not(feature = "mdns"))]
pub fn discover(
    _service_type: &str,
    _timeout: Duration,
) -> Result<Vec<DiscoveredPeer>, DiscoveryError> {
    Err(DiscoveryError::NotCompiledIn)
}
