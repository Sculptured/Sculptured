//! A team's starts: the current one and the one before it, and how each ended.
use crate::store::Store;
use model::{Desired, EventBody, Subject};

/// Why a start no longer hands work over.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Ended {
    Stopped,
    StartedAgain,
}

/// A start: its `RigBooted` seq, and how and where it ended, if it did.
pub(super) type StartInfo = (i64, Option<(Ended, i64)>);

/// The team's current start and the one before it. Starts at or below the handoff cutoff
/// (booted before handoffs existed) are not planned and do not appear here.
pub(super) fn starts(
    store: &Store,
    rig: &str,
) -> rusqlite::Result<(Option<StartInfo>, Option<StartInfo>)> {
    let rig_subject = Subject::Rig(rig.to_string());
    let cutoff = store.handoff_cutoff()?;
    let boots: Vec<i64> = store
        .latest_of_types(&rig_subject, &["rig-booted"], 2)?
        .into_iter()
        .map(|e| e.seq)
        .filter(|s| *s > cutoff)
        .collect();
    let Some(&current) = boots.first() else {
        return Ok((None, None));
    };
    let stopped_after = |seq: i64, before: Option<i64>| -> rusqlite::Result<Option<i64>> {
        Ok(store
            .events_of_types(&rig_subject, seq, &["rig-desired-changed"])?
            .into_iter()
            .filter(|e| before.is_none_or(|b| e.seq < b))
            .find(|e| {
                matches!(
                    e.body,
                    EventBody::RigDesiredChanged {
                        desired: Desired::Stopped
                    }
                )
            })
            .map(|e| e.seq))
    };
    let previous = match boots.get(1) {
        Some(&p) => Some((
            p,
            Some(match stopped_after(p, Some(current))? {
                Some(at) => (Ended::Stopped, at),
                None => (Ended::StartedAgain, current),
            }),
        )),
        None => None,
    };
    let now = (
        current,
        stopped_after(current, None)?.map(|at| (Ended::Stopped, at)),
    );
    Ok((Some(now), previous))
}
