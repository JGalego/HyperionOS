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
