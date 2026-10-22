//! Reading one request line from the seat-tools socket within its size limit, and what a refused
//! request leaves as evidence.
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt};

/// How much of a refused call's request is kept as evidence.
const EVIDENCE_BYTES: usize = 1024;

/// What came in on a connection.
pub(super) enum Request {
    Line(Vec<u8>),
    /// Longer than the limit: its first bytes, for evidence (the rest was read and discarded).
    TooLarge(Vec<u8>),
    Closed,
}

/// The request line (without its newline), reading at most `max` bytes of it into memory.
pub(super) async fn read_request(
    r: &mut (impl AsyncBufReadExt + Unpin),
    max: usize,
) -> std::io::Result<Request> {
    let mut line = Vec::new();
    (&mut *r)
        .take(max as u64 + 1)
        .read_until(b'\n', &mut line)
        .await?;
    if line.last() == Some(&b'\n') {
        line.pop();
        return Ok(Request::Line(line));
    }
    if line.len() > max {
        line.truncate(EVIDENCE_BYTES);
        discard_line(r).await;
        return Ok(Request::TooLarge(line));
    }
    // The caller closed its end: a request without a final newline still counts.
    Ok(if line.is_empty() {
        Request::Closed
    } else {
        Request::Line(line)
    })
}

/// Reads up to the end of the line (or of the stream) without keeping it, so the caller is not cut off
/// mid-write before it reads the reply.
pub(super) async fn discard_line(mut r: impl AsyncBufReadExt + Unpin) {
    loop {
        let (used, done) = match r.fill_buf().await {
            Ok([]) | Err(_) => return,
            Ok(buf) => match buf.iter().position(|b| *b == b'\n') {
                Some(i) => (i + 1, true),
                None => (buf.len(), false),
            },
        };
        r.consume(used);
        if done {
            return;
        }
    }
}

/// `"name":"value"` from the start of a request too large to parse (a `ToolRequest` starts with its
/// token and tool), so a refused call can still be recorded on its agent.
pub(super) fn field_in_prefix(prefix: &[u8], name: &str) -> Option<String> {
    let text = String::from_utf8_lossy(prefix);
    let key = format!("\"{name}\":\"");
    let start = text.find(&key)? + key.len();
    let len = text[start..].find('"')?;
    Some(text[start..start + len].to_string())
}

/// A refused request as evidence: its first bytes as text.
pub(super) fn evidence(bytes: &[u8]) -> Value {
    let s = String::from_utf8_lossy(&bytes[..bytes.len().min(EVIDENCE_BYTES)]);
    Value::String(if bytes.len() > EVIDENCE_BYTES {
        format!("{s}…")
    } else {
        s.into_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_calls_agent_is_found_from_the_start_of_its_request() {
        let prefix = br#"{"token":"abc123","tool":"report","args":{"summary":"xxxxxxxx"#;
        assert_eq!(field_in_prefix(prefix, "token").as_deref(), Some("abc123"));
        assert_eq!(field_in_prefix(prefix, "tool").as_deref(), Some("report"));
        assert_eq!(field_in_prefix(b"garbage", "token"), None);
    }

    #[tokio::test]
    async fn a_line_over_the_limit_is_cut_to_its_start_and_the_rest_is_read() {
        let mut input: &[u8] = b"0123456789abcdef\nnext";
        let Request::TooLarge(start) = read_request(&mut input, 8).await.unwrap() else {
            panic!("too large")
        };
        assert_eq!(start, b"012345678");
        assert_eq!(input, b"next", "read up to the end of the long line");
    }
}
