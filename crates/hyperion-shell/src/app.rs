//! [`ShellApp`]: the real `eframe::App` that paints a [`crate::TurnOutcome`] -- panels laid out
//! by `RegionAffinity`, each `AccessibilityNode` rendered as a real, AccessKit-visible widget.

use std::sync::mpsc::{Receiver, Sender};

use hyperion_workspace::{AccessibilityNode, Panel, RegionAffinity};

use crate::{IntentSink, TurnOutcome};

/// Runs an [`IntentSink`] on its own thread so a slow turn (a real, blocking capability
/// dispatch) never freezes the window -- CLAUDE.md's "avoid blocking operations." `ShellApp`
/// only ever talks to it through `to_sink`/`from_sink`.
pub struct ShellApp {
    to_sink: Sender<String>,
    from_sink: Receiver<TurnOutcome>,
    input: String,
    current: Option<TurnOutcome>,
    busy: bool,
}

impl ShellApp {
    /// Takes ownership of `sink` and moves it onto a dedicated thread for this window's entire
    /// lifetime -- one thread, one sink, reused turn after turn (matching
    /// `hyperion_console::ConsoleSession`'s own "spawn once, reuse every turn" choice for its
    /// assistant Agent instance: a fresh sink per turn would lose exactly the same
    /// capability-grant/session state that crate's doc comment already names).
    pub fn spawn(sink: impl IntentSink + Send + 'static) -> Self {
        let (to_sink, utterances) = std::sync::mpsc::channel::<String>();
        let (outcomes, from_sink) = std::sync::mpsc::channel::<TurnOutcome>();
        std::thread::spawn(move || {
            let mut sink = sink;
            for utterance in utterances {
                let outcome = sink.handle_utterance(&utterance);
                if outcomes.send(outcome).is_err() {
                    break;
                }
            }
        });
        ShellApp {
            to_sink,
            from_sink,
            input: String::new(),
            current: None,
            busy: false,
        }
    }

    fn submit(&mut self, utterance: String) {
        if self.busy || utterance.trim().is_empty() {
            return;
        }
        self.busy = true;
        // The channel's receiver only ever drops if the sink's own thread has already exited
        // (e.g. it panicked) -- nothing left to do about that from here beyond staying idle,
        // which `self.busy` already leaves the UI in the moment `try_recv` never delivers a
        // reply.
        let _ = self.to_sink.send(utterance);
    }
}

impl eframe::App for ShellApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Ok(outcome) = self.from_sink.try_recv() {
            self.current = Some(outcome);
            self.busy = false;
        }

        // The always-present intent input -- this shell's visual equivalent of the console's
        // own `>` prompt, and the only input surface every other panel's own click handler
        // (below) also funnels through.
        egui::TopBottomPanel::bottom("intent_bar").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let field = ui.add_enabled(
                    !self.busy,
                    egui::TextEdit::singleline(&mut self.input)
                        .hint_text("Tell Hyperion what you'd like to accomplish...")
                        .desired_width(f32::INFINITY - 80.0),
                );
                let submitted_by_enter =
                    field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let submitted_by_button = ui
                    .add_enabled(!self.busy, egui::Button::new("Go"))
                    .clicked();
                if self.busy {
                    ui.spinner();
                }
                if submitted_by_enter || submitted_by_button {
                    let utterance = std::mem::take(&mut self.input);
                    self.submit(utterance);
                }
            });
            ui.add_space(4.0);
        });

        let panels: &[Panel] = self
            .current
            .as_ref()
            .and_then(|o| o.graph.as_ref())
            .map(|g| g.panels.as_slice())
            .unwrap_or(&[]);

        let mut activated: Option<String> = None;

        if panels.iter().any(|p| p.region_affinity == RegionAffinity::TopBar) {
            egui::TopBottomPanel::top("top_region").show(ctx, |ui| {
                render_region(ui, panels, RegionAffinity::TopBar, &mut activated);
            });
        }
        if panels.iter().any(|p| p.region_affinity == RegionAffinity::Left) {
            egui::SidePanel::left("left_region").show(ctx, |ui| {
                render_region(ui, panels, RegionAffinity::Left, &mut activated);
            });
        }
        if panels.iter().any(|p| p.region_affinity == RegionAffinity::Right) {
            egui::SidePanel::right("right_region").show(ctx, |ui| {
                render_region(ui, panels, RegionAffinity::Right, &mut activated);
            });
        }
        if panels.iter().any(|p| p.region_affinity == RegionAffinity::BottomBar) {
            egui::TopBottomPanel::bottom("bottom_region").show(ctx, |ui| {
                render_region(ui, panels, RegionAffinity::BottomBar, &mut activated);
            });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            if panels.iter().any(|p| p.region_affinity == RegionAffinity::Center) {
                render_region(ui, panels, RegionAffinity::Center, &mut activated);
            } else if let Some(outcome) = &self.current {
                // Fell out of the Intent Engine with no compiled workspace at all (a parse
                // error or a needs-clarification turn) -- show its own narration as plain
                // status text rather than a fabricated panel, per `TurnOutcome`'s own contract.
                for line in &outcome.narration {
                    ui.label(line);
                }
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Say what you'd like to accomplish.");
                });
            }
        });

        if let Some(utterance) = activated {
            self.submit(utterance);
        }

        // A turn in flight has nothing else driving repaints (no animation, no input) --
        // without this, `from_sink`'s reply could sit unnoticed until the next real user
        // interaction.
        if self.busy {
            ctx.request_repaint();
        }
    }
}

fn render_region(
    ui: &mut egui::Ui,
    panels: &[Panel],
    affinity: RegionAffinity,
    activated: &mut Option<String>,
) {
    for panel in panels.iter().filter(|p| p.region_affinity == affinity) {
        render_node(ui, &panel.accessibility_node, activated);
    }
}

/// One `AccessibilityNode` -> one real widget: a button (AccessKit exposes it with the real
/// `accessible_name` as its accessible label) if the node declares any real action, a plain
/// label otherwise. A click sends `accessible_name` back as a new utterance -- the exact same
/// phrase `Modality::Voice`'s own grammar maps to this node, so a click and a spoken command hit
/// identical code (see lib.rs's doc comment).
fn render_node(ui: &mut egui::Ui, node: &AccessibilityNode, activated: &mut Option<String>) {
    ui.group(|ui| {
        ui.set_min_size(egui::vec2(
            node.target_size.0 as f32,
            node.target_size.1 as f32,
        ));
        if node.is_interactive && !node.actions.is_empty() {
            let response = ui
                .add(egui::Button::new(&node.accessible_name))
                .on_hover_text(&node.description);
            if response.clicked() && activated.is_none() {
                *activated = Some(node.accessible_name.clone());
            }
        } else {
            ui.strong(&node.accessible_name);
            if !node.description.is_empty() {
                ui.label(&node.description);
            }
        }
    });
}
