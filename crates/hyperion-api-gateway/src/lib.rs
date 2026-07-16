//! Hyperion L2 Platform Services — the API Gateway, Phase 9 third slice.
//!
//! Implements docs/26-apis.md's "thin, uniform gateway in front of five
//! subsystem servers": one auth path, one route dispatch, real backends
//! for four of the five subsystems (Intent, Knowledge Graph, Memory,
//! Context), and the Capability Invocation path that is docs/24's Plugin
//! Framework and docs/25's SDK's shared runtime entry point — "26 is the
//! thing a published Capability's implementation is ultimately invoked
//! through."
//!
//! Real: [`gateway::ApiGateway::authorize`]'s two-step check — live-
//! token verify via `hyperion-capability`'s real generation-based
//! revocation, then a scope match against this gateway's own grant table
//! (keyed by the token's real `TokenId`, never a parallel identity) —
//! matches docs/26 §3's "mints no separate identity model, it re-checks
//! the same tokens the kernel issues" exactly.
//! [`gateway::ApiGateway::submit_intent`]/`kg_query`/`kg_write`/
//! `memory_write`/`context_assemble` are real pass-throughs to the
//! already-real `hyperion-intent`/`hyperion-knowledge-graph`/
//! `hyperion-memory`/`hyperion-context` crates, not mocks —
//! `context_assemble` shares the exact same `ContextEngine` instance the
//! caller already threads into `IntentEngine::new`, rather than a second,
//! disconnected one, so its working-set hysteresis genuinely reflects
//! the same context Intent grounding sees. [`gateway::ApiGateway::memory_erase`]/`memory_export`
//! implement docs/26 §3's explicit carve-out — bypassing the scope check
//! entirely for a user's own export/erase, per the doc's own words, not
//! merely widening it. [`gateway::ApiGateway::invoke_capability`]
//! implements docs/26 §4's `invokeCapability` pseudocode: registry
//! lookup → real `hyperion-model-router` selection (via
//! [`router_bridge`]'s adapter — see below for exactly what it does and
//! doesn't carry) → dispatch → (on failure) report the failure to the
//! Model Router's real circuit breaker and retry against its
//! `fallback_chain` → explain-then-commit via `hyperion-explainability`,
//! exactly the doc's own bundled-unit framing ("also handles token
//! check, sandbox creation, and explainability recording as a bundled
//! unit"). [`router_bridge::to_router_descriptor`] is the adapter this
//! crate's and `hyperion-model-router`'s doc comments both name as
//! missing — it converts a `hyperion_plugin_framework::ImplementationDescriptor`
//! into the shape `hyperion_model_router::ModelRouter::register_implementation`
//! expects, reusing the real `quality_score` as the router's
//! `quality_profile` entry, so a third-party Capability and a first-party
//! equivalent genuinely compete on the Model Router's real weighted
//! scoring, not a placeholder sort — the Phase 9 exit criterion this
//! crate previously only satisfied at the registry level. The bridge
//! lives in the gateway (not either subsystem crate) because
//! `hyperion-model-router`'s own doc comment explicitly doesn't want a
//! dependency on the Plugin Framework, and the gateway already depends
//! on both. [`gateway::ApiGateway::invoke_capability`] also now calls
//! the real `hyperion-security` Risk-Assessment Engine
//! (`hyperion_security::assess_and_prepare`) against caller-supplied
//! [`RiskHints`] before dispatch: an action assessed at
//! `RequireExplicitConfirm` or above is rejected with
//! [`ApiError::ConfirmationRequired`] unless [`InvokeRequest::confirmed`]
//! is set, and `RequireBackupFirst` additionally gets a real
//! `hyperion-recovery` recovery point created synchronously and attached
//! to the action's Explanation Record via
//! `hyperion_explainability::ExplanationStore::attach_undo_ref` — closing
//! the loop `attach_undo_ref`'s own doc comment names
//! (`record.undo_ref = risk.recovery_point_ref`) and completing Phase 8's
//! literal exit criterion ("a risky action... correctly triggers
//! backup-then-confirm") in the one real production call site that
//! reaches `hyperion-explainability` at all. The risk-assessment
//! rationale is also recorded as a real `ReasoningStep` via `append_step`
//! — this integration's one production caller had previously only ever
//! exercised `begin`/`transition`. The real routing decision's own
//! `chosen_reason` is recorded as a second `ReasoningStep`, and
//! [`router_bridge::to_confidence_and_alternatives`] turns the winning
//! candidate's real composite fitness score and every other considered/
//! excluded candidate into a real `set_confidence` call — the winning
//! score genuinely is a confidence-shaped signal (unlike the risk score,
//! see below), so this is the first real `set_confidence` caller in the
//! workspace.
//!
//! [`gateway::ApiGateway::verify_with_ensemble`] (2026-07-16) closes `hyperion-model-router`'s
//! own previously-named "ensemble/verification dispatch" gap (docs/23 §Algorithms 5) — real for
//! the first time, and deliberately here rather than in that crate: `hyperion-model-router` is
//! "a decision, never an execution," so once its own `route()` says a call
//! (`Rationale::needs_verification`) needs it, this gateway — the real place invocation already
//! happens — dispatches a real, architecturally distinct second candidate (a different
//! `hyperion_model_router::ImplKind` than the primary) via the same real
//! [`gateway::ApiGateway::dispatch_one`] every ordinary dispatch already uses, and reconciles.
//! Real agreement (identical real outputs) genuinely boosts the recorded confidence — a second,
//! superseding `set_confidence` call tagged `ConfidenceMethod::Ensemble` — never just an
//! assertion. Real disagreement is never silently resolved: this crate has no designated
//! tiebreaker to consult, so it surfaces as [`ApiError::EnsembleDisagreement`], carrying both
//! real outputs, rather than one being discarded. Fails open (no ensemble dispatch at all) when
//! there's no architecturally distinct candidate to verify against, or when the verifying
//! candidate itself can't run — the primary's already-successful result is never blocked on a
//! verification that can't happen.
//!
//! Deliberately deferred, and why:
//!
//! - **Deriving `RiskHints` from real signals.** The gateway still takes
//!   `scope_size`/`reversible`/`sensitivity`/`intent_confidence`/
//!   `corroboration`/`provenance` as caller-supplied hints (matching
//!   `hyperion-security::PendingAction`'s own framing) rather than
//!   deriving them from the actual request — e.g. blast radius from a
//!   real object-touch count, or provenance taint from the real Context/
//!   Intent chain. `hyperion-security` itself defers the classifiers;
//!   this gateway integration doesn't build them either.
//! - **Reusing the risk-assessment score as confidence.** A risk
//!   *composite score* and a decision *confidence score* are different
//!   signals — reporting the former as the latter would misrepresent
//!   what the record means. `set_confidence` is wired here from the
//!   Model Router's own routing score instead, which is a genuine
//!   confidence-shaped signal, precisely to avoid that conflation.
//! - **`resolve_entity`/`expand`/`explain`/`current_expertise`.**
//!   [`gateway::ApiGateway::context_assemble`] wires the one method
//!   docs/26 §2's Context API actually needs (`assemble`); the rest of
//!   `hyperion-context`'s surface isn't part of that API shape and stays
//!   unexposed through the gateway.
//! - **`urgency_class` feeding the Model Router.**
//!   [`router_bridge::build_invocation`] now derives `consequence_tier`
//!   from the real `hyperion-security::RiskAssessment` this same call
//!   already computes, but `urgency_class` stays a fixed `Interactive` —
//!   deliberately, not by omission. See [`router_bridge::build_invocation`]'s
//!   own doc comment for why: `invoke_capability` is always a synchronous,
//!   blocking call, so `Interactive` is already objectively correct
//!   rather than a placeholder. ~~`cloud_consent` stays a fixed `true`~~ (2026-07-16) — now
//!   real when [`gateway::ApiGateway::new_with_consent_ledger`] is used:
//!   [`router_bridge::build_invocation_with_consent`] checks a real, live
//!   `hyperion-privacy::ConsentLedger` standing grant, never assuming consent — the same
//!   `ConsentedCloudUpgrade` check `hyperion-scalability::degrade::degrade_capability` already
//!   established as this workspace's convention. This is *not* the "rewiring
//!   `hyperion-model-router`'s already-shipped privacy gate onto `ConsentLedger`" migration
//!   `hyperion-privacy`'s own crate doc asks not to be done as an incidental side effect —
//!   `hyperion-model-router`'s own two-value `PrivacyTier` gate is untouched; only the plain
//!   `cloud_consent: bool` value fed into it becomes real, from this gateway's own new
//!   integration seam (it already depends on both crates), exactly the path that same doc
//!   comment invites. Every caller still using plain [`gateway::ApiGateway::new`] keeps the
//!   unchanged, permissive `true` default.
//! - ~~Per-implementation privacy tier from the Plugin Framework manifest~~ (2026-07-16) — now
//!   real: `hyperion_plugin_framework::CapabilityManifest`/`ImplementationDescriptor` both gained
//!   a real `privacy_tier` field, and [`router_bridge::to_router_descriptor`] reads
//!   `descriptor.privacy_tier` via a new `to_router_privacy_tier` adapter instead of hardcoding
//!   `PrivacyTier::Local` for every bridged candidate. `hyperion-sdk`'s own publish pipeline is
//!   the real first populator: `Implementation.requires_consent` (previously folded only into
//!   `package_hash`'s canonical bytes, never acted on) now maps straight to `ConsentedCloud`.
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
mod router_bridge;
mod types;

pub use gateway::ApiGateway;
pub use hyperion_context::{Budget, ContextBundle, Scope};
pub use types::{
    ApiError, ApiScope, InvokeRequest, InvokeResponse, RiskHints, SubmitIntentRequest,
    SubmitIntentResponse,
};
