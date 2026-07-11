//! A companion binary for hyperion-ipc's real-transport integration test. A genuinely separate,
//! `exec`'d process that only ever receives a bare `WireToken` *claim* via an environment
//! variable (exactly the realistic shape for a real cross-process IPC client: it has no local
//! `CapabilityMonitor` to validate its own token against -- that's the server's job) and uses
//! `Endpoint::ipc_call_with_claim` to make one real call over a real socket, reporting the
//! result on stdout for the test harness to check.

use std::env;
use std::time::Duration;

use hyperion_ipc::{Endpoint, Request, SchemaId};

fn main() {
    let wire_token_json = env::var("HYPERION_WIRE_TOKEN").expect("HYPERION_WIRE_TOKEN not set");
    let claim: hyperion_capability::WireToken =
        serde_json::from_str(&wire_token_json).expect("HYPERION_WIRE_TOKEN is not valid JSON");
    let server_sock = env::var("HYPERION_SERVER_SOCK").expect("HYPERION_SERVER_SOCK not set");
    let client_sock = env::var("HYPERION_CLIENT_SOCK").expect("HYPERION_CLIENT_SOCK not set");

    let endpoint = Endpoint::bind(&client_sock).expect("bind client endpoint");

    let result = endpoint.ipc_call_with_claim(
        &server_sock,
        &claim,
        SchemaId(1),
        Request {
            op: hyperion_ipc::Operation(1),
            payload: b"ping".to_vec(),
        },
        Duration::from_secs(5),
    );

    match result {
        Ok(response) => {
            println!("CALL_OK:{}", String::from_utf8_lossy(&response.payload));
        }
        Err(fault) => {
            println!("CALL_ERR:{fault}");
        }
    }
}
