//! Turning a model's answer into an [`AppDefinition`].
//!
//! Generation itself is deliberately *not* this crate's job -- [`crate::registry::AppRegistry`]
//! stays deterministic and really testable because nothing in it calls a model. What lives here is
//! the other half: reading back what a model produced, strictly, and saying plainly what was wrong
//! when it isn't usable. That half is pure, so it is tested against real malformed answers rather
//! than hoped about.

use crate::types::{AppDefinition, InputField, InputKind};

/// What to ask a model for. Kept next to the parser that reads the answer, so the two can never
/// drift into describing different shapes.
pub const APP_PLAN_INSTRUCTIONS: &str = "\
Reply with one JSON object and nothing else -- no prose, no markdown fence. Shape:
{
  \"name\": short-lowercase-identifier (letters, digits, dashes, underscores),
  \"goal\": one sentence, in the person's own words, saying what this is for,
  \"inputs\": [ { \"name\": lowercase_identifier,
                 \"kind\": one of text|integer|number|boolean|path|choice,
                 \"choices\": [..] (only when kind is choice),
                 \"required\": true|false,
                 \"description\": plain language, shown when asking a person for this } ],
  \"script\": the complete script source
}
The script is run as: <interpreter> <script> <input.json> <output.json>.
Read the named inputs from the JSON object in input.json. Write a JSON object to output.json with
a \"result\" key holding what a person should see. It runs with no network access and may only
touch the directory those two files are in.";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    #[error("I couldn't tell what to build from that answer")]
    NoJsonObject,
    #[error("I couldn't read the plan for that app: {0}")]
    Malformed(String),
    #[error("the plan didn't say what the app should be called")]
    MissingName,
    #[error("the plan didn't include anything to run")]
    MissingScript,
    #[error("the plan describes an input of a kind I don't know how to ask for: \"{0}\"")]
    UnknownKind(String),
    #[error("the plan describes a choice input (\"{0}\") without saying what the choices are")]
    ChoiceWithoutOptions(String),
}

/// Finds the one JSON object in `text`, tolerating what models really do around it.
///
/// A model told to reply with only JSON will still sometimes wrap it in a markdown fence or a
/// sentence of preamble. Refusing those would fail on an answer whose actual content is perfectly
/// good -- so this scans for the first `{` and its matching `}`, tracking string literals and
/// escapes so a brace inside the script's own source text doesn't end the object early.
fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, &byte) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..=offset]);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_kind(
    raw: &str,
    choices: Option<&serde_json::Value>,
    field: &str,
) -> Result<InputKind, PlanError> {
    match raw {
        "text" | "string" => Ok(InputKind::Text),
        "integer" | "int" => Ok(InputKind::Integer),
        "number" | "float" => Ok(InputKind::Number),
        "boolean" | "bool" => Ok(InputKind::Boolean),
        "path" | "file" => Ok(InputKind::Path),
        "choice" | "enum" => {
            let options: Vec<String> = choices
                .and_then(|c| c.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            if options.is_empty() {
                return Err(PlanError::ChoiceWithoutOptions(field.to_string()));
            }
            Ok(InputKind::Choice(options))
        }
        other => Err(PlanError::UnknownKind(other.to_string())),
    }
}

/// Reads a model's answer into a definition ready for [`crate::registry::AppRegistry::build`].
///
/// Deliberately does no validation of its own beyond what it takes to *build the struct*: names,
/// descriptions and duplicate fields are `AppRegistry::build`'s own
/// [`crate::contract::validate_contract`] to reject, and having one place that decides what a
/// legal contract is matters more than failing a few lines earlier.
pub fn from_model_answer(answer: &str, engine_id: &str) -> Result<AppDefinition, PlanError> {
    let json = extract_json_object(answer).ok_or(PlanError::NoJsonObject)?;
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| PlanError::Malformed(e.to_string()))?;

    let name = value
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or(PlanError::MissingName)?
        .to_ascii_lowercase();
    let script = value
        .get("script")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or(PlanError::MissingScript)?
        .to_string();
    let goal = value
        .get("goal")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();

    let mut inputs = Vec::new();
    if let Some(declared) = value.get("inputs").and_then(|v| v.as_array()) {
        for entry in declared {
            let field_name = entry
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .trim()
                .to_string();
            let kind = parse_kind(
                entry.get("kind").and_then(|v| v.as_str()).unwrap_or("text"),
                entry.get("choices"),
                &field_name,
            )?;
            inputs.push(InputField {
                name: field_name,
                kind,
                description: entry
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
                // Absent means required. An input a model forgot to mark is far more likely to be
                // one the script needs than one it can do without, and being asked for something
                // unnecessary is a much smaller harm than a script failing for a missing value.
                required: entry
                    .get("required")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
            });
        }
    }

    Ok(AppDefinition {
        name,
        goal,
        engine_id: engine_id.to_string(),
        script,
        inputs,
    })
}
