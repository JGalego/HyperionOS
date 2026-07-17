//! docs/16 §4's `ConsentGrant.proof` -- real Ed25519 signing at issuance
//! (`ConsentLedger::request`) and real, independent verification at import
//! (`ConsentLedger::import`), the "real signature with a real verifier" this crate's own doc
//! comment named as the reason `proof` was deferred until now: a grant relayed from another
//! device (e.g. over `hyperion-federation`'s own real `SyncEnvelope` transport) is only ever
//! trusted if its signature verifies against the issuing device's own public key.

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId};
use hyperion_crypto::Keystore;
use hyperion_privacy::{ConsentLedger, DataScope, PrivacyError};

#[test]
fn a_requested_grant_carries_a_real_signature_that_verifies_against_the_issuing_device() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let ledger = ConsentLedger::new();
    let device_key = Keystore::ephemeral();

    let grant = ledger
        .request(
            &monitor,
            &root,
            1,
            DataScope::Domain("notes".to_string()),
            "summarize notes",
            None,
            1_000,
            &device_key,
        )
        .unwrap();

    // Importing a grant this same ledger already issued, against its own issuing device's real
    // public key, must succeed -- proof `proof` is a real signature over these exact fields.
    let importer = ConsentLedger::new();
    assert!(importer
        .import(&monitor, &root, grant, &device_key.verifying_key())
        .is_ok());
}

#[test]
fn importing_a_grant_against_a_different_devices_key_is_rejected_not_silently_trusted() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let issuer_ledger = ConsentLedger::new();
    let issuer_key = Keystore::ephemeral();
    let some_other_device_key = Keystore::ephemeral();

    let grant = issuer_ledger
        .request(
            &monitor,
            &root,
            1,
            DataScope::Domain("notes".to_string()),
            "summarize notes",
            None,
            1_000,
            &issuer_key,
        )
        .unwrap();

    let importer = ConsentLedger::new();
    let result = importer.import(
        &monitor,
        &root,
        grant,
        &some_other_device_key.verifying_key(),
    );
    assert!(matches!(result, Err(PrivacyError::SignatureInvalid)));
}

#[test]
fn a_tampered_grant_field_invalidates_the_signature() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let ledger = ConsentLedger::new();
    let device_key = Keystore::ephemeral();

    let mut grant = ledger
        .request(
            &monitor,
            &root,
            1,
            DataScope::Domain("notes".to_string()),
            "summarize notes",
            None,
            1_000,
            &device_key,
        )
        .unwrap();
    // Widening the purpose after the fact -- the signature was only ever over the original
    // bytes, so this must be caught, not silently accepted as if nothing changed.
    grant.purpose = "delete all notes".to_string();

    let importer = ConsentLedger::new();
    let result = importer.import(&monitor, &root, grant, &device_key.verifying_key());
    assert!(matches!(result, Err(PrivacyError::SignatureInvalid)));
}

#[test]
fn import_requires_write_rights_the_same_as_request() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let read_only = monitor
        .cap_derive(&root, RightsMask::READ, None, TrustBoundaryId(2))
        .unwrap();
    let ledger = ConsentLedger::new();
    let device_key = Keystore::ephemeral();

    let grant = ledger
        .request(
            &monitor,
            &root,
            1,
            DataScope::Domain("notes".to_string()),
            "summarize notes",
            None,
            1_000,
            &device_key,
        )
        .unwrap();

    let importer = ConsentLedger::new();
    let result = importer.import(&monitor, &read_only, grant, &device_key.verifying_key());
    assert!(matches!(result, Err(PrivacyError::Unauthorized)));
}

#[test]
fn a_successfully_imported_grant_really_stands_for_routing() {
    let mut monitor = CapabilityMonitor::new();
    let root = monitor.mint_root(RightsMask::all(), TrustBoundaryId(1), None);
    let issuer_ledger = ConsentLedger::new();
    let device_key = Keystore::ephemeral();
    let scope = DataScope::Domain("notes".to_string());

    let grant = issuer_ledger
        .request(
            &monitor,
            &root,
            1,
            scope.clone(),
            "summarize notes",
            None,
            1_000,
            &device_key,
        )
        .unwrap();

    let importer = ConsentLedger::new();
    importer
        .import(&monitor, &root, grant, &device_key.verifying_key())
        .unwrap();

    assert!(importer.standing_grant(1, &scope, 1_000).is_some());
}
