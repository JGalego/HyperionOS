//! docs/998-roadmap.md's "tool creation" gap, proven end to end: freshly generated Rust source
//! text is really compiled and really linted by a real `cargo`/`cargo clippy` subprocess, never
//! a simulated pass/fail. Each rejection path is proven to actually reject, and the one source
//! that survives every check produces a real binary this test really executes.

use std::process::Command;

use hyperion_sdk::{review_and_build, CodegenRejection, GeneratedSource};

#[test]
fn clean_source_really_compiles_lints_and_runs() {
    let workspace = tempfile::tempdir().unwrap();
    let generated = GeneratedSource {
        source: r#"
fn main() {
    println!("hello from a generated capability");
}
"#
        .to_string(),
        package_name: "generated-clean-tool".to_string(),
    };

    let descriptor = review_and_build(&generated, workspace.path())
        .expect("clean, unsafe-free source that builds and lints clean must be accepted");

    assert!(
        descriptor.program.exists(),
        "compiled binary must really exist on disk"
    );
    let output = Command::new(&descriptor.program)
        .output()
        .expect("the compiled binary must really run");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "hello from a generated capability"
    );
}

#[test]
fn unsafe_source_is_rejected_before_any_subprocess_runs() {
    let workspace = tempfile::tempdir().unwrap();
    let generated = GeneratedSource {
        source: r#"
fn main() {
    let ptr = std::ptr::null::<i32>();
    unsafe {
        println!("{:?}", ptr.is_null());
    }
}
"#
        .to_string(),
        package_name: "generated-unsafe-tool".to_string(),
    };

    let result = review_and_build(&generated, workspace.path());
    assert!(matches!(result, Err(CodegenRejection::UnsafeCodeForbidden)));
    assert!(
        !workspace.path().join("generated-unsafe-tool").exists(),
        "an unsafe-rejected source must never even reach the scratch build directory"
    );
}

#[test]
fn source_that_fails_to_compile_is_rejected() {
    let workspace = tempfile::tempdir().unwrap();
    let generated = GeneratedSource {
        source: r#"
fn main() {
    let x: i32 = "this is not a number";
    println!("{x}");
}
"#
        .to_string(),
        package_name: "generated-broken-tool".to_string(),
    };

    let result = review_and_build(&generated, workspace.path());
    assert!(matches!(result, Err(CodegenRejection::BuildFailed(_))));
}

#[test]
fn source_that_compiles_but_fails_clippy_is_rejected() {
    let workspace = tempfile::tempdir().unwrap();
    let generated = GeneratedSource {
        source: r#"
fn main() {
    let x = true;
    if x == true {
        println!("yes");
    }
}
"#
        .to_string(),
        package_name: "generated-clippy-fail-tool".to_string(),
    };

    let result = review_and_build(&generated, workspace.path());
    assert!(matches!(result, Err(CodegenRejection::ClippyFailed(_))));
}
