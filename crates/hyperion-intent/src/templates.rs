use hyperion_plugin_framework::PluginRegistry;

/// One leaf in a flat (non-nested) HTN decomposition — see this crate's
/// doc comment on the "nested subtree" simplification. `depends_on`
/// indexes other entries in the same template's `leaves`.
#[derive(Debug, Clone)]
pub(crate) struct TemplateLeaf {
    pub(crate) predicate: String,
    pub(crate) depends_on: Vec<usize>,
}

/// Owned so a plugin-contributed `hyperion_plugin_framework::AutomationWorkflowContribution`
/// (real, caller-supplied data, not `'static`) and a built-in [`StaticTemplate`] can share one
/// shape — see [`match_template_with_plugins`].
#[derive(Debug, Clone)]
pub(crate) struct Template {
    pub(crate) root_predicate: String,
    pub(crate) leaves: Vec<TemplateLeaf>,
}

struct StaticTemplateLeaf {
    predicate: &'static str,
    depends_on: &'static [usize],
}

struct StaticTemplate {
    trigger_keywords: &'static [&'static str],
    root_predicate: &'static str,
    leaves: &'static [StaticTemplateLeaf],
}

impl From<&StaticTemplate> for Template {
    fn from(t: &StaticTemplate) -> Self {
        Template {
            root_predicate: t.root_predicate.to_string(),
            leaves: t
                .leaves
                .iter()
                .map(|l| TemplateLeaf {
                    predicate: l.predicate.to_string(),
                    depends_on: l.depends_on.to_vec(),
                })
                .collect(),
        }
    }
}

/// docs/05 §Worked Example's "launch my startup," trimmed from the doc's
/// full 7-node graph to the four leaves that already exercise the
/// dependency-chain/critical-path/priority machinery (market research has
/// no prerequisite and starts `Executing`; branding and the business model
/// both wait on it; legal waits on branding) — see this crate's doc
/// comment on why only one built-in template exists yet (a plugin can add more — see
/// [`match_template_with_plugins`]).
const TEMPLATES: &[StaticTemplate] = &[StaticTemplate {
    trigger_keywords: &["startup", "launch my", "found a company", "found_company"],
    root_predicate: "found_company",
    leaves: &[
        StaticTemplateLeaf {
            predicate: "market_research",
            depends_on: &[],
        },
        StaticTemplateLeaf {
            predicate: "business_model",
            depends_on: &[0],
        },
        StaticTemplateLeaf {
            predicate: "branding",
            depends_on: &[0],
        },
        StaticTemplateLeaf {
            predicate: "legal_formation",
            depends_on: &[2],
        },
    ],
}];

/// docs/998-roadmap.md's Resourceful pillar, closed for real: the built-in, hardcoded
/// `TEMPLATES` roster this crate's own doc comment named as the only one that could ever exist,
/// now merged with every currently-installed, non-quarantined plugin's own
/// `hyperion_plugin_framework::Contribution::AutomationWorkflow` entries — a plugin-contributed
/// goal template really competes for a real utterance match, not just the built-in list.
/// Built-ins are checked first (the same "the existing roster wins ties" convention
/// `hyperion-coordination::catalog::best_fit_manifest_with_plugins` already established).
pub(crate) fn match_template_with_plugins(
    utterance: &str,
    plugins: Option<&PluginRegistry>,
) -> Option<Template> {
    let lower = utterance.to_lowercase();
    if let Some(t) = TEMPLATES
        .iter()
        .find(|t| t.trigger_keywords.iter().any(|kw| lower.contains(kw)))
    {
        return Some(Template::from(t));
    }

    plugins?
        .automation_workflow_contributions()
        .into_iter()
        .find(|wf| {
            wf.trigger_keywords
                .iter()
                .any(|kw| lower.contains(&kw.to_lowercase()))
        })
        .map(|wf| Template {
            root_predicate: wf.root_predicate,
            leaves: wf
                .leaves
                .into_iter()
                .map(|l| TemplateLeaf {
                    predicate: l.predicate,
                    depends_on: l.depends_on,
                })
                .collect(),
        })
}
