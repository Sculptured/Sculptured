use crate::auth::AuthPolicy;
use crate::hosts::HostRegistry;
use crate::store::Store;
use crate::views::StreamItem;
use model::{Actor, EventBody, Provenance, Subject};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// The session and usage restored when a team's next launch resumes an agent.
pub struct ResumeSession {
    pub native_session_id: String,
    pub usage_baseline: [u64; 4],
}

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
    /// Rigs with a boot in flight; a second boot of the same rig is a no-op. A team action (a pause, a
    /// resume, a budget continuation, a stop) holds it too, with the safety-stop count when it was asked
    /// for (`try_begin_team_action`): a safety stop does not wait for one (`collaboration::preempt`).
    pub booting: Arc<Mutex<HashMap<String, Option<u64>>>>,
    /// Safety stops so far, and the latest one of each team or agent (`mark_stopped`).
    pub stops: Arc<Mutex<StopMarks>>,
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
    pub start_resumes: Arc<Mutex<HashMap<String, ResumeSession>>>,
    /// Hosted beta analytics (None: local mode, which sends nothing).
    pub analytics: Option<Arc<crate::analytics::Analytics>>,
    /// Hosted beta: where the person's model connections are read at launch (None: local mode).
    pub gateway: Option<Arc<crate::gateway::Gateway>>,
    /// Local mode: arenad, which the Tests pages reach through `/api/arena` (None: hosted, or not wired).
    pub arena: Option<Arc<crate::arena::Arena>>,
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
            stops: Arc::default(),
            auth: AuthPolicy::default(),
            home: home_dir(),
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
            gateway: None,
            arena: None,
            community: None,
        }
    }

    pub fn with_analytics(mut self, a: Arc<crate::analytics::Analytics>) -> Self {
        self.analytics = Some(a);
        self
    }

    pub fn with_gateway(mut self, g: Arc<crate::gateway::Gateway>) -> Self {
        self.gateway = Some(g);
        self
    }

    pub fn with_arena(mut self, a: Arc<crate::arena::Arena>) -> Self {
        self.arena = Some(a);
        self
    }

    pub fn with_community(mut self, link: crate::community::CommunityLink) -> Self {
        self.community = Some(Arc::new(link));
        self
    }

    #[must_use]
    pub fn analytics(&self) -> Option<&crate::analytics::Analytics> {
        self.analytics.as_deref()
    }

    #[must_use]
    pub fn with_cli_programs(mut self, claude: Option<PathBuf>, codex: Option<PathBuf>) -> Self {
        let (c, x) = (*self.cli_programs).clone();
        self.cli_programs = Arc::new((claude.unwrap_or(c), codex.unwrap_or(x)));
        self
    }

    #[must_use]
    pub fn cli_program(&self, name: &str) -> PathBuf {
        if name == "claude" {
            self.cli_programs.0.clone()
        } else {
            self.cli_programs.1.clone()
        }
    }

    #[must_use]
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
    #[must_use]
    pub fn try_begin_boot(&self, rig: &str) -> Option<BootGuard> {
        self.claim(rig, None)
    }

    /// Claims team `rig` for a team action asked for when the safety-stop count was `since`
    /// (`stop_count`). None while anything else holds the claim.
    #[must_use]
    pub fn try_begin_team_action(&self, rig: &str, since: u64) -> Option<BootGuard> {
        self.claim(rig, Some(since))
    }

    fn claim(&self, rig: &str, action: Option<u64>) -> Option<BootGuard> {
        let mut booting = self
            .booting
            .lock()
            .expect("booting mutex must not be poisoned");
        if booting.contains_key(rig) {
            return None;
        }
        booting.insert(rig.to_string(), action);
        Some(BootGuard {
            st: self.clone(),
            rig: rig.to_string(),
        })
    }

    /// True while a boot of `rig` is running (Task 11 answers 409 for a second one).
    #[must_use]
    pub fn is_booting(&self, rig: &str) -> bool {
        self.booting
            .lock()
            .expect("booting mutex must not be poisoned")
            .contains_key(rig)
    }

    /// The claim on `rig` for a safety stop: Ok(Some) when it was free; Ok(None) when a team action
    /// holds it (the stop goes ahead without it, and that action yields); Err while a boot or another
    /// change holds it.
    pub fn claim_or_preempt(&self, rig: &str) -> Result<Option<BootGuard>, TeamStarting> {
        let since = self.stop_count();
        if let Some(guard) = self.try_begin_team_action(rig, since) {
            return Ok(Some(guard));
        }
        match self
            .booting
            .lock()
            .expect("booting mutex must not be poisoned")
            .get(rig)
        {
            Some(Some(_)) => Ok(None),
            // Released meanwhile: claim it after all.
            None => self
                .try_begin_team_action(rig, since)
                .map(Some)
                .ok_or(TeamStarting),
            Some(None) => Err(TeamStarting),
        }
    }

    /// True while a team action (not a boot) holds the claim on `rig`.
    #[must_use]
    pub fn team_action_running(&self, rig: &str) -> bool {
        matches!(
            self.booting
                .lock()
                .expect("booting mutex must not be poisoned")
                .get(rig),
            Some(Some(_))
        )
    }

    /// How many safety stops there have been: a team action notes it when it is asked for.
    #[must_use]
    pub fn stop_count(&self) -> u64 {
        self.stops
            .lock()
            .expect("stops mutex must not be poisoned")
            .count
    }

    /// Records a safety stop of each of `keys` (a team's id, or agents' ids).
    pub fn mark_stopped<'a>(&self, keys: impl IntoIterator<Item = &'a str>) {
        let mut stops = self.stops.lock().expect("stops mutex must not be poisoned");
        stops.count += 1;
        let n = stops.count;
        for k in keys {
            stops.latest.insert(k.to_string(), n);
        }
    }

    /// True when team `rig`, or agent `seat`, had a safety stop after the team action that holds the
    /// claim on `rig` was asked for (false when no team action holds it).
    #[must_use]
    pub fn stopped_during_action(&self, rig: &str, seat: &str) -> bool {
        let since = match self
            .booting
            .lock()
            .expect("booting mutex must not be poisoned")
            .get(rig)
        {
            Some(Some(since)) => *since,
            _ => return false,
        };
        let stops = self.stops.lock().expect("stops mutex must not be poisoned");
        [rig, seat]
            .iter()
            .any(|k| stops.latest.get(*k).is_some_and(|n| *n > since))
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
        let mut store = self.store.lock().expect("store mutex must not be poisoned");
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

fn home_dir() -> Option<Arc<PathBuf>> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(|h| Arc::new(PathBuf::from(h)))
}

/// A boot or another change (not a team action) holds a team's claim.
#[derive(Debug)]
pub struct TeamStarting;

/// Safety stops: how many so far, and the latest one (by that count) of each team or agent.
#[derive(Default)]
pub struct StopMarks {
    count: u64,
    latest: HashMap<String, u64>,
}

/// Holds a rig's boot claim; removes the rig from the booting set on drop, however the boot ends.
pub struct BootGuard {
    st: AppState,
    rig: String,
}

impl Drop for BootGuard {
    fn drop(&mut self) {
        self.st
            .booting
            .lock()
            .expect("booting mutex must not be poisoned")
            .remove(&self.rig);
    }
}
