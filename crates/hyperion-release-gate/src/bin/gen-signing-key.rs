//! PRODUCTION_BOOT_PROMPT.md M13 release pipeline: generates a NEW real Ed25519 release-signing
//! keystore at the given path and prints its verifying key in hex -- the value to publish
//! alongside releases (e.g. in the README) so a download can be checked against a known-good key
//! independent of anything recorded inside a downloaded manifest itself. Refuses to overwrite an
//! existing keystore file, since silently replacing a release-signing key would orphan every
//! previously-published release's verification path.
//!
//! The keystore file this writes holds the raw 32-byte private seed -- treat it exactly like a
//! private key (e.g. `base64 -w0 <path>` to get the value for `gh secret set`) and never commit
//! it to the repository.
//!
//! Usage: gen-signing-key <keystore-output-path>

use std::env;
use std::path::PathBuf;

use hyperion_crypto::Keystore;

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: gen-signing-key <keystore-output-path>");
        std::process::exit(2);
    }
    let keystore_path = PathBuf::from(&args[1]);

    if keystore_path.exists() {
        eprintln!(
            "refusing to overwrite existing keystore at {} -- move it aside first if you really \
             mean to rotate the release-signing key",
            keystore_path.display()
        );
        std::process::exit(1);
    }

    let keystore = Keystore::open_or_create(&keystore_path).unwrap_or_else(|e| {
        eprintln!(
            "failed to create keystore at {}: {e}",
            keystore_path.display()
        );
        std::process::exit(1);
    });

    println!(
        "Generated a new real Ed25519 keystore at {}",
        keystore_path.display()
    );
    println!(
        "Verifying key (hex, public -- safe to publish): {}",
        to_hex(keystore.verifying_key().as_bytes())
    );
    println!(
        "Private seed: the file at {} itself. Treat it as a secret -- e.g. `base64 -w0 {}` to \
         get the value for `gh secret set`.",
        keystore_path.display(),
        keystore_path.display()
    );
}
