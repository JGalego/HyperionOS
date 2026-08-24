//! The typed input contract: what it survives, and what it refuses.

use hyperion_app::contract::{self, AppContract};
use hyperion_app::{validate_args, ArgError, ContractError, InputField, InputKind};
use serde_json::json;

fn field(name: &str, kind: InputKind, required: bool) -> InputField {
    InputField {
        name: name.to_string(),
        kind,
        description: format!("what {name} is for"),
        required,
    }
}

#[test]
fn a_whole_contract_survives_a_round_trip_through_the_signed_manifest_field() {
    let original = AppContract {
        name: "invoice-tally".to_string(),
        goal: "Add up this month's invoices".to_string(),
        fields: vec![
            field("month", InputKind::Text, true),
            field("year", InputKind::Integer, true),
            field("rate", InputKind::Number, false),
            field("include_drafts", InputKind::Boolean, false),
            field("sheet", InputKind::Path, false),
            field(
                "currency",
                InputKind::Choice(vec!["eur".to_string(), "usd".to_string()]),
                true,
            ),
        ],
    };

    let encoded = contract::encode(&original);
    let decoded = contract::decode(&encoded).expect("what we just encoded must decode");
    assert_eq!(decoded, original);
}

#[test]
fn the_delimiters_themselves_survive_a_round_trip() {
    // The encoding uses `|` and `,` as delimiters and `\` to escape them. A goal or description
    // containing all three is exactly the case that would corrupt a naive split -- and a person
    // writing "revenue | costs, before tax" has done nothing unusual.
    let original = AppContract {
        name: "pipes".to_string(),
        goal: r"revenue | costs, before tax \ after".to_string(),
        fields: vec![InputField {
            name: "mode".to_string(),
            kind: InputKind::Choice(vec!["a|b".to_string(), r"c,d\e".to_string()]),
            description: r"pick a|b or c,d\e".to_string(),
            required: true,
        }],
    };

    let decoded = contract::decode(&contract::encode(&original)).expect("must decode");
    assert_eq!(decoded, original);
}

#[test]
fn a_capability_that_is_not_an_app_simply_does_not_decode() {
    // Exactly what `/apps` relies on to tell an app apart from every other installed capability.
    assert!(contract::decode(&[]).is_none());
    assert!(contract::decode(&["prompt".to_string(), "text".to_string()]).is_none());
    assert!(contract::decode(&["hyperion-app/v99|app|x|y".to_string()]).is_none());
}

#[test]
fn a_half_understood_contract_is_refused_rather_than_half_decoded() {
    let mut encoded = contract::encode(&AppContract {
        name: "partly".to_string(),
        goal: "a goal".to_string(),
        fields: vec![
            field("good", InputKind::Text, true),
            field("bad", InputKind::Text, true),
        ],
    });
    // Corrupt only the second field's kind. A decoder that returned just the first field would
    // hand a caller a contract missing a required input -- which is worse than no contract.
    encoded[2] = encoded[2].replace("|text|", "|hieroglyphs|");
    assert!(contract::decode(&encoded).is_none());
}

#[test]
fn an_app_name_that_could_escape_its_own_directory_is_refused() {
    for bad in ["../evil", "a/b", "Upper", "", "-leading", &"x".repeat(65)] {
        assert!(
            contract::validate_app_name(bad).is_err(),
            "{bad:?} should not be a legal app name"
        );
    }
    assert!(contract::validate_app_name("invoice-tally_2").is_ok());
}

#[test]
fn a_contract_that_could_never_be_answered_is_refused() {
    let base = AppContract {
        name: "ok".to_string(),
        goal: "a goal".to_string(),
        fields: vec![],
    };

    let mut no_goal = base.clone();
    no_goal.goal = "   ".to_string();
    assert_eq!(
        contract::validate_contract(&no_goal),
        Err(ContractError::MissingGoal)
    );

    let mut impossible_choice = base.clone();
    impossible_choice.fields = vec![field("pick", InputKind::Choice(vec![]), true)];
    assert_eq!(
        contract::validate_contract(&impossible_choice),
        Err(ContractError::EmptyChoice("pick".to_string()))
    );

    let mut duplicated = base.clone();
    duplicated.fields = vec![
        field("month", InputKind::Text, true),
        field("month", InputKind::Integer, true),
    ];
    assert_eq!(
        contract::validate_contract(&duplicated),
        Err(ContractError::DuplicateField("month".to_string()))
    );

    let mut undescribed = base;
    undescribed.fields = vec![InputField {
        name: "month".to_string(),
        kind: InputKind::Text,
        description: "  ".to_string(),
        required: true,
    }];
    assert_eq!(
        contract::validate_contract(&undescribed),
        Err(ContractError::MissingDescription("month".to_string()))
    );
}

#[test]
fn a_missing_required_argument_is_named_in_words_a_person_can_act_on() {
    let fields = vec![field("month", InputKind::Text, true)];
    let err = validate_args("tally", &fields, &json!({})).unwrap_err();
    assert_eq!(
        err,
        ArgError::Missing {
            app: "tally".to_string(),
            field: "month".to_string(),
            description: "what month is for".to_string(),
        }
    );
    // The declared description really reaches the message -- that is the whole point of a typed
    // contract over a bare list of names.
    assert!(err.to_string().contains("what month is for"));
}

#[test]
fn an_optional_argument_is_simply_absent_rather_than_defaulted() {
    let fields = vec![
        field("month", InputKind::Text, true),
        field("rate", InputKind::Number, false),
    ];
    let prepared = validate_args("tally", &fields, &json!({"month": "may"})).unwrap();
    assert_eq!(prepared, json!({"month": "may"}));
}

#[test]
fn an_argument_the_app_never_declared_is_refused() {
    let fields = vec![field("month", InputKind::Text, true)];
    assert_eq!(
        validate_args("tally", &fields, &json!({"month": "may", "yaer": 2026})).unwrap_err(),
        ArgError::Unknown {
            app: "tally".to_string(),
            field: "yaer".to_string(),
        }
    );
}

#[test]
fn typed_text_from_a_console_is_coerced_but_nonsense_is_not() {
    let fields = vec![
        field("year", InputKind::Integer, true),
        field("rate", InputKind::Number, true),
        field("drafts", InputKind::Boolean, true),
    ];
    let prepared = validate_args(
        "tally",
        &fields,
        &json!({"year": "2026", "rate": "1.5", "drafts": "yes"}),
    )
    .unwrap();
    assert_eq!(prepared, json!({"year": 2026, "rate": 1.5, "drafts": true}));

    // A fractional value is really not a whole number, however it was typed.
    let only_year = vec![field("year", InputKind::Integer, true)];
    assert!(validate_args("tally", &only_year, &json!({"year": "3.5"})).is_err());
    assert!(validate_args("tally", &only_year, &json!({"year": 3.5})).is_err());
    assert!(validate_args("tally", &only_year, &json!({"year": "soon"})).is_err());
}

#[test]
fn a_choice_outside_the_declared_options_is_refused() {
    let fields = vec![field(
        "currency",
        InputKind::Choice(vec!["eur".to_string(), "usd".to_string()]),
        true,
    )];
    assert!(validate_args("tally", &fields, &json!({"currency": "eur"})).is_ok());
    let err = validate_args("tally", &fields, &json!({"currency": "gbp"})).unwrap_err();
    // The refusal says what *would* have worked, not just that this didn't.
    assert!(err.to_string().contains("eur, usd"), "{err}");
}

#[test]
fn a_path_argument_can_never_reach_outside_the_apps_own_folder() {
    let fields = vec![field("sheet", InputKind::Path, true)];
    assert!(validate_args("tally", &fields, &json!({"sheet": "data/may.csv"})).is_ok());

    for escape in ["/etc/passwd", "../../etc/passwd", "data/../../secrets"] {
        assert_eq!(
            validate_args("tally", &fields, &json!({"sheet": escape})).unwrap_err(),
            ArgError::PathEscapes {
                field: "sheet".to_string(),
                got: escape.to_string(),
            },
            "{escape} should be refused"
        );
    }
}
