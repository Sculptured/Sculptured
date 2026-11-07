//! Helpers for the live control and computer-link tests: waits as long as a real model turn, a long
//! real turn, and a runner with the real CLIs whose link a test can cut and bring back. Included with
//! `#[path = "common/live_controls.rs"] mod live_controls;` by those tests only.
#![allow(dead_code)]
use crate::common::lifecycle;
use crate::common::live;
use crate::common::m2::{Served, TOKEN};
use controld::store::ControlMode;
use hostd::journal::Journal;
use hostd::supervisor::{SeatToolsHost, Supervisor};
use model::{EventBody, SeatLifecycle};
use tokio::task::JoinHandle;

/// A cheap message: the agent answers in one short turn.
pub const SAY_OK: &str = "Reply with the single word OK and nothing else.";

/// A message that keeps the agent in one long turn (a real `sleep 600` through its Bash tool).
pub const HANG: &str = "Use your Bash tool to run exactly this command and nothing else: sleep 600";

/// Waits up to `live::WAIT` until each of `seats` is `want`.
pub async fn wait_lifecycle(s: &Served, seats: &[&String], want: SeatLifecycle) {
    live::wait_for(&format!("{seats:?} {want:?}"), || {
        seats.iter().all(|x| lifecycle(&s.st, x) == Some(want))
    })
    .await;
}

/// Waits up to `live::WAIT` until each of `seats` has wrapped up, stopped and is paused.
pub async fn wait_paused(s: &Served, seats: &[&String]) {
    wait_lifecycle(s, seats, SeatLifecycle::Stopped).await;
    live::wait_for("paused", || {
        let store = s.st.store.lock().unwrap();
        seats
            .iter()
            .all(|x| store.control(x).unwrap() == Some(ControlMode::Paused))
    })
    .await;
}

/// Asks `seat` to run `sleep 600` and returns once its Bash call is captured: it is mid-turn.
pub async fn hang(s: &Served, seat: &str) {
    crate::common::m2::say(s, seat, HANG).await;
    live::wait_event(
        &s.st,
        seat,
        "the long Bash call",
        |b| matches!(b, EventBody::ToolCalled { name, .. } if name == "Bash"),
    )
    .await;
}

/// This machine's runner (`local`) with the real CLIs, and a link the test can cut and bring back.
pub struct Runner {
    pub sup: Supervisor,
    pub link: JoinHandle<()>,
    /// Holds the seat-tools socket, when the agents have Sculptured's tools.
    _tools: Option<tempfile::TempDir>,
}

impl Runner {
    pub async fn connect(s: &Served) -> Runner {
        let sup = Supervisor::new(
            "local".into(),
            live::drivers(),
            Journal::open_in_memory(10_000).unwrap(),
        );
        let link = Runner::link(s, &sup).await;
        Runner {
            sup,
            link,
            _tools: None,
        }
    }

    /// A runner whose agents call Sculptured's tools through the real shim and socket.
    pub async fn with_tools(s: &Served) -> Runner {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("seat-tools.sock");
        let host = SeatToolsHost {
            socket: socket.clone(),
            shim: live::sculptured_bin(),
        };
        let sup = Supervisor::with_seat_tools(
            "local".into(),
            live::drivers(),
            Journal::open_in_memory(10_000).unwrap(),
            Some(host),
        );
        tokio::spawn(hostd::seat_tools::serve_socket(sup.clone(), socket.clone()));
        crate::common::wait_for("the socket", || socket.exists()).await;
        let link = Runner::link(s, &sup).await;
        Runner {
            sup,
            link,
            _tools: Some(dir),
        }
    }

    async fn link(s: &Served, sup: &Supervisor) -> JoinHandle<()> {
        let (url, sup) = (s.url.clone(), sup.clone());
        let link = tokio::spawn(async move {
            let _ = hostd::run_link(&url, TOKEN, &sup).await;
        });
        crate::common::wait_for("computer connected", || s.st.hosts.is_connected("local")).await;
        link
    }

    /// The link drops (its agents keep running on this computer).
    pub async fn drop_link(&self, s: &Served) {
        self.link.abort();
        crate::common::wait_for("computer gone", || !s.st.hosts.is_connected("local")).await;
    }

    pub async fn reconnect(&mut self, s: &Served) {
        self.link = Runner::link(s, &self.sup).await;
    }
}

/// A runner `host_id` with the real CLIs, allowed only `folders` (the first is where "." resolves).
pub async fn computer(s: &Served, host_id: &str, folders: &[&std::path::Path]) -> Supervisor {
    let mut pol = hostd::policy::RunnerPolicy::default();
    for f in folders {
        std::fs::create_dir_all(f).unwrap();
        pol.allow(f).unwrap();
    }
    let sup = Supervisor::new(
        host_id.into(),
        live::drivers(),
        Journal::open_in_memory(10_000).unwrap(),
    )
    .restricted(pol);
    let (url, link) = (s.url.clone(), sup.clone());
    tokio::spawn(async move {
        let _ = hostd::run_link(&url, TOKEN, &link).await;
    });
    crate::common::wait_for("connected", || s.st.hosts.is_connected(host_id)).await;
    sup
}
