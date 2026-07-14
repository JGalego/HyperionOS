//! A companion binary for `hyperion-plugin-framework`'s `tests/native_binary_execution.rs` --
//! a real `NativeBinary` implementation that always exits with a real, nonzero status, to prove
//! `PluginRegistry::invoke_native_binary` surfaces that as an honest `PluginError::ExecutionFailed`
//! rather than a panic or silent success. Statically built for `x86_64-unknown-linux-musl` for the
//! same reason `uppercase_tool` is -- see that binary's own doc comment.

fn main() {
    std::process::exit(3);
}
