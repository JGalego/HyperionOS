//! Hyperion's real text console, per
//! [docs/998-roadmap.md](../../../docs/998-roadmap.md) M7 stage 1: "a real utterance
//! typed at the real booted console produces a real Intent Graph, a real Agent invocation, and
//! real text output rendered to the real TTY." This crate is that pipeline; `main.rs` is only the
//! real stdin/stdout loop around it.
//!
//! ## What's real here vs. deferred, and why
//!
//! - Every step in [`ConsoleSession::handle_utterance`] calls a real, existing subsystem: a real
//!   `hyperion-intent::IntentEngine` (fully real, deterministic HTN matching -- not a mock), a
//!   real `hyperion-coordination::CoordinationSession` driving real
//!   `hyperion-agent-runtime::AgentRuntime` invocations for a decomposed plan, and a real
//!   `hyperion-workspace::WorkspaceCompiler` + `ModalityInterface::ScreenReader` projection for
//!   the actual rendered text.
//! - Only one HTN template exists yet (`hyperion-intent`'s own "launch my startup" shape); any
//!   other utterance becomes an undecomposed root Intent with no children, which
//!   `hyperion-coordination::create_session` would otherwise have nothing to allocate for (its
//!   task list comes from the root's children alone). Rather than silently do nothing for the
//!   common case, that path drives one real Agent invocation directly against the root goal
//!   itself, via the same real `AgentRuntime::spawn`/`invoke` calls `hyperion-coordination` uses
//!   internally -- see [`session::ConsoleSession::handle_utterance`]'s own docs.
//! - Stage 2 (a real compositor driving real pixels from a compiled `WorkspaceGraph`) is
//!   deliberately not attempted here -- the roadmap's own M7 text calls that out as "its own
//!   large sub-project," not to be blocked on. This crate renders through the real accessibility
//!   tree's `ScreenReader` projection precisely because that's real, existing, and exactly what a
//!   text-first console needs; no pixels are drawn anywhere.
//! - No real (or even mock) model inference is called here: `hyperion-intent`'s HTN matching is
//!   deliberately, permanently deterministic (not something M8 replaces), and `hyperion-model-
//!   router`/`hyperion-ai-runtime`'s mock inference backend is simply not on the critical path
//!   this milestone's exit criterion names ("a real Intent Graph, a real Agent invocation, and
//!   real text output") -- wiring a real model call into a future Agent capability is real,
//!   separate, follow-on work, most naturally motivated once M8 gives it something real to call.

mod graph_explorer;
pub mod peer_trust;
pub mod secret_input;
mod session;

pub use hyperion_turn::TaskProgress;
pub use session::ConsoleSession;
