//! `fake-community --port <u16>`: the stub Sculptured server (device pairing and Community) for
//! e2e runs. Prints `listening on http://127.0.0.1:<port>` and serves until it is stopped.
#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let port = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(14411);
    let stub = match fixtures::community_stub::spawn_on(port).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("fake-community: cannot listen on 127.0.0.1:{port}: {e}");
            std::process::exit(1);
        }
    };
    println!("listening on {}", stub.origin);
    std::future::pending::<()>().await;
}
