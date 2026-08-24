//! Hyperion App Builder, M1 — apps Hyperion builds for a goal.
//!
//! docs/998-roadmap.md's Resourceful pillar ends at *tools*: a capability that takes `input.json`,
//! runs once in a sandbox, and writes `output.json`. This crate is the first slice of what comes
//! after — the "App Builder" section of that roadmap — and it is deliberately only the first two
//! rungs of the ladder that section defines.
//!
//! ## What is real here
//!
//! - **T0 (answer) and T1 (tool).** A goal plus a script becomes a real, signed, installed
//!   capability, dispatched through the same real Landlock/seccomp sandbox a hand-installed
//!   native binary already runs in. [`AppRegistry::build`].
//! - **A real typed input contract.** [`contract`] encodes an app's declared inputs — name, type,
//!   whether it is required, and a plain-language description — *inside* the manifest that gets
//!   signed, so the contract cannot drift from the implementation it describes and there is no
//!   side file for `/apps` to read. [`contract::validate_args`] rejects a missing, unknown, or
//!   wrong-typed argument in a sentence a person can act on, before anything is spawned.
//! - **Real listing and removal over the registry itself.** [`AppRegistry::list`] decodes what is
//!   really installed; [`AppRegistry::remove`] really revokes the capability tokens and really
//!   deletes the script.
//!
//! ## What is deliberately not here, and why
//!
//! - **No new execution mechanism.** An app installs and runs through
//!   `hyperion_sdk::publish` → `PluginRegistry::install` → `invoke_native_binary` — the existing
//!   path, unchanged. This crate adds a contract and a shell around it, never a second way to run
//!   code.
//! - **No `run`.** Dispatch belongs to `AgentRuntime::invoke`, where consent gating and automatic
//!   Explanation Records live. See [`registry`]'s own doc comment.
//! - **No compiler gate for scripts, and no pretence of one.** `hyperion_sdk::codegen::
//!   review_and_build` is real static review *of Rust*; there is no equivalent for a shell or
//!   Python script, and text-scanning one for scary substrings would be security theatre. What
//!   really contains a T0/T1 app is what contains every other native-binary capability: a real
//!   user namespace, a Landlock scope of exactly one throwaway directory, and a seccomp allowlist
//!   containing no network syscalls at all.
//! - **T2 through T5 do not exist.** Durable state, residency, an authenticated human surface,
//!   and composition each need a privilege this crate cannot grant — a persistent `fs_scope`, a
//!   supervisor-owned lifetime, a brokered port plus a real identity subsystem. They are named in
//!   the roadmap with their blockers stated, not half-built here. Notably [`AppTier`] has no
//!   variants for them: a tier no code path can honestly construct would be a claim this crate
//!   does not stand behind.
//!
//! ## The blocker worth knowing before extending this
//!
//! A sandboxed app **cannot open a TCP port**, by construction:
//! `hyperion_trust_boundary::baseline_allowed_syscalls` contains no socket syscalls, and
//! `ipc_allowed_syscalls` adds exactly `socket`/`bind`/`sendto`/`recvfrom` — `connect`, `listen`
//! and `accept` are deliberately absent. T4's design follows from that rather than fighting it:
//! Hyperion owns the listener, authenticates the human once, and forwards to the app over the IPC
//! rendezvous socket the supervisor already mints. Do not widen the seccomp filter to make a
//! generated app into a server.

pub mod contract;
pub mod plan;
pub mod registry;
pub mod suggest;
pub mod types;

pub use contract::{
    validate_app_name, validate_args, validate_contract, AppContract, ArgError, ContractError,
    APP_CAPABILITY_PREFIX, CONTRACT_VERSION,
};
pub use plan::{app_plan_instructions, from_model_answer, PlanError};
pub use registry::{AppError, AppRegistry, APP_PUBLISHER};
pub use suggest::best_match;
pub use types::{AppDefinition, AppPaths, AppTier, InputField, InputKind, InstalledApp};
