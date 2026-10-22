use crate::supervisor::Supervisor;
use futures::{Sink, SinkExt, Stream, StreamExt};
use host_proto::{FromHost, HostEvent, PROTOCOL_VERSION, ToHost};
use std::collections::HashSet;
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::{RecvError, TryRecvError};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::{AUTHORIZATION, HeaderValue};
use tokio_tungstenite::tungstenite::{self, Message};

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    #[error("websocket: {0}")]
    Ws(#[from] tungstenite::Error),
    #[error("journal: {0}")]
    Journal(#[from] rusqlite::Error),
    #[error("connection closed")]
    Closed,
    #[error("protocol: {0}")]
    Protocol(String),
}

impl LinkError {
    /// The server refused this runner's key (401): its computer was disconnected from the account.
    #[must_use]
    pub fn disconnected(&self) -> bool {
        matches!(self, LinkError::Ws(tungstenite::Error::Http(r)) if r.status() == tungstenite::http::StatusCode::UNAUTHORIZED)
    }
}

fn frame(m: &FromHost) -> Message {
    Message::text(host_proto::encode(m))
}

type Done = (String, Result<(), String>);

/// One connection's sending side: the socket, the live event channel, how far this connection
/// has sent, and the seat-tool calls it passed to controld.
struct Conn<'a, S> {
    sink: S,
    sup: &'a Supervisor,
    live: Receiver<HostEvent>,
    last_sent: u64,
    /// Calls this connection already passed to controld, so a call is sent once per connection.
    forwarded: HashSet<String>,
    /// Answers to `ToHost::Op`, sent from the serve loop.
    op_tx: mpsc::UnboundedSender<FromHost>,
}

impl<S> Conn<'_, S>
where
    S: Sink<Message, Error = tungstenite::Error> + Unpin,
{
    /// Sends one event if it is newer than what this connection already sent.
    async fn forward(&mut self, event: HostEvent) -> Result<(), LinkError> {
        if event.host_seq > self.last_sent {
            self.last_sent = event.host_seq;
            self.sink.send(frame(&FromHost::Event { event })).await?;
        }
        Ok(())
    }

    /// Re-sends everything after `last_sent` from the journal (after a broadcast lag, or on connect).
    async fn resend(&mut self) -> Result<(), LinkError> {
        for event in self.sup.unacked_after(self.last_sent)? {
            self.forward(event).await?;
        }
        Ok(())
    }

    /// Forwards whatever is already queued on the live channel, without waiting.
    async fn drain_live(&mut self) -> Result<(), LinkError> {
        loop {
            match self.live.try_recv() {
                Ok(event) => self.forward(event).await?,
                Err(TryRecvError::Lagged(_)) => self.resend().await?,
                Err(TryRecvError::Empty | TryRecvError::Closed) => return Ok(()),
            }
        }
    }

    /// Passes one seat-tool call on to controld, after the events the agent emitted before calling (its
    /// turn start), so controld knows which turn the call belongs to. A call already answered, or
    /// already sent on this connection, is skipped.
    async fn forward_call(&mut self, call: FromHost) -> Result<(), LinkError> {
        let FromHost::SeatTool { call_id, .. } = &call else {
            return Ok(());
        };
        if !self.sup.tool_call_pending(call_id) || self.forwarded.contains(call_id) {
            return Ok(());
        }
        let id = call_id.clone();
        self.drain_live().await?;
        self.sink.send(frame(&call)).await?;
        self.forwarded.insert(id);
        Ok(())
    }

    /// Sends every call still waiting (on connect: those a dropped link left unanswered; after a lag:
    /// those the queue lost). controld answers each call id once, so a repeat is harmless.
    async fn forward_pending(&mut self) -> Result<(), LinkError> {
        let matching_sup = self.sup.pending_tool_calls();
        for call in matching_sup {
            self.forward_call(call).await?;
        }
        Ok(())
    }

    /// Forwards what the command emitted (Launching, a launch-failure Attention, ...) before its
    /// result, so controld has ingested the seat's new state by the time it reads the result.
    async fn command_done(&mut self, (command_id, r): Done) -> Result<(), LinkError> {
        self.drain_live().await?;
        let result = FromHost::CommandResult {
            command_id,
            ok: r.is_ok(),
            error: r.err(),
        };
        self.sink.send(frame(&result)).await?;
        Ok(())
    }

    /// A frame from controld. Commands run in spawned tasks; `done` gets their results.
    fn on_frame(
        &mut self,
        msg: Option<Result<Message, tungstenite::Error>>,
        done: &mpsc::UnboundedSender<Done>,
    ) -> Result<(), LinkError> {
        match msg {
            Some(Ok(Message::Text(t))) => match host_proto::decode::<ToHost>(t.as_str()) {
                Ok(m) => self.on_message(m, done)?,
                Err(e) => tracing::warn!(error = %e, "ignoring undecodable frame from controld"),
            },
            Some(Ok(Message::Close(_))) | None => return Err(LinkError::Closed),
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(e.into()),
        }
        Ok(())
    }

    fn on_message(
        &mut self,
        m: ToHost,
        done: &mpsc::UnboundedSender<Done>,
    ) -> Result<(), LinkError> {
        match m {
            ToHost::Command {
                command_id,
                command,
            } => {
                let (sup, done) = (self.sup.clone(), done.clone());
                tokio::spawn(async move {
                    let r = sup.handle(*command).await;
                    let _ = done.send((command_id, r));
                });
            }
            ToHost::Op { op_id, request } => {
                let (sup, tx) = (self.sup.clone(), self.op_tx.clone());
                tokio::spawn(async move {
                    let _ = tx.send(crate::ops::answer(&sup, op_id, request).await);
                });
            }
            ToHost::Ack { upto } => self.sup.ack(upto)?,
            ToHost::Welcome { .. } => {}
            ToHost::SeatToolReply {
                call_id,
                ok,
                result,
                error,
            } => {
                self.forwarded.remove(&call_id);
                self.sup.resolve_tool_call(
                    &call_id,
                    if ok {
                        Ok(result)
                    } else {
                        Err(error.unwrap_or_else(|| "the tool failed".into()))
                    },
                );
            }
        }
        Ok(())
    }

    /// Serves the connection until it ends: frames from controld, finished commands, seat-tool
    /// calls and live events.
    async fn serve<St>(
        &mut self,
        mut stream: St,
        mut tool_calls: Receiver<FromHost>,
        keepalive: Keepalive,
        mut op_rx: mpsc::UnboundedReceiver<FromHost>,
    ) -> Result<(), LinkError>
    where
        St: Stream<Item = Result<Message, tungstenite::Error>> + Unpin,
    {
        let (done_tx, mut done_rx) = mpsc::unbounded_channel::<Done>();
        let mut tick = tokio::time::interval(keepalive.every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut heard = tokio::time::Instant::now();
        loop {
            tokio::select! {
                msg = stream.next() => { heard = tokio::time::Instant::now(); self.on_frame(msg, &done_tx)? },
                _ = tick.tick() => {
                    if heard.elapsed() > keepalive.dead_after {
                        return Err(LinkError::Protocol(format!("no answer from Sculptured for {} s; reconnecting", keepalive.dead_after.as_secs())));
                    }
                    self.sink.send(Message::Ping(Vec::new().into())).await?;
                }
                Some(done) = done_rx.recv() => self.command_done(done).await?,
                Some(answer) = op_rx.recv() => self.sink.send(frame(&answer)).await?,
                call = tool_calls.recv() => match call {
                    Ok(call) => self.forward_call(call).await?,
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!(lost = n, "seat tool calls queued faster than sent; sending the waiting ones again");
                        self.forward_pending().await?;
                    }
                    Err(RecvError::Closed) => return Ok(()),
                },
                ev = self.live.recv() => match ev {
                    Ok(event) => self.forward(event).await?,
                    Err(RecvError::Lagged(_)) => self.resend().await?,
                    Err(RecvError::Closed) => return Ok(()),
                },
            }
        }
    }
}

async fn hello(sup: &Supervisor) -> FromHost {
    FromHost::Hello {
        host_id: sup.host_id().to_string(),
        protocol: PROTOCOL_VERSION,
        drivers: sup.driver_infos(),
        live_seats: sup.live_seats().await,
        launching_seats: sup.launching_seats().await,
        features: sup.features(),
        policy: sup.policy_info(),
    }
}

/// Waits for controld's Welcome; returns its cursor.
async fn await_welcome<St>(stream: &mut St) -> Result<u64, LinkError>
where
    St: Stream<Item = Result<Message, tungstenite::Error>> + Unpin,
{
    loop {
        match stream.next().await {
            Some(Ok(Message::Text(t))) => match host_proto::decode::<ToHost>(t.as_str()) {
                Ok(ToHost::Welcome { resume_after }) => return Ok(resume_after),
                Ok(other) => {
                    return Err(LinkError::Protocol(format!(
                        "expected welcome, got {other:?}"
                    )));
                }
                Err(e) => return Err(LinkError::Protocol(e.to_string())),
            },
            Some(Ok(Message::Close(_))) | None => return Err(LinkError::Closed),
            Some(Ok(_)) => continue,
            Some(Err(e)) => return Err(e.into()),
        }
    }
}

/// One connection to controld. Returns when the connection ends: Err for a
/// disconnect (the caller reconnects), Ok only if the supervisor shuts down.
/// Commands run in spawned tasks so a wedged command (a stuck Send) never stops
/// the loop from reading the next one (a Stop that recovers it); each result is
/// sent only after the events pending when it finished.
/// Pings controld every `every`; gives the link up when nothing at all came back for `dead_after`
/// (a silent drop: sleep, network change, NAT timeout), so the runner reconnects instead of waiting forever.
#[derive(Clone, Copy, Debug)]
pub struct Keepalive {
    pub every: std::time::Duration,
    pub dead_after: std::time::Duration,
}
impl Default for Keepalive {
    fn default() -> Self {
        Keepalive {
            every: std::time::Duration::from_secs(25),
            dead_after: std::time::Duration::from_secs(75),
        }
    }
}

pub async fn run_link(url: &str, token: &str, sup: &Supervisor) -> Result<(), LinkError> {
    run_link_with(url, token, sup, Keepalive::default()).await
}

pub async fn run_link_with(
    url: &str,
    token: &str,
    sup: &Supervisor,
    keepalive: Keepalive,
) -> Result<(), LinkError> {
    let mut req = url.into_client_request()?;
    let auth = HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|e| LinkError::Protocol(e.to_string()))?;
    req.headers_mut().insert(AUTHORIZATION, auth);
    let (ws, _) = tokio_tungstenite::connect_async(req).await?;
    let (mut sink, mut stream) = ws.split();
    // Subscribe before replaying so nothing emitted in between is missed.
    let live = sup.subscribe();
    let tool_calls = sup.subscribe_tool_calls();
    sink.send(frame(&hello(sup).await)).await?;
    let resume_after = await_welcome(&mut stream).await?;
    // controld drops every host_seq at or below its cursor. A journal that is behind
    // it (reset or replaced) would have every new event dropped, so skip ahead.
    if let Some(last) = sup.resume_after(resume_after)? {
        tracing::warn!(
            journal_last_seq = last,
            resume_after,
            "host journal behind controld cursor; fast-forwarding"
        );
    }
    let (op_tx, op_rx) = mpsc::unbounded_channel();
    let mut conn = Conn {
        sink,
        sup,
        live,
        last_sent: resume_after,
        forwarded: HashSet::new(),
        op_tx,
    };
    conn.resend().await?;
    conn.forward_pending().await?;
    conn.serve(stream, tool_calls, keepalive, op_rx).await
}

#[cfg(test)]
mod refusal_tests {
    use super::*;
    #[test]
    fn only_an_unauthorized_answer_means_this_computer_was_disconnected() {
        let resp = |code: u16| {
            tungstenite::http::Response::builder()
                .status(code)
                .body(None)
                .unwrap()
        };
        assert!(LinkError::Ws(tungstenite::Error::Http(Box::new(resp(401)))).disconnected());
        assert!(!LinkError::Ws(tungstenite::Error::Http(Box::new(resp(502)))).disconnected());
        assert!(!LinkError::Closed.disconnected());
    }
}
