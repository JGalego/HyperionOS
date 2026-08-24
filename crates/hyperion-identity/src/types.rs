//! A principal, and the name a person is known by.

use hyperion_capability::TrustBoundaryId;

/// The longest a user name may be. Long enough for a real name or an email local-part, short
/// enough to stay readable everywhere it is displayed.
pub const MAX_USER_ID: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UserIdError {
    #[error("a user needs a name")]
    Empty,
    #[error(
        "\"{0}\" can't be a user name -- use lowercase letters, digits, dashes, underscores and \
         dots, up to {MAX_USER_ID} characters"
    )]
    Invalid(String),
}

/// The stable name one person is known by on this device.
///
/// Validated as an identifier rather than sanitized into one, for the same reason an app name is:
/// it becomes a directory name, a key-derivation context, and a memory-scope key. Anything that is
/// not a bare identifier — a `/`, a `..`, a leading dot — is refused here rather than quietly
/// turned into something surprising somewhere further down.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UserId(String);

impl UserId {
    pub fn new(name: &str) -> Result<Self, UserIdError> {
        if name.is_empty() {
            return Err(UserIdError::Empty);
        }
        let legal = name.len() <= MAX_USER_ID
            && !name.starts_with('-')
            && !name.starts_with('.')
            && name.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' || c == '.'
            });
        if !legal {
            return Err(UserIdError::Invalid(name.to_string()));
        }
        Ok(UserId(name.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for UserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One person, and the Trust Boundary that *is* their authority on this device.
///
/// See this crate's own doc comment for why the boundary is the principal rather than a subject
/// field on a capability token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub user: UserId,
    pub boundary: TrustBoundaryId,
}

impl Principal {
    /// A stable, collision-free namespace string for this principal.
    ///
    /// Used for three separate things that must never bleed into one another: the per-user
    /// directory under a data root, the key-derivation context that separates one user's encrypted
    /// secrets from another's, and the session scope that keeps working memory apart. One function
    /// so those three can never drift into disagreeing about who a scope belongs to.
    ///
    /// Built from the user name, not the boundary id: a boundary is allocated per device and would
    /// make the same person's data unreachable if the registry were ever rebuilt, whereas the name
    /// is what the person actually is.
    pub fn scope(&self) -> String {
        format!("user.{}", self.user)
    }
}
