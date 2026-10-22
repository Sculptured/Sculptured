//! `sculptured-background`: starts `sculptured <args>` (next to this program) with no window and waits
//! for it, exiting with its code. Windows' logon task runs this so the runner never shows a console.
#![cfg_attr(windows, windows_subsystem = "windows")]
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let Ok(me) = std::env::current_exe() else {
        return ExitCode::FAILURE;
    };
    let runner = me.with_file_name(if cfg!(windows) {
        "sculptured.exe"
    } else {
        "sculptured"
    });
    let mut cmd = Command::new(runner);
    cmd.args(std::env::args_os().skip(1));
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000); // CREATE_NO_WINDOW
    match cmd.status() {
        Ok(s) => ExitCode::from(s.code().unwrap_or(1).clamp(0, 255) as u8),
        Err(_) => ExitCode::FAILURE,
    }
}
