fn main() {
    let (boot, _monitor, _root) = hyperion_sim::cold_boot();
    println!("cold boot (L0/L1 slice only, see hyperion_sim::boot for what's not covered yet):");
    println!(
        "  {:?} elapsed vs {:?} budget ({})",
        boot.elapsed,
        boot.budget,
        if boot.within_budget() {
            "within budget"
        } else {
            "OVER BUDGET"
        }
    );
    println!();

    let outcome = hyperion_sim::run_demo();
    println!(
        "client call over a capability-gated channel -> server (scheduled, admitted) -> reply:"
    );
    println!("  reply payload: {:?}", outcome.first_call_reply);
    println!("client token revoked; retrying the same channel:");
    match outcome.post_revocation_result {
        Ok(payload) => println!("  unexpectedly succeeded: {payload:?}"),
        Err(fault) => println!("  rejected as expected: {fault}"),
    }
}
