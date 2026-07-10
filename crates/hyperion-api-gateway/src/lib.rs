//! Hyperion L2 Platform Services — the API Gateway, Phase 9 third slice.
//!
//! Implements docs/26-apis.md's "thin, uniform gateway in front of five
//! subsystem servers": one auth path, one route dispatch, real backends
//! for three of the five subsystems (Intent, Knowledge Graph, Memory),
//! and the Capability Invocation path that is docs/24's Plugin Framework
//! and docs/25's SDK's shared runtime entry point — "26 is the thing a
//! published Capability's implementation is ultimately invoked through."
//!
//! Real: [`gateway::ApiGateway::authorize`]'s two-step check — live-
//! token verify via `hyperion-capability`'s real generation-based
//! revocation, then a scope match against this gateway's own grant table
//! (keyed by the token's real `TokenId`, never a parallel identity) —
//! matches docs/26 §3's "mints no separate identity model, it re-checks
//! the same tokens the kernel issues" exactly.
//! [`gateway::ApiGateway::submit_intent`]/`kg_query`/`kg_write`/
//! `memory_write` are real pass-throughs to the already-real
//! `hyperion-intent`/`hyperion-knowledge-graph`/`hyperion-memory` crates,
//! not mocks. [`gateway::ApiGateway::memory_erase`]/`memory_export`
//! implement docs/26 §3's explicit carve-out — bypassing the scope check
//! entirely for a user's own export/erase, per the doc's own words, not
//! merely widening it. [`gateway::ApiGateway::invoke_capability`]
//! implements docs/26 §4's `invokeCapability` pseudocode: registry
//! lookup → candidate selection → dispatch → (on failure) retry against
//! the next-best candidate → explain-then-commit via
//! `hyperion-explainability`, exactly the doc's own bundled-unit framing
//! ("also handles token check, sandbox creation, and explainability
//! recording as a bundled unit").
//!
//! Deliberately deferred, and why:
//!
//! - **The Context API entirely.** Wiring `hyperion-context`'s richer
//!   `ContextBundle`/subscription-delta shape faithfully was judged, at
//!   this crate's scope, to add more risk of a subtly wrong integration
//!   than value — three of five subsystems (Intent, KG, Memory) are real
//!   here; Context is left for a follow-up rather than a rushed fourth.
//! - **Real Model Router (23) selection.** `invoke_capability` picks the
//!   highest `quality_score` candidate directly from the registry —
//!   `hyperion-model-router` has its own `ImplementationDescriptor`/
//!   `CapabilityInvocation` shapes with no adapter from
//!   `hyperion-plugin-framework`'s registry yet; docs/26 only requires
//!   this gateway to *call* `modelRouter.select`, not own the algorithm,
//!   so a real bridge is future integration work, not a Phase 9 blocker.
//! - **Real per-Capability dispatch.** `invoke_capability` calls
//!   `hyperion_agent_runtime::dispatch_stub_capability` — the same stub
//!   dispatch first-party Capabilities have used since Phase 4 — rather
//!   than a real callable registered per `ImplementationDescriptor`. The
//!   Plugin Framework's registry stores *descriptors*, not callables;
//!   giving every plugin a genuinely distinct runnable is deferred to
//!   whichever future phase builds real out-of-process Capability
//!   execution.
//! - **A canonical HTTP/WebSocket wire format.** Every route here is an
//!   in-process Rust method, not `POST /kg/write` over a real listener —
//!   docs/26 itself writes these as HTTP verbs, but this hosted simulator
//!   has no real network; `RawRequest`/`RawResponse` framing is not
//!   modeled since nothing serializes across a wire.
//! - **Rate/quota enforcement.** Named only as a diagram box in docs/26,
//!   with no algorithm given — not implemented.
//! - **API schema versioning / cross-version compatibility.** Docs/26
//!   states the invariant (an old-schema request must be served or
//!   typed-rejected, never silently misinterpreted) but gives no
//!   mechanism — no version field exists on any request type here.

mod gateway;
mod types;

pub use gateway::ApiGateway;
pub use types::{
    ApiError, ApiScope, InvokeRequest, InvokeResponse, SubmitIntentRequest, SubmitIntentResponse,
};
