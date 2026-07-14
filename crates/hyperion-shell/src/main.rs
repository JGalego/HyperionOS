fn main() -> eframe::Result<()> {
    // Matches `hyperion-console`'s own default exactly (see that crate's `main.rs`) -- a
    // temp-dir default so a plain `cargo run` never writes a real device signing key/Knowledge
    // Graph into the repo tree itself.
    let data_dir = std::env::var("HYPERION_SHELL_DATA_DIR")
        .unwrap_or_else(|_| std::env::temp_dir().display().to_string());

    let session = hyperion_shell::EmbeddedSession::open(&data_dir)
        .expect("open this shell's own real session state");
    let app = hyperion_shell::ShellApp::spawn(session);

    eframe::run_native(
        "Hyperion",
        eframe::NativeOptions::default(),
        Box::new(|_creation_context| Ok(Box::new(app))),
    )
}
