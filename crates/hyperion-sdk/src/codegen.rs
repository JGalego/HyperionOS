//! Real "tool creation from scratch" gate (docs/998-roadmap.md's Autonomy Roadmap,
//! "Resourceful — use existing tools, create new ones"): before any freshly generated Rust
//! source is ever executed, it must really compile and really pass `cargo clippy -D warnings`,
//! and it must not contain `unsafe` anywhere. This is the "real code review/static analysis of
//! freshly generated code" this crate's own doc comment previously named as separate,
//! substantial work not yet attempted — this module is that work, not a second deferral.

use std::fs;
use std::path::Path;
use std::process::Command;

use hyperion_plugin_framework::NativeBinaryDescriptor;

/// Freshly generated source for a capability implementation, named but not yet trusted.
#[derive(Debug, Clone)]
pub struct GeneratedSource {
    /// A real `fn main()` Rust program's full source text. Generated code always runs as its
    /// own process — the same [`NativeBinaryDescriptor`] shape every other native-binary
    /// capability already uses — never loaded in-process, so a rejected program can never have
    /// already run.
    pub source: String,
    /// A filesystem- and cargo-package-name-safe identifier for the scratch package this
    /// source is built as.
    pub package_name: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CodegenRejection {
    #[error("generated source contains 'unsafe', which this gate never allows through")]
    UnsafeCodeForbidden,
    #[error("generated source failed to build:\n{0}")]
    BuildFailed(String),
    #[error("generated source failed clippy's real lint gate:\n{0}")]
    ClippyFailed(String),
    #[error("scratch build directory could not be prepared: {0}")]
    Io(String),
}

fn cargo_toml_for(package_name: &str) -> String {
    format!(
        "[package]\nname = \"{package_name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [[bin]]\nname = \"{package_name}\"\npath = \"src/main.rs\"\n"
    )
}

/// Closes docs/998-roadmap.md's "tool creation" gap for real: `generated.source` is written
/// into a real, throwaway cargo package under `workspace_root` and really built (`cargo build
/// --release`) and really linted (`cargo clippy -- -D warnings`) — both real subprocesses, not
/// a simulated pass/fail. A source string containing `unsafe` is rejected before either
/// subprocess ever runs — a textual ban, deliberately stricter than clippy's own default
/// `unsafe_code` allowance, since a generated tool has no human reviewer standing by to explain
/// an `unsafe` block. Only a source that survives all three checks becomes a real, runnable
/// [`NativeBinaryDescriptor`] pointing at the real compiled binary — the same descriptor shape
/// `hyperion_plugin_framework::PluginRegistry::invoke_native_binary` already knows how to run
/// inside a real sandbox, so a freshly generated tool is invoked through the exact same trust
/// boundary as a hand-written one, never a separate, less-audited path.
pub fn review_and_build(
    generated: &GeneratedSource,
    workspace_root: &Path,
) -> Result<NativeBinaryDescriptor, CodegenRejection> {
    if generated.source.contains("unsafe") {
        return Err(CodegenRejection::UnsafeCodeForbidden);
    }

    let package_dir = workspace_root.join(&generated.package_name);
    let src_dir = package_dir.join("src");
    fs::create_dir_all(&src_dir).map_err(|e| CodegenRejection::Io(e.to_string()))?;
    fs::write(
        package_dir.join("Cargo.toml"),
        cargo_toml_for(&generated.package_name),
    )
    .map_err(|e| CodegenRejection::Io(e.to_string()))?;
    fs::write(src_dir.join("main.rs"), &generated.source)
        .map_err(|e| CodegenRejection::Io(e.to_string()))?;

    let build = Command::new("cargo")
        .args(["build", "--release", "--offline"])
        .current_dir(&package_dir)
        .output()
        .map_err(|e| CodegenRejection::Io(e.to_string()))?;
    if !build.status.success() {
        return Err(CodegenRejection::BuildFailed(
            String::from_utf8_lossy(&build.stderr).into_owned(),
        ));
    }

    let clippy = Command::new("cargo")
        .args(["clippy", "--release", "--offline", "--", "-D", "warnings"])
        .current_dir(&package_dir)
        .output()
        .map_err(|e| CodegenRejection::Io(e.to_string()))?;
    if !clippy.status.success() {
        return Err(CodegenRejection::ClippyFailed(
            String::from_utf8_lossy(&clippy.stderr).into_owned(),
        ));
    }

    Ok(NativeBinaryDescriptor {
        program: package_dir
            .join("target")
            .join("release")
            .join(&generated.package_name),
        args: Vec::new(),
    })
}
