//! A companion binary for `hyperion-plugin-framework`'s `tests/native_binary_execution.rs` --
//! a real, tiny `NativeBinary` capability implementation, statically built for the
//! `x86_64-unknown-linux-musl` target so it needs nothing outside its own sandboxed fs_scope to
//! start (a dynamically linked binary needs to *read* the host's own dynamic linker/libc from
//! `/lib`/`/usr/lib`, outside any real `fs_scope` -- see
//! `hyperion-trust-boundary/tests/enforcement.rs`'s own `probe_bin()` doc comment for the exact
//! same reasoning, already proven in this workspace). Reads `input.json` (argv[1]), uppercases
//! its `"text"` field, writes `output.json` (argv[2]) -- exactly the real contract
//! `PluginRegistry::invoke_native_binary` expects of any `NativeBinary` implementation.

use std::env;
use std::fs;

fn main() {
    let input_path = env::args()
        .nth(1)
        .expect("usage: uppercase_tool <input.json> <output.json>");
    let output_path = env::args()
        .nth(2)
        .expect("usage: uppercase_tool <input.json> <output.json>");

    let input_bytes = fs::read(&input_path).expect("read input.json");
    let input: serde_json::Value =
        serde_json::from_slice(&input_bytes).expect("parse input.json as real JSON");

    let text = input.get("text").and_then(|v| v.as_str()).unwrap_or("");
    let output = serde_json::json!({ "text": text.to_ascii_uppercase() });

    fs::write(
        &output_path,
        serde_json::to_vec(&output).expect("serialize output.json"),
    )
    .expect("write output.json");
}
