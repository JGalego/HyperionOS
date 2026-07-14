//! A minimal, real process entry point for `hyperion-explainability`, proving
//! [docs/998-roadmap.md](../../../../docs/998-roadmap.md) M5's supervision-tree
//! mechanism (`hyperion-supervisor`) against a second, independent real Phase 2-10 subsystem --
//! see `hyperion-observability`'s own `src/bin/hyperion-observability-service.rs` for the full
//! reasoning on why this process mints its own local capability domain rather than trying to
//! locally re-validate the spawn-time `WireToken` claim it receives.

use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId, WireToken};
use hyperion_explainability::{ControlState, ExplanationStore, ReasoningStep};

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after the Unix epoch")
        .as_secs()
}

fn main() {
    let wire_token_json = std::env::var("HYPERION_WIRE_TOKEN")
        .expect("hyperion-supervisor always sets HYPERION_WIRE_TOKEN for a sandboxed service");
    let claim: WireToken = serde_json::from_str(&wire_token_json)
        .expect("HYPERION_WIRE_TOKEN is always a real WireToken this instance's supervisor minted");
    let state_path = std::env::var("HYPERION_STATE_FILE")
        .expect("the caller (hyperion-init, or a test) always sets HYPERION_STATE_FILE");

    let mut monitor = CapabilityMonitor::new();
    let local_token = monitor.mint_root(
        RightsMask::READ | RightsMask::WRITE,
        TrustBoundaryId(claim.token_id),
        None,
    );
    let store = ExplanationStore::new();

    let record_id = store
        .begin(
            &monitor,
            &local_token,
            /* action_id */ claim.token_id,
            /* triggering_intent_id */ 0,
            /* agent_id */ 0,
            &format!(
                "token_id={} generation={}",
                claim.token_id, claim.generation
            ),
            Vec::new(),
            now_unix(),
        )
        .expect("this process's own local capability check always authorizes its own begin");

    store
        .append_step(
            &monitor,
            &local_token,
            record_id,
            ReasoningStep {
                step_index: 0,
                description: "real service instance started under a real spawn-time capability \
                               grant"
                    .to_string(),
                capability_ref: Some(format!("token_id={}", claim.token_id)),
                inputs_ref: Vec::new(),
                output_ref: None,
            },
            Vec::new(),
        )
        .expect("append_step under this process's own valid local token");

    store
        .transition(&monitor, &local_token, record_id, ControlState::Completed)
        .expect("transition under this process's own valid local token");

    write_state(&state_path, &claim, &store, record_id);

    // A real, long-running supervised service, not a one-shot: idle until hyperion-supervisor
    // kills and (per M5's exit criterion) respawns this under a fresh grant.
    loop {
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn write_state(path: &str, claim: &WireToken, store: &ExplanationStore, record_id: u64) {
    let record = store
        .get(record_id)
        .expect("the record this same process just began must still be readable");
    let still_incomplete = store.incomplete().len();
    let mut f = std::fs::File::create(path).expect("create the real state file inside fs_scope");
    writeln!(f, "pid={}", std::process::id()).unwrap();
    writeln!(f, "spawn_token_id={}", claim.token_id).unwrap();
    writeln!(f, "spawn_generation={}", claim.generation).unwrap();
    writeln!(f, "record_id={record_id}").unwrap();
    writeln!(f, "control_state={:?}", record.control_state).unwrap();
    writeln!(f, "incomplete_count={still_incomplete}").unwrap();
    f.flush().unwrap();
}
