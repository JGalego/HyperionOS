//! docs/998-roadmap.md M13: signs a real release image with this device's real Ed25519
//! keystore (M9's `hyperion_crypto::Keystore`, the same real device identity every other signed
//! artifact in this workspace -- model descriptors, plugin manifests, update manifests -- already
//! verifies against) and writes a real, versioned manifest alongside it: a real BLAKE3 hash of the
//! image's own bytes, a real signature over that hash, and the verifying key a fresh device would
//! need to check it. Not part of the Cargo workspace's own test/build path for anything else --
//! invoked directly by `boot/scripts/package-release.sh` at release-packaging time.
//!
//! Usage: sign-release <image-path> <keystore-path> <version> <platform>

use std::env;
use std::fs;
use std::path::PathBuf;

use hyperion_crypto::Keystore;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 5 {
        eprintln!("usage: sign-release <image-path> <keystore-path> <version> <platform>");
        std::process::exit(2);
    }
    let image_path = PathBuf::from(&args[1]);
    let keystore_path = PathBuf::from(&args[2]);
    let version = &args[3];
    let platform = &args[4];

    let bytes = fs::read(&image_path).unwrap_or_else(|e| {
        eprintln!("failed to read {}: {e}", image_path.display());
        std::process::exit(1);
    });

    let hash = hyperion_crypto::hash(&bytes);
    let keystore = Keystore::open_or_create(&keystore_path).unwrap_or_else(|e| {
        eprintln!(
            "failed to open keystore at {}: {e}",
            keystore_path.display()
        );
        std::process::exit(1);
    });
    let signature = keystore.sign(hash.as_bytes());

    let manifest = serde_json::json!({
        "version": version,
        "platform": platform,
        "image_file": image_path.file_name().unwrap().to_string_lossy(),
        "image_size_bytes": bytes.len(),
        "blake3_hash": hash,
        "ed25519_signature": signature,
        "verifying_key": keystore.verifying_key(),
    });

    let manifest_path = PathBuf::from(format!("{}.release.json", image_path.display()));
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest)
            .expect("manifest is plain, always-serializable data"),
    )
    .unwrap_or_else(|e| {
        eprintln!("failed to write {}: {e}", manifest_path.display());
        std::process::exit(1);
    });

    println!(
        "Signed {} v{version} ({platform}): {}",
        image_path.display(),
        manifest_path.display()
    );
}
