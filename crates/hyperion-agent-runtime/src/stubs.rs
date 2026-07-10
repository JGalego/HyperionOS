use serde_json::json;

/// The small, first-party, in-house Capability set docs/41-implementation-phases.md's
/// Phase 4 guidance calls for ("a small fixed set of first-party
/// Capabilities, e.g. web research and document drafting, is built in-house
/// for this purpose and later migrated onto the Phase 9 Plugin Framework").
///
/// `args = {"force_fail": true}` deterministically fails any capability —
/// see this crate's doc comment for why that exists.
pub fn dispatch(
    capability_ref: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    if args
        .get("force_fail")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return Err(format!("stub capability '{capability_ref}' failed"));
    }

    match capability_ref {
        "web.search" => {
            let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
            Ok(json!({
                "results": [format!("stub finding for query '{query}'")],
            }))
        }
        "document.draft" => {
            let topic = args.get("topic").and_then(|v| v.as_str()).unwrap_or("");
            Ok(json!({
                "draft": format!("Stub draft document about '{topic}'."),
            }))
        }
        other => Ok(json!({ "echo": other, "args": args })),
    }
}
