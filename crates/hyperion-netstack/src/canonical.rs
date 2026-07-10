use crate::types::CanonicalUrl;

/// docs/19 §5.1: stripped so `?utm_source=…` and its bare equivalent dedupe
/// to the same cache entry.
const TRACKING_PARAMS: &[&str] = &[
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_term",
    "utm_content",
    "fbclid",
    "gclid",
    "igshid",
    "ref",
];

struct ParsedUrl {
    scheme: String,
    host: String,
    port: Option<u16>,
    path: String,
    query_params: Vec<(String, String)>,
}

/// A minimal, dependency-free URL parser — this workspace has no `url`
/// crate dependency and none of docs/19's canonicalization rules need a
/// full RFC 3986 parser, only scheme/host/path/query splitting over the
/// fixed http(s) shapes this hosted simulator's fixtures use.
fn parse(raw: &str) -> ParsedUrl {
    let (scheme, rest) = raw.split_once("://").unwrap_or(("http", raw));
    let scheme = scheme.to_lowercase();

    let (authority, path_and_query) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, ""),
    };
    let path_and_query = path_and_query.split('#').next().unwrap_or("");
    let (path, query) = match path_and_query.split_once('?') {
        Some((p, q)) => (p, q),
        None => (path_and_query, ""),
    };
    let path = if path.is_empty() { "/" } else { path };

    let (host, port) = match authority.split_once(':') {
        Some((h, p)) => (h.to_lowercase(), p.parse::<u16>().ok()),
        None => (authority.to_lowercase(), None),
    };
    let port =
        port.filter(|&p| !((scheme == "http" && p == 80) || (scheme == "https" && p == 443)));

    let mut query_params: Vec<(String, String)> = query
        .split('&')
        .filter(|s| !s.is_empty())
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            if TRACKING_PARAMS.contains(&k) {
                None
            } else {
                Some((k.to_string(), v.to_string()))
            }
        })
        .collect();
    query_params.sort();

    ParsedUrl {
        scheme,
        host,
        port,
        path: path.to_string(),
        query_params,
    }
}

fn render(parsed: &ParsedUrl) -> String {
    let port_suffix = parsed.port.map(|p| format!(":{p}")).unwrap_or_default();
    let query_suffix = if parsed.query_params.is_empty() {
        String::new()
    } else {
        let joined = parsed
            .query_params
            .iter()
            .map(|(k, v)| {
                if v.is_empty() {
                    k.clone()
                } else {
                    format!("{k}={v}")
                }
            })
            .collect::<Vec<_>>()
            .join("&");
        format!("?{joined}")
    };
    format!(
        "{}://{}{}{}{}",
        parsed.scheme, parsed.host, port_suffix, parsed.path, query_suffix
    )
}

/// docs/19 §5.1: strip tracking parameters and normalize scheme/host
/// casing. Redirect-chain resolution and `<link rel="canonical">`/
/// `schema.org` identifier preference are folded into
/// [`crate::hub::NetstackHub::web_research`] instead of happening here —
/// see this crate's doc comment on why that ordering deviates slightly
/// from §5.1's literal text.
pub(crate) fn canonicalize(raw_url: &str) -> CanonicalUrl {
    let parsed = parse(raw_url);
    CanonicalUrl {
        raw_url: raw_url.to_string(),
        canonical_form: render(&parsed),
        redirect_chain: Vec::new(),
        domain: parsed.host,
    }
}

/// docs/19 §8's SSRF containment: refuse loopback, link-local, and private
/// address ranges by syntactic hostname/IP-literal pattern — this
/// simulator has no real DNS resolver to consult, so it cannot catch a
/// hostname that *resolves* to a private address, only literal private
/// addresses and well-known local hostnames. See this crate's doc comment.
pub(crate) fn is_private_or_local(host: &str) -> bool {
    if host == "localhost" || host.ends_with(".localhost") || host == "0.0.0.0" {
        return true;
    }
    if host == "::1"
        || host.starts_with("fe80:")
        || host.starts_with("fc00:")
        || host.starts_with("fd00:")
    {
        return true;
    }
    let octets: Vec<&str> = host.split('.').collect();
    if octets.len() == 4 {
        if let Some(parts) = octets
            .iter()
            .map(|o| o.parse::<u8>().ok())
            .collect::<Option<Vec<u8>>>()
        {
            let [a, b, ..] = parts[..] else { return false };
            return a == 127
                || a == 10
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 168)
                || (a == 169 && b == 254);
        }
    }
    false
}
