use std::collections::HashSet;

use crate::types::{
    CaseVerdict, GoldenCase, HarnessReport, ImplementationReport, MockContextBundle,
};

/// docs/25 §2's "one-or-more `defineImplementation`" runnable side — the
/// Rust analog of `hyperion-agent-runtime`'s existing
/// `dispatch(capability_ref, args) -> Result<Value, String>` stub
/// signature, per this crate's doc comment. Neither this trait nor
/// docs/25 itself specifies a base class shared across unrelated
/// developers — compatibility between implementations of the same
/// Contract is checked structurally, by [`run_harness`], never
/// nominally.
pub trait CapabilityImplementation: Send + Sync {
    fn name(&self) -> &str;
    fn invoke(
        &self,
        context: &MockContextBundle,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, String>;
}

fn shape_matches(actual: &serde_json::Value, expected: &serde_json::Value) -> bool {
    match (actual, expected) {
        (serde_json::Value::Object(a), serde_json::Value::Object(e)) => {
            let mut a_keys: Vec<&String> = a.keys().collect();
            let mut e_keys: Vec<&String> = e.keys().collect();
            a_keys.sort();
            e_keys.sort();
            a_keys == e_keys
        }
        _ => std::mem::discriminant(actual) == std::mem::discriminant(expected),
    }
}

fn token_overlap(a: &str, b: &str) -> f32 {
    let ta: HashSet<&str> = a.split_whitespace().collect();
    let tb: HashSet<&str> = b.split_whitespace().collect();
    if ta.is_empty() && tb.is_empty() {
        return 1.0;
    }
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let intersection = ta.intersection(&tb).count() as f32;
    let union = ta.union(&tb).count() as f32;
    intersection / union
}

/// docs/25 §3's `embeddingDistance` — no real embedding model exists in
/// this pipeline (the same gap `hyperion-netstack`'s entity resolution
/// already documents), so a token-overlap ratio over each value's
/// stringified form stands in for semantic distance. `0.0` = identical,
/// `1.0` = no overlap at all.
fn content_distance(actual: &serde_json::Value, expected: &serde_json::Value) -> f32 {
    1.0 - token_overlap(&actual.to_string(), &expected.to_string())
}

/// docs/25 §3's `runHarness(contract, impls, goldens)`: Layer 1 is an
/// exact structural shape check (a mismatch is a hard fail regardless of
/// `tolerance`); Layer 2, only reached if Layer 1 passes, is the content-
/// distance-vs-`tolerance.content` check. After every implementation runs
/// every case, the cross-implementation equivalence check flags any
/// golden case where implementations disagree on pass/fail — the
/// precondition the Model Router (23) needs before treating two
/// implementations as interchangeable candidates at all.
pub fn run_harness(
    implementations: &[Box<dyn CapabilityImplementation>],
    goldens: &[GoldenCase],
) -> HarnessReport {
    let mut per_implementation = Vec::with_capacity(implementations.len());

    for implementation in implementations {
        let mut verdicts = Vec::with_capacity(goldens.len());
        for case in goldens {
            let verdict = match implementation.invoke(&case.context_bundle, &case.input) {
                Err(_) => CaseVerdict::StructuralMismatch,
                Ok(actual) => {
                    if !shape_matches(&actual, &case.expected_output) {
                        CaseVerdict::StructuralMismatch
                    } else if content_distance(&actual, &case.expected_output)
                        > case.tolerance.content
                    {
                        CaseVerdict::ContentDrift
                    } else {
                        CaseVerdict::Pass
                    }
                }
            };
            verdicts.push((case.case_id.clone(), verdict));
        }
        per_implementation.push(ImplementationReport {
            implementation_name: implementation.name().to_string(),
            verdicts,
        });
    }

    let mut equivalence_violations = Vec::new();
    for case in goldens {
        let verdict_set: HashSet<CaseVerdict> = per_implementation
            .iter()
            .filter_map(|report| {
                report
                    .verdicts
                    .iter()
                    .find(|(id, _)| id == &case.case_id)
                    .map(|(_, v)| *v)
            })
            .collect();
        if verdict_set.len() > 1 {
            equivalence_violations.push(case.case_id.clone());
        }
    }

    HarnessReport {
        per_implementation,
        equivalence_violations,
    }
}
