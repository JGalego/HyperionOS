//! Mirrors every other crate in this workspace: every call is capability-
//! gated, re-checked live against the monitor.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_explainability::{ExplainabilityError, ExplanationStore};

#[test]
fn begin_requires_write_rights() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();
    let store = ExplanationStore::new();

    let result = store.begin(&monitor, &read_only, 1, 7, 1, "web.research", vec![], 1_000);
    assert!(matches!(result, Err(ExplainabilityError::Unauthorized)));
}

#[test]
fn revoking_a_token_blocks_further_access_re_checked_live() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let delegate = monitor
        .cap_derive(&root, RightsMask::all(), None, TrustBoundaryId(2))
        .unwrap();
    let store = ExplanationStore::new();

    assert!(store
        .begin(&monitor, &delegate, 1, 7, 1, "web.research", vec![], 1_000)
        .is_ok());

    monitor.cap_revoke(&delegate);

    assert!(matches!(
        store.begin(&monitor, &delegate, 2, 7, 1, "web.research", vec![], 1_001),
        Err(ExplainabilityError::Unauthorized)
    ));
}
