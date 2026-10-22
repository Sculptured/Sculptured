//! Live updates over server-sent events.
//!
//! `GET /api/stream?rig=NAME`: the rig snapshot is always sent first; then `event`
//! frames (StreamItem, id = seq) for this rig's seats, the rig itself and hosts; then
//! `resync` if this client fell behind. The client re-snapshots on resync.
//!
//! `GET /api/stream?all=true`: one connection for every team, so a browser never runs
//! out of connections however many teams exist. A `snapshot` frame per rig, then
//! `ready`; then `event` frames (TaggedStreamItem, id = seq) tagged with their rig (null
//! for a host event). A rig that first appears later, or boots again (its shape may have
//! changed), gets a fresh `snapshot` frame before its later events. `resync` means the
//! client fell behind; it reopens and every rig re-snapshots.
use crate::error::ApiError;
use crate::extract::Query;
use crate::state::AppState;
use crate::views::{RigSnapshot, StreamItem, TaggedStreamItem};
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use model::{EventBody, Subject};
use serde::Deserialize;
use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use tokio::sync::broadcast::error::RecvError;

#[derive(Deserialize)]
pub struct StreamQuery {
    pub rig: Option<String>,
    pub all: Option<bool>,
}

/// The rig an item belongs to; None for a host event.
fn rig_of(item: &StreamItem) -> Option<&str> {
    match &item.event.subject {
        Subject::Seat(id) => Some(crate::seat_ids::rig_or_empty(id)),
        Subject::Rig(r) => Some(r),
        Subject::Host(_) => None,
    }
}

fn belongs(item: &StreamItem, rig: &str) -> bool {
    rig_of(item).is_none_or(|r| r == rig)
}

fn snapshot_frame(snapshot: &RigSnapshot) -> Event {
    Event::default()
        .event("snapshot")
        .json_data(snapshot)
        .expect("snapshot serializes")
}

fn resync_frame() -> Event {
    Event::default().event("resync").data("lagged")
}

pub async fn stream(State(st): State<AppState>, Query(q): Query<StreamQuery>) -> Response {
    match (q.rig, q.all) {
        (Some(rig), None) => one_rig(st, rig),
        (None, Some(true)) => every_rig(st),
        _ => ApiError::bad_request(
            "bad_query",
            "Ask for one team with rig=NAME, or for every team with all=true",
        )
        .into_response(),
    }
}

fn one_rig(st: AppState, rig: String) -> Response {
    // Subscribe before reading the snapshot so no event falls between them.
    let rx = st.hub.subscribe();
    let snapshot = match st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .rig_snapshot(&rig)
    {
        Ok(Some(s)) => s,
        Ok(None) => return ApiError::not_found(format!("no rig {rig:?}")).into_response(),
        Err(e) => return ApiError::from(e).into_response(),
    };
    // Events up to here are already in the snapshot; the hub then carries later ones
    // in seq order (they are published under the store lock).
    let covered = snapshot.last_seq;
    let first = snapshot_frame(&snapshot);
    let head = futures::stream::once(async move { Ok::<Event, Infallible>(first) });
    let live = futures::stream::unfold((rx, rig), move |(mut rx, rig)| async move {
        loop {
            match rx.recv().await {
                Ok(item) => {
                    if item.event.seq <= covered || !belongs(&item, &rig) {
                        continue;
                    }
                    if matches!(item.event.body, EventBody::TeamTopologyChanged { .. }) {
                        return Some((Ok(resync_frame()), (rx, rig)));
                    }
                    let ev = Event::default()
                        .event("event")
                        .id(item.event.seq.to_string())
                        .json_data(&item)
                        .expect("stream item serializes");
                    return Some((Ok(ev), (rx, rig)));
                }
                Err(RecvError::Lagged(_)) => return Some((Ok(resync_frame()), (rx, rig))),
                Err(RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(head.chain(live))
        .keep_alive(KeepAlive::default())
        .into_response()
}

/// What the every-team stream carries between frames.
struct Every {
    rx: tokio::sync::broadcast::Receiver<StreamItem>,
    st: AppState,
    /// Highest seq each rig's latest snapshot already reflects.
    covered: HashMap<String, i64>,
    /// Frames decided but not yet sent (a snapshot may precede an event).
    queue: VecDeque<Event>,
}

impl Every {
    /// The next frame to send; None when the hub closes.
    async fn next_frame(&mut self) -> Option<Event> {
        loop {
            if let Some(ev) = self.queue.pop_front() {
                return Some(ev);
            }
            let item = match self.rx.recv().await {
                Ok(item) => item,
                Err(RecvError::Lagged(_)) => return Some(resync_frame()),
                Err(RecvError::Closed) => return None,
            };
            let rig = rig_of(&item).map(str::to_string);
            if let Some(rig) = &rig {
                let booted = matches!(
                    item.event.body,
                    EventBody::RigBooted { .. } | EventBody::TeamTopologyChanged { .. }
                );
                if (booted || !self.covered.contains_key(rig)) && !self.cover(rig) {
                    return Some(resync_frame());
                }
                if item.event.seq <= self.covered[rig] {
                    continue;
                }
            }
            let ev = Event::default()
                .event("event")
                .id(item.event.seq.to_string())
                .json_data(TaggedStreamItem { rig, item })
                .expect("stream item serializes");
            self.queue.push_back(ev);
        }
    }

    /// Queues the rig's snapshot and records what it covers. Read under the store lock: it
    /// reflects every event up to its last_seq, and later ones still reach this receiver. False
    /// when it cannot be read (the client then resyncs).
    fn cover(&mut self, rig: &str) -> bool {
        let snap = self
            .st
            .store
            .lock()
            .expect("store mutex must not be poisoned")
            .rig_snapshot(rig);
        match snap {
            Ok(Some(snap)) => {
                self.covered.insert(rig.to_string(), snap.last_seq);
                self.queue.push_back(snapshot_frame(&snap));
            }
            // Events for a rig with no record still go out, tagged.
            Ok(None) => {
                self.covered.entry(rig.to_string()).or_insert(0);
            }
            Err(e) => {
                tracing::warn!(%rig, error = %e, "cannot read a snapshot for the stream");
                return false;
            }
        }
        true
    }
}

/// Every rig's snapshot, as the stream's first frames.
fn all_snapshots(st: &AppState) -> rusqlite::Result<Vec<RigSnapshot>> {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    store.list_rigs().and_then(|rigs| {
        rigs.iter()
            .filter_map(|r| store.rig_snapshot(&r.name).transpose())
            .collect()
    })
}

fn every_rig(st: AppState) -> Response {
    // Subscribe before reading the snapshots so no event falls between them.
    let rx = st.hub.subscribe();
    let snapshots = match all_snapshots(&st) {
        Ok(s) => s,
        Err(e) => return ApiError::from(e).into_response(),
    };
    let mut head: Vec<Event> = snapshots.iter().map(snapshot_frame).collect();
    head.push(Event::default().event("ready").data("ready"));
    let covered = snapshots
        .into_iter()
        .map(|s| (s.rig.name, s.last_seq))
        .collect();
    let state = Every {
        rx,
        st,
        covered,
        queue: VecDeque::new(),
    };
    let live = futures::stream::unfold(state, |mut s| async move {
        s.next_frame()
            .await
            .map(|ev| (Ok::<Event, Infallible>(ev), s))
    });
    let head = futures::stream::iter(head.into_iter().map(Ok::<Event, Infallible>));
    Sse::new(head.chain(live))
        .keep_alive(KeepAlive::default())
        .into_response()
}
