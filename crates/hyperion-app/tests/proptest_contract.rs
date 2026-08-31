//! The contract encoding, against inputs nobody thought to write down.
//!
//! `contract.rs` hand-rolls escaping over three delimiters (`\`, `|`, `,`) and round-trips the
//! result through a signed manifest field. Example-based tests cover the cases someone imagined;
//! these cover the ones they did not -- a goal that is nothing but backslashes, a choice option
//! containing the separator that separates choices, a description ending mid-escape.

use hyperion_app::contract::{self, AppContract};
use hyperion_app::{InputField, InputKind};
use proptest::prelude::*;

/// Text that leans on the encoding: mostly delimiters and escapes, with ordinary characters mixed
/// in so a failure is still readable.
fn awkward_text() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            Just('\\'),
            Just('|'),
            Just(','),
            Just('"'),
            Just(' '),
            any::<char>(),
        ],
        0..24,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

fn field_name() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,12}".prop_map(|s| s)
}

fn input_kind() -> impl Strategy<Value = InputKind> {
    prop_oneof![
        Just(InputKind::Text),
        Just(InputKind::Integer),
        Just(InputKind::Number),
        Just(InputKind::Boolean),
        Just(InputKind::Path),
        // Options are the one place a *second* delimiter is nested inside a field, so they get
        // the awkward text too.
        proptest::collection::vec(
            awkward_text().prop_filter("non-empty", |s| !s.is_empty()),
            1..4
        )
        .prop_map(InputKind::Choice),
    ]
}

fn app_contract() -> impl Strategy<Value = AppContract> {
    (
        "[a-z][a-z0-9-]{0,12}",
        "[a-z][a-z0-9.]{0,12}",
        any::<bool>(),
        any::<bool>(),
        awkward_text(),
        proptest::collection::vec(
            (field_name(), input_kind(), awkward_text(), any::<bool>()),
            0..4,
        ),
    )
        .prop_map(
            |(name, owner, keeps_data, resident, goal, fields)| AppContract {
                name,
                owner,
                keeps_data,
                resident,
                goal,
                fields: fields
                    .into_iter()
                    .map(|(name, kind, description, required)| InputField {
                        name,
                        kind,
                        description,
                        required,
                    })
                    .collect(),
            },
        )
}

proptest! {
    /// The property the whole design rests on: what is signed is what comes back. If this can
    /// fail, an app's declared inputs, owner or storage flag can differ from what the signature
    /// covers.
    #[test]
    fn every_contract_survives_encoding(contract in app_contract()) {
        let decoded = contract::decode(&contract::encode(&contract));
        prop_assert_eq!(decoded, Some(contract));
    }

    /// Decoding must never panic, whatever is in the manifest field. These strings come off disk
    /// and out of other people's manifests, so "malformed" has to be an answer rather than a
    /// crash.
    #[test]
    fn decoding_arbitrary_strings_never_panics(inputs in proptest::collection::vec(awkward_text(), 0..6)) {
        let _ = contract::decode(&inputs);
    }

    /// Encoding must be injective on the fields that identify an app: two contracts that differ
    /// anywhere must not encode to the same bytes. If they could, one app's signed record would be
    /// indistinguishable from another's.
    ///
    /// Note what is deliberately *not* asserted here: that editing the encoded block is detectable.
    /// It is not, and never was -- this is a serialization, not a MAC. Tamper-detection is the
    /// manifest's Ed25519 signature, which now really covers these bytes; see
    /// `hyperion-plugin-framework`'s own `signature_coverage` tests. An earlier draft of this file
    /// asserted the stronger property, and it failed exactly as it should have.
    #[test]
    fn different_contracts_never_encode_the_same(
        first in app_contract(),
        second in app_contract(),
    ) {
        prop_assume!(first != second);
        prop_assert_ne!(contract::encode(&first), contract::encode(&second));
    }
}
