//! Planning one start: agent by agent, upstreams first, the handoffs due and why the others wait.
use super::plan::{delivery, fate, groups, job_unsent, push_status, stuck, Delivery, Fate};
use super::starts::Ended;
use super::team::{loop_backs, reachable, topo_order, upstreams_of, Team};
use super::work::{seat_work, work_items, Item, SeatWork};
use super::{restart_key, Action, HANDOFF_ACTOR, LOOP_ROUNDS};
use crate::hosts::LOCAL_HOST;
use crate::store::{HandedOverRecord, HandoffWrite, Store};
use model::{Actor, HandoffSource, HandoffStuck, MessageIntent, SeatLifecycle, Wave};
use std::collections::{BTreeMap, HashMap, HashSet};

/// One start to plan: its `RigBooted` seq, why and where it ended (if it did), and whether it
/// is the team's current start (only that one sets waiting and stuck states).
#[derive(Clone, Copy)]
pub(super) struct StartRef {
    pub(super) seq: i64,
    pub(super) end: Option<(Ended, i64)>,
    pub(super) current: bool,
}

/// Why an agent waits for its next handoff: the upstreams it waits for, and what blocks one of them.
#[derive(Default)]
struct Waiting {
    waiting: Vec<String>,
    blocked: Option<HandoffStuck>,
}

impl Waiting {
    fn add(&mut self, missing: Vec<String>, blocked: impl FnOnce(&[String]) -> Option<HandoffStuck>) {
        if self.blocked.is_none() {
            self.blocked = blocked(&missing);
        }
        for m in missing {
            if !self.waiting.contains(&m) {
                self.waiting.push(m);
            }
        }
    }
}

/// One start being planned: what every agent did in it, and, as the agents are planned
/// upstreams first, which are stuck and which have nothing left to do.
pub(super) struct StartPlan<'a> {
    store: &'a Store,
    team: &'a Team<'a>,
    start: StartRef,
    in_flight: &'a HashSet<String>,
    upstreams: HashMap<String, Vec<String>>,
    /// (from, to) arrows that close a loop: work comes back along them, never waited for.
    loop_backs: HashSet<(String, String)>,
    work: HashMap<String, SeatWork>,
    items: HashMap<String, Vec<Item>>,
    records: HashMap<String, Vec<HandedOverRecord>>,
    /// Stuck reasons, so a downstream agent can name a stuck upstream.
    stuck_of: HashMap<String, HandoffStuck>,
    /// Agents with nothing left to do (see `nothing_left`): a wait on one of them never ends by itself.
    idle: HashSet<String>,
}

impl<'a> StartPlan<'a> {
    /// Reads what every agent did in this start.
    pub(super) fn new(store: &'a Store, team: &'a Team<'a>, start: StartRef, in_flight: &'a HashSet<String>) -> rusqlite::Result<Self> {
        let upstreams: HashMap<String, Vec<String>> = upstreams_of(team.rig).into_iter().collect();
        let before = |seq: i64| start.end.is_none_or(|(_, at)| seq < at);
        let (mut work, mut items, mut records) = (HashMap::new(), HashMap::new(), HashMap::new());
        for s in topo_order(team.rig) {
            let recs = store.handed_over(&s, start.seq)?;
            let head_without_job = !upstreams.contains_key(&s) && team.job(&s).is_none();
            let w = seat_work(store, &s, start.seq, &recs, head_without_job)?;
            items.insert(s.clone(), work_items(&w, &s, &before));
            work.insert(s.clone(), w);
            records.insert(s, recs);
        }
        let (stuck_of, idle) = (HashMap::new(), HashSet::new());
        let loop_backs = loop_backs(team.rig);
        Ok(StartPlan { store, team, start, in_flight, upstreams, loop_backs, work, items, records, stuck_of, idle })
    }

    pub(super) fn run(mut self, actions: &mut Vec<Action>) -> rusqlite::Result<()> {
        for b in topo_order(self.team.rig) {
            match self.upstreams.get(&b).cloned() {
                None => self.plan_head(&b, actions)?,
                Some(ups) => self.plan_downstream(&b, &ups, actions)?,
            }
        }
        Ok(())
    }

    fn ended(&self) -> Option<Ended> {
        self.start.end.map(|(why, _)| why)
    }

    /// The current start, still going: the only one whose agents can wait, be stuck or be idle.
    fn live(&self) -> bool {
        self.start.current && self.ended().is_none()
    }

    fn own_stuck(&self, b: &str) -> Option<HandoffStuck> {
        self.work[b].unfinished.as_ref().map(|(_, why)| stuck(format!("{}'s work did not finish ({why})", self.team.name(b)), false))
    }

    /// Why a wait on upstream `u` would not end by itself, if it would not.
    fn blocked_by(&self, u: &str) -> Option<HandoffStuck> {
        let name = self.team.name(u);
        match self.team.lifecycle_in(u, self.start.seq) {
            Some(SeatLifecycle::Attention) => return Some(stuck(format!("{name} needs attention"), false)),
            Some(SeatLifecycle::Failed) => return Some(stuck(format!("{name} failed"), false)),
            Some(SeatLifecycle::Stopped) => return Some(stuck(format!("{name} stopped"), false)),
            _ => {}
        }
        if self.stuck_of.contains_key(u) {
            return Some(stuck(format!("waiting on {name} (stuck)"), false));
        }
        self.idle.contains(u).then(|| stuck(format!("waiting on {name}, who has nothing left to do"), false))
    }

    /// An agent nobody hands work to: stuck only on its own work or an unsent Job.
    fn plan_head(&mut self, b: &str, actions: &mut Vec<Action>) -> rusqlite::Result<()> {
        let own = match self.own_stuck(b) {
            None if self.live() && job_unsent(self.store, self.team, b, self.start.seq, &self.work[b], self.in_flight)? => {
                Some(stuck(format!("{}'s Job was not sent", self.team.name(b)), true))
            }
            own => own,
        };
        if let Some(s) = &own {
            self.stuck_of.insert(b.to_string(), s.clone());
        }
        if self.live() && self.nothing_left(b, false)? {
            self.idle.insert(b.to_string());
        }
        if self.start.current {
            push_status(self.team, b, Vec::new(), if self.ended().is_some() { None } else { own }, actions);
        }
        Ok(())
    }

    fn plan_downstream(&mut self, b: &str, ups: &[String], actions: &mut Vec<Action>) -> rusqlite::Result<()> {
        let own = self.own_stuck(b);
        let planned_before = actions.len();
        let mut wait = Waiting::default();
        let mut delivery_stuck = None;
        if self.records[b].iter().any(|r| r.resend_of.is_none()) {
            delivery_stuck = self.delivery_stuck(b)?;
            self.wave_handoffs(b, ups, &mut wait, actions);
        } else {
            self.first_handoff(b, ups, &mut wait, actions);
        }
        let status = own.or(delivery_stuck).or(wait.blocked);
        if let Some(s) = &status {
            self.stuck_of.insert(b.to_string(), s.clone());
        }
        // Something this plan is about to hand it is something left to do.
        let handing = actions[planned_before..].iter().any(|a| matches!(a, Action::Hand { write, .. } if write.failed.is_none()));
        if self.live() && wait.waiting.is_empty() && !handing && self.nothing_left(b, true)? {
            self.idle.insert(b.to_string());
        }
        if self.start.current {
            if self.ended().is_some() {
                push_status(self.team, b, Vec::new(), None, actions);
            } else {
                push_status(self.team, b, wait.waiting, status, actions);
            }
        }
        Ok(())
    }

    /// The upstreams `b` waits for: all but those that hand work back to it round a loop.
    fn forward(&self, b: &str, ups: &[String]) -> Vec<String> {
        ups.iter().filter(|u| !self.loop_backs.contains(&((*u).clone(), b.to_string()))).cloned().collect()
    }

    /// A downstream agent's first handoff of a start waits until every upstream agent has a
    /// result (not one that hands work back round a loop); it carries the agent's Job and each
    /// of those upstreams' latest result.
    fn first_handoff(&self, b: &str, ups: &[String], wait: &mut Waiting, actions: &mut Vec<Action>) {
        let ups = &self.forward(b, ups);
        let missing: Vec<String> = ups.iter().filter(|u| self.items[*u].is_empty()).cloned().collect();
        if !missing.is_empty() {
            wait.add(missing, |m| m.iter().find_map(|u| self.blocked_by(u)));
            return;
        }
        let failed = match fate(self.team, b, self.start.seq, self.ended()) {
            Fate::Wait => return,
            Fate::Send => None,
            Fate::Fail(why) => Some(why),
        };
        let names: Vec<String> = ups.iter().map(|u| self.team.name(u)).collect();
        let results: Vec<(String, Option<String>)> =
            ups.iter().map(|u| (self.team.name(u), self.items[u].last().and_then(|i| i.turn.text.clone()))).collect();
        let covered_items: Vec<(&String, &Item)> = ups.iter().flat_map(|u| self.items[u].iter().map(move |i| (u, i))).collect();
        let from = covered_items.iter().map(|(u, i)| i.source(u)).collect();
        // A first handoff that carries a wave's work continues that wave.
        let wave = covered_items.iter().filter_map(|(_, i)| i.wave.clone()).max_by_key(|w| w.id);
        let latest: Vec<(&String, &Item)> = ups.iter().filter_map(|u| self.items[u].last().map(|i| (u, i))).collect();
        if let Some(message_id) = failed.is_none().then(|| self.sent_directly(b, &latest)).flatten() {
            actions.push(Action::Adopt { seat: b.to_string(), start_seq: self.start.seq, first: true, message_id, from, wave });
            return;
        }
        let text = super::first_text(&names, self.team.job(b).as_deref(), &results);
        actions.push(self.hand(b, true, text, from, wave, failed));
    }

    /// Stuck: a handoff never reached the agent (not sent), or was recorded and never sent on
    /// (it is queued, and nothing is sending it).
    fn delivery_stuck(&self, b: &str) -> rusqlite::Result<Option<HandoffStuck>> {
        for (r, copies) in groups(&self.records[b]) {
            let mut from_names: Vec<String> = r.from.iter().map(|f| self.team.name(&f.seat)).collect();
            from_names.dedup();
            let from_words = from_names.join(", ");
            match delivery(self.store, &self.work[b], &copies, self.in_flight)? {
                Delivery::NotSent => return Ok(Some(stuck(format!("the handoff from {from_words} was not sent"), true))),
                Delivery::NotDelivered => return Ok(Some(stuck(format!("the handoff from {from_words} was not delivered"), true))),
                _ => {}
            }
        }
        Ok(None)
    }

    /// The upstream items not handed to `b` yet, by wave, oldest wave first.
    fn fresh_waves(&self, b: &str, ups: &[String]) -> BTreeMap<i64, (Wave, Vec<(String, Item)>)> {
        let covered: HashSet<i64> = self.records[b].iter().flat_map(|r| r.from.iter().map(|f| f.key())).collect();
        let mut waves: BTreeMap<i64, (Wave, Vec<(String, Item)>)> = BTreeMap::new();
        for u in ups {
            for i in self.items[u].iter().filter(|i| !covered.contains(&i.key())) {
                let w = i.wave_of(u);
                waves.entry(w.id).or_insert_with(|| (w.clone(), Vec::new())).1.push((u.clone(), i.clone()));
            }
        }
        waves
    }

    /// Waves: one message per wave, once every upstream the wave reaches finished (work coming
    /// back round a loop is never waited for). Round a loop, one wave stops after LOOP_ROUNDS.
    fn wave_handoffs(&self, b: &str, ups: &[String], wait: &mut Waiting, actions: &mut Vec<Action>) {
        let forward = self.forward(b, ups);
        for (_, (wave, fresh)) in self.fresh_waves(b, ups) {
            if self.rounds(b, &wave) >= LOOP_ROUNDS {
                wait.blocked.get_or_insert_with(|| self.loop_limit(&fresh));
                continue;
            }
            let reach = reachable(self.team.rig, &wave.origin);
            let wave_ups: Vec<&String> = forward.iter().filter(|u| **u == wave.origin || reach.contains(*u)).collect();
            let done = |u: &str| self.items[u].iter().any(|i| i.wave_of(u) == wave);
            let missing: Vec<String> = wave_ups.iter().filter(|u| !done(u)).map(|u| u.to_string()).collect();
            if !missing.is_empty() {
                wait.add(missing, |m| m.iter().find_map(|u| self.blocked_by(u)));
                continue;
            }
            let failed = match fate(self.team, b, self.start.seq, self.ended()) {
                Fate::Wait => break,
                Fate::Send => None,
                Fate::Fail(why) => Some(why),
            };
            actions.push(self.wave_message(b, wave, &fresh, failed));
        }
    }

    /// How many messages of this wave `b` has had: more than one only round a loop.
    fn rounds(&self, b: &str, wave: &Wave) -> usize {
        self.records[b].iter().filter(|r| r.resend_of.is_none() && r.wave.as_ref().is_some_and(|w| w.id == wave.id)).count()
    }

    fn loop_limit(&self, fresh: &[(String, Item)]) -> HandoffStuck {
        let mut names: Vec<String> = fresh.iter().map(|(u, _)| self.team.name(u)).collect();
        names.dedup();
        let names = names.join(", ");
        stuck(format!("the loop with {names} stopped after {LOOP_ROUNDS} rounds of this work; choose Hand over again on {names} to go round again"), false)
    }

    /// The "More work" message for one wave: each upstream's latest result in it.
    fn wave_message(&self, b: &str, wave: Wave, fresh: &[(String, Item)], failed: Option<String>) -> Action {
        let mut from_seats: Vec<&String> = Vec::new();
        for (u, _) in fresh {
            if !from_seats.contains(&u) {
                from_seats.push(u);
            }
        }
        let names: Vec<String> = from_seats.iter().map(|u| self.team.name(u)).collect();
        let results: Vec<(String, Option<String>)> = from_seats
            .iter()
            .map(|u| (self.team.name(u), fresh.iter().rfind(|(s, _)| s == *u).and_then(|(_, i)| i.turn.text.clone())))
            .collect();
        let from = fresh.iter().map(|(u, i)| i.source(u)).collect();
        let latest: Vec<(&String, &Item)> = from_seats.iter().filter_map(|u| fresh.iter().rfind(|(s, _)| s == *u).map(|(s, i)| (s, i))).collect();
        if let Some(message_id) = failed.is_none().then(|| self.sent_directly(b, &latest)).flatten() {
            return Action::Adopt { seat: b.to_string(), start_seq: self.start.seq, first: false, message_id, from, wave: Some(wave) };
        }
        self.hand(b, false, super::more_text(&names, &results), from, Some(wave), failed)
    }

    /// The message each upstream's agent sent `b` itself during the turn whose result would be
    /// passed along, when every one of them sent one: those messages already handed the work over,
    /// and the result again would only wake `b` to re-read its conversation. The latest of them
    /// stands in for the handoff (a "Hand over again" press always sends).
    fn sent_directly(&self, b: &str, latest: &[(&String, &Item)]) -> Option<String> {
        let adopted: HashSet<&str> = self.records[b].iter().map(|r| r.message_id.as_str()).collect();
        let mut newest: Option<&(i64, String, String)> = None;
        for (u, i) in latest {
            if i.requested_seq.is_some() {
                return None;
            }
            let during = |(seq, id, from): &&(i64, String, String)| from == *u && *seq > i.turn.started && *seq < i.turn.seq && !adopted.contains(id.as_str());
            let m = self.work[b].from_agents.iter().rfind(during)?;
            if newest.is_none_or(|n| m.0 > n.0) {
                newest = Some(m);
            }
        }
        newest.map(|(_, id, _)| id.clone())
    }

    fn hand(&self, b: &str, first: bool, text: String, from: Vec<HandoffSource>, wave: Option<Wave>, failed: Option<String>) -> Action {
        let host = self.team.view(b).and_then(|v| v.host.clone()).unwrap_or_else(|| LOCAL_HOST.to_string());
        let write = HandoffWrite {
            seat: b.to_string(),
            start_seq: self.start.seq,
            first,
            actor: Actor::System(HANDOFF_ACTOR.into()),
            message_id: uuid::Uuid::new_v4().to_string(),
            intent: MessageIntent::Inform,
            text,
            from,
            wave,
            failed,
        };
        Action::Hand { write, host, name: self.team.name(b), team: self.team.names.clone() }
    }

    /// True when `seat` has nothing left to do in this start, so an agent waiting for it would
    /// wait for ever: it is running and not in a turn; it has no work read and unfinished (that
    /// shows as its own "stuck"), no message unread (queued or delivered), no restart whose
    /// owed messages are still being sent, and its Job, if it has one, has come. A first
    /// agent with no Job is never idle in this sense: its next message is its work, so it waits
    /// for you. `handed` says whether it is handed work by other agents. The caller adds that it
    /// waits for nobody and is not being handed anything right now.
    fn nothing_left(&self, seat: &str, handed: bool) -> rusqlite::Result<bool> {
        let w = &self.work[seat];
        if self.team.lifecycle_in(seat, self.start.seq) != Some(SeatLifecycle::Running)
            || w.in_turn
            || w.reading
            || w.unfinished.is_some()
            || self.in_flight.contains(&restart_key(seat))
        {
            return Ok(false);
        }
        if !handed && !self.job_came(seat, w)? {
            return Ok(false);
        }
        for m in w.queued.iter().filter(|m| !w.read.contains(*m) && !w.lost.contains(*m)) {
            if matches!(self.store.message_state(m)?, Some(model::MessageState::Queued | model::MessageState::Delivered)) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// A first agent's Job has come once one was queued and not recorded as failed (a previous
    /// start's unsent Job can be recorded after this start began). One with no Job waits for you.
    fn job_came(&self, seat: &str, w: &SeatWork) -> rusqlite::Result<bool> {
        if self.team.job(seat).is_none() {
            return Ok(false);
        }
        let mut job_came = false;
        for j in &w.jobs {
            job_came |= w.read.contains(j) || self.store.message_state(j)? != Some(model::MessageState::Failed);
        }
        Ok(job_came)
    }
}
