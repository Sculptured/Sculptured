//! Shared shell setup for checks and dispatch commands.
use std::io;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::{Child, Command};

pub(crate) fn spawn(cmd: &str, cwd: &str) -> io::Result<Child> {
    shell(cmd)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
}

/// `sh -c` on Unix, `cmd /C` without a console window on Windows.
pub(crate) fn shell(cmd: &str) -> Command {
    #[cfg(windows)]
    {
        let mut command = Command::new("cmd");
        command.arg("/C").raw_arg(cmd).creation_flags(0x0800_0000);
        command
    }
    #[cfg(not(windows))]
    {
        let mut command = Command::new("sh");
        command.arg("-c").arg(cmd);
        command
    }
}

pub(crate) fn timeout_message(limit: Duration) -> String {
    format!("Stopped after {} s without finishing.", limit.as_secs())
}
