//! Hyperion L1 IPC framework: capability-scoped message passing.
//!
//! Implements docs/30-ipc-framework.md's capability-gated channel discovery,
//! synchronous call/response, asynchronous one-way notify, and zero-copy
//! region sharing, built entirely on `hyperion-capability`'s tokens — no
//! endpoint is ever reachable without one, and no server ever trusts a
//! cached copy of a caller's authority instead of re-checking it live.
//!
//! Two transports exist side by side, both built on the same [`Frame`] shape
//! and call/notify semantics, per
//! [PRODUCTION_BOOT_PROMPT.md](../../../PRODUCTION_BOOT_PROMPT.md) M3's reuse map:
//! [`IpcBus`] is the original hosted-simulator translation (every Trust Boundary is
//! a thread in one process, so a "frame" is an in-memory value moved between
//! threads, never actually encoded); [`Endpoint`] (`transport`) is the real
//! transport — real Unix domain sockets between real, separate Linux processes,
//! with [`hyperion_capability::WireToken`] carrying a capability's *claimed*
//! fields since a real `CapabilityToken` cannot cross a real process boundary
//! at all (see that type's own docs on why). `Route::Local` remains accurate
//! for both: neither is the actual remote-host case docs/30 also describes,
//! which still waits on docs/21-distributed-execution.md's federation work.

mod bus;
mod channel;
mod frame;
mod region;
mod transport;
mod types;

pub use bus::{AuthenticatedCall, IpcBus, Notification, Request, Response};
pub use channel::{channel_open, Channel};
pub use frame::{Frame, FrameBody};
pub use hyperion_capability::Operation;
pub use region::{region_map, region_share, RegionDescriptor};
pub use transport::{Endpoint, IncomingFrame};
pub use types::{ChannelClass, FrameFlags, IpcFault, Route, SchemaId};
