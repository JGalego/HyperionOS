//! docs/24's "execution engines register runtimes usable by Capability implementations" gap,
//! closed for real: a plugin's own `hyperion_plugin_framework::Contribution::ExecutionEngine`
//! supplies a reusable launcher; this module turns a caller's own script into a concrete,
//! runnable `NativeBinaryDescriptor` by prepending that launcher — so a capability published
//! "via" an engine ends up installed and invoked through the exact same
//! `ImplementationKind::NativeBinary` path a hand-written native binary already uses, never a
//! second, parallel execution mechanism.

use std::path::PathBuf;

use hyperion_plugin_framework::{NativeBinaryDescriptor, PluginRegistry};

use crate::types::SdkError;

/// Looks `engine_id` up in `registry` (an installed, non-quarantined
/// `Contribution::ExecutionEngine`) and prepends its own real launcher to `script` and
/// `script_args`, producing a `NativeBinaryDescriptor` ready for
/// [`crate::publish::to_plugin_manifest`]/[`crate::publish::publish`] the exact same way a
/// hand-written `NativeBinary` implementation's own descriptor already is. Fails with
/// [`SdkError::UnknownExecutionEngine`] if no installed plugin ever contributed this
/// `engine_id` — a capability can't silently publish against an engine that was never real.
pub fn resolve_via_engine(
    registry: &PluginRegistry,
    engine_id: &str,
    script: PathBuf,
    script_args: Vec<String>,
) -> Result<NativeBinaryDescriptor, SdkError> {
    let engine = registry
        .execution_engine(engine_id)
        .ok_or_else(|| SdkError::UnknownExecutionEngine(engine_id.to_string()))?;

    let mut args = engine.launcher.args.clone();
    args.push(script.to_string_lossy().into_owned());
    args.extend(script_args);

    Ok(NativeBinaryDescriptor {
        program: engine.launcher.program,
        args,
    })
}
