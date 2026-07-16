//! A companion binary for `hyperion-sdk`'s own `tests/resolve_via_engine.rs` -- a real
//! `ExecutionEngine` launcher, statically built for `x86_64-unknown-linux-musl` for the same
//! reason `uppercase_tool` is (see that binary's own doc comment). Real argv shape a launcher
//! actually receives once `PluginRegistry::invoke_native_binary` runs the `NativeBinaryDescriptor`
//! `hyperion_sdk::resolve_via_engine` built: `[...engine's own launcher args, script, input.json,
//! output.json]` -- the last two are always `invoke_native_binary`'s own trailing args, so this
//! reads them from the *end* of argv and treats everything in between as the script path this
//! engine was asked to run, proving that path really threaded through end to end by echoing it
//! back in the real output.

use std::env;
use std::fs;

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let output_path = args
        .pop()
        .expect("usage: engine_echo_tool [...] <script> <input.json> <output.json>");
    let input_path = args
        .pop()
        .expect("usage: engine_echo_tool [...] <script> <input.json> <output.json>");
    let script = args.pop().unwrap_or_default();

    let input_bytes = fs::read(&input_path).expect("read input.json");
    let input: serde_json::Value =
        serde_json::from_slice(&input_bytes).expect("parse input.json as real JSON");
    let text = input.get("text").and_then(|v| v.as_str()).unwrap_or("");

    let output = serde_json::json!({ "text": text, "received_script": script });
    fs::write(
        &output_path,
        serde_json::to_vec(&output).expect("serialize output.json"),
    )
    .expect("write output.json");
}
