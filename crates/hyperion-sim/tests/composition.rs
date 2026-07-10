use hyperion_capability::Fault;
use hyperion_ipc::IpcFault;

#[test]
fn capability_ipc_and_scheduler_compose_end_to_end() {
    let outcome = hyperion_sim::run_demo();
    assert_eq!(outcome.first_call_reply, vec![1, 2, 3, 0xFF]);
    assert_eq!(
        outcome.post_revocation_result.unwrap_err(),
        IpcFault::Kernel(Fault::Revoked)
    );
}
