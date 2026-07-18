use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use hyperion_capability::{CapabilityMonitor, CapabilityToken, RightsMask, TrustBoundaryId};
use hyperion_knowledge_graph::{KnowledgeGraph, NodeId};
use hyperion_netstack::{DomainEgressGrant, FetchedPage, NetstackHub};

use crate::browser;
use crate::sandbox;
use crate::types::{
    CompatError, CompatSession, CompatibilityProfile, IngestedArtifact, LegacyTarget,
    NetworkPolicy, PromotionPolicy, PromotionState, RenderedPage, SandboxExecution, SessionId,
    TrustDepth,
};

/// docs/27 — Compatibility Layer. See this crate's doc comment for the
/// full real/deferred split.
pub struct CompatHost {
    graph: Arc<KnowledgeGraph>,
    netstack: Arc<NetstackHub>,
    sessions: Mutex<HashMap<SessionId, CompatSession>>,
    artifacts: Mutex<HashMap<(SessionId, String), IngestedArtifact>>,
    next_session_id: AtomicU64,
    next_boundary_ordinal: AtomicU64,
}

impl CompatHost {
    pub fn new(graph: Arc<KnowledgeGraph>, netstack: Arc<NetstackHub>) -> Self {
        CompatHost {
            graph,
            netstack,
            sessions: Mutex::new(HashMap::new()),
            artifacts: Mutex::new(HashMap::new()),
            next_session_id: AtomicU64::new(1),
            next_boundary_ordinal: AtomicU64::new(1),
        }
    }

    fn require(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        rights: RightsMask,
    ) -> Result<(), CompatError> {
        monitor
            .check_rights_ok_result(token, rights)
            .map_err(|_| CompatError::Unauthorized)
    }

    /// docs/27 §4's `compat_launch`: mint a fresh Trust Boundary for the
    /// guest at `max(profile.min_depth, target.default_depth())`, and —
    /// for a Web target with `NetworkPolicy::Allow` — resolve that policy
    /// at admission time into a real `web.fetch.raw` domain-egress grant
    /// scoped to the same domain pattern, per docs/27 §5: "the concrete
    /// mechanism behind this policy enum's `Allow` variant for Web
    /// targets, not a second, unrelated network-access path."
    pub fn launch(
        &self,
        monitor: &mut CapabilityMonitor,
        admin_token: &CapabilityToken,
        profile: CompatibilityProfile,
        available_depth: TrustDepth,
        now: u64,
    ) -> Result<SessionId, CompatError> {
        self.require(monitor, admin_token, RightsMask::GRANT)?;

        let depth = std::cmp::max(profile.min_depth, profile.target.default_depth());
        if depth > available_depth {
            return Err(CompatError::Unauthorized);
        }

        if profile.target == LegacyTarget::Web {
            if let NetworkPolicy::Allow { scope } = &profile.network_default {
                self.netstack.grant_domain_egress(
                    monitor,
                    admin_token,
                    admin_token,
                    DomainEgressGrant {
                        domain_patterns: vec![scope.clone()],
                        rate_limit_per_window: 100,
                        window_secs: 60,
                        max_depth: 1,
                        expiry: None,
                    },
                    now,
                )?;
            }
        }

        let session_id = self.next_session_id.fetch_add(1, Ordering::Relaxed);
        let boundary =
            TrustBoundaryId(2_000_000 + self.next_boundary_ordinal.fetch_add(1, Ordering::Relaxed));
        self.sessions.lock().unwrap().insert(
            session_id,
            CompatSession {
                session_id,
                boundary,
                profile,
                grants: Vec::new(),
            },
        );
        Ok(session_id)
    }

    /// docs/27 §4's `compat_grant` — "the only path to any capability
    /// beyond launch-time defaults."
    pub fn grant(
        &self,
        monitor: &mut CapabilityMonitor,
        admin_token: &CapabilityToken,
        session_id: SessionId,
        rights: RightsMask,
    ) -> Result<(), CompatError> {
        self.require(monitor, admin_token, RightsMask::GRANT)?;
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions
            .get_mut(&session_id)
            .ok_or(CompatError::NoSuchSession)?;
        let token = monitor.cap_derive(admin_token, rights, None, session.boundary)?;
        session.grants.push(token);
        Ok(())
    }

    fn path_declared(session: &CompatSession, guest_path: &str) -> bool {
        session
            .profile
            .filesystem_roots
            .iter()
            .any(|root| guest_path.starts_with(root.as_str()))
    }

    /// docs/27 §3's `shim_open`: default-deny path resolution, then (for
    /// a write) an explicit write-grant check, then Stage A capture — an
    /// automatic, KG-write-free `IngestedArtifact` record, never a
    /// Knowledge Graph write on this path. Promotion (Stage B) is a
    /// wholly separate, explicit step — see [`Self::promote_artifact`].
    pub fn shim_open(
        &self,
        session_id: SessionId,
        guest_path: &str,
        write: bool,
    ) -> Result<(), CompatError> {
        let sessions = self.sessions.lock().unwrap();
        let session = sessions
            .get(&session_id)
            .ok_or(CompatError::NoSuchSession)?;
        if !Self::path_declared(session, guest_path) {
            return Err(CompatError::PathOutsideDeclaredRoots);
        }
        if write
            && !session
                .grants
                .iter()
                .any(|g| g.rights().contains(RightsMask::WRITE))
        {
            return Err(CompatError::WriteNotGranted);
        }
        drop(sessions);

        if write {
            self.artifacts.lock().unwrap().insert(
                (session_id, guest_path.to_string()),
                IngestedArtifact {
                    guest_path: guest_path.to_string(),
                    sniffed_type: "unknown".to_string(),
                    promotion_state: PromotionState::Pending,
                    draft_metadata: None,
                    promoted_object_id: None,
                },
            );
        }
        Ok(())
    }

    pub fn capture_artifact(
        &self,
        session_id: SessionId,
        guest_path: &str,
    ) -> Option<IngestedArtifact> {
        self.artifacts
            .lock()
            .unwrap()
            .get(&(session_id, guest_path.to_string()))
            .cloned()
    }

    /// docs/27 §3's Stage B, `promote_artifact` — the *only* place this
    /// crate ever writes to the Knowledge Graph on a legacy app's behalf,
    /// and only when explicitly approved: "promotion itself never
    /// happens on the write path — it is a separate, explicit step."
    #[allow(clippy::too_many_arguments)]
    pub fn promote_artifact(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        session_id: SessionId,
        guest_path: &str,
        policy: PromotionPolicy,
        sniffed_type: &str,
        draft_metadata: serde_json::Value,
        user_confirmed: bool,
    ) -> Result<NodeId, CompatError> {
        let approved = match policy {
            PromotionPolicy::AskEveryTime => user_confirmed,
            PromotionPolicy::StandingRuleApprove => true,
            PromotionPolicy::StandingRuleDeny => false,
        };

        if !approved {
            if let Some(artifact) = self
                .artifacts
                .lock()
                .unwrap()
                .get_mut(&(session_id, guest_path.to_string()))
            {
                artifact.promotion_state = PromotionState::Ignored;
            }
            return Err(CompatError::PromotionDeclined);
        }

        let object_id = self.graph.put_node(
            monitor,
            token,
            None,
            sniffed_type,
            None,
            draft_metadata.clone(),
        )?;

        let mut artifacts = self.artifacts.lock().unwrap();
        let artifact = artifacts
            .get_mut(&(session_id, guest_path.to_string()))
            .ok_or(CompatError::NoSuchArtifact)?;
        artifact.sniffed_type = sniffed_type.to_string();
        artifact.promotion_state = PromotionState::Promoted;
        artifact.draft_metadata = Some(draft_metadata);
        artifact.promoted_object_id = Some(object_id);
        Ok(object_id)
    }

    /// This crate's own previously-named "Real Linux container/namespace runtime" gap, closed
    /// for real via [`sandbox::exec_in_sandbox`] — see that module's own doc comment for the
    /// real kernel-namespace guarantees this provides and its honestly-named scope. Only valid
    /// for a session whose target is one this crate can actually run something real for: `Linux`,
    /// `Container`, or `Cli` (`Windows`/`Android`/`Vm` would need a foreign kernel or ART runtime
    /// this hosted simulator has neither of — see this crate's own doc comment).
    pub fn exec_in_sandbox(
        &self,
        session_id: SessionId,
        command: &str,
        args: &[String],
    ) -> Result<SandboxExecution, CompatError> {
        let sessions = self.sessions.lock().unwrap();
        let session = sessions
            .get(&session_id)
            .ok_or(CompatError::NoSuchSession)?;
        if !matches!(
            session.profile.target,
            LegacyTarget::Linux | LegacyTarget::Container | LegacyTarget::Cli
        ) {
            return Err(CompatError::NotASandboxableSession);
        }
        let writable = session
            .grants
            .iter()
            .any(|g| g.rights().contains(RightsMask::WRITE));
        let roots = session.profile.filesystem_roots.clone();
        let network_policy = session.profile.network_default.clone();
        drop(sessions);

        sandbox::exec_in_sandbox(&roots, writable, &network_policy, command, args)
    }

    /// This crate's own previously-named "a real browser rendering engine... renders nothing"
    /// gap, closed for real: reuses [`Self::web_fetch`]'s own existing capability/SSRF/rate-limit
    /// gate for `url` first (so a guest can never reach this for a domain it wasn't granted), then
    /// hands that same URL to [`browser::render_dom`] for a genuine headless-browser render — see
    /// that module's own doc comment for why the render itself is a second, independent real
    /// fetch rather than reusing `web_fetch`'s own bytes.
    pub fn render_web_page(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        session_id: SessionId,
        url: &str,
        agent_id: u64,
        now: u64,
    ) -> Result<RenderedPage, CompatError> {
        let fetched = self.web_fetch(monitor, token, session_id, url, agent_id, now)?;
        let rendered_dom = browser::render_dom(url, std::time::Duration::from_secs(10))?;
        Ok(RenderedPage {
            fetched,
            rendered_dom,
        })
    }

    /// docs/27 §5: the Web-target fetch path, mediated entirely by the
    /// Compatibility Host — the guest never touches `hyperion-netstack`
    /// directly.
    pub fn web_fetch(
        &self,
        monitor: &CapabilityMonitor,
        token: &CapabilityToken,
        session_id: SessionId,
        url: &str,
        agent_id: u64,
        now: u64,
    ) -> Result<FetchedPage, CompatError> {
        let sessions = self.sessions.lock().unwrap();
        let session = sessions
            .get(&session_id)
            .ok_or(CompatError::NoSuchSession)?;
        let allowed = session.profile.target == LegacyTarget::Web
            && matches!(session.profile.network_default, NetworkPolicy::Allow { .. });
        if !allowed {
            return Err(CompatError::NotAnAllowedWebSession);
        }
        drop(sessions);

        Ok(self
            .netstack
            .web_fetch_raw(monitor, token, url, agent_id, now)?)
    }

    /// docs/27 §5's crash/escape recovery: "microreboot" — tear down
    /// every token this session was granted, releasing its Trust
    /// Boundary entirely, per the same cascade-revocation guarantee
    /// every other crate in this workspace relies on.
    pub fn terminate(
        &self,
        monitor: &mut CapabilityMonitor,
        admin_token: &CapabilityToken,
        session_id: SessionId,
    ) -> Result<(), CompatError> {
        self.require(monitor, admin_token, RightsMask::REVOKE)?;
        let session = self
            .sessions
            .lock()
            .unwrap()
            .remove(&session_id)
            .ok_or(CompatError::NoSuchSession)?;
        for token in &session.grants {
            monitor.cap_revoke(token);
        }
        Ok(())
    }

    pub fn session(&self, session_id: SessionId) -> Option<CompatSession> {
        self.sessions.lock().unwrap().get(&session_id).cloned()
    }

    /// Every artifact this session has actually promoted (Stage B) into
    /// the Knowledge Graph so far — the real objects
    /// [`crate::workspace_bridge::present_as_workspace`] binds a legacy
    /// app's Workspace panel to, exactly like any natively generated
    /// panel binds to its Context Bundle entries.
    pub fn promoted_artifacts(&self, session_id: SessionId) -> Vec<IngestedArtifact> {
        self.artifacts
            .lock()
            .unwrap()
            .iter()
            .filter(|((sid, _), artifact)| {
                *sid == session_id && artifact.promotion_state == PromotionState::Promoted
            })
            .map(|(_, artifact)| artifact.clone())
            .collect()
    }
}
