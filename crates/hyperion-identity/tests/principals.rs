//! Principals: stable across restarts, never colliding, and honest about what they aren't.

use hyperion_capability::TrustBoundaryId;
use hyperion_identity::{Principal, PrincipalRegistry, UserId, UserIdError};

fn registry(dir: &tempfile::TempDir) -> PrincipalRegistry {
    PrincipalRegistry::open_or_create(dir.path().join("users.json")).expect("open")
}

#[test]
fn two_people_get_genuinely_different_authorities() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = registry(&dir);
    let alice = registry.principal_for("alice").unwrap();
    let bob = registry.principal_for("bob").unwrap();

    assert_ne!(alice.boundary, bob.boundary);
    // Their scopes are what keep data apart, so those must differ too -- a shared scope would
    // separate the authority and then store both users' data in the same place anyway.
    assert_ne!(alice.scope(), bob.scope());
}

#[test]
fn the_same_person_is_the_same_authority_across_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let first_boot = {
        let mut registry = registry(&dir);
        registry.principal_for("alice").unwrap()
    };
    let second_boot = {
        let mut registry = registry(&dir);
        registry.principal_for("alice").unwrap()
    };

    // A boundary that changed across a restart would orphan everything scoped to it: every
    // explanation record filtered by it, and every capability derived from it.
    assert_eq!(first_boot, second_boot);
}

#[test]
fn a_new_person_never_inherits_a_departed_ones_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = registry(&dir);
    let alice = registry.principal_for("alice").unwrap();
    let bob = registry.principal_for("bob").unwrap();
    let carol = registry.principal_for("carol").unwrap();

    let boundaries = [alice.boundary, bob.boundary, carol.boundary];
    for (i, one) in boundaries.iter().enumerate() {
        for other in &boundaries[i + 1..] {
            assert_ne!(one, other, "every principal needs its own boundary");
        }
    }
}

#[test]
fn a_users_boundary_never_collides_with_a_hand_minted_one() {
    // Callers across this workspace mint their own root tokens at `TrustBoundaryId(1)` and other
    // low ids. A user landing on one of those would silently share an identity with an unrelated
    // authority, and the separation this crate exists for would not be true.
    let dir = tempfile::tempdir().unwrap();
    let mut registry = registry(&dir);
    for name in ["alice", "bob", "carol"] {
        let principal = registry.principal_for(name).unwrap();
        assert!(
            principal.boundary.0 >= 1_000,
            "{name} landed on {:?}, in the range callers mint by hand",
            principal.boundary
        );
    }
}

#[test]
fn a_name_that_could_escape_its_own_directory_is_refused() {
    for bad in [
        "../etc",
        "a/b",
        "Alice",
        "",
        ".hidden",
        "-lead",
        &"x".repeat(65),
    ] {
        assert!(
            UserId::new(bad).is_err(),
            "{bad:?} should not be a legal user name"
        );
    }
    assert!(UserId::new("alice.smith_2").is_ok());
}

#[test]
fn an_empty_name_says_so_rather_than_failing_obscurely() {
    assert_eq!(UserId::new("").unwrap_err(), UserIdError::Empty);
}

#[test]
fn the_registry_remembers_everyone_it_has_seen() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = registry(&dir);
    registry.principal_for("bob").unwrap();
    registry.principal_for("alice").unwrap();

    assert_eq!(
        registry.known_users(),
        vec![UserId::new("alice").unwrap(), UserId::new("bob").unwrap()]
    );
    assert!(registry.knows(&UserId::new("alice").unwrap()));
    assert!(!registry.knows(&UserId::new("carol").unwrap()));
}

#[test]
fn a_damaged_registry_is_an_error_rather_than_a_silent_fresh_start() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("users.json");
    {
        let mut registry = PrincipalRegistry::open_or_create(&path).unwrap();
        registry.principal_for("alice").unwrap();
    }
    std::fs::write(&path, "{ this is not json").unwrap();

    // Quietly starting again would re-mint every boundary, which is indistinguishable from every
    // user's history vanishing.
    assert!(PrincipalRegistry::open_or_create(&path).is_err());
}

#[test]
fn a_scope_is_built_from_the_name_rather_than_the_boundary() {
    // A boundary is allocated per device; a name is what the person actually is. Scoping data by
    // the boundary would make the same person's data unreachable if the registry were rebuilt.
    let principal = Principal {
        user: UserId::new("alice").unwrap(),
        boundary: TrustBoundaryId(1_000),
    };
    let relocated = Principal {
        user: UserId::new("alice").unwrap(),
        boundary: TrustBoundaryId(2_000),
    };
    assert_eq!(principal.scope(), relocated.scope());
}
