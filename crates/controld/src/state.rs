use crate::auth::AuthPolicy;
use crate::hosts::HostRegistry;
use crate::store::Store;
use crate::views::StreamItem;
use model::{Actor, EventBody, Provenance, Subject};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Mutex<Store>>,
    pub hosts: HostRegistry,
    /// Every stored event, published after commit and before the store lock is
    /// released, so the hub carries events in `seq` order. SSE clients subscribe here.
    pub hub: broadcast::Sender<StreamItem>,
    pub host_token: Arc<String>,
    /// `Host` header values accepted from browsers and hosts, e.g. `127.0.0.1:4410`.
    pub allowed_hosts: Arc<Vec<String>>,
    /// One lock per in-flight `request_id`, so concurrent duplicates execute once.
    pub request_locks: Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
    /// Rigs with a boot in flight; a second boot of the same rig is a no-op.
    pub booting: Arc<Mutex<HashSet<String>>>,
    /// Login seam; AllowAll until a login system exists.
    pub auth: AuthPolicy,
    /// The user's home folder: root of the folder picker and working folder of Practice agents without one.
    pub home: Option<Arc<PathBuf>>,
    /// Set once the handoff dispatcher runs for this state (`handoffs::ensure_running`).
    pub handoffs_started: Arc<std::sync::atomic::AtomicBool>,
    /// Handoff messages being sent right now (recorded, send not finished). One that is
    /// still queued and not in here was lost between recording and sending.
    pub handoffs_in_flight: Arc<Mutex<HashSet<String>>>,
    /// Agents asked for their notes at the context ceiling: seat → the event seq when asked.
    pub context_asked: Arc<Mutex<HashMap<String, i64>>>,
    /// `request_input` and `approve` calls waiting for the operator, by question id (Task 6 fills, Task 3 resolves).
    pub seat_waits: Arc<Mutex<HashMap<String, crate::seat_tools::SeatWait>>>,
    /// Cancels a team's start in flight (Task 9's Cancel start).
    pub boot_cancel: Arc<Mutex<HashMap<String, tokio::sync::watch::Sender<bool>>>>,
    /// Set once the M2 background tasks run for this state (`background::ensure_running`).
    pub background_started: Arc<std::sync::atomic::AtomicBool>,
    /// The `claude` and `codex` programs agents run (`--claude-program`, `--codex-program`).
    pub cli_programs: Arc<(PathBuf, PathBuf)>,
    /// Check stages and merges running for workflows (`workflow::jobs`).
    pub workflow_jobs: Arc<crate::workflow::Jobs>,
    /// Agents a Start asked to resume: seat → (its last session id, that session's usage so far).
    /// `launch_one` takes each entry once.
    pub start_resumes: Arc<Mutex<HashMap<String, (String, [u64; 4])>>>,
    /// Hosted beta analytics (None: local mode, which sends nothing).
    pub analytics: Option<Arc<crate::analytics::Analytics>>,
    /// Community on a self-run install (C5); None when hosted, where the gateway serves Community.
    pub community: Option<Arc<crate::community::CommunityLink>>,
}

impl AppState {
    pub fn new(store: Store, host_token: String, allowed_hosts: Vec<String>) -> Self {
        let (hub, _) = broadcast::channel(1024);
        let store = Arc::new(Mutex::new(store));
        let hosts = HostRegistry::default();
        hosts.set_gate(crate::budget::gate::hook(store.clone(), hub.clone()));
        Self {
            store,
            hosts,
            hub,
            host_token: Arc::new(host_token),
            allowed_hosts: Arc::new(allowed_hosts),
            request_locks: Arc::default(),
            booting: Arc::default(),
            auth: AuthPolicy::default(),
            home: std::env::var_os("HOME").filter(|h| !h.is_empty()).map(|h| Arc::new(PathBuf::from(h))),
            handoffs_started: Arc::default(),
            handoffs_in_flight: Arc::default(),
            context_asked: Arc::default(),
            seat_waits: Arc::default(),
            boot_cancel: Arc::default(),
            background_started: Arc::default(),
            cli_programs: Arc::new((PathBuf::from("claude"), PathBuf::from("codex"))),
            workflow_jobs: Arc::default(),
            start_resumes: Arc::default(),
            analytics: None,
            community: None,
        }
    }

    pub fn with_analytics(mut self, a: Arc<crate::analytics::Analytics>) -> Self {
        self.analytics = Some(a);
        self
    }

    pub fn with_community(mut self, link: crate::community::CommunityLink) -> Self {
        self.community = Some(Arc::new(link));
        self
    }

    pub fn analytics(&self) -> Option<&crate::analytics::Analytics> {
        self.analytics.as_deref()
    }

    pub fn with_cli_programs(mut self, claude: Option<PathBuf>, codex: Option<PathBuf>) -> Self {
        let (c, x) = (*self.cli_programs).clone();
        self.cli_programs = Arc::new((claude.unwrap_or(c), codex.unwrap_or(x)));
        self
    }

    pub fn cli_program(&self, name: &str) -> PathBuf {
        if name == "claude" { self.cli_programs.0.clone() } else { self.cli_programs.1.clone() }
    }

    pub fn with_auth(mut self, auth: AuthPolicy) -> Self {
        self.auth = auth;
        self
    }

    pub fn with_home(mut self, home: impl Into<PathBuf>) -> Self {
        self.home = Some(Arc::new(home.into()));
        self
    }

    /// Claims the boot of `rig` synchronously. None when a boot is already in flight.
    /// The claim is released when the returned guard drops.
    pub fn try_begin_boot(&self, rig: &str) -> Option<BootGuard> {
        if self.booting.lock().unwrap().insert(rig.to_string()) {
            Some(BootGuard { st: self.clone(), rig: rig.to_string() })
        } else {
            None
        }
    }

    /// True while a boot of `rig` is running (Task 11 answers 409 for a second one).
    pub fn is_booting(&self, rig: &str) -> bool {
        self.booting.lock().unwrap().contains(rig)
    }

    /// Appends a controld-originated event and publishes it while still holding the
    /// store lock, so a concurrent ingest cannot publish a later seq first.
    pub fn local_event(
        &self,
        actor: Actor,
        subject: Subject,
        provenance: Provenance,
        body: EventBody,
    ) -> rusqlite::Result<crate::views::StreamItem> {
        let mut store = self.store.lock().unwrap();
        let item = store.append_local(actor, subject, provenance, body)?;
        let _ = self.hub.send(item.clone());
        Ok(item)
    }

    /// Publishes stored items. Call it while holding the store lock that stored them.
    /// `broadcast::Sender::send` never blocks.
    pub fn publish(&self, items: Vec<StreamItem>) {
        for item in items {
            let _ = self.hub.send(item);
        }
    }
}

/// Holds a rig's boot claim; removes the rig from the booting set on drop, however the boot ends.
pub struct BootGuard {
    st: AppState,
    rig: String,
}

impl Drop for BootGuard {
    fn drop(&mut self) {
        self.st.booting.lock().unwrap().remove(&self.rig);
    }
}
