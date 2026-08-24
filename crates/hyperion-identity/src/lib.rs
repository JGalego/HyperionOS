//! Who is acting — docs/998-roadmap.md §0, Decision 2: Hyperion is multi-user.
//!
//! ## Identity, deliberately not authentication
//!
//! This crate answers "which person is this action attributable to". It does **not** answer "is
//! this person who they claim to be". Nothing here checks a credential, and a caller can name any
//! principal it likes.
//!
//! That split is the point rather than a shortcut. Data separation needs only the first question
//! answered: a device whose users have separate memory, separate secrets and separate audit trails
//! is coherent and useful even before anyone is challenged for a password. A device that
//! authenticates people carefully into one *shared* memory is not. So principals come first, and
//! the login flow arrives with the roadmap's T4 broker.
//!
//! **What that means concretely, and must not be forgotten:** this **separates** people, it does
//! not **protect** them from each other. Anyone who can reach the console can claim to be anyone.
//! Until authentication exists, treat every principal boundary as a guard against accident, never
//! against an adversary. Any code that starts relying on it for the latter is relying on something
//! that isn't there yet.
//!
//! ## Why a principal is a Trust Boundary
//!
//! [`hyperion_capability::CapabilityToken`] carries `token_id`, `object_id`, `rights`,
//! `generation`, `origin: TrustBoundaryId` and `expiry` — no subject. Rather than adding one, this
//! crate gives every user their own `TrustBoundaryId` and lets `origin` *be* the principal.
//!
//! That is not a workaround; it is what the existing model already means, and two real properties
//! fall out of it rather than having to be built:
//!
//! - **Revocation in one graph walk.** "Revoke everything this person could do" is already exactly
//!   `cap_revoke` on their root token, and it leaves every other principal untouched.
//! - **An audit trail that knows who.** `hyperion_explainability::ExplanationStore::begin` already
//!   seeds every record's `trust_boundary_span` with the calling boundary, and every read path
//!   (`trace_intent`, `get_by_action`, `resolve_why`) already *filters* by it. Binding a user to a
//!   boundary therefore makes every record attributable, and makes one user's records unreadable
//!   through another's token — with no change to that crate at all. It was already built to tell
//!   two callers apart; it had simply never been given two. `hyperion-console`'s `a2a` module
//!   states outright that it "never authenticates its caller, so honestly recording *who* isn't
//!   possible here"; for any caller that has a principal, it now is.
//!
//! Adding a `subject` field to the token instead would have put a second notion of authority beside
//! the one that already works, and left both properties above still to implement.
//!
//! ## The limit of that, stated exactly
//!
//! A boundary is **not** an unforgeable claim about which human acted. `CapabilityMonitor::
//! cap_derive` takes the child's `new_origin` as a parameter — it attenuates *rights*, and does not
//! inherit *origin* — so any holder of a live token can mint a child attributing itself to any
//! boundary it names, including another person's.
//!
//! This is deliberate and load-bearing, not an oversight to fix here: re-origining is precisely how
//! confinement happens everywhere else in this workspace. `PluginRegistry::install` derives a
//! plugin's tokens into a *fresh* boundary, and `hyperion-compat` derives into a session's. Making
//! `cap_derive` inherit the parent's origin would break the mechanism that isolates plugins.
//!
//! So a boundary is an **attribution and confinement label**, trustworthy exactly as far as the
//! code deriving tokens is trusted — which, under identity-without-authentication, is the same
//! thing this crate's own opening says: it separates people, it does not protect them from each
//! other. When authentication lands, what has to become true is narrower and checkable: a user's
//! *root* token is minted only by whatever authenticated them, and nothing else may mint one at a
//! user boundary. That is a property of the broker, not of `cap_derive`.

mod registry;
mod types;

pub use registry::{PrincipalRegistry, RegistryError};
pub use types::{Principal, UserId, UserIdError};
