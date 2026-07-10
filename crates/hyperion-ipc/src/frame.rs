use hyperion_capability::{CapabilityToken, Fault, Operation};

use crate::region::RegionDescriptor;
use crate::types::{FrameFlags, SchemaId};

pub(crate) const HYIP_MAGIC: u32 = 0x4859_4950; // "HYIP"
pub(crate) const WIRE_VERSION: u16 = 1;

/// The wire frame — docs/30-ipc-framework.md §Data Structures calls this
/// "the only byte-level representation that crosses a Trust Boundary."
///
/// This hosted simulator has no real remote route yet (see [`crate::Route`]),
/// so there is no actual wire to serialize onto — every "frame" here is an
/// in-memory value moved between threads. `body` is therefore a plain Rust
/// enum instead of a `Vec<u8>` the two ends would otherwise have to encode
/// and decode for no reason; the encode/decode step returns
/// (`26-apis.md`-driven schema codegen) once federation gives frames
/// somewhere real to travel. The framing fields that *do* matter already —
/// `magic`/`version`/`schema_id`/`flags`/`request_id` — are kept as-is so
/// the shape survives that later change unchanged.
#[derive(Debug, Clone)]
pub struct Frame {
    pub magic: u32,
    pub version: u16,
    pub schema_id: SchemaId,
    pub flags: FrameFlags,
    /// Correlates `CALL` <-> `REPLY`; `0` for `NOTIFY`.
    pub request_id: u64,
    /// The caller's own capability, carried explicitly in the frame rather
    /// than relying on the channel's identity — this is what preserves
    /// confused-deputy prevention at the framing layer
    /// (docs/30 §Security Considerations): a receiver is handed the
    /// caller's authority, never substituting its own ambient rights.
    /// Present on `CALL`/`NOTIFY` frames; `None` on `REPLY` frames, which
    /// flow server-to-client and assert no fresh authority of their own.
    pub cap_token: Option<CapabilityToken>,
    pub op: Operation,
    pub body: FrameBody,
}

#[derive(Debug, Clone)]
pub enum FrameBody {
    Payload(Vec<u8>),
    Region(RegionDescriptor),
    /// Carried by an `ERROR`-flagged reply: the specific capability check
    /// that failed, so a rejected caller gets a real reason instead of a
    /// bare timeout.
    Fault(Fault),
}
