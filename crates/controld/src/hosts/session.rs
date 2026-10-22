//! One host's connection: Hello, Welcome{resume_after}, then events in (ingested and acked one by
//! one), commands out, and seat-tool calls relayed.
use super::registry::ConnectionDetails;
use super::{CallStart, HELLO_TIMEOUT, Pending};
use crate::error::ApiError;
use crate::state::AppState;
use crate::store::LOCAL_EVENT_HOST;
use axum::extract::State;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade, close_code};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use host_proto::{DriverInfo, FromHost, HostEvent, PROTOCOL_VERSION, ToHost};
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

type Sink = SplitSink<WebSocket, Message>;
type Frames = SplitStream<WebSocket>;

pub async fn host_ws(
    State(st): State<AppState>,
    headers: HeaderMap,
    uri: axum::http::Uri,
    ws: WebSocketUpgrade,
) -> Response {
    let expected = format!("Bearer {}", st.host_token);
    if headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        != Some(expected.as_str())
    {
        return ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "a valid host token is required",
        )
        .into_response();
    }
    let hosted = matches!(st.auth, crate::auth::AuthPolicy::Gateway { .. });
    let host = match expected_host(hosted, uri.query()) {
        Ok(h) => h,
        Err(e) => return e.into_response(),
    };
    ws.on_upgrade(move |socket| host_session(st, socket, host))
}

/// Hosted: the computer the gateway authenticated (`/host?device=<id>`, inside the signed request).
/// A connected computer's Hello must name exactly that id. None in local mode.
fn expected_host(hosted: bool, query: Option<&str>) -> Result<Option<String>, ApiError> {
    let device = query
        .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("device=")))
        .filter(|d| !d.is_empty())
        .map(str::to_string);
    match (hosted, device) {
        (true, None) => Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "the gateway must name the connecting computer",
        )),
        (true, d) => Ok(d),
        (false, _) => Ok(None),
    }
}

/// What a host said in its Hello.
struct Hello {
    host_id: String,
    protocol: u32,
    drivers: Vec<DriverInfo>,
    live_seats: Vec<String>,
    launching_seats: Vec<String>,
    features: Vec<String>,
    policy: Option<host_proto::HostPolicyInfo>,
}

/// The host's Hello; None when the connection ends first or no Hello comes in time.
async fn await_hello(stream: &mut Frames) -> Option<Hello> {
    let hello = tokio::time::timeout(HELLO_TIMEOUT, async {
        loop {
            match stream.next().await {
                Some(Ok(Message::Text(t))) => match host_proto::decode::<FromHost>(t.as_str()) {
                    Ok(FromHost::Hello {
                        host_id,
                        protocol,
                        drivers,
                        live_seats,
                        launching_seats,
                        features,
                        policy,
                    }) => {
                        break Some(Hello {
                            host_id,
                            protocol,
                            drivers,
                            live_seats,
                            launching_seats,
                            features,
                            policy,
                        });
                    }
                    Ok(other) => tracing::warn!(?other, "ignoring frame before hello"),
                    Err(e) => tracing::warn!(error = %e, "ignoring undecodable frame before hello"),
                },
                Some(Ok(_)) => continue,
                _ => break None,
            }
        }
    })
    .await;
    hello.unwrap_or_else(|_| {
        tracing::warn!(
            "host sent no hello within {}s; closing",
            HELLO_TIMEOUT.as_secs()
        );
        None
    })
}

/// Why this Hello is refused, as the close frame to send: an unsupported protocol, or the
/// host id reserved for controld's own events (the store refuses to ingest for it).
fn refusal(hello: &Hello, expected: Option<&str>) -> Option<CloseFrame> {
    if let Some(e) = expected.filter(|e| *e != hello.host_id) {
        tracing::error!(host = %hello.host_id, expected = %e, "a computer named another host id; closing");
        return Some(CloseFrame {
            code: close_code::POLICY,
            reason: "this computer can only connect as itself".into(),
        });
    }
    if hello.protocol != PROTOCOL_VERSION {
        let reason = format!(
            "protocol {} is not supported; controld speaks {PROTOCOL_VERSION}",
            hello.protocol
        );
        tracing::error!(host = %hello.host_id, %reason, "closing host connection");
        return Some(CloseFrame {
            code: close_code::PROTOCOL,
            reason: reason.into(),
        });
    }
    if hello.host_id == LOCAL_EVENT_HOST {
        tracing::error!(host = %hello.host_id, "protocol violation: host id is reserved; closing host connection");
        return Some(CloseFrame {
            code: close_code::POLICY,
            reason: "host id is reserved".into(),
        });
    }
    None
}

/// Sends Welcome with the host's cursor; None when the cursor cannot be read or the send fails.
async fn welcome(st: &AppState, host_id: &str, sink: &mut Sink) -> Option<u64> {
    let resume_after = match st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .host_cursor(host_id)
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(%e, "cannot read host cursor");
            return None;
        }
    };
    sink.send(Message::text(host_proto::encode(&ToHost::Welcome {
        resume_after,
    })))
    .await
    .ok()?;
    Some(resume_after)
}

async fn host_session(st: AppState, socket: WebSocket, expected: Option<String>) {
    let (mut sink, mut stream) = socket.split();
    let Some(hello) = await_hello(&mut stream).await else {
        return;
    };
    if let Some(close) = refusal(&hello, expected.as_deref()) {
        let _ = sink.send(Message::Close(Some(close))).await;
        return;
    }
    let host_id = hello.host_id.clone();
    let Some(resume_after) = welcome(&st, &host_id, &mut sink).await else {
        return;
    };
    let (tx, rx) = mpsc::unbounded_channel::<ToHost>();
    let pending: Pending = Arc::default();
    let (evict, evicted) = watch::channel(false);
    let session_lock = st.hosts.session_lock(&host_id);
    let conn_id = {
        let _serialized = session_lock.lock().await;
        register(
            &st,
            hello,
            resume_after,
            Registration {
                tx,
                pending: pending.clone(),
                evict,
            },
        )
    };
    Conn {
        st: &st,
        host_id: &host_id,
        pending,
        sink,
    }
    .serve(stream, rx, evicted)
    .await;
    let _serialized = session_lock.lock().await;
    if st.hosts.remove_if(&host_id, conn_id) {
        tracing::warn!(host = %host_id, "host disconnected; its seats are now stale");
        crate::hosts::mark_stale(&st, &host_id);
    }
}

/// What the registry keeps for a connection: its command channel, the commands waiting for a
/// reply, and how a newer connection evicts it.
struct Registration {
    tx: mpsc::UnboundedSender<ToHost>,
    pending: Pending,
    evict: watch::Sender<bool>,
}

/// Registers the connection, evicting an older one with the same host id, and re-attaches the
/// seats the Hello names. Runs under the host's session lock. Returns the connection id.
fn register(st: &AppState, hello: Hello, resume_after: u64, r: Registration) -> u64 {
    let host_id = hello.host_id;
    let (conn_id, evicted) = st.hosts.insert(
        host_id.clone(),
        ConnectionDetails {
            tx: r.tx,
            pending: r.pending,
            drivers: hello.drivers,
            features: hello.features,
            policy: hello.policy,
            evict: r.evict,
        },
    );
    if evicted {
        tracing::warn!(host = %host_id, "a new connection with the same host id evicted the existing one");
    }
    tracing::info!(host = %host_id, resume_after, "host connected");
    crate::restore::on_hello(st, &host_id, &hello.live_seats, &hello.launching_seats);
    conn_id
}

/// A registered connection while it serves.
struct Conn<'a> {
    st: &'a AppState,
    host_id: &'a str,
    pending: Pending,
    sink: Sink,
}

impl Conn<'_> {
    /// Until the host goes, the link breaks, or a newer connection for this host id replaces this one.
    async fn serve(
        mut self,
        mut stream: Frames,
        mut rx: mpsc::UnboundedReceiver<ToHost>,
        mut evicted: watch::Receiver<bool>,
    ) {
        loop {
            let go_on = tokio::select! {
                _ = evicted.changed() => false,
                out = rx.recv() => match out {
                    Some(m) => self.sink.send(Message::text(host_proto::encode(&m))).await.is_ok(),
                    None => false,
                },
                msg = stream.next() => match msg {
                    Some(Ok(Message::Text(t))) => self.on_frame(t.as_str()).await,
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => false,
                    Some(Ok(_)) => true,
                },
            };
            if !go_on {
                break;
            }
        }
    }

    /// One frame from the host; false when the connection must end.
    async fn on_frame(&mut self, text: &str) -> bool {
        let host_id = self.host_id;
        match host_proto::decode::<FromHost>(text) {
            Ok(FromHost::Event { event }) => return self.ingest(event).await,
            Ok(FromHost::CommandResult {
                command_id,
                ok,
                error,
            }) => {
                if let Some(done) = self
                    .pending
                    .lock()
                    .expect("pending mutex must not be poisoned")
                    .remove(&command_id)
                {
                    let _ = done.send(if ok {
                        Ok(())
                    } else {
                        Err(error.unwrap_or_else(|| "command failed".into()))
                    });
                }
            }
            Ok(FromHost::SeatTool {
                call_id,
                seat,
                tool,
                args,
            }) => self.on_call(call_id, seat, tool, args),
            Ok(FromHost::OpResult {
                op_id,
                ok,
                value,
                error,
            }) => {
                let result = if ok {
                    Ok(value)
                } else {
                    Err(error.unwrap_or(host_proto::OpError {
                        status: 500,
                        code: "op_failed".into(),
                        message: "The computer could not do that".into(),
                    }))
                };
                self.st.hosts.op_answered(host_id, &op_id, result);
            }
            Ok(FromHost::Hello { .. }) => {
                tracing::warn!(host = %host_id, "ignoring repeated hello")
            }
            Err(e) => {
                tracing::warn!(host = %host_id, error = %e, "ignoring undecodable host frame")
            }
        }
        true
    }

    /// A runner sends a call again on each new link until it is answered: act on each id once.
    fn on_call(&self, call_id: String, seat: String, tool: String, args: serde_json::Value) {
        let host_id = self.host_id;
        match self.st.hosts.begin_call(host_id, &call_id) {
            CallStart::New => {
                let (reply_tx, mut replies) = mpsc::unbounded_channel();
                tokio::spawn(crate::seat_tools::handle_call(
                    self.st.clone(),
                    reply_tx,
                    call_id.clone(),
                    seat,
                    tool,
                    args,
                ));
                // The reply goes on the host's link of the moment, which may be a newer one.
                let (hosts, host) = (self.st.hosts.clone(), host_id.to_string());
                tokio::spawn(async move {
                    if let Some(reply) = replies.recv().await {
                        hosts.finish_call(&host, &call_id, reply);
                    }
                });
            }
            CallStart::Running => {
                tracing::debug!(host = %host_id, %call_id, "call sent again while it is being answered")
            }
            CallStart::Answered(reply) => self.st.hosts.send_now(host_id, *reply),
        }
    }

    /// Ingests one event and acks it; false when either fails.
    async fn ingest(&mut self, event: HostEvent) -> bool {
        let upto = event.host_seq;
        // Publish under the store lock so the hub stays in seq order. The
        // guard is dropped before the next `.await`.
        let ingested = {
            let mut store = self
                .st
                .store
                .lock()
                .expect("store mutex must not be poisoned");
            store
                .ingest(self.host_id, event)
                .map(|items| self.st.publish(items))
        };
        if let Err(e) = ingested {
            tracing::error!(%e, "ingest failed; dropping host connection");
            return false;
        }
        self.sink
            .send(Message::text(host_proto::encode(&ToHost::Ack { upto })))
            .await
            .is_ok()
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    fn hello(id: &str) -> Hello {
        Hello {
            host_id: id.into(),
            protocol: PROTOCOL_VERSION,
            drivers: vec![],
            live_seats: vec![],
            launching_seats: vec![],
            features: vec![],
            policy: None,
        }
    }

    #[test]
    fn a_connected_computer_can_only_be_itself() {
        assert!(refusal(&hello("dev-a"), Some("dev-a")).is_none());
        assert!(
            refusal(&hello("dev-b"), Some("dev-a")).is_some(),
            "claims another computer"
        );
        assert!(
            refusal(&hello("local"), Some("dev-a")).is_some(),
            "claims this machine's runner"
        );
        assert!(
            refusal(&hello("local"), None).is_none(),
            "local mode: the local runner names itself"
        );
    }

    #[test]
    fn hosted_mode_requires_the_gateway_to_name_the_computer() {
        assert_eq!(expected_host(false, None).unwrap(), None);
        assert_eq!(
            expected_host(true, Some("device=dev-a")).unwrap(),
            Some("dev-a".to_string())
        );
        assert!(expected_host(true, None).is_err());
        assert!(expected_host(true, Some("other=1")).is_err());
    }
}
