use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};

use crate::types::{ChannelClass, IpcFault, Route, SchemaId};

/// A capability-scoped, schema-typed communication endpoint
/// (docs/30-ipc-framework.md §Data Structures). Wraps the caller's own
/// [`CapabilityToken`] for the target endpoint object plus what's needed to
/// route a call.
#[derive(Debug, Clone)]
pub struct Channel {
    pub(crate) endpoint: CapabilityToken,
    pub(crate) schema_id: SchemaId,
    pub(crate) route: Route,
    pub(crate) class: ChannelClass,
}

impl Channel {
    pub fn class(&self) -> ChannelClass {
        self.class
    }

    pub fn route(&self) -> Route {
        self.route
    }
}

/// `channel_open` — docs/30-ipc-framework.md §Architecture's
/// "Capability-scoped discovery": `token` must already authorize sending
/// into the named endpoint (`RightsMask::WRITE`), and every way that check
/// can fail — wrong token, revoked token, expired token — is deliberately
/// reported as the same [`IpcFault::NoSuchCapability`] a genuinely
/// nonexistent endpoint would produce. A caller without the right
/// authority must not be able to tell "you have the wrong key" from
/// "there is no door here."
pub fn channel_open(
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    schema: SchemaId,
    class: ChannelClass,
) -> Result<Channel, IpcFault> {
    monitor
        .check_rights_ok_result(token, RightsMask::WRITE)
        .map_err(|_| IpcFault::NoSuchCapability)?;

    Ok(Channel {
        endpoint: token.clone(),
        schema_id: schema,
        route: Route::Local, // no federation yet — see Route's docs
        class,
    })
}
