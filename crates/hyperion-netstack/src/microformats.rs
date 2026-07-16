//! Real, deterministic `schema.org`/JSON-LD and OpenGraph microformat extraction
//! (docs/19-networking-stack.md §5.3's "structured signals... preferred over model inference") --
//! pure HTML parsing, no network I/O and no model in the loop. [`crate::fetch::
//! ReqwestFetchBackend`] calls [`parse`] on every real fetched page's own HTML body and, when it
//! finds a real signal, populates [`crate::types::FetchedPage::structured`] for real rather than
//! always `None`.
//!
//! ~~**Named scope boundary, not silently assumed complete:** [`StructuredSignal::relationships`]
//! is always empty here~~ -- now real for the JSON-LD path: [`extract_relationships`] walks every
//! top-level property whose value is itself a real JSON-LD object (or array of objects) declaring
//! its own `@type` (`"author": {"@type": "Person", ...}`, `"publisher": {...}`) and turns each one
//! into a real `(predicate, related_entity_identifier)` pair, the property name itself as the
//! predicate. `crate::extract::HtmlHeuristicExtractionBackend`'s own `relationships: Vec::new()`
//! remains exactly as scoped -- a real HTML heuristic (title/meta-description) has no nested
//! entity structure to walk at all, unlike a real JSON-LD document.

use crate::types::{EntityType, StructuredSignal};

/// Maps a real schema.org `@type` (or OpenGraph `og:type`) string to this crate's own
/// [`EntityType`] -- a deliberately narrow, explicit allowlist. An unrecognized or generic type
/// (`"Article"`, `"WebPage"`, anything not listed) maps to [`EntityType::WebPage`], the same
/// honest floor [`crate::extract::MockExtractionBackend`]'s own doc comment already establishes
/// for "no confident entity", not a guess at a more specific type this crate has no real evidence
/// for.
fn entity_type_for(schema_type: &str) -> EntityType {
    match schema_type {
        "Person" => EntityType::Person,
        "Organization" | "Corporation" | "NGO" | "LocalBusiness" | "EducationalOrganization" => {
            EntityType::Organization
        }
        "Product" => EntityType::Product,
        "Event" | "BusinessEvent" | "SocialEvent" | "SportsEvent" => EntityType::Event,
        "Place" | "City" | "Country" | "AdministrativeArea" => EntityType::Place,
        "ScholarlyArticle" | "Report" | "Thesis" => EntityType::Paper,
        _ => EntityType::WebPage,
    }
}

/// docs/19 §5.5's `(predicate, related_entity_identifier)` extraction, real: every top-level
/// property of a real JSON-LD object whose own value is itself an object (or array of objects)
/// declaring a real `@type` is a genuine nested entity -- schema.org's own convention for
/// `"author"`/`"publisher"`/similarly-shaped properties, not this module inventing a schema of
/// its own. The property name itself is the real predicate; the nested entity's own identifier
/// falls back through the same `@id`/`url`/`identifier` chain [`parse_json_ld`] uses for the
/// top-level entity, plus `name` as an honest last resort -- many nested stubs (a `Person` naming
/// just an author) carry nothing else. A nested value with no real `@type` of its own (a plain
/// string, number, or untyped object -- most schema.org properties) contributes nothing; this
/// never guesses a relationship out of untyped data.
fn extract_relationships(value: &serde_json::Value) -> Vec<(String, String)> {
    let Some(object) = value.as_object() else {
        return Vec::new();
    };
    object
        .iter()
        .filter(|(key, _)| !key.starts_with('@'))
        .flat_map(|(predicate, related)| {
            let candidates: Vec<&serde_json::Value> = match related {
                serde_json::Value::Object(_) => vec![related],
                serde_json::Value::Array(items) => items.iter().collect(),
                _ => Vec::new(),
            };
            candidates.into_iter().filter_map(move |candidate| {
                let candidate_object = candidate.as_object()?;
                candidate_object.get("@type")?.as_str()?;
                let identifier = candidate_object
                    .get("@id")
                    .or_else(|| candidate_object.get("url"))
                    .or_else(|| candidate_object.get("identifier"))
                    .or_else(|| candidate_object.get("name"))
                    .and_then(|v| v.as_str())?;
                Some((predicate.clone(), identifier.to_string()))
            })
        })
        .collect()
}

/// Parses a real `<script type="application/ld+json">` block's own JSON body into a
/// [`StructuredSignal`] -- schema.org's own real, standardized shape (`@type`, `@id`/`url`, plus
/// whatever other real fields the publisher included), not a heuristic guess. `None` if the block
/// isn't valid JSON, or has no real `@type` string to map at all (nothing here to classify it
/// by).
fn parse_json_ld(raw: &str) -> Option<StructuredSignal> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let schema_type = value.get("@type")?.as_str()?;
    let identifier = value
        .get("@id")
        .or_else(|| value.get("url"))
        .or_else(|| value.get("identifier"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let relationships = extract_relationships(&value);
    Some(StructuredSignal {
        entity_type: entity_type_for(schema_type),
        identifier,
        fields: value,
        relationships,
    })
}

/// Parses a real page's own `<meta property="og:*">` tags into a [`StructuredSignal`] -- the de
/// facto OpenGraph fallback when no JSON-LD block exists (or none parsed cleanly). Requires at
/// least a real, non-empty `og:title` -- this module's own floor for "there is something real
/// worth naming here"; `og:type` is optional and, when present, maps through the same
/// [`entity_type_for`] the JSON-LD path uses (schema.org and OpenGraph share the same type
/// vocabulary in practice).
fn parse_open_graph(document: &scraper::Html) -> Option<StructuredSignal> {
    let selector =
        scraper::Selector::parse(r#"meta[property^="og:"]"#).expect("a real, valid selector");
    let mut fields = serde_json::Map::new();
    for el in document.select(&selector) {
        let (Some(property), Some(content)) =
            (el.value().attr("property"), el.value().attr("content"))
        else {
            continue;
        };
        if let Some(key) = property.strip_prefix("og:") {
            fields.insert(
                key.to_string(),
                serde_json::Value::String(content.to_string()),
            );
        }
    }

    let title = fields.get("title").and_then(|v| v.as_str());
    if title.is_none_or(str::is_empty) {
        return None;
    }
    let entity_type = fields
        .get("type")
        .and_then(|v| v.as_str())
        .map(entity_type_for)
        .unwrap_or(EntityType::WebPage);
    let identifier = fields
        .get("url")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    Some(StructuredSignal {
        entity_type,
        identifier,
        fields: serde_json::Value::Object(fields),
        relationships: Vec::new(),
    })
}

/// Real `schema.org`/JSON-LD and OpenGraph microformat extraction over a real fetched HTML
/// document -- JSON-LD preferred (schema.org's own, more structured/typed vocabulary) over
/// OpenGraph (a flatter, social-preview-oriented vocabulary): docs/19 §5.3's own "structured
/// signals... preferred over model inference" priority, applied one level deeper -- among
/// structured signals themselves, the more structured one wins. `None` if neither real signal is
/// present at all; [`crate::extract::extract_entity`]'s own model-based fallback path is exactly
/// what a `None` here defers to.
pub fn parse(html: &str) -> Option<StructuredSignal> {
    let document = scraper::Html::parse_document(html);
    let selector = scraper::Selector::parse(r#"script[type="application/ld+json"]"#)
        .expect("a real, valid selector");
    let json_ld = document
        .select(&selector)
        .find_map(|el| parse_json_ld(&el.text().collect::<String>()));
    json_ld.or_else(|| parse_open_graph(&document))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_json_ld_block_with_a_recognized_type_is_parsed() {
        let html = r#"<html><head>
            <script type="application/ld+json">
                {"@type": "Person", "@id": "https://example.com/people/ada", "name": "Ada Lovelace"}
            </script>
        </head></html>"#;
        let signal = parse(html).expect("a real JSON-LD block must parse");
        assert_eq!(signal.entity_type, EntityType::Person);
        assert_eq!(
            signal.identifier.as_deref(),
            Some("https://example.com/people/ada")
        );
        assert_eq!(signal.fields["name"], "Ada Lovelace");
        assert!(signal.relationships.is_empty());
    }

    #[test]
    fn an_unrecognized_schema_type_falls_back_to_webpage() {
        let html = r#"<script type="application/ld+json">{"@type": "BlogPosting", "url": "https://example.com/post"}</script>"#;
        let signal = parse(html).unwrap();
        assert_eq!(signal.entity_type, EntityType::WebPage);
        assert_eq!(
            signal.identifier.as_deref(),
            Some("https://example.com/post")
        );
    }

    #[test]
    fn malformed_json_ld_falls_back_to_open_graph() {
        let html = r#"<html><head>
            <script type="application/ld+json">{ not valid json </script>
            <meta property="og:title" content="Fallback Title">
            <meta property="og:type" content="Product">
        </head></html>"#;
        let signal = parse(html).expect("a real OpenGraph fallback must still parse");
        assert_eq!(signal.entity_type, EntityType::Product);
        assert_eq!(signal.fields["title"], "Fallback Title");
    }

    #[test]
    fn open_graph_alone_is_parsed_when_no_json_ld_exists() {
        let html = r#"<html><head>
            <meta property="og:title" content="A Real Product">
            <meta property="og:type" content="Product">
            <meta property="og:url" content="https://example.com/products/1">
        </head></html>"#;
        let signal = parse(html).expect("real OpenGraph tags must parse");
        assert_eq!(signal.entity_type, EntityType::Product);
        assert_eq!(
            signal.identifier.as_deref(),
            Some("https://example.com/products/1")
        );
        assert_eq!(signal.fields["title"], "A Real Product");
    }

    #[test]
    fn open_graph_with_no_title_is_not_a_real_signal() {
        let html = r#"<meta property="og:type" content="Product">"#;
        assert!(parse(html).is_none());
    }

    #[test]
    fn no_structured_markup_at_all_returns_none() {
        let html = "<html><head><title>Just a page</title></head><body><p>Hi</p></body></html>";
        assert!(parse(html).is_none());
    }

    #[test]
    fn a_nested_typed_author_and_publisher_become_real_relationships() {
        let html = r#"<script type="application/ld+json">
            {
                "@type": "Article",
                "url": "https://example.com/articles/1",
                "author": {"@type": "Person", "@id": "https://example.com/people/ada", "name": "Ada Lovelace"},
                "publisher": {"@type": "Organization", "name": "Analytical Engine Press"}
            }
        </script>"#;
        let signal = parse(html).expect("a real JSON-LD block must parse");
        let mut relationships = signal.relationships.clone();
        relationships.sort();
        assert_eq!(
            relationships,
            vec![
                (
                    "author".to_string(),
                    "https://example.com/people/ada".to_string()
                ),
                (
                    "publisher".to_string(),
                    "Analytical Engine Press".to_string()
                ),
            ],
            "author's real @id and publisher's own real name (its only real identifier) must \
             both surface as real relationships, got: {relationships:?}"
        );
    }

    #[test]
    fn multiple_authors_in_a_real_json_ld_array_each_become_their_own_relationship() {
        let html = r#"<script type="application/ld+json">
            {
                "@type": "Article",
                "url": "https://example.com/articles/2",
                "author": [
                    {"@type": "Person", "name": "Ada Lovelace"},
                    {"@type": "Person", "name": "Charles Babbage"}
                ]
            }
        </script>"#;
        let signal = parse(html).expect("a real JSON-LD block must parse");
        let mut names: Vec<&str> = signal
            .relationships
            .iter()
            .map(|(_, identifier)| identifier.as_str())
            .collect();
        names.sort();
        assert_eq!(names, vec!["Ada Lovelace", "Charles Babbage"]);
        assert!(
            signal
                .relationships
                .iter()
                .all(|(predicate, _)| predicate == "author"),
            "each array entry keeps the same real predicate, got: {:?}",
            signal.relationships
        );
    }

    #[test]
    fn an_untyped_nested_object_contributes_no_relationship() {
        // `address` here is a plain nested object with no real `@type` of its own -- schema.org's
        // own common shape for a property that just groups fields, not a related entity.
        let html = r#"<script type="application/ld+json">
            {
                "@type": "Organization",
                "name": "Acme Corp",
                "address": {"streetAddress": "123 Main St", "addressCountry": "US"}
            }
        </script>"#;
        let signal = parse(html).expect("a real JSON-LD block must parse");
        assert!(
            signal.relationships.is_empty(),
            "an untyped nested object must never be guessed as a relationship, got: {:?}",
            signal.relationships
        );
    }
}
