use crate::types::{EntityType, ExtractedEntity, ExtractionMethod, FetchedPage};

/// docs/19 §5.3's "local model pass," made swappable so this crate never
/// actually loads a model — see this crate's doc comment on why wiring
/// the real [`hyperion_ai_runtime`] residency/quantization machinery to a
/// one-shot text-extraction call is deferred rather than half-built here.
pub trait ExtractionBackend: Send + Sync {
    fn extract(&self, text: &str, purpose: &str) -> ExtractedEntity;
}

/// Deterministically derives a low-confidence generic `WebPage` from the
/// fetched text's first line and a leading excerpt — docs/19 §9's
/// "extraction produces no confident entity → generic WebPage" outcome,
/// reached without a real model in the loop.
pub struct MockExtractionBackend;

impl ExtractionBackend for MockExtractionBackend {
    fn extract(&self, text: &str, _purpose: &str) -> ExtractedEntity {
        let title = text.lines().next().unwrap_or("").trim().to_string();
        let summary: String = text.chars().take(240).collect();
        ExtractedEntity {
            entity_type: EntityType::WebPage,
            identifier: None,
            fields: serde_json::json!({ "title": title, "summary": summary }),
            confidence: 0.2,
            extraction_method: ExtractionMethod::ModelBased,
            relationships: Vec::new(),
        }
    }
}

/// A real (PRODUCTION_BOOT_PROMPT.md M10), deterministic, non-model extraction from a real
/// fetched HTML document's own tags -- no `schema.org`/JSON-LD/OpenGraph microformat parsing (see
/// this crate's own doc comment on that separately-named, still-deferred gap), and no ML model
/// runs. This crate's own Mock backend's doc comment already frames the mock as reaching "no
/// confident entity → generic `WebPage`" *without* a model in the loop; this real backend reaches
/// the same honest floor via real `<title>`/`<meta name="description">`/`<p>` tags instead of a
/// hardcoded low-confidence stand-in.
#[cfg(feature = "real-http")]
pub struct HtmlHeuristicExtractionBackend;

#[cfg(feature = "real-http")]
impl ExtractionBackend for HtmlHeuristicExtractionBackend {
    fn extract(&self, text: &str, _purpose: &str) -> ExtractedEntity {
        let document = scraper::Html::parse_document(text);

        let title_selector = scraper::Selector::parse("title").expect("a real, valid selector");
        let title = document
            .select(&title_selector)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .filter(|s| !s.is_empty());

        let description_selector = scraper::Selector::parse(r#"meta[name="description"]"#)
            .expect("a real, valid selector");
        let description = document
            .select(&description_selector)
            .next()
            .and_then(|el| el.value().attr("content"))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let summary = description.unwrap_or_else(|| real_body_summary(&document));

        let confidence = if title.is_some() { 0.5 } else { 0.3 };
        ExtractedEntity {
            entity_type: EntityType::WebPage,
            identifier: None,
            fields: serde_json::json!({
                "title": title.unwrap_or_default(),
                "summary": summary,
            }),
            // A real <title> tag is a stronger real signal than none at all -- still well below
            // StructuredData's 0.95 (no schema.org/JSON-LD/OpenGraph signal found, only the
            // page's own real HTML tags), but a real step above the old Mock backend's fixed 0.2
            // (which had no real content behind it to weigh at all).
            confidence,
            extraction_method: ExtractionMethod::HtmlHeuristic,
            relationships: Vec::new(),
        }
    }
}

/// Prefers real `<p>` tag text (the most reliable "this is real visible body content" heuristic
/// for most real pages) over the whole document's raw text, which can otherwise include real
/// `<script>`/`<style>` tag *contents* -- `scraper`'s own `.text()` does not distinguish visible
/// text from non-visible tag bodies. Falling all the way back to whole-body text when a page has
/// no real `<p>` tags at all can still pull in that noise; a real "readability"-style content
/// extractor would filter it out properly, and is a real, separate feature this heuristic
/// deliberately does not attempt -- named here, not silently assumed correct.
#[cfg(feature = "real-http")]
fn real_body_summary(document: &scraper::Html) -> String {
    let paragraph_selector = scraper::Selector::parse("p").expect("a real, valid selector");
    let from_paragraphs = document
        .select(&paragraph_selector)
        .flat_map(|el| el.text())
        .collect::<Vec<_>>()
        .join(" ");

    let source = if from_paragraphs.trim().is_empty() {
        let body_selector = scraper::Selector::parse("body").expect("a real, valid selector");
        document
            .select(&body_selector)
            .next()
            .map(|el| el.text().collect::<Vec<_>>().join(" "))
            .unwrap_or_default()
    } else {
        from_paragraphs
    };
    source
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(240)
        .collect()
}

/// docs/19 §5.3: structured signals (`schema.org`/JSON-LD, OpenGraph,
/// identifier microformats) are preferred over model inference; only when
/// none resolves an entity does the model-based fallback run.
pub(crate) fn extract_entity(
    page: &FetchedPage,
    backend: &dyn ExtractionBackend,
    purpose: &str,
) -> ExtractedEntity {
    match &page.structured {
        Some(structured) => ExtractedEntity {
            entity_type: structured.entity_type,
            identifier: structured.identifier.clone(),
            fields: structured.fields.clone(),
            confidence: 0.95,
            extraction_method: ExtractionMethod::StructuredData,
            relationships: structured.relationships.clone(),
        },
        None => backend.extract(&page.text, purpose),
    }
}
