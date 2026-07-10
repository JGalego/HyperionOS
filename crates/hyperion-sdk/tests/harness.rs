//! docs/25 §3's `runHarness`: structural mismatches hard-fail regardless
//! of tolerance, content drift is tolerance-gated, and disagreeing
//! implementations are flagged as an equivalence violation.

use hyperion_sdk::{
    run_harness, CapabilityImplementation, GoldenCase, MockContextBundle, Tolerance,
};

struct EchoUpper;
impl CapabilityImplementation for EchoUpper {
    fn name(&self) -> &str {
        "echo-upper"
    }
    fn invoke(
        &self,
        _context: &MockContextBundle,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let text = input
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_uppercase();
        Ok(serde_json::json!({ "summary": text }))
    }
}

struct WrongShape;
impl CapabilityImplementation for WrongShape {
    fn name(&self) -> &str {
        "wrong-shape"
    }
    fn invoke(
        &self,
        _context: &MockContextBundle,
        _input: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({ "result": "not the expected key" }))
    }
}

struct AlwaysErrors;
impl CapabilityImplementation for AlwaysErrors {
    fn name(&self) -> &str {
        "always-errors"
    }
    fn invoke(
        &self,
        _context: &MockContextBundle,
        _input: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        Err("boom".to_string())
    }
}

fn golden(case_id: &str, tolerance_content: f32) -> GoldenCase {
    GoldenCase {
        case_id: case_id.to_string(),
        context_bundle: MockContextBundle::default(),
        input: serde_json::json!({ "text": "hello world" }),
        expected_output: serde_json::json!({ "summary": "HELLO WORLD" }),
        tolerance: Tolerance {
            content: tolerance_content,
        },
    }
}

#[test]
fn a_matching_implementation_passes() {
    let report = run_harness(&[Box::new(EchoUpper)], &[golden("case-1", 0.1)]);
    assert_eq!(
        report.per_implementation[0].verdicts[0].1,
        hyperion_sdk::CaseVerdict::Pass
    );
    assert!(report.equivalence_violations.is_empty());
}

#[test]
fn a_structurally_wrong_shape_is_a_hard_fail_regardless_of_tolerance() {
    let report = run_harness(&[Box::new(WrongShape)], &[golden("case-1", 1.0)]);
    assert_eq!(
        report.per_implementation[0].verdicts[0].1,
        hyperion_sdk::CaseVerdict::StructuralMismatch
    );
}

#[test]
fn an_erroring_implementation_is_treated_as_a_structural_mismatch() {
    let report = run_harness(&[Box::new(AlwaysErrors)], &[golden("case-1", 1.0)]);
    assert_eq!(
        report.per_implementation[0].verdicts[0].1,
        hyperion_sdk::CaseVerdict::StructuralMismatch
    );
}

#[test]
fn content_drift_beyond_tolerance_fails_even_with_the_right_shape() {
    struct DifferentContent;
    impl CapabilityImplementation for DifferentContent {
        fn name(&self) -> &str {
            "different-content"
        }
        fn invoke(
            &self,
            _context: &MockContextBundle,
            _input: &serde_json::Value,
        ) -> Result<serde_json::Value, String> {
            Ok(serde_json::json!({ "summary": "completely unrelated text" }))
        }
    }

    let report = run_harness(&[Box::new(DifferentContent)], &[golden("case-1", 0.1)]);
    assert_eq!(
        report.per_implementation[0].verdicts[0].1,
        hyperion_sdk::CaseVerdict::ContentDrift
    );
}

#[test]
fn two_implementations_that_disagree_on_a_case_are_flagged_as_an_equivalence_violation() {
    let report = run_harness(
        &[Box::new(EchoUpper), Box::new(WrongShape)],
        &[golden("case-1", 0.1)],
    );
    assert_eq!(report.equivalence_violations, vec!["case-1".to_string()]);
}

#[test]
fn two_equivalent_implementations_produce_no_violations() {
    struct AlsoEchoUpper;
    impl CapabilityImplementation for AlsoEchoUpper {
        fn name(&self) -> &str {
            "also-echo-upper"
        }
        fn invoke(
            &self,
            _context: &MockContextBundle,
            input: &serde_json::Value,
        ) -> Result<serde_json::Value, String> {
            let text = input
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_uppercase();
            Ok(serde_json::json!({ "summary": text }))
        }
    }

    let report = run_harness(
        &[Box::new(EchoUpper), Box::new(AlsoEchoUpper)],
        &[golden("case-1", 0.1)],
    );
    assert!(
        report.equivalence_violations.is_empty(),
        "two implementations passing (or failing) identically must never be flagged"
    );
}
