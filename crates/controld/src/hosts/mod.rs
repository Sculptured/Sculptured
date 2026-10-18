//! Host connections: hostd dials `GET /host` with a bearer token, says Hello, and
//! receives Welcome{resume_after}. Events are then ingested and acked one by one,
//! and commands flow back with request/response correlation.
mod registry;
mod session;

use crate::state::AppState;
use host_proto::{features, LaunchParams};
use model::{Actor, EventBody, Provenance, SeatLifecycle, Subject};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

pub use registry::{CallStart, CommandFailure, GateFn, HostRegistry, OpFailure, RECONNECT_GRACE};
pub use session::host_ws;

pub const LOCAL_HOST: &str = "local";
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<Result<(), String>>>>>;

/// The actor for events controld records itself.
pub(crate) fn controld() -> Actor {
    Actor::System("controld".into())
}

/// `provenance` is ProtocolObserved when the host's Hello says so (a live seat) and
/// Heuristic when controld infers it (a missing process, an unreachable host).
pub(crate) fn record_lifecycle(st: &AppState, seat: String, provenance: Provenance, lifecycle: SeatLifecycle, reason: String) {
    let body = EventBody::SeatLifecycleChanged { lifecycle, reason: Some(reason) };
    if let Err(e) = st.local_event(controld(), Subject::Seat(seat.clone()), provenance, body) {
        tracing::error!(%seat, error = %e, "cannot record seat lifecycle change");
    }
}

/// A host at the start of a sentence, in plain words: the local host is "Sculptured's
/// agent runner", another one "Host {id}".
fn runner(host: &str) -> String {
    if host == LOCAL_HOST { "Sculptured's agent runner".into() } else { format!("Host {host}") }
}

/// How to update an older agent runner, in working steps. A hostd outlives a Sculptured restart by
/// design (it owns the agents), so restarting Sculptured alone never updates it. Once it is ended,
/// `sculptured serve` starts the new one: its supervisor checks back every 30 seconds while another
/// runner holds the data folder. Teams are stopped first so no agent outlives its runner.
fn update_steps(host: &str) -> String {
    if host == LOCAL_HOST {
        "Stop all teams, then end the old agent runner: its process number is in the file hostd.pid, in the host-local folder of Sculptured's data folder. Sculptured starts the new runner within a minute.".into()
    } else {
        format!("Stop the agents on host {host}, then end its agent runner and start the new one.")
    }
}

/// The refusal for a command an older host cannot do, with the steps that update it.
pub fn older_runner(host: &str) -> String {
    let what = if host == LOCAL_HOST { "Sculptured's agent runner".to_string() } else { format!("Host {host}'s agent runner") };
    format!("{what} is from an older version. {}", update_steps(host))
}

/// Fits launch parameters to what `host` can do. Posture is safety-critical: a restricted agent on a
/// host that cannot honour it is refused (`older_runner`), never run unrestricted. Optional abilities the
/// host lacks (seat tools, a usage baseline) are dropped, and the drop is recorded on the agent as a
/// `FeaturesDropped` event with a plain note. Returns what was dropped.
pub fn fit_params(st: &AppState, host: &str, seat: &str, params: &mut LaunchParams) -> Result<Vec<&'static str>, String> {
    let missing: Vec<&'static str> =
        params.required_features().into_iter().filter(|f| st.hosts.is_connected(host) && !st.hosts.host_supports(host, f)).collect();
    if missing.contains(&features::POSTURE) {
        return Err(older_runner(host));
    }
    if missing.is_empty() {
        return Ok(missing);
    }
    let lost = drop_features(params, &missing);
    let note = format!(
        "This agent runs without {}, because {} is from an older version. To get them back: {}",
        lost.join(" and without "),
        if host == LOCAL_HOST { "Sculptured's agent runner".to_string() } else { format!("host {host}'s agent runner") },
        update_steps(host)
    );
    let body = EventBody::FeaturesDropped { features: missing.iter().map(|f| f.to_string()).collect(), note };
    if let Err(e) = st.local_event(controld(), Subject::Seat(seat.to_string()), Provenance::ProtocolObserved, body) {
        tracing::error!(%seat, error = %e, "cannot record the features dropped for an older runner");
    }
    Ok(missing)
}

/// Drops the optional abilities in `missing` from `params`; returns them in plain words.
fn drop_features(params: &mut LaunchParams, missing: &[&str]) -> Vec<&'static str> {
    let mut lost = Vec::new();
    for f in missing {
        match *f {
            features::SEAT_TOOLS => {
                params.seat_tools = None;
                lost.push("Sculptured's agent tools");
            }
            features::USAGE_BASELINE => {
                params.usage_baseline = None;
                lost.push("its earlier usage count");
            }
            features::CLAUDE_SETUP => {
                params.claude_setup = None;
                lost.push("the setting that leaves out your own Claude Code plugins, skills and hooks");
            }
            features::PROTECTION => {
                params.protection = None;
                lost.push("Sculptured's protections (reinstall the runner from Computers to get them)");
            }
            features::SKILLS => {
                params.skills.clear();
                lost.push("its skills");
            }
            features::INTEGRATIONS => {
                params.integrations.clear();
                lost.push("the tools on its computer it was given, such as Kindex");
            }
            features::COMPONENTS => {
                // An older runner still starts the built-in ones by name, when it can.
                let custom = params.components.iter().any(|c| !c.is_builtin());
                params.components.clear();
                if custom {
                    lost.push("its components that are not built in");
                }
            }
            features::COMPONENT_KINDS => {
                params.components.retain(|c| !c.kind.is_newer());
                lost.push("its remote MCP servers and Claude plugins");
            }
            _ => {}
        }
    }
    lost
}

/// "Sculptured's agent runner is not connected", or "Host {id} is not connected" for
/// another host: the reason a command could not reach a host, in plain words.
pub fn not_connected(host: &str) -> String {
    format!("{} is not connected", runner(host))
}

fn mark_stale(st: &AppState, host: &str) {
    let seats = match st.store.lock().unwrap().seats_on_host(host) {
        Ok(seats) => seats,
        Err(e) => {
            tracing::error!(%host, error = %e, "cannot read seats to mark stale");
            return;
        }
    };
    for s in seats {
        if matches!(s.lifecycle, SeatLifecycle::Running | SeatLifecycle::Launching | SeatLifecycle::Stopping) {
            let reason = if host == LOCAL_HOST {
                "Lost contact with this agent's runner; reconnecting…".to_string()
            } else {
                format!("Lost contact with host {host}; reconnecting…")
            };
            record_lifecycle(st, s.id, Provenance::Heuristic, SeatLifecycle::Stale, reason);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_words_are_plain() {
        assert_eq!(not_connected(LOCAL_HOST), "Sculptured's agent runner is not connected");
        assert_eq!(not_connected("box"), "Host box is not connected");
        assert_eq!(format!("{} did not reply within 30 s", runner(LOCAL_HOST)), "Sculptured's agent runner did not reply within 30 s");
    }
}
