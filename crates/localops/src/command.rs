//! Running a command for its whole standard output (a workflow's dispatch command, whose output is
//! the batch): like a check, but stdout is kept whole up to `MAX_STDOUT` and stderr only by its end.
use crate::process;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Child;

/// The most standard output kept. More than this is reported as too large, never cut.
pub const MAX_STDOUT: usize = 4 << 20;
/// How much of the end of stderr is kept.
pub const KEEP_STDERR: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CommandOutput {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    /// The whole standard output, unless `too_large`.
    pub stdout: String,
    /// The end of standard error (or why the command could not run).
    pub stderr: String,
    /// Standard output went past `MAX_STDOUT`; `stdout` is then empty.
    pub too_large: bool,
}

impl CommandOutput {
    fn failed(stderr: String, timed_out: bool) -> Self {
        CommandOutput {
            exit_code: None,
            timed_out,
            stdout: String::new(),
            stderr,
            too_large: false,
        }
    }
}

/// Runs `cmd` in `cwd` (`sh -c`, or `cmd /C` on Windows), stopped after `limit`.
pub async fn capture(cmd: &str, cwd: &str, limit: Duration) -> CommandOutput {
    let mut child = match process::spawn(cmd, cwd) {
        Ok(c) => c,
        Err(e) => {
            return CommandOutput::failed(
                format!("Could not run the command in {cwd}: {e}"),
                false,
            );
        }
    };
    match tokio::time::timeout(limit, collect(&mut child)).await {
        Err(_) => CommandOutput::failed(process::timeout_message(limit), true),
        Ok(output) => output,
    }
}

async fn collect(child: &mut Child) -> CommandOutput {
    let mut stdout = child.stdout.take().expect("dispatch stdout is piped");
    let mut stderr = child.stderr.take().expect("dispatch stderr is piped");
    let ((output, too_large), (errors, _), status) = tokio::join!(
        bounded(&mut stdout, MAX_STDOUT),
        bounded(&mut stderr, 1 << 20),
        child.wait()
    );
    CommandOutput {
        exit_code: status.ok().and_then(|status| status.code()),
        timed_out: false,
        stdout: String::from_utf8_lossy(&output).into_owned(),
        stderr: model::cap::tail(String::from_utf8_lossy(&errors).trim_end(), KEEP_STDERR)
            .to_string(),
        too_large,
    }
}

/// Up to `max` bytes of `r`, and whether there was more; the rest is read and dropped, so the command
/// never blocks on a full pipe.
async fn bounded(r: &mut (impl AsyncRead + Unpin), max: usize) -> (Vec<u8>, bool) {
    let mut kept = Vec::new();
    let _ = (&mut *r).take(max as u64 + 1).read_to_end(&mut kept).await;
    let over = kept.len() > max;
    if over {
        kept.clear();
        let _ = tokio::io::copy(r, &mut tokio::io::sink()).await;
    }
    (kept, over)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stdout_is_kept_whole_and_stderr_apart() {
        let dir = tempfile::tempdir().unwrap();
        let big = 60_000;
        let out = capture(
            &format!("head -c {big} /dev/zero | tr '\\0' x; echo oops >&2; exit 2"),
            dir.path().to_str().unwrap(),
            Duration::from_secs(10),
        )
        .await;
        assert_eq!(
            (
                out.exit_code,
                out.timed_out,
                out.too_large,
                out.stdout.len(),
                out.stderr.as_str()
            ),
            (Some(2), false, false, big, "oops")
        );
    }

    #[tokio::test]
    async fn too_much_output_and_a_hung_command_say_so() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().to_str().unwrap();
        let out = capture(
            &format!("head -c {} /dev/zero", MAX_STDOUT + 1),
            d,
            Duration::from_secs(20),
        )
        .await;
        assert!(out.too_large && out.stdout.is_empty() && out.exit_code == Some(0));
        let hung = capture("sleep 5", d, Duration::from_millis(200)).await;
        assert!(hung.timed_out && hung.exit_code.is_none());
    }
}
