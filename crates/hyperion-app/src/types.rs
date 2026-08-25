//! The app model: what tier an app is, what inputs it declares, and what a built one looks like.

use std::path::PathBuf;

/// Where an app sits on docs/998-roadmap.md's App Builder ladder.
///
/// Only the two tiers that are **really buildable today** exist as variants. T2 (durable state),
/// T3 (residency), T4 (an authenticated human surface) and T5 (composition) are named in the
/// roadmap and deliberately absent here: each needs a privilege this crate cannot currently
/// grant — a persistent `fs_scope`, a supervisor-owned lifetime, a brokered port plus a real
/// identity subsystem — and a variant that no code path can honestly construct would be a claim
/// this crate does not stand behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppTier {
    /// T0 — runs, produces its answer, exits. Declares no inputs.
    Answer,
    /// T1 — the same one-shot lifetime, but with typed, validated inputs.
    Tool,
}

impl AppTier {
    /// The human-facing name. Never "T0"/"T1" — a tier label is an implementation detail, and
    /// docs/01 is explicit that those do not belong in front of a person.
    pub fn label(self) -> &'static str {
        match self {
            AppTier::Answer => "answer",
            AppTier::Tool => "tool",
        }
    }

    /// Which tier a set of declared inputs implies. The tier is *derived*, never chosen by a
    /// caller: an app that takes inputs is a tool, and one that takes none is an answer. Letting
    /// a caller declare a tier that contradicts its own contract would create a second source of
    /// truth for the same fact.
    pub fn for_inputs(inputs: &[InputField]) -> Self {
        if inputs.is_empty() {
            AppTier::Answer
        } else {
            AppTier::Tool
        }
    }
}

/// The type of one declared input.
///
/// This exists so Hyperion can do three things it structurally could not when a contract's inputs
/// were a bare `Vec<String>`: prompt for a missing argument in words a person understands,
/// reject a wrong one *before* anything is spawned, and let the Context Engine fill one from what
/// it already knows rather than asking a person to repeat themselves (docs/06).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputKind {
    Text,
    Integer,
    Number,
    Boolean,
    /// A path *relative to the app's own sandbox directory*. Validation really rejects an
    /// absolute path and any `..` component — not as defence-in-depth theatre, but because such a
    /// path could only ever name something outside the one directory Landlock actually grants,
    /// so accepting it would guarantee a confusing failure inside the sandbox instead of an
    /// honest refusal outside it.
    Path,
    /// One of a fixed set. Empty choices are rejected at construction time by
    /// [`crate::contract::validate_contract`] — a choice of nothing can never be satisfied.
    Choice(Vec<String>),
}

impl InputKind {
    /// How to describe this kind to a person being asked to supply one.
    pub fn describe(&self) -> String {
        match self {
            InputKind::Text => "text".to_string(),
            InputKind::Integer => "a whole number".to_string(),
            InputKind::Number => "a number".to_string(),
            InputKind::Boolean => "yes or no".to_string(),
            InputKind::Path => "a file inside the app's own folder".to_string(),
            InputKind::Choice(options) => format!("one of: {}", options.join(", ")),
        }
    }
}

/// One declared input of a T1 app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputField {
    pub name: String,
    pub kind: InputKind,
    /// Plain language, shown when Hyperion asks a person for this value.
    pub description: String,
    pub required: bool,
}

/// Everything needed to build an app, as produced by whatever generated it — a model, a template,
/// or a human typing it directly. Deliberately inert data with no model dependency of its own:
/// generation is the caller's job, so this crate stays deterministic and really testable.
#[derive(Debug, Clone)]
pub struct AppDefinition {
    /// Lowercase `[a-z0-9_-]`, 1..=64 characters. Becomes both a capability id and a real
    /// directory name, so it is validated as an identifier rather than trusted — see
    /// [`crate::contract::validate_app_name`].
    pub name: String,
    /// The human goal this app exists to serve, in the person's own terms. Carried into the
    /// signed manifest (as the permission justification) so `/app <name>` can answer "why does
    /// this exist" from the signed record rather than a side file.
    pub goal: String,
    /// The principal building it (docs/998-roadmap.md §0, Decision 2). Everyone on the device can
    /// use the result; only this person can remove or rebuild it.
    pub owner: String,
    /// Whether it needs to keep anything between runs (App Builder T2). A real permission, not a
    /// hint: it decides whether the sandbox grants durable storage, and it puts the app through the
    /// SDK's own human-review gate.
    pub keeps_data: bool,
    /// The `engine_id` of an already-installed `Contribution::ExecutionEngine` that runs this
    /// script. Resolved through `hyperion_sdk::resolve_via_engine`, so an app installs and runs
    /// through the exact same `ImplementationKind::NativeBinary` path a hand-installed binary
    /// does — never a second execution mechanism.
    pub engine_id: String,
    /// The script's full source text.
    pub script: String,
    pub inputs: Vec<InputField>,
}

/// An app that really installed, as read back from the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledApp {
    pub name: String,
    /// Read back out of the signed contract, never from a side record -- so it cannot be edited
    /// without invalidating the manifest's signature.
    pub owner: String,
    /// Whether it keeps anything between runs (App Builder T2), read back from the same signed
    /// contract.
    pub keeps_data: bool,
    pub goal: String,
    pub tier: AppTier,
    pub inputs: Vec<InputField>,
    /// The capability id it installed as: `app.<name>`.
    pub capability_id: String,
    pub version: u32,
}

/// Where an app's script lives on disk. Distinct from the throwaway directory its *runs* happen
/// in: the script has to outlive every run, and the sandbox's own `fs_scope` deliberately does
/// not (see `PluginRegistry::invoke_native_binary`).
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
}

impl AppPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        AppPaths { root: root.into() }
    }

    /// This app's own directory. Safe to join a name onto only because
    /// [`crate::contract::validate_app_name`] has already rejected anything that is not a bare
    /// identifier.
    pub fn dir_for(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    pub fn script_for(&self, name: &str) -> PathBuf {
        self.dir_for(name).join("script")
    }

    /// Where a stateful app's durable data lives, for every person who has run it.
    ///
    /// Beneath the app root but outside `dir_for`, because the two have opposite lifetimes in one
    /// respect that matters: a rebuild rewrites the script and must *not* touch the data, while a
    /// removal must take both. Separate directories make that difference explicit instead of
    /// depending on which files a rebuild happens to overwrite.
    pub fn data_root(&self) -> PathBuf {
        self.root.join("data")
    }
}
