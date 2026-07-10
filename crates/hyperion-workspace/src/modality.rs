use std::collections::HashMap;

use crate::types::AccessibilityTree;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modality {
    ScreenReader,
    Voice,
    SwitchScan,
}

/// docs/14 §5.3: "For each modality present... a projector reads the
/// *same* `AccessibilityTree`" — every variant here is a pure function of
/// one tree, never a re-derivation. See this crate's doc comment for why
/// eye-gaze isn't implemented (needs a real device profile).
#[derive(Debug, Clone)]
pub enum ModalityInterface {
    /// Linearized announcement order — one line per node in `focus_order`.
    ScreenReader(Vec<String>),
    /// `accessible_name -> node_id`, so "click submit" and "submit my
    /// answer" can both resolve to the same node once
    /// [05 — Intent Engine](../05-intent-engine.md) does its own fuzzy
    /// matching over these phrases — docs/14 §5.3's own example.
    Voice(HashMap<String, u64>),
    /// Nodes grouped into fixed-size scan groups, in focus order.
    SwitchScan(Vec<Vec<u64>>),
}

const SWITCH_SCAN_GROUP_SIZE: usize = 4;

/// docs/14 §6's `ModalityRenderer.project`.
pub fn project(tree: &AccessibilityTree, modality: Modality) -> ModalityInterface {
    match modality {
        Modality::ScreenReader => {
            let lines = tree
                .focus_order
                .iter()
                .filter_map(|id| tree.nodes.iter().find(|n| n.node_id == *id))
                .map(|n| format!("{}: {}", n.role, n.accessible_name))
                .collect();
            ModalityInterface::ScreenReader(lines)
        }
        Modality::Voice => {
            let mut grammar = HashMap::new();
            for node in &tree.nodes {
                if !node.is_interactive {
                    continue;
                }
                grammar.insert(node.accessible_name.to_lowercase(), node.node_id);
                for action in &node.actions {
                    grammar.insert(action.to_lowercase(), node.node_id);
                }
            }
            ModalityInterface::Voice(grammar)
        }
        Modality::SwitchScan => {
            let groups = tree
                .focus_order
                .chunks(SWITCH_SCAN_GROUP_SIZE)
                .map(|chunk| chunk.to_vec())
                .collect();
            ModalityInterface::SwitchScan(groups)
        }
    }
}
