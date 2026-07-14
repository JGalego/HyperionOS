//! Hyperion visual shell -- the first real rendering surface over
//! `hyperion-workspace`'s compiled UI.
//!
//! `hyperion-workspace` already compiles a real `WorkspaceGraph` (panels, lifecycle, an
//! `AccessibilityTree` derived in the same pass) from an Intent; `hyperion-console` already
//! proves that pipeline end to end by projecting it through `Modality::ScreenReader` onto a
//! real TTY. This crate is the visual sibling of that, not a second UI model: it renders the
//! same `WorkspaceGraph`/`AccessibilityTree`, and nothing about panel layout, roles, or names is
//! decided here -- only how they're painted and how a click turns back into an utterance.
//!
//! The one new real capability this crate adds is real, OS-level accessibility: `eframe`'s
//! `accesskit` feature exposes every rendered widget to NVDA/VoiceOver/Orca directly, so a
//! screen-reader user gets `hyperion-workspace`'s own accessible names/roles for real, not a
//! simulation of what a screen reader would say (which is what `hyperion-console`'s text
//! rendering necessarily is).
//!
//! Real: a full turn pipeline (Intent Engine -> Coordination or direct Agent invoke ->
//! `WorkspaceCompiler`) behind the [`IntentSink`] trait, an [`EmbeddedSession`] implementing it,
//! and a non-blocking [`ShellApp`] that lays out panels by `RegionAffinity` and turns a click on
//! an interactive node into the exact same utterance `Modality::Voice`'s grammar would recognize
//! for that node -- a visual click and a spoken command hit the identical code path.
//!
//! Deliberately deferred, and why:
//!
//! - **One shared turn-orchestrator crate.** [`EmbeddedSession`] is a deliberately trimmed
//!   sibling of `hyperion_console::ConsoleSession`'s pipeline, not a shared dependency --
//!   `hyperion-console` is a binary crate (nothing to depend on) and its own session module is
//!   under active concurrent development as of this writing. Once that work lands, the real
//!   utterance -> outcome -> `WorkspaceGraph` turn pipeline both crates need belongs in one
//!   modality-agnostic crate that each of `hyperion-console`/`hyperion-shell` calls, rather than
//!   two copies drifting apart. Tracked here, not solved here.
//! - **Cloud/local-engine backend selection, "connect my `<provider>`."** [`EmbeddedSession`]
//!   always runs `hyperion-ai-runtime`'s deterministic mock backend -- `hyperion-console` already
//!   solved real backend switching and cloud consent; re-deriving it here isn't this crate's
//!   contribution.
//! - **`/recall`/`/why`/`/related`-style Knowledge Graph browsing** -- no visual graph explorer
//!   yet, only live-turn workspace rendering.
//! - **Per-tick `Starting`/`Done` progress streaming** for a decomposed multi-task plan -- a
//!   turn's UI shows a busy spinner for the whole turn, not per-task progress (`IntentSink`
//!   returns once per turn, not once per coordination tick).
//! - **Motion and audio accessibility rules.** `AccessibilityNode::has_motion`/`emits_audio` are
//!   rendered as static content today (nothing here animates or emits audio), so the linter's
//!   motion-alternative/audio-alert rules stay real but unexercised by this crate specifically.

mod app;
mod session;

pub use app::ShellApp;
pub use hyperion_workspace::{AccessibilityNode, AccessibilityTree, Panel, WorkspaceGraph};
pub use session::EmbeddedSession;

/// The seam between this crate (rendering + input capture) and whatever owns the real
/// utterance -> outcome -> `WorkspaceGraph` turn pipeline. Kept as a trait, not a concrete
/// dependency on any one session type, so a future shared orchestrator (see this crate's own
/// doc comment) can implement it without [`ShellApp`] changing at all.
pub trait IntentSink {
    /// Runs one full turn. Never panics on a real failure along the way -- any error becomes a
    /// plain-language [`TurnOutcome::narration`] line instead (CLAUDE.md's "never expose
    /// technical errors directly"), the same contract
    /// `hyperion_console::ConsoleSession::handle_utterance` already holds itself to.
    fn handle_utterance(&mut self, utterance: &str) -> TurnOutcome;
}

/// One turn's real, renderable result.
///
/// `graph`/`tree` are `None` only when the turn never reached a compiled workspace at all (an
/// utterance the Intent Engine couldn't parse, or one needing clarification) -- there is no
/// `NodeId` in that case to compile a workspace against, so [`ShellApp`] falls back to showing
/// `narration` as plain status text rather than a fabricated panel.
pub struct TurnOutcome {
    pub graph: Option<hyperion_workspace::WorkspaceGraph>,
    pub tree: Option<hyperion_workspace::AccessibilityTree>,
    /// The same `Modality::ScreenReader` linearization `hyperion-console` renders as text --
    /// kept here too so a future history/log pane (or a real screen reader reading this crate's
    /// own window) has it without re-deriving it.
    pub narration: Vec<String>,
}
