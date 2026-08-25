//! The typed input contract, and the encoding that lets it ride *inside* the signed manifest.
//!
//! ## Why it is encoded rather than stored alongside
//!
//! `hyperion_plugin_framework::SemanticContract.inputs` is a `Vec<String>`, and it is covered by
//! the manifest's real Ed25519 signature and by `hyperion_sdk`'s canonical package hash. Encoding
//! the typed contract into that field means an app's declared inputs are signed by the same key,
//! over the same bytes, as the implementation they describe: they cannot be edited without
//! invalidating the signature, and there is no second file to fall out of sync with the registry.
//! `/apps` and `/app <name>` therefore read from the signed record and nothing else.
//!
//! The alternative -- a widened `SemanticContract` carrying real typed fields -- is the better
//! long-term shape, and is deliberately not attempted here: it changes the canonical bytes every
//! existing signature in this workspace was computed over, and ripples through
//! `hyperion-api-gateway`'s router bridge and every existing manifest. That is a real migration
//! worth doing on its own, not folded into the first App Builder slice.
//!
//! ## The format
//!
//! `SemanticContract.inputs` carries one header entry naming the app, then one entry per declared
//! field:
//!
//! ```text
//! hyperion-app/v3|app|<name>|<owner>|keeps-data|<goal>
//! hyperion-app/v3|in|<name>|<kind>|required|<description>
//! ```
//!
//! `<owner>` is the principal who built it (docs/998-roadmap.md §0, Decision 2). It rides inside
//! the signed manifest for the same reason the inputs do: an ownership record that could be edited
//! without invalidating a signature would not be an ownership record.
//!
//! The fourth header field is `keeps-data` or `stateless` — the App Builder's T2. It is signed for
//! the same reason: whether an app may keep anything between runs decides whether it is granted
//! durable storage at all, so it must not be editable without breaking the signature.
//!
//! The version has moved twice, once per header field added (`v1`→`v2` for the owner, `v2`→`v3`
//! for this). An older app no longer decodes, and simply stops being listed rather than being
//! silently reinterpreted as one whose owner or storage is unknown.
//!
//! `<kind>` is one of `text`, `integer`, `number`, `boolean`, `path`, or `choice:<a>,<b>,...`.
//! Backslash, `|` and `,` are backslash-escaped everywhere, so a goal or description containing a
//! pipe survives a round trip.
//!
//! The header is what makes the goal readable back at all: the registry does not expose a
//! manifest's `requested_permissions`, so a goal stored only in a permission justification would
//! be signed but never displayable. Here it is both. It is also a better "is this an app?" test
//! than the capability id -- a capability whose inputs do not begin with this header is simply not
//! an app, and decoding returns `None` rather than an error.

use std::collections::BTreeSet;

use crate::types::{AppTier, InputField, InputKind};

/// Bumped only for a genuinely incompatible change to the layout above. A decoder that meets a
/// version it does not know treats the capability as "not an app" rather than guessing.
pub const CONTRACT_VERSION: &str = "hyperion-app/v3";

/// The longest an app name may be. Long enough to be descriptive, short enough that the resulting
/// capability id and directory name stay manageable everywhere they are displayed.
pub const MAX_APP_NAME: usize = 64;

/// The capability id prefix every app installs under: `app.<name>`. Not the thing that decides
/// whether a capability *is* an app (the header does that) -- it exists so an app is recognizable
/// at a glance in a registry listing and in an audit record alike.
pub const APP_CAPABILITY_PREFIX: &str = "app.";

/// An app's whole declared contract, as it round-trips through the signed manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppContract {
    pub name: String,
    /// The principal who built this app. Apps are device-wide so everyone can *use* one -- the
    /// Resourceful pillar exists so a capability is reused rather than regenerated per person --
    /// but only its owner may remove or rebuild it.
    pub owner: String,
    /// Whether this app keeps anything between runs (App Builder T2).
    ///
    /// A stateless app is granted one throwaway directory per invocation and can keep nothing even
    /// if it tries. A stateful one is granted a durable directory of its own, per app and per
    /// person -- which is a real permission, so declaring this makes the app request `Write` and
    /// puts it through the SDK's own human-review gate.
    pub keeps_data: bool,
    pub goal: String,
    pub fields: Vec<InputField>,
}

impl AppContract {
    pub fn tier(&self) -> AppTier {
        AppTier::for_inputs(&self.fields)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContractError {
    #[error("an app needs a name")]
    EmptyName,
    #[error(
        "\"{0}\" can't be an app name -- use lowercase letters, digits, dashes and underscores, \
         up to {MAX_APP_NAME} characters"
    )]
    InvalidName(String),
    #[error("an app needs to say what it's for")]
    MissingGoal,
    #[error("an app needs to belong to someone")]
    MissingOwner,
    #[error("an input needs a name")]
    EmptyFieldName,
    #[error("\"{0}\" can't be an input name -- use lowercase letters, digits and underscores")]
    InvalidFieldName(String),
    #[error("\"{0}\" is declared twice as an input")]
    DuplicateField(String),
    #[error("the input \"{0}\" offers a choice between nothing at all")]
    EmptyChoice(String),
    #[error("the input \"{0}\" needs a description, so Hyperion can ask for it in plain words")]
    MissingDescription(String),
}

/// A real identifier check, not a courtesy one: this name becomes a capability id *and* a real
/// directory name under the app root, so anything that is not a bare identifier -- a `/`, a `..`,
/// a leading dash, a NUL -- is refused here rather than sanitized into something surprising.
pub fn validate_app_name(name: &str) -> Result<(), ContractError> {
    if name.is_empty() {
        return Err(ContractError::EmptyName);
    }
    if name.len() > MAX_APP_NAME
        || name.starts_with('-')
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err(ContractError::InvalidName(name.to_string()));
    }
    Ok(())
}

/// Checks a whole contract is answerable: named, with a stated goal, and inputs that are uniquely
/// named, described in plain words, and never a choice between nothing.
pub fn validate_contract(contract: &AppContract) -> Result<(), ContractError> {
    validate_app_name(&contract.name)?;
    if contract.goal.trim().is_empty() {
        return Err(ContractError::MissingGoal);
    }
    // Validated as a real principal name rather than accepted as free text: it is compared against
    // a live principal on every removal, and a name that could never belong to anyone would make
    // an app unremovable by design.
    hyperion_identity::UserId::new(contract.owner.trim())
        .map_err(|_| ContractError::MissingOwner)?;
    let mut seen = BTreeSet::new();
    for field in &contract.fields {
        if field.name.is_empty() {
            return Err(ContractError::EmptyFieldName);
        }
        if !field
            .name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return Err(ContractError::InvalidFieldName(field.name.clone()));
        }
        if !seen.insert(field.name.clone()) {
            return Err(ContractError::DuplicateField(field.name.clone()));
        }
        if field.description.trim().is_empty() {
            return Err(ContractError::MissingDescription(field.name.clone()));
        }
        if let InputKind::Choice(options) = &field.kind {
            if options.is_empty() {
                return Err(ContractError::EmptyChoice(field.name.clone()));
            }
        }
    }
    Ok(())
}

fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if c == '\\' || c == '|' || c == ',' {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Splits on unescaped `delim`, leaving each piece still escaped for the caller to unescape (or
/// to split again, as `choice:` needs).
fn split_escaped(value: &str, delim: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for c in value.chars() {
        if escaped {
            current.push('\\');
            current.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == delim {
            parts.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
    }
    if escaped {
        current.push('\\');
    }
    parts.push(current);
    parts
}

fn encode_kind(kind: &InputKind) -> String {
    match kind {
        InputKind::Text => "text".to_string(),
        InputKind::Integer => "integer".to_string(),
        InputKind::Number => "number".to_string(),
        InputKind::Boolean => "boolean".to_string(),
        InputKind::Path => "path".to_string(),
        InputKind::Choice(options) => {
            let encoded: Vec<String> = options.iter().map(|o| escape(o)).collect();
            format!("choice:{}", encoded.join(","))
        }
    }
}

fn decode_kind(raw: &str) -> Option<InputKind> {
    if let Some(rest) = raw.strip_prefix("choice:") {
        let options: Vec<String> = split_escaped(rest, ',')
            .iter()
            .map(|o| unescape(o))
            .collect();
        if options.iter().any(|o| o.is_empty()) {
            return None;
        }
        return Some(InputKind::Choice(options));
    }
    match unescape(raw).as_str() {
        "text" => Some(InputKind::Text),
        "integer" => Some(InputKind::Integer),
        "number" => Some(InputKind::Number),
        "boolean" => Some(InputKind::Boolean),
        "path" => Some(InputKind::Path),
        _ => None,
    }
}

/// Encodes a whole contract into the `SemanticContract.inputs` strings that get signed.
pub fn encode(contract: &AppContract) -> Vec<String> {
    let mut encoded = vec![format!(
        "{CONTRACT_VERSION}|app|{}|{}|{}|{}",
        escape(&contract.name),
        escape(&contract.owner),
        if contract.keeps_data {
            "keeps-data"
        } else {
            "stateless"
        },
        escape(&contract.goal),
    )];
    encoded.extend(contract.fields.iter().map(|field| {
        format!(
            "{CONTRACT_VERSION}|in|{}|{}|{}|{}",
            escape(&field.name),
            encode_kind(&field.kind),
            if field.required {
                "required"
            } else {
                "optional"
            },
            escape(&field.description),
        )
    }));
    encoded
}

/// The inverse of [`encode`]. `None` means "these inputs are not an app's typed contract" -- an
/// absent or unrecognized header, a malformed entry, or an unknown kind.
///
/// Never a partial decode. A contract that is half-understood is not understood, and acting on
/// half of one would be worse than declining to treat the capability as an app at all: the half
/// that failed to parse is exactly where a required input, or a choice's allowed values, would
/// have been.
pub fn decode(inputs: &[String]) -> Option<AppContract> {
    let (header, rest) = inputs.split_first()?;
    let header_parts = split_escaped(header, '|');
    if header_parts.len() != 6 || header_parts[0] != CONTRACT_VERSION || header_parts[1] != "app" {
        return None;
    }
    let name = unescape(&header_parts[2]);
    let owner = unescape(&header_parts[3]);
    let keeps_data = match header_parts[4].as_str() {
        "keeps-data" => true,
        "stateless" => false,
        // Neither, and therefore not understood. Refused rather than defaulted: guessing
        // "stateless" would silently strip an app of storage it may already have written to.
        _ => return None,
    };
    let goal = unescape(&header_parts[5]);
    if name.is_empty() || owner.is_empty() {
        return None;
    }

    let mut fields = Vec::with_capacity(rest.len());
    for raw in rest {
        let parts = split_escaped(raw, '|');
        if parts.len() != 6 || parts[0] != CONTRACT_VERSION || parts[1] != "in" {
            return None;
        }
        let field_name = unescape(&parts[2]);
        if field_name.is_empty() {
            return None;
        }
        let kind = decode_kind(&parts[3])?;
        let required = match parts[4].as_str() {
            "required" => true,
            "optional" => false,
            _ => return None,
        };
        fields.push(InputField {
            name: field_name,
            kind,
            description: unescape(&parts[5]),
            required,
        });
    }
    Some(AppContract {
        name,
        owner,
        keeps_data,
        goal,
        fields,
    })
}

/// Why a supplied set of arguments could not be used. Every message is written to be shown to a
/// person as-is (docs/01: never expose a technical error) -- which is also why the app's own
/// declared descriptions are quoted back inside them.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArgError {
    #[error("\"{app}\" expects its details as a set of named values")]
    NotAnObject { app: String },
    #[error("\"{app}\" still needs {field} -- {description}")]
    Missing {
        app: String,
        field: String,
        description: String,
    },
    #[error("\"{app}\" doesn't take anything called {field}")]
    Unknown { app: String, field: String },
    #[error("{field} should be {expected}, and \"{got}\" isn't")]
    WrongType {
        field: String,
        expected: String,
        got: String,
    },
    #[error("{field} has to stay inside the app's own folder, so it can't be \"{got}\"")]
    PathEscapes { field: String, got: String },
}

fn render(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn coerce(field: &InputField, value: &serde_json::Value) -> Result<serde_json::Value, ArgError> {
    let wrong = || ArgError::WrongType {
        field: field.name.clone(),
        expected: field.kind.describe(),
        got: render(value),
    };
    match &field.kind {
        InputKind::Text => match value {
            serde_json::Value::String(_) => Ok(value.clone()),
            _ => Err(wrong()),
        },
        // A person typing at a console supplies text, always. Parsing it here is what lets
        // `/run tally count 3` work without asking anyone to think about JSON types -- while
        // `3.5` is still really rejected for an integer, since `parse::<i64>` decides, not
        // rounding.
        InputKind::Integer => match value {
            serde_json::Value::Number(n) if n.is_i64() || n.is_u64() => Ok(value.clone()),
            serde_json::Value::String(s) => s
                .trim()
                .parse::<i64>()
                .map(|parsed| serde_json::json!(parsed))
                .map_err(|_| wrong()),
            _ => Err(wrong()),
        },
        InputKind::Number => match value {
            serde_json::Value::Number(_) => Ok(value.clone()),
            serde_json::Value::String(s) => s
                .trim()
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(serde_json::Value::Number)
                .ok_or_else(wrong),
            _ => Err(wrong()),
        },
        InputKind::Boolean => match value {
            serde_json::Value::Bool(_) => Ok(value.clone()),
            serde_json::Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
                "true" | "yes" | "y" | "on" | "1" => Ok(serde_json::json!(true)),
                "false" | "no" | "n" | "off" | "0" => Ok(serde_json::json!(false)),
                _ => Err(wrong()),
            },
            _ => Err(wrong()),
        },
        InputKind::Path => {
            let serde_json::Value::String(raw) = value else {
                return Err(wrong());
            };
            let path = std::path::Path::new(raw);
            // Refused, never sanitized: the sandbox grants exactly one directory, so a path
            // reaching outside it could only ever fail confusingly *inside* the sandbox. An
            // honest refusal here, in words, before anything is spawned, is the better outcome.
            if path.is_absolute()
                || path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(ArgError::PathEscapes {
                    field: field.name.clone(),
                    got: raw.clone(),
                });
            }
            Ok(value.clone())
        }
        InputKind::Choice(options) => match value {
            serde_json::Value::String(s) if options.iter().any(|o| o == s) => Ok(value.clone()),
            _ => Err(wrong()),
        },
    }
}

/// Checks a caller's arguments against an app's declared contract, returning the normalized
/// object the app will actually receive as its `input.json`.
///
/// This runs *before* anything is spawned. That ordering is the point: a wrong argument becomes a
/// sentence a person can act on, instead of a sandboxed process exiting non-zero for reasons
/// nobody can see.
pub fn validate_args(
    app: &str,
    fields: &[InputField],
    args: &serde_json::Value,
) -> Result<serde_json::Value, ArgError> {
    let serde_json::Value::Object(supplied) = args else {
        return Err(ArgError::NotAnObject {
            app: app.to_string(),
        });
    };

    let declared: BTreeSet<&str> = fields.iter().map(|f| f.name.as_str()).collect();
    for key in supplied.keys() {
        if !declared.contains(key.as_str()) {
            return Err(ArgError::Unknown {
                app: app.to_string(),
                field: key.clone(),
            });
        }
    }

    let mut normalized = serde_json::Map::new();
    for field in fields {
        match supplied.get(&field.name) {
            Some(value) => {
                normalized.insert(field.name.clone(), coerce(field, value)?);
            }
            None if field.required => {
                return Err(ArgError::Missing {
                    app: app.to_string(),
                    field: field.name.clone(),
                    description: field.description.clone(),
                })
            }
            None => {}
        }
    }
    Ok(serde_json::Value::Object(normalized))
}
