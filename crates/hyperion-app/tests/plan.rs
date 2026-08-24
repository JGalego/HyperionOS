//! Reading a model's answer back into something buildable -- against the answers models really
//! give, not only the one they were asked for.

use hyperion_app::plan::{from_model_answer, PlanError};
use hyperion_app::InputKind;

const ENGINE: &str = "sh";

#[test]
fn a_clean_answer_becomes_a_buildable_definition() {
    let answer = r#"{
      "name": "Invoice-Tally",
      "goal": "Add up this month's invoices",
      "inputs": [
        {"name": "month", "kind": "text", "required": true, "description": "which month"},
        {"name": "currency", "kind": "choice", "choices": ["eur", "usd"],
         "required": false, "description": "which currency"}
      ],
      "script": "echo hi\n"
    }"#;

    let definition = from_model_answer(answer, ENGINE).expect("must parse");
    assert_eq!(definition.name, "invoice-tally");
    assert_eq!(definition.goal, "Add up this month's invoices");
    assert_eq!(definition.engine_id, ENGINE);
    assert_eq!(definition.inputs.len(), 2);
    assert_eq!(definition.inputs[0].kind, InputKind::Text);
    assert!(definition.inputs[0].required);
    assert_eq!(
        definition.inputs[1].kind,
        InputKind::Choice(vec!["eur".to_string(), "usd".to_string()])
    );
    assert!(!definition.inputs[1].required);
}

#[test]
fn a_fenced_answer_with_preamble_still_parses() {
    // Models told to reply with only JSON still wrap it. Refusing this would fail on an answer
    // whose actual content is perfectly good.
    let answer = "Sure! Here's the app:\n\n```json\n\
        {\"name\":\"greet\",\"goal\":\"say hello\",\"inputs\":[],\"script\":\"echo hi\"}\n\
        ```\nLet me know if you want changes.";
    let definition = from_model_answer(answer, ENGINE).expect("must parse");
    assert_eq!(definition.name, "greet");
    assert!(definition.inputs.is_empty());
}

#[test]
fn a_brace_inside_the_scripts_own_source_does_not_end_the_object_early() {
    // The exact case a naive "find the last }" would corrupt: the script is shell/awk-ish and
    // full of braces and escaped quotes.
    let answer = r#"{"name":"count","goal":"count lines",
      "inputs":[],
      "script":"awk '{ n++ } END { print \"{\" n \"}\" }' \"$1\"\n"}"#;
    let definition = from_model_answer(answer, ENGINE).expect("must parse");
    assert_eq!(definition.name, "count");
    assert!(definition.script.contains("END { print"));
    assert!(definition.script.contains(r#""{" n "}""#));
}

#[test]
fn an_input_whose_kind_was_left_out_is_treated_as_text_and_required() {
    let answer = r#"{"name":"x","goal":"g","inputs":[{"name":"who","description":"who to greet"}],
                     "script":"echo hi"}"#;
    let definition = from_model_answer(answer, ENGINE).expect("must parse");
    assert_eq!(definition.inputs[0].kind, InputKind::Text);
    // Being asked for something unnecessary is a much smaller harm than a script failing for a
    // value nobody was asked for.
    assert!(definition.inputs[0].required);
}

#[test]
fn an_answer_with_nothing_to_run_is_refused() {
    let answer = r#"{"name":"x","goal":"g","inputs":[]}"#;
    assert_eq!(
        from_model_answer(answer, ENGINE).unwrap_err(),
        PlanError::MissingScript
    );
    let blank = r#"{"name":"x","goal":"g","inputs":[],"script":"   "}"#;
    assert_eq!(
        from_model_answer(blank, ENGINE).unwrap_err(),
        PlanError::MissingScript
    );
}

#[test]
fn an_answer_that_is_not_a_plan_at_all_is_refused_in_plain_words() {
    let err = from_model_answer("I'm afraid I can't help with that.", ENGINE).unwrap_err();
    assert_eq!(err, PlanError::NoJsonObject);
    assert_eq!(
        err.to_string(),
        "I couldn't tell what to build from that answer"
    );
}

#[test]
fn a_choice_with_no_choices_is_refused_rather_than_installed_unanswerable() {
    let answer = r#"{"name":"x","goal":"g","script":"echo hi",
                     "inputs":[{"name":"mode","kind":"choice","description":"which mode"}]}"#;
    assert_eq!(
        from_model_answer(answer, ENGINE).unwrap_err(),
        PlanError::ChoiceWithoutOptions("mode".to_string())
    );
}

#[test]
fn an_input_kind_hyperion_cannot_ask_for_is_named_rather_than_guessed() {
    let answer = r#"{"name":"x","goal":"g","script":"echo hi",
                     "inputs":[{"name":"when","kind":"datetime","description":"when"}]}"#;
    assert_eq!(
        from_model_answer(answer, ENGINE).unwrap_err(),
        PlanError::UnknownKind("datetime".to_string())
    );
}

#[test]
fn truncated_json_is_refused_rather_than_half_read() {
    let answer = r#"{"name":"x","goal":"g","script":"echo hi","inputs":[{"name":"a""#;
    assert_eq!(
        from_model_answer(answer, ENGINE).unwrap_err(),
        PlanError::NoJsonObject
    );
}
