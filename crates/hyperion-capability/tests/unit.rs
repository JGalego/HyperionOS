use std::time::Duration;

use hyperion_capability::{CapabilityMonitor, Fault, RightsMask, TrustBoundaryId};

fn boundary(n: u64) -> TrustBoundaryId {
    TrustBoundaryId(n)
}

#[test]
fn root_token_is_live_and_authorizes_its_own_rights() {
    let mut m = CapabilityMonitor::new();
    let root = m.mint_root(RightsMask::READ | RightsMask::WRITE, boundary(1), None);
    assert!(m.is_live(&root));
    assert!(m.check_rights_ok(&root, RightsMask::READ));
}

#[test]
fn derive_narrows_rights_and_rejects_escalation() {
    let mut m = CapabilityMonitor::new();
    let root = m.mint_root(RightsMask::READ | RightsMask::WRITE, boundary(1), None);

    let child = m
        .cap_derive(&root, RightsMask::READ, None, boundary(2))
        .expect("attenuation within parent rights must succeed");
    assert_eq!(child.rights(), RightsMask::READ);
    assert_eq!(child.object_id(), root.object_id());

    let escalated = m.cap_derive(&root, RightsMask::EXEC, None, boundary(2));
    assert_eq!(escalated.unwrap_err(), Fault::CannotEscalate);
}

#[test]
fn revoking_root_invalidates_every_descendant_but_not_siblings() {
    let mut m = CapabilityMonitor::new();
    let root_a = m.mint_root(RightsMask::all(), boundary(1), None);
    let root_b = m.mint_root(RightsMask::all(), boundary(1), None); // unrelated object/tree

    let child = m
        .cap_derive(&root_a, RightsMask::READ, None, boundary(2))
        .unwrap();
    let grandchild = m
        .cap_derive(&child, RightsMask::READ, None, boundary(3))
        .unwrap();

    let receipt = m.cap_revoke(&child);
    assert_eq!(receipt.descendants_invalidated, 1); // grandchild only

    assert!(!m.is_live(&child), "revoked token itself must be dead");
    assert!(
        !m.is_live(&grandchild),
        "descendant of revoked token must be dead"
    );
    assert!(
        m.is_live(&root_a),
        "ancestor of the revoked token must be unaffected"
    );
    assert!(m.is_live(&root_b), "unrelated tree must be unaffected");
}

#[test]
fn stale_generation_token_is_rejected_even_after_new_siblings_are_derived() {
    let mut m = CapabilityMonitor::new();
    let root = m.mint_root(RightsMask::all(), boundary(1), None);
    let child = m
        .cap_derive(&root, RightsMask::READ, None, boundary(2))
        .unwrap();

    m.cap_revoke(&child);
    // A stale handle to the same token must still be rejected identically
    // to "no such capability" from the caller's point of view.
    assert!(!m.is_live(&child));
    assert_eq!(
        m.check_rights_ok_result(&child, RightsMask::READ),
        Err(Fault::Revoked)
    );

    // Deriving a fresh sibling from the (still-live) root must not resurrect
    // the revoked child, and the new sibling itself must be independently live.
    let sibling = m
        .cap_derive(&root, RightsMask::READ, None, boundary(4))
        .unwrap();
    assert!(m.is_live(&sibling));
    assert!(!m.is_live(&child));
}

#[test]
fn expired_token_is_rejected() {
    let mut m = CapabilityMonitor::new();
    let root = m.mint_root(
        RightsMask::READ,
        boundary(1),
        Some(Duration::from_millis(1)),
    );
    std::thread::sleep(Duration::from_millis(20));
    assert!(!m.is_live(&root));
    assert_eq!(
        m.check_rights_ok_result(&root, RightsMask::READ),
        Err(Fault::Expired)
    );
}

#[test]
fn derived_ttl_cannot_outlive_parent() {
    let mut m = CapabilityMonitor::new();
    let root = m.mint_root(
        RightsMask::READ,
        boundary(1),
        Some(Duration::from_millis(20)),
    );
    let child = m
        .cap_derive(
            &root,
            RightsMask::READ,
            Some(Duration::from_secs(3600)),
            boundary(2),
        )
        .unwrap();

    std::thread::sleep(Duration::from_millis(40));
    assert!(!m.is_live(&root));
    assert!(
        !m.is_live(&child),
        "child requested a 1-hour TTL but must still inherit the parent's earlier expiry"
    );
}

#[test]
fn table_and_monitor_gate_invocation_end_to_end() {
    use hyperion_capability::CapabilityTable;

    let mut m = CapabilityMonitor::new();
    let mut table = CapabilityTable::new(boundary(1), None);
    let root = m.mint_root(RightsMask::READ | RightsMask::WRITE, boundary(1), None);
    let slot = table.insert(root.clone());

    let result = m.cap_invoke(&table, slot, RightsMask::READ, |tok| tok.rights());
    assert_eq!(result.unwrap(), RightsMask::READ | RightsMask::WRITE);

    m.cap_revoke(&root);
    let result = m.cap_invoke(&table, slot, RightsMask::READ, |_| ());
    assert_eq!(result.unwrap_err(), Fault::Revoked);
}
