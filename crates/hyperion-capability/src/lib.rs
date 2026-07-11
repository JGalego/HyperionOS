//! Hyperion L0 capability security core.
//!
//! Implements docs/03-kernel-architecture.md's "Capability Security as the
//! Kernel Primitive": unforgeable, scoped, revocable, attenuable tokens,
//! minted and checked by a single [`CapabilityMonitor`]. This is the
//! foundation every other Phase 1 crate (`hyperion-ipc`, `hyperion-scheduler`)
//! builds on — no resource in Hyperion is ever granted to a caller who
//! cannot present the right token for it.
//!
//! This crate is a hosted-simulator translation of docs/03, not the doc
//! verbatim — notably it tracks revocation generation per delegation-graph
//! node (via [`TokenId`]) rather than per raw `ObjectId`, a correction to a
//! design gap in the original pseudocode; see [`TokenId`]'s docs and the
//! matching fix in docs/03-kernel-architecture.md.
//!
//! [`CapabilityToken`] itself never crosses a real process boundary — its
//! constructors are `pub(crate)` and it has no `Serialize`/`Deserialize` impl,
//! deliberately: see [`WireToken`]'s docs for what real IPC (`hyperion-ipc`,
//! M3) carries instead, and why a naively serializable token would have
//! broken the "unforgeable" half of "unforgeable, scoped, revocable,
//! attenuable" above the moment it left this process.

mod monitor;
mod revocation;
mod table;
mod token;
mod types;
mod wire;

pub use monitor::CapabilityMonitor;
pub use revocation::RevocationReceipt;
pub use table::{CapabilityTable, SlotIndex};
pub use token::CapabilityToken;
pub use types::{Fault, ObjectId, Operation, RightsMask, TokenId, TrustBoundaryId};
pub use wire::WireToken;
