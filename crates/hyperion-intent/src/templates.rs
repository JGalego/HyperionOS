/// One leaf in a flat (non-nested) HTN decomposition — see this crate's
/// doc comment on the "nested subtree" simplification. `depends_on`
/// indexes other entries in the same template's `leaves` slice.
pub(crate) struct TemplateLeaf {
    pub(crate) predicate: &'static str,
    pub(crate) depends_on: &'static [usize],
}

pub(crate) struct Template {
    pub(crate) trigger_keywords: &'static [&'static str],
    pub(crate) root_predicate: &'static str,
    pub(crate) leaves: &'static [TemplateLeaf],
}

/// docs/05 §Worked Example's "launch my startup," trimmed from the doc's
/// full 7-node graph to the four leaves that already exercise the
/// dependency-chain/critical-path/priority machinery (market research has
/// no prerequisite and starts `Executing`; branding and the business model
/// both wait on it; legal waits on branding) — see this crate's doc
/// comment on why only one template exists yet.
pub(crate) const TEMPLATES: &[Template] = &[Template {
    trigger_keywords: &["startup", "launch my", "found a company", "found_company"],
    root_predicate: "found_company",
    leaves: &[
        TemplateLeaf {
            predicate: "market_research",
            depends_on: &[],
        },
        TemplateLeaf {
            predicate: "business_model",
            depends_on: &[0],
        },
        TemplateLeaf {
            predicate: "branding",
            depends_on: &[0],
        },
        TemplateLeaf {
            predicate: "legal_formation",
            depends_on: &[2],
        },
    ],
}];

pub(crate) fn match_template(utterance: &str) -> Option<&'static Template> {
    let lower = utterance.to_lowercase();
    TEMPLATES
        .iter()
        .find(|t| t.trigger_keywords.iter().any(|kw| lower.contains(kw)))
}
