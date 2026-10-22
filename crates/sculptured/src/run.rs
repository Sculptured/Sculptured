//! `sculptured run <file>`: a team from one MADL file, run here in the terminal. It starts its own
//! controld and local runner (data under `~/.sculptured/runs/`), imports the file, applies the options,
//! starts the team, and prints a line whenever an agent's state changes. A question is asked in the
//! terminal when there is one; without one the run stops and says what was asked. The run is over when
//! every agent has stopped, or every agent has been idle with nothing waiting for `settle`. Ctrl+C asks
//! every agent to wrap up; a second Ctrl+C stops them now.
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub struct RunArgs {
    pub file: PathBuf,
    pub folder: Option<PathBuf>,
    pub input_text: Option<String>,
    pub tokens: Option<u64>,
    pub minutes: Option<u32>,
    pub data_dir: Option<PathBuf>,
    pub settle: Duration,
    pub claude_program: Option<PathBuf>,
    pub codex_program: Option<PathBuf>,
}

/// How a run ended, as its exit code.
pub const FINISHED: u8 = 0;
pub const AGENT_FAILED: u8 = 1;
pub const BUDGET_STOPPED: u8 = 2;
pub const NEEDS_ANSWER: u8 = 3;
pub const INVALID: u8 = 4;
pub const INTERRUPTED: u8 = 130;

/// Runs the file and returns the exit code; every problem is printed on stderr first.
pub async fn run(a: RunArgs) -> u8 {
    match go(a).await {
        Ok(code) => code,
        Err((code, why)) => {
            eprintln!("sculptured run: {why}");
            code
        }
    }
}

type Fail = (u8, String);

fn invalid(why: impl Into<String>) -> Fail {
    (INVALID, why.into())
}

async fn go(a: RunArgs) -> Result<u8, Fail> {
    let text = std::fs::read_to_string(&a.file)
        .map_err(|e| invalid(format!("cannot read {}: {e}", a.file.display())))?;
    let folder = match &a.folder {
        Some(f) => f
            .canonicalize()
            .map_err(|e| invalid(format!("no folder {}: {e}", f.display())))?,
        None => std::env::current_dir().map_err(|e| invalid(e.to_string()))?,
    };
    let data_dir = match a.data_dir.clone() {
        Some(d) => d,
        None => default_dir(&a.file)?,
    };
    std::fs::create_dir_all(&data_dir)
        .map_err(|e| invalid(format!("cannot make {}: {e}", data_dir.display())))?;
    let (base, supervisor) =
        crate::start_local(&data_dir, a.claude_program.clone(), a.codex_program.clone())
            .await
            .map_err(|e| (AGENT_FAILED, e))?;
    let api = Api {
        base,
        http: crate::connect::http(),
    };
    let result = drive(&api, &text, &folder, &a).await;
    // The run's own runner: stop supervising it, then ask it to stop (it stops its agents first).
    if let Some(s) = supervisor {
        s.abort();
    }
    stop_runner(&data_dir.join("host-local").join("hostd.pid"));
    result
}

fn default_dir(file: &std::path::Path) -> Result<PathBuf, Fail> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| invalid("HOME is not set: give --data-dir"))?;
    let stem = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("team")
        .trim_end_matches(".madl")
        .to_string();
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok(home
        .join(".sculptured")
        .join("runs")
        .join(format!("{stem}-{at}")))
}

fn stop_runner(pid_file: &std::path::Path) {
    let Some(pid) = std::fs::read_to_string(pid_file)
        .ok()
        .and_then(|t| t.trim().parse::<i32>().ok())
    else {
        return;
    };
    #[cfg(unix)]
    if pid > 1 {
        // One process, not a group: the runner stops its own agents on SIGTERM.
        // SAFETY: this is the runner PID returned by our startup path. kill takes no pointers;
        // the standard library has no API for requesting graceful POSIX termination.
        unsafe { libc::kill(pid, libc::SIGTERM) };
    }
    #[cfg(windows)]
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string()])
        .status();
}

struct Api {
    base: String,
    http: reqwest::Client,
}

impl Api {
    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, (u16, Value)> {
        let mut req = self.http.request(method, format!("{}{path}", self.base));
        if let Some(b) = body {
            req = req.json(&b);
        }
        let res = req
            .send()
            .await
            .map_err(|e| (0, json!({ "error": { "message": e.to_string() } })))?;
        let status = res.status().as_u16();
        let v: Value = res.json().await.unwrap_or(Value::Null);
        if (200..300).contains(&status) {
            Ok(v)
        } else {
            Err((status, v))
        }
    }
    async fn get(&self, path: &str) -> Option<Value> {
        self.call(reqwest::Method::GET, path, None).await.ok()
    }
    async fn post(&self, path: &str, body: Value) -> Result<Value, (u16, Value)> {
        self.call(reqwest::Method::POST, path, Some(body)).await
    }
}

fn rid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// The error's message, with each thing to fix it lists.
fn message(v: &Value) -> String {
    let issues: Vec<String> = v["error"]["issues"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| i["message"].as_str())
        .map(|m| format!("\n  - {m}"))
        .collect();
    format!(
        "{}{}",
        v["error"]["message"]
            .as_str()
            .unwrap_or("the request failed"),
        issues.concat()
    )
}

async fn drive(api: &Api, text: &str, folder: &std::path::Path, a: &RunArgs) -> Result<u8, Fail> {
    let imported = api
        .post(
            "/api/teams/import-madl",
            json!({ "request_id": rid(), "text": text }),
        )
        .await
        .map_err(|(_, v)| {
            let found: Vec<String> = v["error"]["diagnostics"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|d| {
                    format!(
                        "  {}: {}",
                        d["path"].as_str().unwrap_or(""),
                        d["message"].as_str().unwrap_or("")
                    )
                })
                .collect();
            invalid(format!("{}\n{}", message(&v), found.join("\n")))
        })?;
    let flattened_items = imported["dropped"].as_array().into_iter().flatten();
    for d in flattened_items {
        eprintln!(
            "not kept: {} {}",
            d["path"].as_str().unwrap_or(""),
            d["message"].as_str().unwrap_or("")
        );
    }
    let mut draft = imported["team"]["draft"].clone();
    let id = draft["id"].as_str().unwrap_or_default().to_string();
    let names: BTreeMap<String, String> = draft["agents"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|g| {
            (
                g["id"].as_str().unwrap_or("").to_string(),
                g["name"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    apply_options(&mut draft, folder, a)?;
    let saved = api
        .call(
            reqwest::Method::PUT,
            &format!("/api/teams/{id}"),
            Some(json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft })),
        )
        .await;
    saved
        .map_err(|(_, v)| invalid(format!("the options could not be applied: {}", message(&v))))?;
    api.post(
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": rid() }),
    )
    .await
    .map_err(|(_, v)| {
        (
            AGENT_FAILED,
            format!("the team could not start: {}", message(&v)),
        )
    })?;
    println!(
        "Started {} ({} agents) in {}",
        draft["name"].as_str().unwrap_or(&id),
        names.len(),
        folder.display()
    );
    follow(api, &id, &names, a.settle).await
}

/// `--folder` for every agent without its own, `--input-text` as the Input, and lower budgets.
fn apply_options(draft: &mut Value, folder: &std::path::Path, a: &RunArgs) -> Result<(), Fail> {
    draft["folder"] = json!(folder.display().to_string());
    if let Some(t) = &a.input_text {
        let to = draft["input"]["to"].as_array().cloned().unwrap_or_else(|| {
            draft["agents"]
                .as_array()
                .and_then(|g| g.first())
                .map(|g| vec![g["id"].clone()])
                .unwrap_or_default()
        });
        draft["input"] = json!({ "text": t, "x": -260, "y": 40, "to": to });
    }
    if let Some(n) = a.tokens {
        lower(draft, "tokens", n, "tokens")?;
    }
    if let Some(n) = a.minutes {
        lower(draft, "minutes", u64::from(n), "minutes")?;
    }
    Ok(())
}

fn lower(draft: &mut Value, key: &str, n: u64, unit: &str) -> Result<(), Fail> {
    if let Some(hard) = draft["budget"][key]["hard"].as_u64().filter(|h| n > *h) {
        return Err(invalid(format!(
            "--{key} {n} is above the file's {hard} {unit}: an option can only lower a budget"
        )));
    }
    // Agents wrap up at the soft value: the file's if it is lower, otherwise 85% (time to finish before
    // a hard limit such as a job's end).
    let soft = draft["budget"][key]["soft"]
        .as_u64()
        .filter(|s| *s < n)
        .unwrap_or(n * 85 / 100);
    draft["budget"][key] = json!({ "soft": soft, "hard": n });
    Ok(())
}

/// What the run looked like a moment ago, to print only what changed.
#[derive(Default)]
struct Seen {
    states: BTreeMap<String, String>,
    asked: BTreeSet<String>,
    /// Each seat's last event read; a turn that ended (and how) comes from the events, not a poll.
    cursors: BTreeMap<String, i64>,
    worked: bool,
    turn_failed: bool,
    quiet_since: Option<Instant>,
}

async fn follow(
    api: &Api,
    id: &str,
    names: &BTreeMap<String, String>,
    settle: Duration,
) -> Result<u8, Fail> {
    let start = Instant::now();
    let mut seen = Seen::default();
    let mut stops = 0u8;
    let mut ctrl_c = Box::pin(tokio::signal::ctrl_c());
    let mut ended = ended();
    loop {
        tokio::select! {
            // Asked to end (the terminal closed, `kill`, a job's time limit): stop now, then clean up.
            // `Some` only: where there are no such signals the stream is closed, and this branch is off.
            Some(()) = ended.recv() => {
                println!("Asked to end: stopping every agent now");
                let _ = api.post(&format!("/api/teams/{id}/stop-now"), json!({ "request_id": rid() })).await;
                wait_stopped(api, id, Duration::from_secs(10)).await;
                return Ok(INTERRUPTED);
            }
            _ = &mut ctrl_c => {
                stops += 1;
                ctrl_c = Box::pin(tokio::signal::ctrl_c());
                if stops == 1 {
                    println!("Wrapping up; press Ctrl+C again to stop now");
                    let _ = api.post(&format!("/api/teams/{id}/stop"), json!({ "request_id": rid() })).await;
                } else {
                    println!("Stopping every agent now");
                    let _ = api.post(&format!("/api/teams/{id}/stop-now"), json!({ "request_id": rid() })).await;
                    wait_stopped(api, id, Duration::from_secs(10)).await;
                    return Ok(INTERRUPTED);
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(500)) => {}
        }
        let Some(rig) = api.get(&format!("/api/rigs/{id}")).await else {
            continue;
        };
        let seats = rig["seats"].as_array().cloned().unwrap_or_default();
        report_seats(&seats, names, start, &mut seen);
        read_turns(api, &seats, names, start, &mut seen).await;
        if let Some(code) = ask_questions(api, id, names, start, &mut seen).await {
            return Ok(code);
        }
        if stops == 1
            && seats
                .iter()
                .all(|s| matches!(s["lifecycle"].as_str(), Some("stopped" | "failed")))
        {
            return Ok(INTERRUPTED);
        }
        if let Some(code) = over(api, id, &seats, settle, &mut seen).await {
            return Ok(code);
        }
    }
}

/// SIGTERM and SIGHUP on Unix, as one stream; nothing elsewhere (Ctrl+C covers the console there).
fn ended() -> tokio::sync::mpsc::Receiver<()> {
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        for kind in [SignalKind::terminate(), SignalKind::hangup()] {
            let tx = tx.clone();
            if let Ok(mut s) = signal(kind) {
                tokio::spawn(async move {
                    while s.recv().await.is_some() {
                        let _ = tx.send(()).await;
                    }
                });
            }
        }
    }
    drop(tx);
    rx
}

fn clock(start: Instant) -> String {
    let s = start.elapsed().as_secs();
    format!("[{:02}:{:02}]", s / 60, s % 60)
}

fn seat_name<'a>(names: &'a BTreeMap<String, String>, seat: &'a Value) -> &'a str {
    let member = seat["member"].as_str().unwrap_or("");
    names
        .get(member)
        .map(String::as_str)
        .filter(|n| !n.is_empty())
        .unwrap_or(member)
}

/// One line per change: working, idle (finished its turn), needs attention, failed or stopped.
fn report_seats(
    seats: &[Value],
    names: &BTreeMap<String, String>,
    start: Instant,
    seen: &mut Seen,
) {
    for s in seats {
        let (lifecycle, activity) = (
            s["lifecycle"].as_str().unwrap_or(""),
            s["activity"].as_str().unwrap_or(""),
        );
        let state = match (lifecycle, activity) {
            ("running", "working") => "working".to_string(),
            ("running", "waiting-input") => "waiting for an answer".to_string(),
            ("running", "idle") => "finished its turn".to_string(),
            ("attention", _) => format!(
                "needs attention: {}",
                s["lifecycle_reason"].as_str().unwrap_or("")
            ),
            ("failed", _) => format!("failed: {}", s["lifecycle_reason"].as_str().unwrap_or("")),
            ("stopped", _) => "stopped".to_string(),
            (other, _) => other.to_string(),
        };
        if activity == "working" {
            seen.worked = true;
        }
        let key = s["id"].as_str().unwrap_or("").to_string();
        if seen.states.get(&key) != Some(&state) {
            println!("{} {}: {state}", clock(start), seat_name(names, s));
            seen.states.insert(key, state);
        }
    }
}

/// Turns that ended since the last look: any counts as work; a failed one is said and remembered.
async fn read_turns(
    api: &Api,
    seats: &[Value],
    names: &BTreeMap<String, String>,
    start: Instant,
    seen: &mut Seen,
) {
    for s in seats {
        let Some(id) = s["id"].as_str() else { continue };
        let after = seen.cursors.get(id).copied().unwrap_or(0);
        let Some(events) = api
            .get(&format!("/api/seats/{id}/events?after={after}"))
            .await
        else {
            continue;
        };
        let flattened_events = events.as_array().into_iter().flatten();
        for e in flattened_events {
            if let Some(seq) = e["seq"].as_i64() {
                seen.cursors.insert(id.to_string(), seq);
            }
            if e["body"]["type"] == "turn-ended" {
                seen.worked = true;
                if e["body"]["status"] == "failed" {
                    seen.turn_failed = true;
                    println!("{} {}: its turn failed", clock(start), seat_name(names, s));
                }
            }
        }
    }
}

/// Open questions: asked in the terminal (an answer goes back to the agent), or, with no one at the
/// terminal, printed with the run stopped.
async fn ask_questions(
    api: &Api,
    id: &str,
    names: &BTreeMap<String, String>,
    start: Instant,
    seen: &mut Seen,
) -> Option<u8> {
    let questions = api.get(&format!("/api/teams/{id}/questions")).await?;
    let matching_questions = questions
        .as_array()
        .into_iter()
        .flatten()
        .filter(|q| q["state"] == "open");
    for q in matching_questions {
        let qid = q["id"].as_str().unwrap_or("").to_string();
        if !seen.asked.insert(qid.clone()) {
            continue;
        }
        let who = names
            .get(
                q["seat"]
                    .as_str()
                    .unwrap_or("")
                    .split('@')
                    .next()
                    .unwrap_or("")
                    .rsplit('.')
                    .next()
                    .unwrap_or(""),
            )
            .cloned()
            .unwrap_or_default();
        let options: Vec<&str> = q["prompt"]["options"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|o| o["label"].as_str())
            .collect();
        println!(
            "{} {who} asks: {}",
            clock(start),
            q["prompt"]["text"].as_str().unwrap_or("")
        );
        let indexed_options = options.iter().enumerate();
        for (i, o) in indexed_options {
            println!("    {}. {o}", i + 1);
        }
        if !std::io::stdin().is_terminal() {
            println!("No one is at the terminal to answer, so the run stops here.");
            let _ = api
                .post(
                    &format!("/api/teams/{id}/stop-now"),
                    json!({ "request_id": rid() }),
                )
                .await;
            wait_stopped(api, id, Duration::from_secs(10)).await;
            return Some(NEEDS_ANSWER);
        }
        let line = tokio::task::spawn_blocking(|| {
            print!("Answer: ");
            let _ = std::io::Write::flush(&mut std::io::stdout());
            let mut l = String::new();
            std::io::stdin()
                .read_line(&mut l)
                .map(|_| l.trim().to_string())
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default();
        let text = line
            .parse::<usize>()
            .ok()
            .and_then(|n| options.get(n.wrapping_sub(1)).map(|o| o.to_string()))
            .unwrap_or(line);
        if let Err((_, v)) = api
            .post(
                &format!("/api/questions/{qid}/answer"),
                json!({ "request_id": rid(), "answer": { "kind": "text", "text": text } }),
            )
            .await
        {
            println!("The answer was not taken: {}", message(&v));
        }
    }
    None
}

/// Whether the run is over, and how: every agent stopped or failed, or every agent idle with nothing
/// waiting for `settle` after some work (then the team is stopped).
async fn over(
    api: &Api,
    id: &str,
    seats: &[Value],
    settle: Duration,
    seen: &mut Seen,
) -> Option<u8> {
    if seats.is_empty() {
        return None;
    }
    let budget_stopped = || async {
        api.get(&format!("/api/teams/{id}/budget"))
            .await
            .is_some_and(|b| b.to_string().contains("\"state\":\"stopped\""))
    };
    let failed = seen.turn_failed || seats.iter().any(|s| s["lifecycle"] == "failed");
    if seats
        .iter()
        .all(|s| matches!(s["lifecycle"].as_str(), Some("stopped" | "failed")))
    {
        return Some(if budget_stopped().await {
            BUDGET_STOPPED
        } else if failed {
            AGENT_FAILED
        } else {
            FINISHED
        });
    }
    let quiet = seen.worked
        && seats.iter().all(|s| {
            (s["lifecycle"] == "running" && s["activity"] == "idle")
                || matches!(s["lifecycle"].as_str(), Some("stopped" | "failed"))
        })
        && seats
            .iter()
            .all(|s| s["waiting_for"].as_array().is_none_or(|w| w.is_empty()));
    if !quiet {
        seen.quiet_since = None;
        return None;
    }
    let since = *seen.quiet_since.get_or_insert_with(Instant::now);
    if since.elapsed() < settle {
        return None;
    }
    println!("Every agent has finished; stopping the team.");
    let _ = api
        .post(
            &format!("/api/teams/{id}/stop"),
            json!({ "request_id": rid() }),
        )
        .await;
    wait_stopped(api, id, Duration::from_secs(30)).await;
    Some(if failed { AGENT_FAILED } else { FINISHED })
}

async fn wait_stopped(api: &Api, id: &str, within: Duration) {
    let until = Instant::now() + within;
    while Instant::now() < until {
        let stopped = api.get(&format!("/api/rigs/{id}")).await.is_some_and(|r| {
            r["seats"].as_array().is_some_and(|s| {
                s.iter()
                    .all(|s| matches!(s["lifecycle"].as_str(), Some("stopped" | "failed")))
            })
        });
        if stopped {
            return;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lowered_limit_keeps_room_to_wrap_up() {
        let mut d = json!({ "budget": {} });
        lower(&mut d, "minutes", 100, "minutes").unwrap();
        assert_eq!(
            d["budget"]["minutes"],
            json!({ "soft": 85, "hard": 100 }),
            "agents wrap up before a job's time limit"
        );
        let mut d = json!({ "budget": { "minutes": { "soft": 30, "hard": 120 } } });
        lower(&mut d, "minutes", 60, "minutes").unwrap();
        assert_eq!(
            d["budget"]["minutes"],
            json!({ "soft": 30, "hard": 60 }),
            "the file's own lower soft value stays"
        );
        let mut d = json!({ "budget": { "tokens": { "hard": 10 } } });
        assert!(lower(&mut d, "tokens", 20, "tokens").is_err());
    }
}
