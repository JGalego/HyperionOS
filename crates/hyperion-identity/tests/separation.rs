//! The properties that follow from a principal *being* a Trust Boundary, proven rather than
//! asserted -- and proven against the real crates, none of which were changed to make them true.

use hyperion_capability::{CapabilityMonitor, RightsMask};
use hyperion_explainability::ExplanationStore;
use hyperion_identity::PrincipalRegistry;

struct Device {
    monitor: CapabilityMonitor,
    registry: PrincipalRegistry,
    explanations: ExplanationStore,
    _dir: tempfile::TempDir,
}

fn device() -> Device {
    let dir = tempfile::tempdir().unwrap();
    let registry = PrincipalRegistry::open_or_create(dir.path().join("users.json")).unwrap();
    Device {
        monitor: CapabilityMonitor::new(),
        registry,
        explanations: ExplanationStore::new(),
        _dir: dir,
    }
}

#[test]
fn one_persons_audit_trail_is_unreadable_from_anothers_token() {
    // The claim this test exists for: binding a user to a Trust Boundary makes the audit trail
    // per-person *with no change to hyperion-explainability at all*. That crate already seeds
    // every record's `trust_boundary_span` with the calling boundary, and already filters every
    // read by it -- it was simply never given two distinct callers to tell apart.
    let mut device = device();
    let alice = device.registry.principal_for("alice").unwrap();
    let bob = device.registry.principal_for("bob").unwrap();

    let alice_token = device
        .monitor
        .mint_root(RightsMask::all(), alice.boundary, None);
    let bob_token = device
        .monitor
        .mint_root(RightsMask::all(), bob.boundary, None);

    // Alice really does something, and it is really recorded.
    device
        .explanations
        .begin(
            &device.monitor,
            &alice_token,
            1,
            42,
            7,
            "app.invoice-tally",
            Vec::new(),
            1_000,
        )
        .expect("Alice's action records");

    let alice_sees = device
        .explanations
        .trace_intent(&device.monitor, &alice_token, 42)
        .expect("Alice can read her own trail");
    assert_eq!(alice_sees.len(), 1);
    assert_eq!(alice_sees[0].capability_ref, "app.invoice-tally");
    // Attributable: the record really carries Alice's own boundary, so "who did this" has an
    // answer where before there was none.
    assert!(alice_sees[0]
        .trust_boundary_span
        .contains(&alice.boundary.0));

    let bob_sees = device
        .explanations
        .trace_intent(&device.monitor, &bob_token, 42)
        .expect("Bob's read is allowed, and finds nothing of Alice's");
    assert!(
        bob_sees.is_empty(),
        "Bob must not be able to read Alice's audit trail, got: {bob_sees:?}"
    );
}

#[test]
fn revoking_one_person_leaves_the_other_untouched() {
    // "Revoke everything this person could do" is already one `cap_revoke` graph walk, because
    // their authority is a boundary rather than a scattered set of grants.
    let mut device = device();
    let alice = device.registry.principal_for("alice").unwrap();
    let bob = device.registry.principal_for("bob").unwrap();

    let alice_token = device
        .monitor
        .mint_root(RightsMask::all(), alice.boundary, None);
    let bob_token = device
        .monitor
        .mint_root(RightsMask::all(), bob.boundary, None);

    assert!(device
        .monitor
        .check_rights_ok_result(&alice_token, RightsMask::EXEC)
        .is_ok());

    device.monitor.cap_revoke(&alice_token);

    assert!(
        device
            .monitor
            .check_rights_ok_result(&alice_token, RightsMask::EXEC)
            .is_err(),
        "Alice's authority must really be gone"
    );
    assert!(
        device
            .monitor
            .check_rights_ok_result(&bob_token, RightsMask::EXEC)
            .is_ok(),
        "Bob must be entirely unaffected by revoking Alice"
    );
}

#[test]
fn a_boundary_is_an_attribution_label_and_not_an_unforgeable_identity() {
    // Pins a real limitation rather than a guarantee, so it is recorded instead of assumed away.
    //
    // `cap_derive` takes the child's origin as a parameter: it attenuates *rights* and does not
    // inherit *origin*. So a holder of a live token really can mint a child attributed to any
    // boundary it names, another person's included. This is deliberate and load-bearing --
    // re-origining is exactly how `PluginRegistry::install` confines a plugin into a fresh
    // boundary -- which is why the fix is not to change `cap_derive`.
    //
    // It is also why this crate says it separates people rather than protects them. When
    // authentication lands, the property that has to become true is narrower: only whatever
    // authenticated a person may mint a *root* token at their boundary.
    let mut device = device();
    let alice = device.registry.principal_for("alice").unwrap();
    let bob = device.registry.principal_for("bob").unwrap();

    let alice_token = device
        .monitor
        .mint_root(RightsMask::all(), alice.boundary, None);

    let attributed_to_bob = device
        .monitor
        .cap_derive(&alice_token, RightsMask::READ, None, bob.boundary)
        .expect("re-origining is a real, supported operation");
    assert_eq!(
        attributed_to_bob.origin(),
        bob.boundary,
        "if this ever starts returning Alice's boundary, cap_derive's contract changed and this \
         crate's stated limit needs revisiting"
    );

    // What *is* guaranteed, and is the reason revocation still works: the derived token remains a
    // child of Alice's in the revocation graph regardless of the origin it claims, so revoking
    // Alice really does reach it.
    device.monitor.cap_revoke(&alice_token);
    assert!(
        device
            .monitor
            .check_rights_ok_result(&attributed_to_bob, RightsMask::READ)
            .is_err(),
        "a token derived from Alice's must die with Alice's, whatever origin it claims"
    );
}

#[test]
fn rights_can_never_be_widened_across_a_boundary_change() {
    // The half of `cap_derive` that *is* a guarantee: re-origining does not launder rights.
    let mut device = device();
    let alice = device.registry.principal_for("alice").unwrap();
    let bob = device.registry.principal_for("bob").unwrap();

    let alice_read_only = device
        .monitor
        .mint_root(RightsMask::READ, alice.boundary, None);
    assert!(
        device
            .monitor
            .cap_derive(&alice_read_only, RightsMask::all(), None, bob.boundary)
            .is_err(),
        "naming another boundary must not be a way to gain rights"
    );
}
