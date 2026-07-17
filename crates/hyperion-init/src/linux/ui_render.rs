//! docs/998-roadmap.md M7 stage 2's own still-open gap, closed here: "a real compositor, real
//! `WorkspaceGraph` rasterization, real font/text rendering... remains real, separate, large
//! future work." This module is the minimal, bounded slice of exactly that: a real
//! [`hyperion_workspace::WorkspaceCompiler`] compiles one real, tiny `system.boot_status`
//! Capability UI Contract (the same real compiler path `hyperion-console`/`hyperion-shell`
//! project the exact same way) into a real [`WorkspaceGraph`], and [`rasterize_workspace`] draws
//! that graph's real `Panel`/`AccessibilityNode` tree — not a hardcoded pattern — onto a real
//! pixel buffer: one filled rectangle per panel positioned by its own `region_affinity`, with its
//! own `accessible_name` rasterized as real glyphs via `ab_glyph` (already this workspace's own
//! dependency, through `hyperion-shell`'s egui/epaint chain) over `epaint_default_fonts`' already-
//! vendored embedded TrueType bytes. Still explicitly not a compositor: no window management, no
//! input routing, no live re-render loop -- a single compile-then-rasterize pass, same bounded
//! shape as this crate's own `display_probe` mode-set proof.
//!
//! [`compile_boot_workspace`] is deliberately given its own scratch Knowledge Graph directory
//! (never the console's) rather than reusing this crate's own `console_data_dir` -- this compiles
//! once, at boot, before any console session exists, and never needs to be read again afterward.

use std::path::Path;
use std::sync::Arc;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_context::{Budget, ContextEngine, Scope};
use hyperion_knowledge_graph::KnowledgeGraph;
use hyperion_workspace::{
    CapabilityUiContract, ComplexityTier, Panel, RegionAffinity, WorkspaceCompiler, WorkspaceGraph,
};

/// Real BGRX background fills -- an interactive panel gets Hyperion's own violet accent (the
/// same hue `display_probe`'s old three-band pattern used for "Hyperion's own band"); a
/// non-interactive one (this boot splash's only real panel today) gets a dark neutral so white
/// text stays legible against it.
const PANEL_BG_INTERACTIVE: [u8; 4] = [0x4a, 0x2c, 0x6b, 0x00];
const PANEL_BG_STATIC: [u8; 4] = [0x20, 0x20, 0x20, 0x00];
const TEXT_FG: [u8; 3] = [0xff, 0xff, 0xff];

/// Compiles one real, minimal `WorkspaceGraph` for a `system.boot_status` Capability -- a real
/// Intent node, a real Context Bundle, and a real `WorkspaceCompiler::compile` call, exactly the
/// pipeline any other real caller in this workspace uses, never a hand-built `WorkspaceGraph`
/// literal standing in for one.
pub fn compile_boot_workspace(kg_dir: &Path) -> Result<WorkspaceGraph, String> {
    std::fs::create_dir_all(kg_dir).map_err(|e| format!("couldn't create {kg_dir:?}: {e}"))?;
    let kg_path = kg_dir.join("boot_workspace_kg.jsonl");
    let graph = KnowledgeGraph::open(&kg_path)
        .map_err(|e| format!("couldn't open a real Knowledge Graph at {kg_path:?}: {e}"))?;
    let graph = Arc::new(graph);

    let mut monitor = CapabilityMonitor::new();
    let boundary = TrustBoundaryId(1);
    let token = monitor.mint_root(RightsMask::all(), boundary, None);

    let intent_id = graph
        .put_node(
            &monitor,
            &token,
            None,
            "intent",
            None,
            serde_json::json!({ "predicate": "system.boot" }),
        )
        .map_err(|e| format!("couldn't create a real intent node: {e}"))?;

    let context_engine = ContextEngine::new(graph.clone());
    let scope = Scope {
        intent_id: intent_id.0.to_string(),
        session_id: "boot".to_string(),
        mentions: Vec::new(),
        anchors: Vec::new(),
    };
    let context_bundle = context_engine
        .assemble(&monitor, &token, &scope, Budget::default())
        .map_err(|e| format!("couldn't assemble a real Context Bundle: {e}"))?;

    let contracts = vec![CapabilityUiContract {
        capability_ref: "system.boot_status".to_string(),
        panel_template: "boot_status".to_string(),
        region_affinity: RegionAffinity::Center,
        min_size: (400, 120),
        priority: 1.0,
        binds_category: None,
        variants: Default::default(),
        accessible_role: Some("status".to_string()),
        label_template: Some("Hyperion is starting".to_string()),
        keyboard_operations: Vec::new(),
        alt_text_hook: None,
        contrast_ratio: 7.0,
        has_motion: false,
        reduced_motion_alternative: true,
        language_tag: "en-US".to_string(),
        emits_audio: false,
        has_visual_alert_equivalent: true,
    }];

    let compiler = WorkspaceCompiler::new();
    compiler
        .compile(
            &monitor,
            &token,
            intent_id,
            "system.boot",
            &contracts,
            &context_bundle,
            ComplexityTier::Beginner,
            1.0,
        )
        .map_err(|e| format!("couldn't compile a real WorkspaceGraph: {e}"))
}

#[derive(Clone, Copy)]
struct Rect {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

/// The whole-screen bounds of each of docs/13's five layout regions -- proportions mirror
/// `hyperion-shell::app::render_region`'s own region split, ported from egui panels to raw pixel
/// rectangles.
fn region_bounds(width: usize, height: usize) -> [(RegionAffinity, Rect); 5] {
    let top_h = height / 8;
    let bottom_h = height / 10;
    let side_w = width / 5;
    let mid_h = height.saturating_sub(top_h + bottom_h);
    [
        (
            RegionAffinity::TopBar,
            Rect {
                x: 0,
                y: 0,
                w: width,
                h: top_h,
            },
        ),
        (
            RegionAffinity::BottomBar,
            Rect {
                x: 0,
                y: height.saturating_sub(bottom_h),
                w: width,
                h: bottom_h,
            },
        ),
        (
            RegionAffinity::Left,
            Rect {
                x: 0,
                y: top_h,
                w: side_w,
                h: mid_h,
            },
        ),
        (
            RegionAffinity::Right,
            Rect {
                x: width.saturating_sub(side_w),
                y: top_h,
                w: side_w,
                h: mid_h,
            },
        ),
        (
            RegionAffinity::Center,
            Rect {
                x: side_w,
                y: top_h,
                w: width.saturating_sub(2 * side_w),
                h: mid_h,
            },
        ),
    ]
}

/// Every real panel this `WorkspaceGraph` carries, positioned within its own declared region --
/// panels sharing one region stack evenly along that region's cross axis (horizontally for the
/// two bars, vertically otherwise), so two panels in `Center` never overlap.
fn panel_rects(panels: &[Panel], width: usize, height: usize) -> Vec<(&Panel, Rect)> {
    let bounds = region_bounds(width, height);
    let mut out = Vec::new();
    for (region, rect) in bounds {
        let in_region: Vec<&Panel> = panels
            .iter()
            .filter(|p| p.region_affinity == region)
            .collect();
        if in_region.is_empty() {
            continue;
        }
        let stack_horizontal = matches!(region, RegionAffinity::TopBar | RegionAffinity::BottomBar);
        let n = in_region.len();
        for (i, panel) in in_region.into_iter().enumerate() {
            let sub = if stack_horizontal {
                Rect {
                    x: rect.x + rect.w * i / n,
                    y: rect.y,
                    w: rect.w / n,
                    h: rect.h,
                }
            } else {
                Rect {
                    x: rect.x,
                    y: rect.y + rect.h * i / n,
                    w: rect.w,
                    h: rect.h / n,
                }
            };
            out.push((panel, sub));
        }
    }
    out
}

fn fill_rect(buf: &mut [u8], width: usize, height: usize, rect: Rect, color: [u8; 4]) {
    let stride = width * 4;
    let x_end = (rect.x + rect.w).min(width);
    let y_end = (rect.y + rect.h).min(height);
    for row in rect.y..y_end {
        for col in rect.x..x_end {
            let offset = row * stride + col * 4;
            if offset + 4 <= buf.len() {
                buf[offset..offset + 4].copy_from_slice(&color);
            }
        }
    }
}

/// Rasterizes `text`'s real glyphs (via `ab_glyph`, no shaping/kerning beyond plain per-glyph
/// horizontal advance -- this is a boot splash, not a text editor) into `rect`, alpha-blended
/// over whatever `fill_rect` already drew there.
fn draw_text(buf: &mut [u8], width: usize, height: usize, rect: Rect, text: &str, font: &FontRef) {
    let stride = width * 4;
    let scale = PxScale::from((rect.h as f32 * 0.4).clamp(10.0, 32.0));
    let scaled = font.as_scaled(scale);
    let mut cursor_x = rect.x as f32 + 8.0;
    let baseline_y = rect.y as f32 + rect.h as f32 * 0.65;
    let right_edge = (rect.x + rect.w) as f32;

    for c in text.chars() {
        if cursor_x >= right_edge {
            break;
        }
        let glyph_id = scaled.glyph_id(c);
        let glyph = glyph_id.with_scale_and_position(scale, ab_glyph::point(cursor_x, baseline_y));
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, coverage| {
                if coverage <= 0.02 {
                    return;
                }
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                if px < 0 || py < 0 {
                    return;
                }
                let (px, py) = (px as usize, py as usize);
                if px >= width || py >= height || px >= rect.x + rect.w || py >= rect.y + rect.h {
                    return;
                }
                let offset = py * stride + px * 4;
                if offset + 4 > buf.len() {
                    return;
                }
                for (channel, fg) in TEXT_FG.iter().enumerate() {
                    let bg = buf[offset + channel] as f32;
                    buf[offset + channel] = (bg + (*fg as f32 - bg) * coverage).round() as u8;
                }
            });
        }
        cursor_x += scaled.h_advance(glyph_id);
    }
}

/// Draws `graph`'s real panels -- one filled rectangle per panel plus its own rasterized
/// `accessible_name` -- into `buf`, a real `width * height` XRGB8888 pixel buffer with a
/// `width * 4`-byte stride (the same layout the real DRM dumb buffer this is written into uses).
/// Never touches DRM/KMS itself, so this is directly unit-testable against a plain in-memory
/// buffer with no real display device required.
pub fn rasterize_workspace(buf: &mut [u8], width: u32, height: u32, graph: &WorkspaceGraph) {
    let width = width as usize;
    let height = height as usize;

    let font = match FontRef::try_from_slice(epaint_default_fonts::HACK_REGULAR) {
        Ok(font) => Some(font),
        Err(e) => {
            eprintln!(
                "[hyperion-init] DISPLAY: warning -- couldn't parse the embedded boot-splash \
                 font, drawing panel rectangles without text: {e:?}"
            );
            None
        }
    };

    for (panel, rect) in panel_rects(&graph.panels, width, height) {
        let bg = if panel.accessibility_node.is_interactive {
            PANEL_BG_INTERACTIVE
        } else {
            PANEL_BG_STATIC
        };
        fill_rect(buf, width, height, rect, bg);
        if let Some(font) = &font {
            draw_text(
                buf,
                width,
                height,
                rect,
                &panel.accessibility_node.accessible_name,
                font,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_boot_workspace_produces_one_real_center_status_panel() {
        let dir =
            tempfile::tempdir().expect("create a real tempdir for this test's Knowledge Graph");
        let graph = compile_boot_workspace(dir.path()).expect("compile a real WorkspaceGraph");

        assert_eq!(
            graph.panels.len(),
            1,
            "expected exactly one real compiled panel"
        );
        let panel = &graph.panels[0];
        assert_eq!(panel.region_affinity, RegionAffinity::Center);
        assert_eq!(
            panel.accessibility_node.accessible_name,
            "Hyperion is starting"
        );
        assert!(
            !panel.accessibility_node.is_interactive,
            "a bare status panel with no bindings/keyboard operations must not be interactive"
        );
    }

    #[test]
    fn rasterize_workspace_draws_real_background_and_real_glyphs_only_inside_the_panel_rect() {
        let dir =
            tempfile::tempdir().expect("create a real tempdir for this test's Knowledge Graph");
        let graph = compile_boot_workspace(dir.path()).expect("compile a real WorkspaceGraph");

        let (width, height) = (640u32, 480u32);
        let mut buf = vec![0u8; (width as usize) * (height as usize) * 4];
        rasterize_workspace(&mut buf, width, height, &graph);

        let rects = panel_rects(&graph.panels, width as usize, height as usize);
        assert_eq!(rects.len(), 1);
        let (_, rect) = rects[0];

        // Real background fill really happened inside the panel's own rect -- every corner
        // pixel differs from the all-zero buffer this test started from.
        let stride = (width as usize) * 4;
        let corner_offset = rect.y * stride + rect.x * 4;
        assert_ne!(
            &buf[corner_offset..corner_offset + 4],
            &[0, 0, 0, 0],
            "expected a real, non-zero background fill inside the panel rect"
        );

        // Real glyph rasterization happened somewhere inside that same rect -- at least one
        // pixel trends toward TEXT_FG's white, not just the flat background color, proving
        // ab_glyph really drew something, not just the rectangle fill.
        let mut saw_bright_pixel = false;
        for row in rect.y..(rect.y + rect.h).min(height as usize) {
            for col in rect.x..(rect.x + rect.w).min(width as usize) {
                let offset = row * stride + col * 4;
                if buf[offset] > 200 && buf[offset + 1] > 200 && buf[offset + 2] > 200 {
                    saw_bright_pixel = true;
                }
            }
        }
        assert!(
            saw_bright_pixel,
            "expected at least one real, bright (near-white) rasterized glyph pixel inside the \
             panel rect"
        );

        // Nothing was drawn outside any real panel's rect -- this doesn't blindly fill the
        // whole buffer the way the old three-band pattern did.
        assert_eq!(
            &buf[0..4],
            &[0, 0, 0, 0],
            "the top-left corner is outside every real panel's rect and must stay untouched"
        );
    }

    #[test]
    fn panels_sharing_one_region_stack_without_overlapping() {
        let base = Panel {
            panel_id: 0,
            capability_ref: "test.panel".to_string(),
            region_affinity: RegionAffinity::Center,
            min_size: (10, 10),
            priority: 1.0,
            bindings: Vec::new(),
            accessibility_node: hyperion_workspace::AccessibilityNode {
                node_id: 0,
                panel_ref: 0,
                role: "generic".to_string(),
                accessible_name: "panel".to_string(),
                description: String::new(),
                language_tag: "en".to_string(),
                target_size: (10, 10),
                is_interactive: false,
                has_motion: false,
                reduced_motion_alternative: true,
                contrast_ratio: 7.0,
                actions: Vec::new(),
                emits_audio: false,
                has_visual_alert_equivalent: true,
            },
            render_state: hyperion_workspace::RenderState::Ready,
        };
        let panels = vec![base.clone(), base];

        let rects = panel_rects(&panels, 640, 480);
        assert_eq!(rects.len(), 2);
        let (_, first) = rects[0];
        let (_, second) = rects[1];
        assert_eq!(
            first.y + first.h,
            second.y,
            "two stacked panels must not overlap vertically"
        );
    }
}
