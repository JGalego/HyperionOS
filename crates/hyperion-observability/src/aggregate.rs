use crate::types::{AggregateReport, ConsentCategory, ConsentScope};

/// docs/34 §5's privacy filtering before aggregation: k-anonymity —
/// `cohort_size >= aggregation_min_cohort` else suppressed *entirely*,
/// never partial, and gated on `category != None`. Consent is checked
/// against the scope in effect *at send time*, not whatever scope was
/// active when the underlying samples were collected — this function
/// takes the caller's current `ConsentScope` for exactly that reason.
pub fn build_aggregate(
    scope: &ConsentScope,
    cohort_size: u32,
    raw_summaries: Vec<(String, f64)>,
) -> AggregateReport {
    let opted_in = scope.category != ConsentCategory::None;
    let cohort_large_enough = cohort_size >= scope.aggregation_min_cohort;

    if opted_in && cohort_large_enough {
        AggregateReport {
            cohort_size,
            summaries: raw_summaries,
            suppressed: false,
        }
    } else {
        AggregateReport {
            cohort_size,
            summaries: Vec::new(),
            suppressed: true,
        }
    }
}
