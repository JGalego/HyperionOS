//! A minimal, real process entry point for `hyperion-observability`, proving
//! [docs/998-roadmap.md](../../../../docs/998-roadmap.md) M5's supervision-tree
//! mechanism (`hyperion-supervisor`) against real, unmodified crate logic -- per the roadmap's own
//! reuse map, this crate needs nothing structural changed to become a real supervised service, only
//! a real entry point that constructs its existing API and does something with it.
//!
//! `hyperion-supervisor` mints a real capability grant per spawn/respawn and hands this process a
//! *claim* to it (`HYPERION_WIRE_TOKEN`, a [`WireToken`]) -- not a live [`CapabilityMonitor`]
//! reference, since that monitor lives in a genuinely different process (the supervisor) and a
//! `WireToken` can only be authenticated by the same monitor that minted it. This process does not
//! attempt to locally re-validate that claim (there is nothing here that could): instead it mints
//! its *own*, separate local root token over its *own* `AuditLedger` instance -- a different,
//! narrower capability domain (this service's internal access control over its own ledger) than
//! the spawn-time grant, and legitimately so: docs/03's model is many independently capability-
//! guarded objects, not one universal token. Every real ledger entry this process appends is
//! tagged with the spawn-time claim's `token_id`/`generation`, so an outside observer reading this
//! instance's real state file can see, concretely, which capability grant produced which work --
//! exactly what `hyperion-supervisor`'s own test reads back across a kill+respawn to confirm
//! "restarted with a fresh capability grant."

use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId, WireToken};
use hyperion_observability::{AuditAction, AuditLedger, AuditPayload, PrincipalRef};

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
    let ledger = AuditLedger::new();

    let entry = ledger
        .append(
            &monitor,
            &local_token,
            PrincipalRef::System,
            AuditAction::Grant,
            Some(format!("spawn_token_id={}", claim.token_id)),
            AuditPayload::Grant {
                capability_ref: format!(
                    "token_id={} generation={}",
                    claim.token_id, claim.generation
                ),
            },
            now_unix(),
        )
        .expect("this process's own local capability check always authorizes its own append");

    write_state(&state_path, &claim, &ledger, entry.seq);

    // A real, long-running supervised service, not a one-shot: idle until hyperion-supervisor
    // kills and (per M5's exit criterion) respawns this under a fresh grant.
    loop {
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn write_state(path: &str, claim: &WireToken, ledger: &AuditLedger, up_to_seq: u64) {
    let report = ledger.verify_chain(1, up_to_seq);
    let mut f = std::fs::File::create(path).expect("create the real state file inside fs_scope");
    writeln!(f, "pid={}", std::process::id()).unwrap();
    writeln!(f, "spawn_token_id={}", claim.token_id).unwrap();
    writeln!(f, "spawn_generation={}", claim.generation).unwrap();
    writeln!(f, "ledger_seq={up_to_seq}").unwrap();
    writeln!(f, "verify_chain={report:?}").unwrap();
    f.flush().unwrap();
}
