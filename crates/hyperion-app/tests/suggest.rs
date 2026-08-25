//! Which app a goal is about -- and, more importantly, when the honest answer is "I don't know".

use hyperion_app::suggest::best_match;
use hyperion_app::{AppTier, InstalledApp};

fn app(name: &str, goal: &str) -> InstalledApp {
    InstalledApp {
        owner: "alice".to_string(),
        name: name.to_string(),
        goal: goal.to_string(),
        tier: AppTier::Answer,
        inputs: vec![],
        capability_id: format!("app.{name}"),
        version: 1,
    }
}

fn word_counter() -> InstalledApp {
    app(
        "word_counter",
        "This program counts the number of words in a piece of text.",
    )
}

fn invoice_tally() -> InstalledApp {
    app("invoice-tally", "Add up this month's invoices.")
}

#[test]
fn a_goal_phrased_the_way_a_person_would_finds_the_app() {
    let apps = vec![word_counter(), invoice_tally()];
    // Neither the wording nor the grammatical number matches the app's stated goal, which is
    // exactly the gap between how a person asks and how a model wrote it down.
    let found = best_match(&apps, "count the words in this text").expect("should match");
    assert_eq!(found.name, "word_counter");
}

#[test]
fn naming_the_app_finds_it() {
    let apps = vec![word_counter(), invoice_tally()];
    let found = best_match(&apps, "run the word counter please").expect("should match");
    assert_eq!(found.name, "word_counter");
}

#[test]
fn an_unrelated_question_matches_nothing() {
    let apps = vec![word_counter(), invoice_tally()];
    for utterance in [
        "what is the capital of France",
        "explain how DNS resolution works",
        "hello",
        "",
    ] {
        assert!(
            best_match(&apps, utterance).is_none(),
            "{utterance:?} should not match any app"
        );
    }
}

#[test]
fn a_single_shared_word_is_not_enough() {
    let apps = vec![invoice_tally()];
    // "invoices" alone is the subject, not the task -- plenty of things someone might say about
    // invoices are not a request to run this.
    assert!(best_match(&apps, "what are invoices anyway").is_none());
}

#[test]
fn an_ambiguous_tie_says_nothing_rather_than_picking_one() {
    // Two apps that describe the same task equally well. Naming either would be inventing a
    // preference nobody expressed.
    let apps = vec![
        app("counter-one", "count the words in text"),
        app("counter-two", "count the words in text"),
    ];
    assert!(best_match(&apps, "count the words in my text").is_none());
}

#[test]
fn a_clear_leader_still_wins_when_another_app_also_overlaps() {
    let apps = vec![
        app("word_counter", "count the words in a piece of text"),
        app("text_shouter", "make text louder"),
    ];
    let found = best_match(&apps, "count the words in this text").expect("should match");
    assert_eq!(found.name, "word_counter");
}

#[test]
fn nothing_installed_matches_nothing() {
    assert!(best_match(&[], "count the words in this text").is_none());
}

#[test]
fn short_and_common_words_never_carry_a_match_on_their_own() {
    // "this", "the", "a" and friends appear in almost every stated goal; if they counted, every
    // utterance would match every app.
    let apps = vec![app("x", "this is the one that you would want to use")];
    assert!(best_match(&apps, "is this the one that you want").is_none());
}

#[test]
fn a_stopword_that_looks_plural_is_still_filtered() {
    // The regression this exists for. "this" folds to "thi", which is in no list and looks exactly
    // like a meaningful word -- so checking stopwords only *after* the fold let every utterance
    // containing it earn a free point of overlap against every app whose goal contained it too.
    // Two apps whose goals share nothing with the utterance except such words must not match.
    let apps = vec![
        app("alpha", "this does count these things"),
        app("beta", "something about this"),
    ];
    assert!(best_match(&apps, "this does count these things").is_none());
}

#[test]
fn a_plural_in_a_stated_goal_still_matches_its_singular() {
    // The fold has to keep earning its place: this is the case it exists for.
    let apps = vec![app("invoice-tally", "add up the invoices for a month")];
    let found = best_match(&apps, "add up my invoice for the month").expect("should match");
    assert_eq!(found.name, "invoice-tally");
}
