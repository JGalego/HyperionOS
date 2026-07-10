//! docs/17 §8's `ThreatRegistry` — the catalog itself is well-formed.

use hyperion_threat_model::{catalog, find, ThreatStatus};

#[test]
fn catalog_has_eight_unique_threat_ids() {
    let records = catalog();
    assert_eq!(records.len(), 8);
    let mut ids: Vec<_> = records.iter().map(|r| r.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 8);
}

#[test]
fn every_threat_names_a_mitigation_and_an_owner() {
    for record in catalog() {
        assert!(!record.mitigation_owner_crate.is_empty());
        assert!(!record.mitigation.is_empty());
        assert!(!record.attacker_goal.is_empty());
    }
}

#[test]
fn find_resolves_a_known_id_and_rejects_an_unknown_one() {
    assert!(find("T1").is_some());
    assert!(find("T99").is_none());
}

#[test]
fn every_threat_is_at_least_partially_mitigated() {
    for record in catalog() {
        assert!(matches!(
            record.status,
            ThreatStatus::Mitigated | ThreatStatus::PartiallyMitigated
        ));
    }
}
