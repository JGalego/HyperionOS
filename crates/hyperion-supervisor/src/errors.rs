use std::io;

#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    #[error("failed to create the IPC rendezvous directory at {path}: {source}")]
    RendezvousDir { path: String, source: io::Error },
    #[error("failed to spawn service {name:?}: {source}")]
    Spawn { name: String, source: io::Error },
    #[error("failed to wait on a supervised child: {0}")]
    Wait(io::Error),
}
