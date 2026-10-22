//! The seat-tools socket: accepts calls from agents' shims (and Practice agents), checks each one, and
//! relays it to controld through the supervisor.
use super::request::Request;
use crate::supervisor::Supervisor;
use host_proto::seat_tools::{ToolReply, ToolRequest};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncWriteExt, BufReader};
#[cfg(windows)]
use tokio::net::{TcpListener, TcpStream as Stream, tcp::OwnedReadHalf};
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream as Stream, unix::OwnedReadHalf};
use tokio::sync::Semaphore;

/// Bounds on the seat-tools socket, so one agent (or a stray process of this user) cannot stop the
/// others from reaching Sculptured.
#[derive(Clone, Copy, Debug)]
pub struct SocketLimits {
    /// The longest request line accepted. A longer call is refused in plain words: forwarded, it could
    /// exceed the host link's frame limit and drop the link for every agent.
    pub max_line: usize,
    /// How long a connection may take to send its request line.
    pub read_timeout: Duration,
    /// Connections served at once; more are refused.
    pub max_connections: usize,
    /// Calls one agent (one token) may have in flight at once; more are refused.
    pub max_per_agent: usize,
}

impl Default for SocketLimits {
    fn default() -> Self {
        Self {
            max_line: 1024 * 1024,
            read_timeout: Duration::from_secs(10),
            max_connections: 256,
            max_per_agent: 16,
        }
    }
}

/// The words an agent reads when the socket refuses its call.
pub const TOO_LARGE: &str = "This call is too large for Sculptured (the limit is 1 MiB). Send a shorter question or report.";
pub const TOO_SLOW: &str = "Sculptured did not receive the whole tool call in time.";
pub const TOO_MANY: &str =
    "Sculptured is handling too many tool calls right now. Try again in a moment.";
pub const TOO_MANY_FOR_AGENT: &str =
    "This agent already has too many Sculptured tool calls waiting. Wait for one to finish.";
pub const NOT_TEXT: &str = "Sculptured could not read this tool call: it is not UTF-8 text.";
pub const NO_APPROVE: &str = "The approve tool is not available to this agent.";
const NOT_VALID: &str = "this agent's tool token is not valid (its process is not running)";

/// Serves the seat-tools socket (mode 0600) until the process ends, with the default limits. A call
/// is accepted only with the token of an agent whose process is running.
pub async fn serve_socket(sup: Supervisor, path: PathBuf) -> std::io::Result<()> {
    serve_socket_with(sup, path, SocketLimits::default()).await
}

/// `serve_socket` with explicit limits. Returns only if the socket cannot be created; a failed accept
/// is logged and retried with backoff, never the end of the socket.
pub async fn serve_socket_with(
    sup: Supervisor,
    path: PathBuf,
    limits: SocketLimits,
) -> std::io::Result<()> {
    // A socket left by a hostd that died: nobody listens on it any more (the data dir lock is ours).
    let _ = std::fs::remove_file(&path);
    let listener = Arc::new(listen(&path).await?);
    let accept = move || -> AcceptFuture {
        let l = listener.clone();
        Box::pin(async move { l.accept().await.map(|(s, _)| s) })
    };
    serve_accepted(sup, accept, limits).await;
    Ok(())
}

/// Unix: the socket at `path`, this user's only (0600).
#[cfg(unix)]
async fn listen(path: &std::path::Path) -> std::io::Result<UnixListener> {
    use std::os::unix::fs::PermissionsExt;
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Windows: a loopback port, its address written to `path` for the shim (`host_proto::seat_tools::call`).
/// Every call must carry a live agent's token, so another local process reaching the port gets nowhere.
#[cfg(windows)]
async fn listen(path: &std::path::Path) -> std::io::Result<TcpListener> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    crate::private::write_file(path, listener.local_addr()?.to_string().as_bytes())?;
    Ok(listener)
}

type AcceptFuture = Pin<Box<dyn Future<Output = std::io::Result<Stream>> + Send>>;

/// The accept loop: each connection is served while there is room, refused in plain words otherwise.
async fn serve_accepted(
    sup: Supervisor,
    mut accept: impl FnMut() -> AcceptFuture,
    limits: SocketLimits,
) {
    let connections = Arc::new(Semaphore::new(limits.max_connections));
    let per_agent: PerAgent = Arc::default();
    let mut backoff = Backoff::default();
    loop {
        let stream = match accept().await {
            Ok(s) => s,
            Err(e) => {
                backoff.wait_after(&e).await;
                continue;
            }
        };
        backoff = Backoff::default();
        let (sup, per_agent) = (sup.clone(), per_agent.clone());
        match connections.clone().try_acquire_owned() {
            Ok(permit) => tokio::spawn(async move {
                serve_conn(sup, stream, limits, per_agent).await;
                drop(permit);
            }),
            Err(_) => tokio::spawn(refuse(stream, limits.read_timeout, TOO_MANY)),
        };
    }
}

/// The wait after a failed accept (too many open files, say): from 50 ms, doubling up to 5 s.
struct Backoff(Duration);

impl Default for Backoff {
    fn default() -> Self {
        Backoff(Duration::from_millis(50))
    }
}

impl Backoff {
    async fn wait_after(&mut self, e: &std::io::Error) {
        tracing::warn!(error = %e, retry_in_ms = self.0.as_millis() as u64, "the agent tool socket could not accept a connection");
        tokio::time::sleep(self.0).await;
        self.0 = (self.0 * 2).min(Duration::from_secs(5));
    }
}

/// Reads (and discards) the request, so the caller is not cut off mid-write, then refuses it.
async fn refuse(stream: Stream, read_timeout: Duration, why: &'static str) {
    let (r, mut w) = stream.into_split();
    let _ = tokio::time::timeout(
        read_timeout,
        crate::seat_tools::request::discard_line(BufReader::new(r)),
    )
    .await;
    write_reply(&mut w, &failed(why.into())).await;
}

/// Calls in flight per token.
type PerAgent = Arc<Mutex<HashMap<String, usize>>>;

/// One of an agent's calls in flight; frees its place when dropped.
struct AgentSlot {
    per_agent: PerAgent,
    token: String,
}

impl AgentSlot {
    fn take(per_agent: &PerAgent, token: &str, max: usize) -> Option<AgentSlot> {
        let mut m = per_agent
            .lock()
            .expect("per_agent mutex must not be poisoned");
        let n = m.entry(token.to_string()).or_insert(0);
        if *n >= max {
            return None;
        }
        *n += 1;
        Some(AgentSlot {
            per_agent: per_agent.clone(),
            token: token.to_string(),
        })
    }
}

impl Drop for AgentSlot {
    fn drop(&mut self) {
        let mut m = self
            .per_agent
            .lock()
            .expect("per_agent mutex must not be poisoned");
        if let Some(n) = m.get_mut(&self.token) {
            *n -= 1;
            if *n == 0 {
                m.remove(&self.token);
            }
        }
    }
}

fn failed(e: String) -> ToolReply {
    ToolReply {
        ok: false,
        result: Value::Null,
        error: Some(e),
    }
}

async fn write_reply(w: &mut (impl AsyncWriteExt + Unpin), reply: &ToolReply) {
    let _ = w
        .write_all(
            format!(
                "{}\n",
                serde_json::to_string(reply).expect("reply serializes")
            )
            .as_bytes(),
        )
        .await;
}

async fn serve_conn(sup: Supervisor, stream: Stream, limits: SocketLimits, per_agent: PerAgent) {
    let (r, mut w) = stream.into_split();
    let reply = match read_call(&sup, r, limits).await {
        Ok(Some((req, seat))) => relay(&sup, req, seat, limits, &per_agent).await,
        Ok(None) => return,
        Err(refused) => failed(refused),
    };
    write_reply(&mut w, &reply).await;
}

/// The connection's call and its agent, once it passed every check; Err is the refusal the caller reads.
/// None: the caller went away without sending anything.
async fn read_call(
    sup: &Supervisor,
    r: OwnedReadHalf,
    limits: SocketLimits,
) -> Result<Option<(ToolRequest, String)>, String> {
    let request = tokio::time::timeout(
        limits.read_timeout,
        crate::seat_tools::request::read_request(&mut BufReader::new(r), limits.max_line),
    )
    .await;
    let bytes = match request {
        Err(_) => return Err(TOO_SLOW.into()),
        Ok(Err(_)) | Ok(Ok(Request::Closed)) => return Ok(None),
        Ok(Ok(Request::TooLarge(prefix))) => return Err(too_large(sup, &prefix)),
        Ok(Ok(Request::Line(bytes))) => bytes,
    };
    let line = String::from_utf8(bytes).map_err(|_| NOT_TEXT.to_string())?;
    let req: ToolRequest = serde_json::from_str(&line)
        .map_err(|e| format!("Sculptured could not read this tool call: {e}"))?;
    let (seat, approve) = sup.tool_grant(&req.token).ok_or(NOT_VALID)?;
    // `approve` is only for an agent launched with it (a restricted Claude agent), whatever its shim says.
    if req.tool == "approve" && !approve {
        sup.reject_tool_call(
            &seat,
            &req.tool,
            NO_APPROVE,
            crate::seat_tools::request::evidence(line.as_bytes()),
        );
        return Err(NO_APPROVE.into());
    }
    Ok(Some((req, seat)))
}

/// Records an oversized call on its agent when its start names a live token; the refusal's words.
fn too_large(sup: &Supervisor, prefix: &[u8]) -> String {
    if let Some(seat) = crate::seat_tools::request::field_in_prefix(prefix, "token")
        .and_then(|t| sup.seat_for_token(&t))
    {
        let tool = crate::seat_tools::request::field_in_prefix(prefix, "tool")
            .unwrap_or_else(|| "unknown".into());
        sup.reject_tool_call(
            &seat,
            &tool,
            TOO_LARGE,
            crate::seat_tools::request::evidence(prefix),
        );
    }
    tracing::warn!("an agent tool call over the size limit was refused");
    TOO_LARGE.into()
}

/// Relays a checked call to controld, within the agent's share of calls in flight.
async fn relay(
    sup: &Supervisor,
    req: ToolRequest,
    seat: String,
    limits: SocketLimits,
    per_agent: &PerAgent,
) -> ToolReply {
    let Some(_slot) = AgentSlot::take(per_agent, &req.token, limits.max_per_agent) else {
        return failed(TOO_MANY_FOR_AGENT.into());
    };
    match sup.tool_call(&seat, &req.tool, req.args).await {
        Ok(result) => ToolReply {
            ok: true,
            result,
            error: None,
        },
        Err(e) => failed(e),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::journal::Journal;
    use tokio::io::AsyncBufReadExt;
    use tokio::net::UnixStream;

    /// A failed accept (too many open files, say) is retried; the socket keeps serving.
    #[tokio::test]
    async fn an_accept_error_is_retried_and_serving_goes_on() {
        let sup = Supervisor::new("local".into(), vec![], Journal::open_in_memory(10).unwrap());
        let (ours, theirs) = UnixStream::pair().unwrap();
        let too_many = || Err(std::io::Error::other("too many open files"));
        let script = Arc::new(Mutex::new(std::collections::VecDeque::from([
            too_many(),
            too_many(),
            Ok(theirs),
        ])));
        let accept = move || -> AcceptFuture {
            let next = script.lock().unwrap().pop_front();
            Box::pin(async move {
                if let Some(r) = next {
                    r
                } else {
                    std::future::pending().await
                }
            })
        };
        tokio::spawn(serve_accepted(sup, accept, SocketLimits::default()));
        let (r, mut w) = ours.into_split();
        w.write_all(b"{\"token\":\"nope\",\"tool\":\"report\",\"args\":{}}\n")
            .await
            .unwrap();
        let mut line = String::new();
        let got = tokio::time::timeout(
            Duration::from_secs(5),
            BufReader::new(r).read_line(&mut line),
        )
        .await;
        assert!(
            got.is_ok(),
            "the connection after two failed accepts is served"
        );
        assert!(line.contains("not valid"), "{line}");
    }
}
