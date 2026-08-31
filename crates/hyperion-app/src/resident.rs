//! Apps that are left running — docs/998-roadmap.md's App Builder T3.
//!
//! ## What residency changes
//!
//! A one-shot app is dispatched when someone asks for it: it reads `input.json`, writes
//! `output.json`, and exits, with a timeout bounding what it can cost. A resident app has none of
//! those. It runs until something stops it, watches for whatever it was built to watch for, and
//! writes into its own durable directory. That is a different lifetime, a different failure mode
//! (it can die while nobody is looking) and a different cost (a budget held for as long as it
//! exists) — which is why residency is declared in the signed contract and started explicitly,
//! never inferred from a goal.
//!
//! ## Why `hyperion-supervisor` rather than something new
//!
//! Because supervision is exactly the problem it already solves: capability-scoped spawn, crash
//! detection, respawn under a *fresh* grant, cgroup placement, and a give-up policy for a service
//! that crash-loops. Reimplementing any of that here would be a second restart policy to keep in
//! agreement with the first.
//!
//! Making that reuse safe needed one real fix in that crate. `Supervisor::reap_and_restart_one`
//! blocks on `waitpid(-1)`, which reaps *any* child — including the one-shot sandboxed invocations
//! this same process runs, whose own `try_wait` would then get `ECHILD` and report a failure for a
//! program that had actually succeeded. `Supervisor::poll_and_restart` polls only the pids it owns,
//! non-blocking, which is what lets a device run both kinds of app at once.
//!
//! ## What is not verified
//!
//! Nothing here has ever run. `Supervisor::spawn_sandboxed` needs user namespaces and Landlock, and
//! the container this was written in has neither — the same reason seven pre-existing sandbox tests
//! fail there. The tests below cover what can be established without executing anything: that a
//! resident app is declared, that a service spec is derived correctly from it, and that a one-shot
//! app is never given one. Whether a resident app really survives a crash is a claim this crate
//! does **not** yet make.

use std::collections::BTreeMap;
use std::path::PathBuf;

use hyperion_capability::RightsMask;
use hyperion_supervisor::{ServiceScheduling, ServiceSpec, Supervisor, SupervisorError};
use hyperion_trust_boundary::TrustDepth;

use crate::registry::{AppError, AppRegistry};
use crate::types::InstalledApp;

/// What a resident app is allowed to consume while it is running.
///
/// Larger than a one-shot app's admission request because the two measure different things: that
/// one is a momentary claim on a Scheduler queue, this is a standing cgroup allocation held for as
/// long as the app exists. Still deliberately modest and uniform — nothing has measured any of
/// these apps, and a figure that varied would imply something had.
const RESIDENT_BUDGET: hyperion_scheduler::ResourceVector = hyperion_scheduler::ResourceVector {
    cpu_shares: 128,
    ram_mb: 128,
    gpu_shares: 0,
    vram_mb: 0,
    storage_iops: 32,
    network_bw_kbps: 0,
    inference_tokens_per_sec: 0,
    context_window_slots: 0,
    battery_budget_mw: 0,
};

/// How a resident app is currently doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResidentState {
    /// Supervised and believed to be running.
    Running,
    /// The supervisor stopped restarting it — it crashed repeatedly, or a respawn itself failed.
    /// Recorded rather than retried forever, so a person is told instead of a device quietly
    /// burning a budget on something that cannot start.
    GaveUp { restarts: u32 },
}

/// The resident apps this device is running, and the supervisor running them.
pub struct ResidentApps {
    supervisor: Supervisor,
    /// Which app each supervised service name belongs to, and for whom. A service name has to be
    /// unique across the whole supervisor, while an app name is only unique per device -- and the
    /// same app really can be resident for two different people at once, each with its own data.
    running: BTreeMap<String, ResidentApp>,
}

#[derive(Debug, Clone)]
struct ResidentApp {
    app: String,
    owner: String,
}

/// The supervisor's own name for one person's instance of one app.
fn service_name(app: &str, principal_scope: &str) -> String {
    format!("app:{app}:{principal_scope}")
}

impl ResidentApps {
    /// Opens a supervisor rooted at `runtime_dir`.
    ///
    /// `cgroup_parent` is where real resource budgets are enforced; `None` degrades to "supervised
    /// but unweighted" rather than refusing to start anything, matching what `Supervisor` already
    /// does when no delegated cgroup subtree is available.
    pub fn open(
        runtime_dir: impl Into<PathBuf>,
        cgroup_parent: Option<PathBuf>,
    ) -> Result<Self, SupervisorError> {
        Ok(ResidentApps {
            supervisor: Supervisor::new(runtime_dir.into(), cgroup_parent)?,
            running: BTreeMap::new(),
        })
    }

    /// The service spec one resident app runs as, for one person.
    ///
    /// Public so it can be checked without spawning anything, which on a kernel without Landlock is
    /// the only way to check it at all.
    pub fn spec_for(
        app: &InstalledApp,
        program: PathBuf,
        args: Vec<String>,
        data_dir: PathBuf,
        principal_scope: &str,
    ) -> ServiceSpec {
        ServiceSpec {
            name: service_name(&app.name, principal_scope),
            program,
            args,
            // Read and write, because a resident app's whole purpose is to keep something; execute,
            // because the interpreter has to run. No more: it gets no grant to reach anything
            // beyond its own directory.
            rights: RightsMask::READ | RightsMask::WRITE | RightsMask::EXEC,
            // The strongest confinement this workspace implements, same as a one-shot app.
            depth: TrustDepth::Container,
            // Its own durable directory *is* its working scope. Unlike a one-shot app, whose scope
            // is a throwaway temp directory, a resident app has nowhere else to put anything.
            fs_scope: data_dir,
            scheduling: Some(ServiceScheduling {
                priority_weight: 1.0,
                request: RESIDENT_BUDGET,
            }),
            extra_env: Vec::new(),
        }
    }

    /// Starts a resident app for one person.
    ///
    /// Refuses an app that never declared residency: being left running is a permission, and one an
    /// app has to have asked for in its signed contract.
    pub fn start(
        &mut self,
        app: &InstalledApp,
        program: PathBuf,
        args: Vec<String>,
        data_dir: PathBuf,
        principal_scope: &str,
    ) -> Result<(), AppError> {
        if !app.resident {
            return Err(AppError::NotResident {
                app: app.name.clone(),
            });
        }
        let name = service_name(&app.name, principal_scope);
        if self.running.contains_key(&name) {
            return Err(AppError::AlreadyRunning {
                app: app.name.clone(),
            });
        }
        let spec = Self::spec_for(app, program, args, data_dir, principal_scope);
        self.supervisor
            .spawn_sandboxed(spec)
            .map_err(|e| AppError::Residency(e.to_string()))?;
        self.running.insert(
            name,
            ResidentApp {
                app: app.name.clone(),
                owner: app.owner.clone(),
            },
        );
        Ok(())
    }

    /// Stops a resident app, if it is running for this person.
    pub fn stop(&mut self, app: &str, principal_scope: &str) -> Result<(), AppError> {
        let name = service_name(app, principal_scope);
        if self.running.remove(&name).is_none() {
            return Err(AppError::NotRunning {
                app: app.to_string(),
            });
        }
        self.supervisor
            .terminate(&name)
            .map_err(|e| AppError::Residency(e.to_string()))?;
        Ok(())
    }

    /// Lets the supervisor notice and restart anything that has died.
    ///
    /// Non-blocking and own-pids-only (see this module's doc comment), so it is safe to call from a
    /// process that also dispatches one-shot apps. Nothing here runs on a timer: a caller decides
    /// when to look, which for a console means once per turn -- a crashed app is noticed the next
    /// time somebody types something, which is honest about there being no scheduler here rather
    /// than implying a liveness guarantee this does not provide.
    pub fn poll(&mut self) {
        let _ = self.supervisor.poll_and_restart();
    }

    /// What is running for this person, and how each one is doing.
    pub fn status_for(&self, principal_scope: &str) -> Vec<(String, ResidentState)> {
        let suffix = format!(":{principal_scope}");
        self.running
            .iter()
            .filter(|(name, _)| name.ends_with(&suffix))
            .map(|(name, resident)| {
                let state = match self.supervisor.given_up(name) {
                    Some(given_up) => ResidentState::GaveUp {
                        restarts: given_up.restart_count,
                    },
                    None => ResidentState::Running,
                };
                (resident.app.clone(), state)
            })
            .collect()
    }

    /// `true` if this app is running for this person.
    pub fn is_running(&self, app: &str, principal_scope: &str) -> bool {
        self.running
            .contains_key(&service_name(app, principal_scope))
    }

    /// Everyone this app is currently running for -- what [`AppRegistry::remove`]'s caller needs to
    /// stop before deleting it, since a removed app whose process kept running would be an app that
    /// outlived its own removal.
    pub fn owners_running(&self, app: &str) -> Vec<String> {
        self.running
            .values()
            .filter(|resident| resident.app == app)
            .map(|resident| resident.owner.clone())
            .collect()
    }

    /// The service name one person's instance of an app runs under.
    pub fn service_name_for(app: &str, principal_scope: &str) -> String {
        service_name(app, principal_scope)
    }
}

/// Re-exported so a caller can name the registry that owns the app being started without importing
/// two crates for one call.
pub type Registry = AppRegistry;
