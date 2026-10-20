use crate::{reconciler, state::AppState};
use model::{Actor, Desired, EventBody, MessageIntent, Provenance, SeatLifecycle, Subject};
use rigspec::{CompiledEdge, EdgeKind};
use serde_json::{Value, json};

/// A store or protocol error, in words for the agent.
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn arg<'a>(v: &'a Value, key: &str, max: usize) -> Result<&'a str, String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty() && s.len() <= max)
        .ok_or_else(|| format!("{key} must contain 1–{max} bytes"))
}
fn active(st: &AppState, seat: &str) -> Result<String, String> {
    let store = st.store.lock().unwrap();
    let view = store
        .seat(seat)
        .map_err(err)?
        .ok_or("Unknown agent")?;
    if store.budget_exhausted(seat).map_err(err)? {return Err("Output budget exhausted".into());}
    if store.control(seat).map_err(err)?.is_some() {
        return Err("This agent is stopping. Finish current work; do not start more work.".into());
    }
    let snap = store
        .rig_snapshot(&view.rig)
        .map_err(err)?
        .ok_or("Unknown project")?;
    if snap.desired != Desired::Running || view.lifecycle != SeatLifecycle::Running {
        return Err("The project and agent must be running".into());
    }
    Ok(view.rig)
}

pub async fn call(st: &AppState, seat: &str, tool: &str, args: &Value) -> Result<Value, String> {
    if tool == "team_status" {
        let (rig, _) = crate::launch::compiled_seat(st, seat).map_err(|e| e.message)?;
        let store = st.store.lock().unwrap();
        let snap = store
            .rig_snapshot(&rig.name)
            .map_err(err)?
            .ok_or("Unknown project")?;
        let agents=snap.seats.iter().map(|a| json!({"id":a.id,"name":a.label,"lifecycle":a.lifecycle,"activity":a.activity,"parent":store.parent(&a.id).ok().flatten(),"control":store.control(&a.id).ok().flatten()})).collect::<Vec<_>>();
        return Ok(
            json!({"text":json!({"project":rig.name,"desired":snap.desired,"agents":agents}).to_string()}),
        );
    }
    let request = arg(args, "request_id", 128)?;
    let key = format!("agent-tool:{seat}:{tool}:{request}");
    let lock = {
        st.request_locks
            .lock()
            .unwrap()
            .entry(key.clone())
            .or_default()
            .clone()
    };
    let _guard = lock.lock().await;
    let payload = args.to_string();
    if let Some((old, result)) = st.store.lock().unwrap().tool_receipt(&key).map_err(err)? {
        if old != payload {
            return Err("Use a new request_id for changed arguments".into());
        }
        return serde_json::from_str(&result).map_err(err);
    }
    match tool {
        // Saves its own receipt before sending: an uncertain acknowledgement must never send twice.
        "send_agent_message" => message(st, seat, args, &key).await,
        "spawn_agent" => {
            let result = spawn(st, seat, args, &key).await?;
            st.store.lock().unwrap().save_tool_receipt(&key, &payload, &result.to_string()).map_err(err)?;
            Ok(result)
        }
        _ => Err("Unknown collaboration tool".into()),
    }
}

async fn message(st: &AppState, seat: &str, args: &Value, key: &str) -> Result<Value, String> {
    let (rig, _) = crate::launch::compiled_seat(st, seat).map_err(|e| e.message)?;
    let to = arg(args, "to", 256)?;
    let text = arg(args, "text", 32768)?;
    let target = rig
        .seats
        .iter()
        .find(|a| a.id == to || a.member == to)
        .ok_or("Recipient must be an agent in this project; use team_status for IDs")?;
    if target.id == seat {
        return Err("Choose another agent".into());
    }
    {
        let store = st.store.lock().unwrap();
        if matches!(
            store.control(seat).map_err(err)?,
            Some(crate::store::ControlMode::Hard | crate::store::ControlMode::Finished | crate::store::ControlMode::Budget)
        ) {
            return Err("Sender is stopped".into());
        }
    }
    {
        let store = st.store.lock().unwrap();
        if !store
            .rig_snapshot(&rig.name)
            .map_err(err)?
            .is_some_and(|s| s.desired == Desired::Running)
            || store
                .control(&target.id)
                .map_err(err)?
                .is_some()
        {
            return Err("Recipient or project is stopped/stopping".into());
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    // A status note to an idle agent waits for its next message instead of starting a turn (an idle
    // agent re-reads its whole context to answer it). A note to a busy agent rides its current turn.
    let fyi = args.get("fyi").and_then(Value::as_bool).unwrap_or(false);
    let idle = {
        let view = st.store.lock().unwrap().seat(&target.id).map_err(err)?;
        view.is_some_and(|v| v.lifecycle == SeatLifecycle::Running && v.activity == model::SeatActivity::Idle)
    };
    if fyi && idle {
        let result = json!({"text":format!("Note {id} held for {}: it is idle, so the note reaches it with its next message.",target.id),"message_id":id});
        let mut store = st.store.lock().unwrap();
        store.save_tool_receipt(key, &args.to_string(), &result.to_string()).map_err(err)?;
        store.hold_note(&id, &target.id, seat, text).map_err(err)?;
        let item = store
            .append_local(Actor::Seat(seat.into()), Subject::Seat(target.id.clone()), Provenance::AgentReported, EventBody::MessageQueued { message_id: id.clone(), intent: MessageIntent::Inform, text: text.into() })
            .map_err(err)?;
        st.publish(vec![item]);
        return Ok(result);
    }
    let text = crate::team_context::with_context(st, &target.id, text.into());
    let result = json!({"text":format!("Message {id} recorded for {}. Inspect its delivery/read status before retrying; this request ID never sends twice.",target.id),"message_id":id});
    // Persist the receipt before dispatch: an uncertain acknowledgement must never duplicate work.
    {
        let mut store = st.store.lock().unwrap();
        store.save_tool_receipt(key, &args.to_string(), &result.to_string()).map_err(err)?;
        let item = store
            .append_local(
                Actor::Seat(seat.into()),
                Subject::Seat(target.id.clone()),
                Provenance::AgentReported,
                EventBody::MessageQueued {
                    message_id: id.clone(),
                    intent: MessageIntent::Inform,
                    text: text.clone(),
                },
            )
            .map_err(err)?;
        st.publish(vec![item]);
    }
    let host = crate::launch::seat_host(st, &target.id).map_err(|e| e.message)?;
    if let Err(reason) = st
        .hosts
        .command(
            &host,
            host_proto::HostCommand::Send {
                seat: target.id.clone(),
                message_id: id.clone(),
                intent: MessageIntent::Inform,
                text,
            },
        )
        .await
    {
        st.local_event(
            Actor::System("collaboration".into()),
            Subject::Seat(target.id.clone()),
            Provenance::ProtocolObserved,
            EventBody::MessageFailed {
                message_id: id,
                reason,
            },
        )
        .map_err(err)?;
    }
    Ok(result)
}

async fn spawn(st: &AppState, parent: &str, args: &Value, key: &str) -> Result<Value, String> {
    let rig_name = active(st, parent)?;
    let name = arg(args, "name", 100)?.trim();
    let task = arg(args, "task", 32768)?;
    let runtime = st.store.lock().unwrap().rig_snapshot(&rig_name).ok().flatten()
        .and_then(|s| s.rig.seats.iter().find(|s| s.id == parent).map(|s| s.runtime));
    let held = runtime.and_then(crate::plan_usage::provider_of).and_then(|p| crate::plan_usage::held(st, p));
    if let Some(why) = crate::budget::gate::refusal(&st.store.lock().unwrap(), &rig_name).map_err(err)? {
        return Err(format!("{why}. Finish with the agents you have."));
    }
    let _claim = st
        .try_begin_boot(&rig_name)
        .ok_or("Project is changing; retry with the same request_id")?;
    let (child, existing) = {
        let mut store = st.store.lock().unwrap();
        let existing = store.child_for_request(key).map_err(err)?;
        if store.control(parent).map_err(err)?.is_some() {
            return Err("Parent is stopping".into());
        }
        let mut snap = store
            .rig_snapshot(&rig_name)
            .map_err(err)?
            .ok_or("Project missing")?;
        if snap.desired != Desired::Running {
            return Err("Project is stopped".into());
        }
        if let Some(id) = existing {
            let child =
                snap.rig.seats.iter().find(|s| s.id == id).cloned().ok_or(
                    "The prior child belongs to an earlier project run; use a new request_id",
                )?;
            if child.label.as_deref() != Some(name) || child.role.as_deref() != Some(task) {
                return Err("Use a new request_id for changed arguments".into());
            }
            (child, true)
        } else {
            if let Some(why) = held {
                return Err(format!("{why} Finish with the agents you have, or ask the operator."));
            }
            check_spawn_limits(&store, &rig_name, parent, snap.rig.seats.len())?;
            let child = new_child_seat(&mut snap.rig, parent, &rig_name, name, task)?;
            let source = store
                .rig_source(&rig_name)
                .map_err(err)?
                .ok_or("Project source missing")?;
            store
                .register_child(&snap.rig, &source, parent, key, task)
                .map_err(err)?;
            (child, false)
        }
    };
    if !existing {
        st.local_event(
            Actor::Seat(parent.into()),
            Subject::Rig(rig_name.clone()),
            Provenance::ProtocolObserved,
            EventBody::TeamTopologyChanged {
                parent: parent.into(),
                child: child.id.clone(),
            },
        )
        .map_err(err)?;
        let dir = crate::launch::spec_dir(st, &rig_name);
        if !reconciler::launch_one(st, &child, &dir).await {
            return Ok(
                json!({"text":format!("Child {} is on the board but launch failed. Inspect its status; retrying does not create a replacement.",child.id),"child":child.id}),
            );
        }
        reconciler::wait_settled(st, std::slice::from_ref(&child.id)).await;
        let prompt = format!(
            "You are a managed subagent of {parent}. Your assignment: {task}\nUse team_status to find teammates. Report progress or your result to your parent using send_agent_message. Own only assigned files. Use spawn_agent for tracked children, never an untracked native/background agent. When finished, send your result and end your turn; the operator can stop your idle session."
        );
        let id = reconciler::send_message_as(
            st,
            &child.id,
            prompt,
            MessageIntent::Instruct,
            false,
            Actor::Seat(parent.into()),
        )
        .await
        .map_err(|e| e.message)?;
        return Ok(
            json!({"text":format!("Created {} ({name}); assignment message {id}. Inspect team_status for observed state.",child.id),"child":child.id}),
        );
    }
    Ok(
        json!({"text":format!("Existing child {}. Inspect team_status; no duplicate work was sent.",child.id),"child":child.id}),
    )
}

/// The limits the team set on spawned agents, if any. Sculptured sets none of its own.
fn check_spawn_limits(store: &crate::store::Store, team: &str, parent: &str, seats: usize) -> Result<(), String> {
    let limits = store.draft(team).map_err(err)?.map(|d| d.limits).unwrap_or_default();
    if let Some(max) = limits.max_agents.filter(|&m| seats >= m as usize) {
        return Err(format!("Team limit you set: {max} agents, children included"));
    }
    if let Some(max) = limits.max_child_levels {
        let (mut ancestor, mut levels) = (parent.to_string(), 1);
        while let Some(p) = store.parent(&ancestor).map_err(err)? {
            levels += 1;
            ancestor = p;
        }
        if levels > max {
            return Err(format!("Team limit you set: {max} levels of children"));
        }
    }
    Ok(())
}

/// A copy of the parent's seat as a new child on the board, spawned by the parent and launched after it.
fn new_child_seat(rig: &mut rigspec::CompiledRig, parent: &str, rig_name: &str, name: &str, task: &str) -> Result<rigspec::CompiledSeat, String> {
    let mut child = rig.seats.iter().find(|s| s.id == parent).cloned().ok_or("Parent missing")?;
    child.member = format!("child-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    child.id = format!("{}.{}@{}", child.pod, child.member, rig_name);
    child.label = Some(name.into());
    child.role = Some(task.into());
    rig.seats.push(child.clone());
    rig.edges.push(CompiledEdge { kind: EdgeKind::SpawnedBy, from: child.id.clone(), to: parent.into(), enforced: true });
    rig.launch_layers.push(vec![child.id.clone()]);
    Ok(child)
}
