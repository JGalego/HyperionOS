//! A real, minimal `robots.txt` parser (docs/19-networking-stack.md's own named "`robots.txt`
//! fetching/parsing" gap) -- pure text parsing, no network I/O of its own. [`fetch::
//! ReqwestFetchBackend`] owns the real HTTP round trip (a real `GET {scheme}://{host}/robots.txt`)
//! and a real per-host cache; this module is just the real group-selection and longest-prefix-wins
//! matching logic real crawlers use, not a naive first-match or "apply every group" merge.

/// A `(is_allow, path_prefix)` directive in file order.
type Directive = (bool, String);
/// A single `User-agent:` group: every agent name the group's directives apply to, plus the
/// directives themselves.
type Group = (Vec<String>, Vec<Directive>);

/// One real, parsed ruleset for whichever single `User-agent` group [`Self::parse`] selected --
/// `Disallow`/`Allow` prefixes in file order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RobotsRules {
    rules: Vec<Directive>,
}

impl RobotsRules {
    /// Parses a real `robots.txt` body, selecting the group whose `User-agent` line names
    /// `agent` (case-insensitive substring match) if one exists, falling back to the
    /// `User-agent: *` group -- the real precedence crawlers use, not a merge of every group. A
    /// body with no group matching either `agent` or `*` allows everything, same as no
    /// `robots.txt` existing at all.
    pub fn parse(body: &str, agent: &str) -> Self {
        let agent = agent.to_lowercase();
        let mut groups: Vec<Group> = Vec::new();
        let mut current: Option<Group> = None;
        // Per the de facto spec, consecutive `User-agent` lines belong to the same group;
        // a `User-agent` line seen *after* this group's own directives already started closes
        // the previous group and opens a new one.
        let mut collecting_agents = true;

        for raw_line in body.lines() {
            let line = raw_line.split('#').next().unwrap_or("").trim();
            let Some((field, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim().to_string();

            match field.trim().to_lowercase().as_str() {
                "user-agent" => {
                    if collecting_agents {
                        match &mut current {
                            Some((agents, _)) => agents.push(value.to_lowercase()),
                            None => current = Some((vec![value.to_lowercase()], Vec::new())),
                        }
                    } else {
                        if let Some(group) = current.take() {
                            groups.push(group);
                        }
                        current = Some((vec![value.to_lowercase()], Vec::new()));
                        collecting_agents = true;
                    }
                }
                "disallow" => {
                    collecting_agents = false;
                    if let Some((_, rules)) = &mut current {
                        rules.push((false, value));
                    }
                }
                "allow" => {
                    collecting_agents = false;
                    if let Some((_, rules)) = &mut current {
                        rules.push((true, value));
                    }
                }
                _ => {}
            }
        }
        if let Some(group) = current.take() {
            groups.push(group);
        }

        let specific = groups.iter().find(|(agents, _)| {
            agents
                .iter()
                .any(|a| a != "*" && agent.contains(a.as_str()))
        });
        let wildcard = groups
            .iter()
            .find(|(agents, _)| agents.iter().any(|a| a == "*"));
        let rules = specific
            .or(wildcard)
            .map_or_else(Vec::new, |(_, r)| r.clone());
        RobotsRules { rules }
    }

    /// `true` if `path` (the request's own URL path, e.g. `/private/page`) is allowed under this
    /// ruleset -- longest matching prefix wins (a real `Disallow` only beats a real `Allow` of
    /// the same or shorter length, matching real crawlers' own precedence); no matching rule at
    /// all is allowed by default. An empty `Disallow`/`Allow` value is a real, explicit
    /// "no restriction" per the de facto spec, modeled as the weakest possible (zero-length)
    /// match so any real, non-empty rule always outranks it.
    pub fn allows(&self, path: &str) -> bool {
        let mut best: Option<(usize, bool)> = None;
        for (is_allow, prefix) in &self.rules {
            if prefix.is_empty() {
                if best.is_none() {
                    best = Some((0, true));
                }
                continue;
            }
            if path.starts_with(prefix.as_str()) && prefix.len() >= best.map_or(0, |(len, _)| len) {
                best = Some((prefix.len(), *is_allow));
            }
        }
        best.is_none_or(|(_, is_allow)| is_allow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_matching_group_at_all_allows_everything() {
        let rules = RobotsRules::parse("User-agent: Googlebot\nDisallow: /\n", "hyperionos");
        assert!(rules.allows("/anything"));
    }

    #[test]
    fn wildcard_group_is_used_when_no_specific_agent_group_exists() {
        let rules = RobotsRules::parse(
            "User-agent: *\nDisallow: /private/\n",
            "hyperionos-netstack",
        );
        assert!(!rules.allows("/private/page"));
        assert!(rules.allows("/public/page"));
    }

    #[test]
    fn a_specific_agent_group_is_preferred_over_the_wildcard_group() {
        let body = "User-agent: *\nDisallow: /\n\nUser-agent: hyperionos\nDisallow: /admin/\n";
        let rules = RobotsRules::parse(body, "hyperionos-netstack");
        // The wildcard group disallows everything; the specific group (matched instead) only
        // disallows /admin/ -- proving the specific group really won, not a merge of both.
        assert!(rules.allows("/public/page"));
        assert!(!rules.allows("/admin/page"));
    }

    #[test]
    fn longest_matching_prefix_wins_over_a_shorter_conflicting_one() {
        let body = "User-agent: *\nDisallow: /docs/\nAllow: /docs/public/\n";
        let rules = RobotsRules::parse(body, "hyperionos-netstack");
        assert!(
            rules.allows("/docs/public/page"),
            "the longer, more specific Allow must win over the shorter Disallow"
        );
        assert!(!rules.allows("/docs/private/page"));
    }

    #[test]
    fn an_empty_disallow_value_means_no_restriction() {
        let rules = RobotsRules::parse("User-agent: *\nDisallow:\n", "hyperionos-netstack");
        assert!(rules.allows("/anything"));
    }

    #[test]
    fn consecutive_user_agent_lines_share_one_group() {
        let body = "User-agent: agent-a\nUser-agent: hyperionos\nDisallow: /blocked/\n";
        let rules = RobotsRules::parse(body, "hyperionos-netstack");
        assert!(!rules.allows("/blocked/page"));
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let body = "# a real comment\nUser-agent: *\n\n# another comment\nDisallow: /private/\n";
        let rules = RobotsRules::parse(body, "hyperionos-netstack");
        assert!(!rules.allows("/private/page"));
    }
}
