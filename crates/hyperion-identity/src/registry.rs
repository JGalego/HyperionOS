//! Which people this device knows, and which Trust Boundary each one is.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use hyperion_capability::TrustBoundaryId;

use crate::types::{Principal, UserId, UserIdError};

/// The first boundary id this registry will hand out.
///
/// Deliberately above the low ids callers in this workspace mint by hand for their own root tokens
/// (`TrustBoundaryId(1)` appears throughout). A user's boundary must never collide with one of
/// those, or two unrelated authorities would share an identity and the separation this crate
/// exists for would be silently untrue.
const FIRST_USER_BOUNDARY: u64 = 1_000;

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("{0}")]
    UserId(#[from] UserIdError),
    #[error("I couldn't read who uses this device: {0}")]
    Read(String),
    #[error("I couldn't record who uses this device: {0}")]
    Write(String),
}

/// The people this device knows, and the boundary that is each one's authority.
///
/// Persisted, because a boundary that changed across a restart would orphan everything scoped to
/// it — every explanation record filtered by it, and every capability derived from it. The mapping
/// has to outlive the process for the separation to mean anything past one boot.
///
/// Deliberately not encrypted and not secret: it holds names and integers, no credentials — there
/// are no credentials in this crate to hold. Knowing that a device has users named `alice` and
/// `bob` is not the thing that needs protecting; what each of them stored is, and that is
/// protected by the per-principal key derivation those names feed.
pub struct PrincipalRegistry {
    path: PathBuf,
    boundaries: BTreeMap<UserId, u64>,
    next_boundary: u64,
}

impl PrincipalRegistry {
    /// Opens the registry at `path`, or starts an empty one if nothing is there yet.
    ///
    /// A malformed or hand-edited file is a real error rather than a silent fresh start: quietly
    /// beginning again would re-mint every user's boundary, which reads as "everyone's history
    /// vanished" and is indistinguishable from data loss.
    pub fn open_or_create(path: impl AsRef<Path>) -> Result<Self, RegistryError> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Ok(PrincipalRegistry {
                path,
                boundaries: BTreeMap::new(),
                next_boundary: FIRST_USER_BOUNDARY,
            });
        }

        let raw = std::fs::read_to_string(&path).map_err(|e| RegistryError::Read(e.to_string()))?;
        let value: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| RegistryError::Read(e.to_string()))?;
        let entries = value
            .get("users")
            .and_then(|u| u.as_object())
            .ok_or_else(|| RegistryError::Read("it isn't in the shape I wrote it".to_string()))?;

        let mut boundaries = BTreeMap::new();
        for (name, boundary) in entries {
            let user = UserId::new(name)?;
            let boundary = boundary
                .as_u64()
                .ok_or_else(|| RegistryError::Read(format!("{name}'s entry isn't a number")))?;
            boundaries.insert(user, boundary);
        }
        let next_boundary = boundaries
            .values()
            .copied()
            .max()
            .map(|highest| highest + 1)
            .unwrap_or(FIRST_USER_BOUNDARY);

        Ok(PrincipalRegistry {
            path,
            boundaries,
            next_boundary,
        })
    }

    /// The principal for `name`, registering them if this device hasn't seen them before.
    ///
    /// Registering on first use rather than through a separate "create user" step: without
    /// authentication there is nothing to set up for a new person — no password to choose, no key
    /// to escrow — so a separate step would exist only to be a step. When authentication lands,
    /// *that* is what a real enrolment flow attaches to.
    ///
    /// A user's boundary is allocated once and never reallocated, so the same name always resolves
    /// to the same authority, across restarts.
    pub fn principal_for(&mut self, name: &str) -> Result<Principal, RegistryError> {
        let user = UserId::new(name)?;
        if let Some(&boundary) = self.boundaries.get(&user) {
            return Ok(Principal {
                user,
                boundary: TrustBoundaryId(boundary),
            });
        }

        let boundary = self.next_boundary;
        self.next_boundary += 1;
        self.boundaries.insert(user.clone(), boundary);
        self.persist()?;
        Ok(Principal {
            user,
            boundary: TrustBoundaryId(boundary),
        })
    }

    /// Everyone this device knows, in a stable order.
    pub fn known_users(&self) -> Vec<UserId> {
        self.boundaries.keys().cloned().collect()
    }

    /// `true` if this device has seen `user` before.
    pub fn knows(&self, user: &UserId) -> bool {
        self.boundaries.contains_key(user)
    }

    fn persist(&self) -> Result<(), RegistryError> {
        let users: serde_json::Map<String, serde_json::Value> = self
            .boundaries
            .iter()
            .map(|(user, boundary)| (user.to_string(), serde_json::json!(boundary)))
            .collect();
        let document = serde_json::json!({ "users": users });
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| RegistryError::Write(e.to_string()))?;
        }
        std::fs::write(
            &self.path,
            serde_json::to_vec_pretty(&document)
                .map_err(|e| RegistryError::Write(e.to_string()))?,
        )
        .map_err(|e| RegistryError::Write(e.to_string()))
    }
}
