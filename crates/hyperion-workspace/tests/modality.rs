//! docs/14-accessibility.md §5.3: every modality projects from the same
//! `AccessibilityTree` — "click submit" and "submit my answer" resolve to
//! the same node.

use hyperion_workspace::{
    project, AccessibilityNode, AccessibilityTree, Modality, ModalityInterface,
};

fn tree() -> AccessibilityTree {
    let submit = AccessibilityNode {
        node_id: 1,
        panel_ref: 1,
        role: "button".to_string(),
        accessible_name: "Submit".to_string(),
        description: "Submits the form".to_string(),
        language_tag: "en".to_string(),
        target_size: (48, 48),
        is_interactive: true,
        has_motion: false,
        reduced_motion_alternative: true,
        contrast_ratio: 7.0,
        actions: vec!["submit my answer".to_string()],
        emits_audio: false,
        has_visual_alert_equivalent: true,
    };
    let heading = AccessibilityNode {
        node_id: 2,
        panel_ref: 1,
        role: "heading".to_string(),
        accessible_name: "Question 3".to_string(),
        description: String::new(),
        language_tag: "en".to_string(),
        target_size: (0, 0),
        is_interactive: false,
        has_motion: false,
        reduced_motion_alternative: true,
        contrast_ratio: 7.0,
        actions: Vec::new(),
        emits_audio: false,
        has_visual_alert_equivalent: true,
    };
    AccessibilityTree {
        tree_id: 1,
        workspace_graph_id: 1,
        nodes: vec![heading.clone(), submit.clone()],
        focus_order: vec![heading.node_id, submit.node_id],
    }
}

#[test]
fn screen_reader_projection_linearizes_in_focus_order() {
    let ModalityInterface::ScreenReader(lines) = project(&tree(), Modality::ScreenReader) else {
        panic!("expected ScreenReader");
    };
    assert_eq!(lines, vec!["heading: Question 3", "button: Submit"]);
}

#[test]
fn voice_grammar_maps_the_accessible_name_and_every_declared_action_to_the_same_node() {
    let ModalityInterface::Voice(grammar) = project(&tree(), Modality::Voice) else {
        panic!("expected Voice");
    };
    assert_eq!(grammar.get("submit"), Some(&1));
    assert_eq!(
        grammar.get("submit my answer"),
        Some(&1),
        "\"click submit\" and \"submit my answer\" must resolve identically"
    );
    assert!(
        !grammar.contains_key("question 3"),
        "non-interactive nodes get no voice command"
    );
}

#[test]
fn switch_scan_groups_only_the_focus_order_nodes() {
    let ModalityInterface::SwitchScan(groups) = project(&tree(), Modality::SwitchScan) else {
        panic!("expected SwitchScan");
    };
    let flattened: Vec<u64> = groups.into_iter().flatten().collect();
    assert_eq!(flattened, vec![2, 1]);
}
