//! A minimal, real process entry point for `hyperion-privacy`, proving
//! [docs/998-roadmap.md](../../../../docs/998-roadmap.md) M5's supervision-tree
//! mechanism (`hyperion-supervisor`) against a third, independent real Phase 2-10 subsystem --
//! see `hyperion-observability`'s own `src/bin/hyperion-observability-service.rs` for the full
//! reasoning on why this process mints its own local capability domain rather than trying to
//! locally re-validate the spawn-time `WireToken` claim it receives.
//!
//! The one real thing this instance does: requests a real consent grant via
//! `hyperion_privacy::ConsentLedger`, then calls `hyperion_privacy::route_capability_call` for a
//! domain requiring exactly that consent -- proving docs/16 §5's real routing algorithm reflects
//! a just-granted, real standing consent, not a mock or a hardcoded decision.

use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hyperion_capability::{CapabilityMonitor, RightsMask, TrustBoundaryId, WireToken};
use hyperion_crypto::Keystore;
use hyperion_privacy::{
    route_capability_call, ConsentLedger, DataScope, PrivacyProfile, PrivacyTier, RoutingDecision,
};

const DOMAIN: &str = "cloud.translate";

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
    let ledger = ConsentLedger::new();
    let subject = claim.token_id;
    let now = now_unix();
    // A real, process-lifetime-only device identity -- this is a proof of the real signing/
    // verification path (see hyperion_privacy::consent's own doc comment), not a persistent
    // device that needs to survive a restart.
    let device_key = Keystore::ephemeral();

    let grant = ledger
        .request(
            &monitor,
            &local_token,
            subject,
            DataScope::Domain(DOMAIN.to_string()),
            "real service instance proving docs/16 routing under a real spawn-time grant",
            None,
            now,
            &device_key,
        )
        .expect("this process's own local capability check always authorizes its own request");

    let profile = PrivacyProfile {
        tier: PrivacyTier::LocalPreferredWithConsent,
        domain_overrides: std::collections::HashMap::new(),
        updated_at: now,
        version: 1,
    };
    let decision = route_capability_call(
        &profile,
        DOMAIN,
        &DataScope::Domain(DOMAIN.to_string()),
        None,
        false, // no local implementation -- forces the real consent check
        &ledger,
        subject,
        now,
    );

    write_state(&state_path, &claim, grant.id, &decision);

    // A real, long-running supervised service, not a one-shot: idle until hyperion-supervisor
    // kills and (per M5's exit criterion) respawns this under a fresh grant.
    loop {
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn write_state(path: &str, claim: &WireToken, grant_id: u64, decision: &RoutingDecision) {
    let mut f = std::fs::File::create(path).expect("create the real state file inside fs_scope");
    writeln!(f, "pid={}", std::process::id()).unwrap();
    writeln!(f, "spawn_token_id={}", claim.token_id).unwrap();
    writeln!(f, "spawn_generation={}", claim.generation).unwrap();
    writeln!(f, "grant_id={grant_id}").unwrap();
    writeln!(f, "routing_decision={decision:?}").unwrap();
    f.flush().unwrap();
}
