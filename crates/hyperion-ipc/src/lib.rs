//! Hyperion L1 IPC framework: capability-scoped message passing.
//!
//! Implements docs/30-ipc-framework.md's capability-gated channel discovery,
//! synchronous call/response, asynchronous one-way notify, and zero-copy
//! region sharing, built entirely on `hyperion-capability`'s tokens — no
//! endpoint is ever reachable without one, and no server ever trusts a
//! cached copy of a caller's authority instead of re-checking it live.
//!
//! This is a hosted-simulator translation: every Trust Boundary is a thread
//! in one process rather than its own address space, so there is no real
//! remote route yet (see [`Route`]) and no wire-encoding step (see
//! [`Frame`]'s docs) — those return once docs/21-distributed-execution.md's
//! federation work is in scope.

mod bus;
mod channel;
mod frame;
mod region;
mod types;

pub use bus::{AuthenticatedCall, IpcBus, Notification, Request, Response};
pub use channel::{channel_open, Channel};
pub use frame::{Frame, FrameBody};
pub use hyperion_capability::Operation;
pub use region::{region_map, region_share, RegionDescriptor};
pub use types::{ChannelClass, FrameFlags, IpcFault, Route, SchemaId};
