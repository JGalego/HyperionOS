//! Finding the app a goal is really about.
//!
//! docs/01's argument is that "application" is the unit people should stop thinking in: someone
//! says what they want, and the right capability is chosen for them. The `/app*` meta-commands are
//! the expert escape hatch, not the intended path -- but on their own they *are* the only path,
//! which leaves the philosophy unimplemented. This module is the smallest honest step toward it.
//!
//! ## Why this suggests rather than runs
//!
//! Deliberate, and not timidity. An app is generated code, and matching an utterance to one is
//! inherently fuzzy -- so auto-running the best match means a wrong guess silently executes a
//! program the person never asked for. docs/01 is explicit that Hyperion assists rather than
//! controls, and that every autonomous action must be interruptible and reversible. A wrong
//! suggestion costs one line of text; a wrong execution costs whatever that program did. Until
//! matching is grounded in something stronger than word overlap (a real Intent Engine template, or
//! a semantic index), suggesting is the honest ceiling.
//!
//! Nothing here replaces an answer either: the caller appends a suggestion to a reply that already
//! happened, so a question that merely shares vocabulary with an installed app is still answered
//! normally.

use crate::types::InstalledApp;

/// The fewest overlapping meaningful words before a match is worth mentioning at all.
///
/// One is too few: a single shared word like "invoice" matches every app about invoices and plenty
/// of utterances that are not about running one. Two is the point where the overlap starts to
/// describe the same *task* rather than the same subject.
const MIN_OVERLAP: usize = 2;

/// Words too common to carry meaning about which app is wanted.
///
/// Every entry is at least four characters, because anything shorter is already dropped by the
/// length filter below and would sit here as an entry that can never match. They are checked
/// against the word as written *and* against its folded form, so that both "program" and
/// "programs" are dropped, and so that a word like "this" is recognised before the fold has a
/// chance to turn it into something unrecognisable.
const STOPWORDS: &[&str] = &[
    "about",
    "counts",
    "does",
    "each",
    "from",
    "give",
    "have",
    "into",
    "just",
    "make",
    "many",
    "over",
    "piece",
    "program",
    "should",
    "show",
    "some",
    "something",
    "that",
    "their",
    "them",
    "then",
    "there",
    "these",
    "they",
    "thing",
    "this",
    "using",
    "want",
    "what",
    "when",
    "where",
    "which",
    "will",
    "with",
    "would",
    "your",
];

/// Folds a trailing plural `s` away, so "words" and "word" are the same word.
///
/// Crude on purpose. Real stemming is a dependency and a source of surprises, and singular/plural
/// is the one variation that actually shows up between how a person phrases a goal ("count the
/// words") and how a model phrases the same goal back ("counts the number of words in a piece of
/// text"). Only folds when what is left is still a real word rather than a stub, so "gas" does not
/// become "ga".
fn fold_plural(word: &str) -> &str {
    match word.strip_suffix('s') {
        Some(singular) if singular.len() >= 3 => singular,
        _ => word,
    }
}

/// Splits text into comparable words: lowercased, punctuation-free, short and common words
/// dropped, and plurals folded.
///
/// The stopword check runs both before and after the fold, and the order matters more than it
/// looks. Checking only afterwards silently lets "this" through -- it folds to "thi", which is in
/// no list and looks exactly like a meaningful word -- so every utterance containing it would earn
/// a free point of overlap against every app whose stated goal also contains it. That was a real
/// bug here, found by asking which entries in the list above could ever actually match.
fn meaningful_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.len() >= 4)
        .map(|word| word.to_ascii_lowercase())
        .filter(|word| !STOPWORDS.contains(&word.as_str()))
        .map(|word| fold_plural(&word).to_string())
        .filter(|word| !STOPWORDS.contains(&word.as_str()))
        .collect()
}

/// How strongly `utterance` looks like a request for `app`.
///
/// Counts *distinct* shared words, so an utterance that repeats one word does not out-score one
/// that genuinely overlaps on two. An app's own name counts alongside its goal: naming the app is
/// the clearest possible signal, and a name is often the words a person would use anyway.
fn overlap(app: &InstalledApp, utterance_words: &[String]) -> usize {
    let mut described = meaningful_words(&app.goal);
    described.extend(meaningful_words(&app.name));
    described.sort();
    described.dedup();
    described
        .iter()
        .filter(|word| utterance_words.contains(word))
        .count()
}

/// The one installed app `utterance` is most likely asking for, or `None`.
///
/// `None` whenever the answer is not clear-cut: nothing overlaps enough, or two apps overlap
/// *equally* well. A tie is the case where guessing is least defensible -- both are plausible, so
/// naming one would be inventing a preference the person never expressed. Better to say nothing
/// and let `/apps` answer it.
pub fn best_match<'a>(apps: &'a [InstalledApp], utterance: &str) -> Option<&'a InstalledApp> {
    let utterance_words = meaningful_words(utterance);
    if utterance_words.is_empty() {
        return None;
    }

    let mut scored: Vec<(usize, &InstalledApp)> = apps
        .iter()
        .map(|app| (overlap(app, &utterance_words), app))
        .filter(|(score, _)| *score >= MIN_OVERLAP)
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));

    match scored.as_slice() {
        [(_, only)] => Some(only),
        [(best, app), (runner_up, _), ..] if best > runner_up => Some(app),
        _ => None,
    }
}
