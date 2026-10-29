//! Durable diagnostic logging with conservative retention and secret redaction.

use crate::error::redact_secrets;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

const MAX_LOG_BYTES: u64 = 8 * 1024 * 1024;
const ROTATED_LOG_BYTES: u64 = 24 * 1024 * 1024;

pub fn install_file_logging(
    data_dir: &Path,
    filter: tracing_subscriber::EnvFilter,
) -> Result<(), std::io::Error> {
    let dir = data_dir.join("logs");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("gitpilot.log");
    tracing::info!(directory = %dir.display(), "durable file logging initialized");
    prune_old_logs(&dir)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let subscriber = tracing_subscriber::registry().with(filter).with(FileLayer {
        file: Mutex::new(file),
    });
    tracing::subscriber::set_global_default(subscriber).map_err(std::io::Error::other)?;
    Ok(())
}

struct FileLayer {
    file: Mutex<std::fs::File>,
}

impl<S> Layer<S> for FileLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = TextVisitor::default();
        event.record(&mut visitor);
        let metadata = event.metadata();
        let line = format!(
            "{} {:<5} {}: {}{}\n",
            chrono::Utc::now().to_rfc3339(),
            metadata.level(),
            metadata.target(),
            visitor.message,
            visitor.fields
        );
        let redacted = redact_secrets(&line);
        if let Ok(mut file) = self.file.lock() {
            let _ = file.write_all(redacted.as_bytes());
            let _ = file.flush();
        }
    }
}

#[derive(Default)]
struct TextVisitor {
    message: String,
    fields: String,
}

impl Visit for TextVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let value = redact_secrets(&format!("{value:?}"));
        if field.name() == "message" {
            self.message = value;
        } else {
            self.fields
                .push_str(&format!(" {}={}", field.name(), value));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        let value = redact_secrets(value);
        if field.name() == "message" {
            self.message = value;
        } else {
            self.fields
                .push_str(&format!(" {}={}", field.name(), value));
        }
    }
}

fn prune_old_logs(dir: &Path) -> Result<(), std::io::Error> {
    let path = dir.join("gitpilot.log");
    if let Ok(metadata) = std::fs::metadata(&path)
        && metadata.len() > MAX_LOG_BYTES
    {
        let rotated = dir.join(format!(
            "gitpilot-{}.log",
            chrono::Utc::now().format("%Y%m%d-%H%M%S")
        ));
        std::fs::rename(&path, rotated)?;
    }
    let mut total = 0u64;
    let mut logs: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|v| v.to_str()) == Some("log"))
        .filter_map(|path| {
            let size = std::fs::metadata(&path).ok()?.len();
            Some((path, size))
        })
        .collect();
    logs.sort_by_key(|(path, _)| {
        std::fs::metadata(path)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    for (path, size) in logs {
        total = total.saturating_add(size);
        if total > ROTATED_LOG_BYTES {
            let _ = std::fs::remove_file(path);
        }
    }
    Ok(())
}
