pub type HardwareProfileId = String;

/// docs/36 §1's `BenchmarkSpec.category`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchmarkCategory {
    Boot,
    Wake,
    WorkspaceGen,
    Inference,
    Battery,
}

/// docs/36 §1's `BudgetTree` — "children must sum ≤ parent's target_ms,
/// always includes a non-zero reserved-margin leaf" is checked by
/// [`BudgetTree::children_within_budget`], not enforced at construction
/// (a caller can build an invalid tree; this crate only reports it).
#[derive(Debug, Clone)]
pub struct BudgetTree {
    pub phase: String,
    pub target_ms: u32,
    pub children: Vec<BudgetTree>,
}

impl BudgetTree {
    pub fn children_within_budget(&self) -> bool {
        self.children.iter().map(|c| c.target_ms).sum::<u32>() <= self.target_ms
    }
}

/// docs/36 §1's `BenchmarkSpec`.
#[derive(Debug, Clone)]
pub struct BenchmarkSpec {
    pub id: String,
    pub category: BenchmarkCategory,
    pub budget: BudgetTree,
    pub hardware_matrix: Vec<HardwareProfileId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Fail,
    Quarantined,
}

/// docs/36 §1's `BenchmarkResult`, narrowed to `p99_ms` as the one
/// sample statistic the regression gate actually consumes — `p50`/`p95`
/// are recorded but not gated on, matching docs/36's own worked example
/// budgets, which are stated exclusively in terms of a critical-path
/// total (effectively p99-shaped: the slowest acceptable case).
#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    pub spec_id: String,
    pub hardware_profile: HardwareProfileId,
    pub p50_ms: u32,
    pub p95_ms: u32,
    pub p99_ms: u32,
}

/// docs/36 §1's `RegressionGate.action`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateAction {
    BlockRelease,
    Warn,
    QuarantineAndRerun,
}

/// docs/36 §1's `RegressionGate`, `baseline_window`/`threshold: {sigma}`
/// narrowed to a flat percentage — this crate has no sample-variance
/// history to compute a sigma-based significance test against (see this
/// crate's doc comment).
#[derive(Debug, Clone, Copy)]
pub struct RegressionGate {
    pub threshold_pct: f32,
    pub action: GateAction,
}

#[derive(Debug, Clone, Copy)]
pub struct BenchmarkBaseline {
    pub p99_ms: u32,
}

/// docs/36 §2's `gate_check`/`GateVerdict` per-result outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateOutcome {
    Pass,
    Blocked,
    Warned,
    Quarantined,
}

/// docs/35 §1's five test layers this crate's `SuiteReport` summarizes —
/// L0-L2 collapsed into `Deterministic` (all exact-match, `must_pass`),
/// matching that band's own "never waived or quarantined" framing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SuiteKind {
    Deterministic,
    GoldenIntent,
    ModelEval,
    Chaos,
    Accessibility,
    ThreatRegression,
}

/// docs/35 §1's `SuiteReport` — this crate's own aggregation shape, not
/// named as a single struct in the doc, but exactly what
/// `ReleaseGate.evaluate(build)` needs per sub-suite. `regressed_previously_mitigated`
/// implements the doc's `ThreatRegressionCase.provenance` distinction —
/// only a `previously_mitigated` failure blocks release; a
/// `never_tested` gap is tracked, not blocking.
#[derive(Debug, Clone)]
pub struct SuiteReport {
    pub kind: SuiteKind,
    pub passed: u32,
    pub failed: u32,
    pub quarantined: u32,
    pub regressed_previously_mitigated: Vec<String>,
}

impl SuiteReport {
    /// docs/35's release-blocking rule: for every suite except threat
    /// regression, any failure blocks; for threat regression
    /// specifically, only a *regression* (previously mitigated, now
    /// failing) blocks — a never-catalogued gap is tracked, not
    /// blocking.
    pub fn is_blocking(&self) -> bool {
        match self.kind {
            SuiteKind::ThreatRegression => !self.regressed_previously_mitigated.is_empty(),
            _ => self.failed > 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseDecision {
    Pass,
    Blocked,
}

/// docs/35 §1's `ReleaseGate.evaluate(build) -> ReleaseDecision`, widened
/// to a full report — which suites (by [`SuiteKind`]) and/or which
/// benchmark outcome actually blocked, not just a bare verdict.
#[derive(Debug, Clone)]
pub struct ReleaseGateReport {
    pub build_id: String,
    pub decision: ReleaseDecision,
    pub blocking_suites: Vec<SuiteKind>,
    pub benchmark_outcome: Option<GateOutcome>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReleaseGateError {
    #[error("capability does not authorize this operation")]
    Unauthorized,
    #[error("observability error: {0}")]
    Observability(#[from] hyperion_observability::ObservabilityError),
}
