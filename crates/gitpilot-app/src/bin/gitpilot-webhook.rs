//! Optional standalone GitHub webhook receiver.
//!
//! This is deliberately a separate binary, not a public listener inside the
//! desktop app. Deploy it behind TLS and a network policy, give it a random
//! secret of at least 32 bytes, and point GitHub webhooks at
//! `POST /github/webhook`.

use gitpilot_core::store::Store;
use gitpilot_core::webhooks::WebhookReceiver;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("GitPilot webhook receiver failed: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let bind =
        std::env::var("GITPILOT_WEBHOOK_BIND").unwrap_or_else(|_| "127.0.0.1:8787".to_string());
    let data_dir = std::env::var("GITPILOT_DATA_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(".gitpilot"));
    let secret_path = std::env::var("GITPILOT_WEBHOOK_SECRET_FILE").map_err(|_| {
        "GITPILOT_WEBHOOK_SECRET_FILE must point to a readable secret file (>=32 bytes)".to_string()
    })?;
    let secret = std::fs::read_to_string(&secret_path)
        .map_err(|error| format!("could not read webhook secret file: {error}"))?;
    let secret = secret.trim();
    if secret.len() < 32 {
        return Err("webhook secret must be at least 32 bytes".into());
    }
    tokio::fs::create_dir_all(&data_dir)
        .await
        .map_err(|error| format!("could not create data directory: {error}"))?;
    let store = Store::open(data_dir.join("gitpilot.sqlite"))
        .await
        .map_err(|error| error.to_string())?;
    let receiver = WebhookReceiver::new(store, secret).map_err(|error| error.to_string())?;
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .map_err(|error| format!("could not bind {bind}: {error}"))?;
    println!("GitPilot webhook receiver listening on {bind}");
    println!(
        "Verified deliveries are persisted. The desktop app can enqueue processing with its job dispatcher; this receiver never performs GitHub writes."
    );
    axum::serve(listener, receiver.router())
        .await
        .map_err(|error| error.to_string())
}
