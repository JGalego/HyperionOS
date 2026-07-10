fn main() {
    let outcome = hyperion_sim::run_demo();
    println!("client call over a capability-gated channel -> server (scheduled, admitted) -> reply:");
    println!("  reply payload: {:?}", outcome.first_call_reply);
    println!("client token revoked; retrying the same channel:");
    match outcome.post_revocation_result {
        Ok(payload) => println!("  unexpectedly succeeded: {payload:?}"),
        Err(fault) => println!("  rejected as expected: {fault}"),
    }
}
