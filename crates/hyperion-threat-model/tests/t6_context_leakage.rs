//! docs/17 T6: Context Propagation leakage across a Trust Boundary — a
//! recipient running under a lower privacy tier must never receive an
//! object whose residency forbids it, and the exclusion must be
//! reported, not silent.

use std::collections::HashSet;

use hyperion_privacy::{filter_for_recipient, PrivacyTier, ResidencyTag, SensitivityClass};

#[test]
fn t6_a_recipient_never_receives_a_restricted_object_even_when_it_declares_needing_it() {
    let health_record = hyperion_storage::ObjectId(1);
    let shopping_list = hyperion_storage::ObjectId(2);
    let tags = vec![
        ResidencyTag::new(
            health_record,
            SensitivityClass::Restricted,
            [PrivacyTier::FullyLocal].into_iter().collect(),
        ),
        ResidencyTag::new(
            shopping_list,
            SensitivityClass::Public,
            [PrivacyTier::FullyLocal, PrivacyTier::CloudAssisted]
                .into_iter()
                .collect(),
        ),
    ];
    let declared_need: HashSet<_> = [health_record, shopping_list].into_iter().collect();

    let (included, withheld) =
        filter_for_recipient(&tags, &declared_need, PrivacyTier::CloudAssisted);

    assert_eq!(included, vec![shopping_list]);
    assert_eq!(withheld.len(), 1);
    assert_eq!(
        withheld[0].0, health_record,
        "the Restricted object must be withheld even though the recipient declared needing it"
    );
    assert!(
        !withheld[0].1.is_empty(),
        "the exclusion must carry a reason, never be silent"
    );
}
