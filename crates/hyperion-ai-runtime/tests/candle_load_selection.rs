//! `CandleBackend::load_selection`/`load_from_catalog`/`load_from_hf_pair` -- the real dispatch
//! `hyperion-console`'s own `/backend candle <model>` meta-command uses, proven against the same
//! real, already-cached Hugging Face Hub files `candle_gguf_safetensors.rs`/`candle_inference.rs`
//! already download.
//!
//! `#[cfg(feature = "candle")]`-gated like the backend itself -- see those two files' own doc
//! comments for why this does not run as part of the default `cargo test --workspace` gate.
//! Invoke explicitly with `cargo test -p hyperion-ai-runtime --features candle --test
//! candle_load_selection`.

#![cfg(feature = "candle")]

use hyperion_ai_runtime::CandleBackend;

#[test]
fn load_selection_dispatches_a_real_catalog_name_to_the_real_bespoke_binary_loader() {
    let backend = CandleBackend::load_selection("stories15m-bin");
    assert!(
        backend.is_ok(),
        "a real, known catalog name must really load, got: {:?}",
        backend.err()
    );
}

#[test]
fn load_selection_dispatches_a_real_catalog_name_to_the_real_gguf_loader() {
    let backend = CandleBackend::load_selection("stories15m-gguf");
    assert!(
        backend.is_ok(),
        "a real, known catalog name must really load, got: {:?}",
        backend.err()
    );
}

#[test]
fn load_selection_dispatches_a_real_catalog_name_to_the_real_safetensors_loader() {
    let backend = CandleBackend::load_selection("stories15m-safetensors");
    assert!(
        backend.is_ok(),
        "a real, known catalog name must really load, got: {:?}",
        backend.err()
    );
}

#[test]
fn load_selection_gives_an_honest_error_for_an_unknown_catalog_name() {
    let result = CandleBackend::load_selection("no-such-model-anyone-has-ever-heard-of");
    let err = result
        .err()
        .expect("an unknown name must be a real error, not a silent default");
    assert!(
        err.to_string().contains("no known catalog entry"),
        "got: {err}"
    );
}

#[test]
fn load_selection_dispatches_a_real_owner_name_filename_triple_to_the_real_gguf_loader() {
    let backend = CandleBackend::load_selection(
        "klosax/tinyllamas-stories-gguf/tinyllamas-stories-15m-f32.gguf",
    );
    assert!(
        backend.is_ok(),
        "a real, well-formed HF triple must really load via its own real .gguf extension, got: \
         {:?}",
        backend.err()
    );
}

#[test]
fn load_selection_gives_an_honest_error_for_a_safetensors_file_outside_the_known_catalog() {
    let result = CandleBackend::load_selection("some-other-owner/some-repo/model.safetensors");
    let err = result
        .err()
        .expect("a non-catalog safetensors file must be a real, honest error, not a guess");
    assert!(
        err.to_string().contains("no real architecture metadata"),
        "got: {err}"
    );
}
