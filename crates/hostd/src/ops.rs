//! Local operations a hosted Sculptured asks this computer to do (folders, git, copies, CLIs, plan
//! usage). The work is `localops`; this runner adds its owner's policy (Task 6) in front of it.
use crate::supervisor::Supervisor;
use host_proto::{FromHost, OpError};

/// Where this computer does the work (home, copies folder, CLI programs, Codex home).
#[derive(Clone, Debug)]
pub struct OpsHost {
    pub context: localops::Context,
}

/// Runs one op and builds the answer for controld.
pub async fn answer(sup: &Supervisor, op_id: String, raw: serde_json::Value) -> FromHost {
    let result = match serde_json::from_value::<localops::LocalOp>(raw) {
        Err(e) => Err(localops::LocalError::bad(
            "unknown_op",
            format!("This computer's runner does not understand that request: {e}"),
        )),
        Ok(op) => sup.run_op(op).await,
    };
    match result {
        Ok(value) => FromHost::OpResult {
            op_id,
            ok: true,
            value,
            error: None,
        },
        Err(e) => FromHost::OpResult {
            op_id,
            ok: false,
            value: serde_json::Value::Null,
            error: Some(OpError {
                status: e.status,
                code: e.code,
                message: e.message,
            }),
        },
    }
}

/// Seats with a check running.
static RUNNING: std::sync::LazyLock<localops::checks::Rerun> =
    std::sync::LazyLock::new(Default::default);

/// Runs `seat`'s check now, or once more after the one already running.
pub(crate) fn queue_check(sup: Supervisor, seat: String, command: String, cwd: String) {
    if !RUNNING.start(&seat) {
        return;
    }
    tokio::spawn(async move {
        loop {
            sup.emit_check(
                &seat,
                model::EventBody::CheckStarted {
                    command: command.clone(),
                },
            );
            let began = std::time::Instant::now();
            let o = localops::checks::execute(&command, &cwd).await;
            sup.emit_check(
                &seat,
                model::EventBody::CheckRan {
                    command: command.clone(),
                    passed: o.passed,
                    exit_code: o.exit_code,
                    timed_out: o.timed_out,
                    output: o.output,
                    duration_ms: began.elapsed().as_millis() as u64,
                },
            );
            if !RUNNING.again(&seat) {
                return;
            }
        }
    });
}
