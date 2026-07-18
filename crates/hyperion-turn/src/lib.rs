//! The shared, modality-agnostic utterance -> outcome -> `WorkspaceGraph` turn pipeline --
//! `hyperion-shell`'s own previously-named "One shared turn-orchestrator crate" gap ("Tracked
//! here, not solved here"): `hyperion_shell::EmbeddedSession` was "a deliberately trimmed sibling
//! of `hyperion_console::ConsoleSession`'s pipeline, not a shared dependency," with the exact
//! same real state-machine (Intent Engine -> Coordination or a direct Agent invoke ->
//! `WorkspaceCompiler`) hand-copied into both crates. This crate is that one real pipeline;
//! `hyperion-console`/`hyperion-shell` each call it instead of maintaining their own copy.
//!
//! Deliberately scoped to what was genuinely, mechanically identical between the two existing
//! copies, confirmed by diffing them line for line rather than assumed:
//!
//! - [`start_turn`] shares `IntentEngine::handle_utterance` -> `HandleOutcome` match ->
//!   `IntentEngine::submit` -> the docs/06 Adaptive Complexity `CapabilityTierReach` signal push --
//!   every early-return narration in [`TurnStart::Reply`] was byte-identical wording in both
//!   crates (parse failure, submit failure, ambiguous-mention clarification); only the
//!   `think`-mode pause differed (`hyperion-console` supports `/think-proceed` resumption and
//!   needs the paused root back, `hyperion-shell` doesn't), so [`TurnStart::PendingThink`] stays
//!   its own variant rather than being folded into `Reply`.
//! - [`drive_decomposed_plan`]/[`drive_ticks_to_completion`] share the real
//!   `CoordinationSession::create_session`/`allocate`/`get_plan` tick loop, parameterized by an
//!   `on_progress` callback (a caller with no progress UI of its own, like `hyperion-shell` today,
//!   passes a no-op closure -- exactly how `hyperion_console::ConsoleSession::handle_utterance`'s
//!   own default (no-progress) entry point already parameterizes the identical call) and a
//!   `render_task_detail` closure (each caller's own real detail-rendering differs on purpose --
//!   `hyperion-console`'s points at its own `/result <task>` command, `hyperion-shell` has no
//!   such command to point at).
//! - [`render_workspace`]/[`contract_for`]/[`empty_context_bundle`] share the real Context Bundle
//!   assembly + `WorkspaceCompiler::compile`/`mount`/`get_template` + `Modality::ScreenReader`
//!   projection sequence -- identical in both down to the exact fallback-on-failure narration.
//!
//! Deliberately **not** shared, because the two real copies didn't just differ syntactically --
//! they did genuinely different things:
//!
//! - The undecomposed-goal dispatch itself. `hyperion-console`'s own real, backend-switchable
//!   Capability selection, real cloud-consent gating, conversation-history prefixing, and URL-shaped
//!   web-research routing is real, substantial business logic with no equivalent in
//!   `hyperion-shell`, which always dispatches one fixed `"assistant.respond"` call under a fixed
//!   mock backend (see that crate's own doc comment on why). Forcing both into one shared function
//!   would mean either a parameter list standing in for half of `hyperion-console`'s own consent/
//!   backend state, or silently dropping that real functionality -- neither is a genuine
//!   unification, just a different kind of duplication.
//! - Meta-commands, `/redo`, secret input, and cloud-consent continuation -- all real,
//!   `hyperion-console`-only UX with no `hyperion-shell` equivalent to unify with.
//! - Per-caller detail/label rendering (`hyperion-console`'s `/result`-hinting preview vs.
//!   `hyperion-shell`'s plain truncation) -- real, deliberate UX differences, not incidental
//!   duplication, left as caller-supplied closures/post-processing rather than forced to agree.

use hyperion_capability::{CapabilityMonitor, CapabilityToken};
use hyperion_context::{
    Budget, CapabilityTierReach, ContextBundle, ContextEngine, ExpertiseEstimate, ExpertiseLevel,
    ExpertiseSignal, Scope,
};
use hyperion_coordination::{CoordinationSession, TaskNode};
use hyperion_intent::{ExecutionTicket, HandleOutcome, IntentEngine};
use hyperion_knowledge_graph::NodeId;
use hyperion_workspace::{
    project, AccessibilityTree, CapabilityUiContract, ComplexityTier, Modality, ModalityInterface,
    RegionAffinity, WorkspaceCompiler, WorkspaceGraph,
};
use std::collections::HashMap;

/// One real outcome (an HTN task, or the single undecomposed goal) about to become one real
/// Workspace panel.
#[derive(Debug, Clone)]
pub struct TaskOutcome {
    pub predicate: String,
    pub detail: String,
}

/// A real per-tick progress event from [`drive_decomposed_plan`]'s own tick loop -- `Starting`
/// fires *before* a tick's real (potentially slow, blocking) dispatch, naming every task about to
/// run concurrently; `Done` fires once per task after that same blocking call returns. A caller
/// with no progress UI passes a no-op `&mut |_| {}` closure rather than this type ever being
/// optional itself, matching `hyperion_console::ConsoleSession::handle_utterance`'s own existing
/// convention for its no-progress entry point.
#[derive(Debug, Clone)]
pub enum TaskProgress {
    Starting(Vec<String>),
    Done(String),
}

/// What resolving an utterance into a real `ExecutionTicket` produced -- see this crate's own doc
/// comment for why `PendingThink` alone stays a distinct variant instead of folding into `Reply`.
pub enum TurnStart {
    /// A real Intent was submitted and is ready to dispatch -- `root`/`ticket` are what
    /// [`drive_decomposed_plan`] (a decomposed plan) or a caller's own undecomposed-goal dispatch
    /// (`ticket.ready_leaves.is_empty()`) need next.
    Ready {
        root: NodeId,
        ticket: ExecutionTicket,
    },
    /// A `think`-mode utterance paused before committing. `root` is the paused-but-unsubmitted
    /// Intent -- a caller supporting `/think-proceed`-style resumption stores it; one that
    /// doesn't can treat this like `Reply` with its own narration instead.
    PendingThink { root: NodeId },
    /// Every other early-return case: real narration wording byte-identical across both of this
    /// crate's real callers (a parse failure, a submit failure, or an ambiguous-mention
    /// clarification), rendered once here rather than duplicated.
    Reply(Vec<String>),
}

/// The real, shared first half of a turn: `IntentEngine::handle_utterance` -> match the real
/// `HandleOutcome` -> `IntentEngine::submit`, plus docs/06's own Adaptive Complexity
/// `CapabilityTierReach` signal push (a real, useful signal for *either* modality this pipeline
/// serves, not `hyperion-console`-specific, so it's pushed here unconditionally rather than left
/// to each caller to remember).
pub fn start_turn(
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    intent_engine: &IntentEngine,
    context: &ContextEngine,
    utterance: &str,
    session_id: &str,
) -> TurnStart {
    let outcome = match intent_engine.handle_utterance(monitor, token, utterance, session_id) {
        Ok(outcome) => outcome,
        Err(e) => return TurnStart::Reply(vec![format!("I couldn't understand that: {e}")]),
    };

    let root = match outcome {
        HandleOutcome::Submitted(root) => root,
        HandleOutcome::PendingThink(root) => return TurnStart::PendingThink { root },
        HandleOutcome::NeedsClarification {
            mention,
            candidates,
        } => {
            return TurnStart::Reply(vec![format!(
                "I'm not sure which \"{mention}\" you mean ({} possibilities) -- could you be \
                 more specific?",
                candidates.len()
            )])
        }
    };

    let ticket = match intent_engine.submit(monitor, token, root) {
        Ok(ticket) => ticket,
        Err(e) => {
            return TurnStart::Reply(vec![format!(
                "I understood that, but couldn't act on it: {e}"
            )])
        }
    };

    let tier_reach = if ticket.ready_leaves.is_empty() {
        CapabilityTierReach::RawApi
    } else {
        CapabilityTierReach::GuidedWorkflow
    };
    context.record_expertise_signal(session_id, ExpertiseSignal::CapabilityTierReach(tier_reach));

    TurnStart::Ready { root, ticket }
}

/// The real, shared result of [`drive_decomposed_plan`] -- `session_id` is the real
/// `hyperion-coordination` session this call created, `None` only when `create_session` itself
/// failed (nothing to tick or read back). A caller that needs to act on this exact plan again
/// later (`hyperion_console::ConsoleSession`'s own real `/redo <task>` re-run) keeps it; one that
/// doesn't (`hyperion-shell`) simply never reads the field.
pub struct DecomposedPlanOutcome {
    pub session_id: Option<u64>,
    pub predicate: String,
    pub outcomes: Vec<TaskOutcome>,
}

/// The real, shared decomposed-plan path: `hyperion-coordination`'s own dependency-respecting
/// allocator, ticked to completion via [`drive_ticks_to_completion`], then read back into real
/// [`TaskOutcome`]s via `render_task_detail` (each caller's own real detail-rendering -- see this
/// crate's own doc comment on why that stays caller-supplied).
pub fn drive_decomposed_plan(
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    coordination: &CoordinationSession,
    intent_engine: &IntentEngine,
    ticket: &ExecutionTicket,
    on_progress: &mut dyn FnMut(TaskProgress),
    render_task_detail: impl Fn(&TaskNode) -> String,
) -> DecomposedPlanOutcome {
    let session_id = match coordination.create_session(monitor, token, intent_engine, ticket) {
        Ok(id) => id,
        Err(e) => {
            return DecomposedPlanOutcome {
                session_id: None,
                predicate: "plan".to_string(),
                outcomes: vec![TaskOutcome {
                    predicate: "plan".to_string(),
                    detail: format!("failed to start coordinating this plan: {e}"),
                }],
            }
        }
    };

    drive_ticks_to_completion(monitor, token, coordination, session_id, on_progress);

    let outcomes = match coordination.get_plan(monitor, token, session_id) {
        Ok(plan) => plan
            .nodes
            .iter()
            .map(|node| TaskOutcome {
                predicate: node.description.clone(),
                detail: render_task_detail(node),
            })
            .collect(),
        Err(e) => vec![TaskOutcome {
            predicate: "plan".to_string(),
            detail: format!("plan completed but couldn't be read back: {e}"),
        }],
    };

    DecomposedPlanOutcome {
        session_id: Some(session_id),
        predicate: "plan".to_string(),
        outcomes,
    }
}

/// Drives every dependency wave of `session_id`'s own real plan to completion -- each `allocate`
/// call is one real tick (its own ready tasks dispatched concurrently, not one at a time -- see
/// `CoordinationSession::allocate`'s own doc comment); an empty result means every task is Done,
/// Failed, or Blocked with nothing left ready. Exposed publicly (not just called from
/// [`drive_decomposed_plan`]) so a caller re-running a single already-amended task --
/// `hyperion_console::ConsoleSession::redo_task`'s own real `/redo` -- shares this exact tick loop
/// too, starting from a different plan state rather than a fresh `create_session`.
pub fn drive_ticks_to_completion(
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    coordination: &CoordinationSession,
    session_id: u64,
    on_progress: &mut dyn FnMut(TaskProgress),
) {
    loop {
        match coordination.ready_task_descriptions(monitor, token, session_id) {
            Ok(ready) if !ready.is_empty() => on_progress(TaskProgress::Starting(ready)),
            _ => {}
        }

        match coordination.allocate(monitor, token, session_id) {
            Ok(records) if records.is_empty() => break,
            Ok(records) => {
                if let Ok(plan) = coordination.get_plan(monitor, token, session_id) {
                    for record in &records {
                        if let Some(node) = plan.nodes.iter().find(|n| n.task_id == record.task_id)
                        {
                            on_progress(TaskProgress::Done(format!(
                                "  {}: {:?}",
                                node.description, node.status
                            )));
                        }
                    }
                }
            }
            Err(_) => break,
        }
    }
}

/// The real, fully-compiled result of [`render_workspace`] -- a caller keeps `graph`/`tree` when
/// it has somewhere to paint them (`hyperion-shell`), or discards them and keeps only `narration`
/// when it doesn't (`hyperion-console`'s text-only rendering).
pub struct RenderedTurn {
    pub graph: WorkspaceGraph,
    pub tree: AccessibilityTree,
    pub narration: Vec<String>,
}

/// The real, shared literal deliverable both callers need: a real Context Bundle assembly
/// (falling back to [`empty_context_bundle`] if assembly itself fails, since a missing context
/// signal shouldn't block rendering the real outcome a user is actually waiting on) feeding
/// `WorkspaceCompiler::compile` + `mount` + `get_template`, projected through the real
/// `Modality::ScreenReader` linearization -- the same accessible narration both a screen reader
/// and a text-first console need.
#[allow(clippy::too_many_arguments)]
pub fn render_workspace(
    monitor: &CapabilityMonitor,
    token: &CapabilityToken,
    context: &ContextEngine,
    workspace: &WorkspaceCompiler,
    root: NodeId,
    session_id: &str,
    predicate: &str,
    contracts: &[CapabilityUiContract],
) -> Result<RenderedTurn, String> {
    let scope = Scope {
        intent_id: root.0.to_string(),
        session_id: session_id.to_string(),
        mentions: Vec::new(),
        anchors: Vec::new(),
    };
    let bundle = context
        .assemble(monitor, token, &scope, Budget::default())
        .unwrap_or_else(|_| empty_context_bundle(scope));

    let graph = workspace
        .compile(
            monitor,
            token,
            root,
            predicate,
            contracts,
            &bundle,
            ComplexityTier::Beginner,
            1.0,
        )
        .map_err(|e| format!("(couldn't render a workspace for this: {e})"))?;
    let _ = workspace.mount(monitor, token, graph.graph_id);

    let template = workspace
        .get_template(predicate, contracts, ComplexityTier::Beginner)
        .expect("the template just compiled above is always cached under this same key");

    let narration = match project(&template.accessibility_tree, Modality::ScreenReader) {
        ModalityInterface::ScreenReader(lines) => lines,
        other => unreachable!(
            "project(_, Modality::ScreenReader) always returns ScreenReader, got {other:?}"
        ),
    };

    Ok(RenderedTurn {
        graph,
        tree: template.accessibility_tree,
        narration,
    })
}

/// A minimal, real `CapabilityUiContract` for one turn outcome's own status panel -- identical in
/// both of this crate's real callers before this extraction.
pub fn contract_for(capability_ref: &str, label: &str) -> CapabilityUiContract {
    CapabilityUiContract {
        capability_ref: capability_ref.to_string(),
        panel_template: format!("{capability_ref}.default"),
        region_affinity: RegionAffinity::Center,
        min_size: (200, 200),
        priority: 0.5,
        binds_category: None,
        variants: HashMap::new(),
        accessible_role: Some("status".to_string()),
        label_template: Some(label.to_string()),
        keyboard_operations: vec!["activate".to_string()],
        alt_text_hook: None,
        contrast_ratio: 7.0,
        has_motion: false,
        reduced_motion_alternative: true,
        language_tag: "en".to_string(),
        emits_audio: false,
        has_visual_alert_equivalent: true,
    }
}

/// A real, valid, empty `ContextBundle` -- [`render_workspace`]'s own fallback when a real Context
/// Bundle assembly fails, so a missing context signal never blocks rendering the real outcome a
/// user is actually waiting on.
pub fn empty_context_bundle(scope: Scope) -> ContextBundle {
    ContextBundle {
        bundle_id: 0,
        scope,
        entries: Vec::new(),
        assembled_at: 0,
        budget: Budget::default(),
        expertise_signal: ExpertiseEstimate {
            domain: "general".to_string(),
            level: ExpertiseLevel::Novice,
            evidence: Vec::new(),
            confidence: 0.0,
        },
    }
}
