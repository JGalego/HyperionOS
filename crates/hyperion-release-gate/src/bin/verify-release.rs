//! PRODUCTION_BOOT_PROMPT.md M13: verifies a real release image against its own real
//! `sign-release`-produced manifest -- recomputes the image's real BLAKE3 hash directly from its
//! bytes (never trusts the manifest's own recorded hash blindly) and checks the real Ed25519
//! signature against the manifest's own recorded verifying key. Exit code 0 means both checks
//! passed; nonzero means the image is either corrupted (hash mismatch) or the signature doesn't
//! verify (tampered, or signed by a different key than the one recorded).
//!
//! Usage: verify-release <image-path> <manifest-path>

use std::env;
use std::fs;
use std::path::PathBuf;

use hyperion_crypto::{Hash, Signature, VerifyingKey};
use serde::Deserialize;

#[derive(Deserialize)]
struct Manifest {
    blake3_hash: Hash,
    ed25519_signature: Signature,
    verifying_key: VerifyingKey,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: verify-release <image-path> <manifest-path>");
        std::process::exit(2);
    }
    let image_path = PathBuf::from(&args[1]);
    let manifest_path = PathBuf::from(&args[2]);

    let bytes = fs::read(&image_path).unwrap_or_else(|e| {
        eprintln!("failed to read {}: {e}", image_path.display());
        std::process::exit(1);
    });
    let manifest_bytes = fs::read(&manifest_path).unwrap_or_else(|e| {
        eprintln!("failed to read {}: {e}", manifest_path.display());
        std::process::exit(1);
    });
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes).unwrap_or_else(|e| {
        eprintln!("failed to parse manifest {}: {e}", manifest_path.display());
        std::process::exit(1);
    });

    let real_hash = hyperion_crypto::hash(&bytes);
    if real_hash != manifest.blake3_hash {
        eprintln!(
            "FAIL: {} does not match the manifest's recorded BLAKE3 hash -- corrupted, or the \
             wrong file for this manifest",
            image_path.display()
        );
        std::process::exit(1);
    }

    if !hyperion_crypto::verify(
        real_hash.as_bytes(),
        &manifest.ed25519_signature,
        &manifest.verifying_key,
    ) {
        eprintln!(
            "FAIL: signature does not verify against the manifest's own recorded verifying key"
        );
        std::process::exit(1);
    }

    println!(
        "PASS: {} matches its real BLAKE3 hash and its real Ed25519 signature verifies",
        image_path.display()
    );
}
