use std::sync::Arc;

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask};

use crate::types::IpcFault;

/// A capability to a shared memory region, minted the same way as any other
/// object capability via [`CapabilityMonitor::mint_root`]
/// (docs/30-ipc-framework.md §Data Structures). Only the descriptor — a
/// token plus an `Arc` handle — crosses the synchronous IPC path; the bytes
/// themselves are never copied into a [`crate::Frame`].
///
/// docs/30 models the shared bytes as a physical page range mapped via the
/// MMU/IOMMU. In this hosted, single-address-space simulator there is no
/// separate physical mapping step to perform: two simulated Trust Boundaries
/// are threads in one process, so "mapping" a region is handing out another
/// `Arc` clone of the same allocation, gated by the same capability check
/// real region_map would perform before touching the MMU.
#[derive(Debug, Clone)]
pub struct RegionDescriptor {
    region_cap: CapabilityToken,
    data: Arc<[u8]>,
}

/// `region_share` — wraps `buf` behind `token`, an already-minted capability
/// over it. Minting happens as its own explicit step (`monitor.mint_root`)
/// rather than inside this function, so the sender can keep a clone of the
/// same token for itself and revoke it later — the descriptor's copy is
/// deliberately opaque to receivers, but the sender is not required to lose
/// its own handle just because it shared one.
pub fn region_share(token: CapabilityToken, buf: Vec<u8>) -> RegionDescriptor {
    RegionDescriptor {
        region_cap: token,
        data: Arc::from(buf.into_boxed_slice()),
    }
}

/// `region_map` — validates `desc`'s capability is still live and authorizes
/// `required` before handing back a view of the shared bytes. Because this
/// re-checks the *live* capability (not a value captured when the
/// descriptor was created), a region whose capability was revoked after
/// sharing can no longer be mapped — the property
/// docs/30-ipc-framework.md §Testing Strategy calls "no receiver can observe
/// a region after its capability's generation has been bumped."
pub fn region_map(
    monitor: &CapabilityMonitor,
    desc: &RegionDescriptor,
    required: RightsMask,
) -> Result<Arc<[u8]>, IpcFault> {
    monitor.check_rights_ok_result(&desc.region_cap, required)?;
    Ok(Arc::clone(&desc.data))
}

impl RegionDescriptor {
    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}
