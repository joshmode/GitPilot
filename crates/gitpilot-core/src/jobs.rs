//! Persisted, bounded job execution with cancellation and restart recovery.

use crate::error::{GitPilotError, Result};
use crate::store::Store;
use serde_json::Value;
use sqlx::Row;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

#[derive(Clone)]
pub struct JobRunner {
    store: Store,
    concurrency: Arc<tokio::sync::Semaphore>,
}

pub trait JobHandler: Send + Sync {
    fn kind(&self) -> &'static str;
    fn handle(
        &self,
        payload: Value,
        cancel: CancellationToken,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;
}

/// Claim a queued job only when it matches a registered handler. This avoids a
/// non-matching job blocking the shared queue for handlers that are not
/// registered in a given runtime.
async fn claim_registered(
    store: &Store,
    kinds: &[&str],
) -> Result<Option<(String, String, Value, i64)>> {
    let placeholders = std::iter::repeat_n("?", kinds.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT id, kind, payload, attempts FROM jobs
         WHERE state = 'queued' AND run_after <= ? AND kind IN ({placeholders})
         ORDER BY created_at LIMIT 1"
    );
    let mut tx = store.pool().begin().await?;
    let mut query = sqlx::query(&sql).bind(chrono::Utc::now().to_rfc3339());
    for kind in kinds {
        query = query.bind(kind);
    }
    let row = query.fetch_optional(&mut *tx).await?;
    let Some(row) = row else {
        tx.commit().await?;
        return Ok(None);
    };
    let id: String = row.get("id");
    let attempts: i64 = row.get::<i64, _>("attempts") + 1;
    sqlx::query("UPDATE jobs SET state='running', attempts=?, updated_at=? WHERE id=?")
        .bind(attempts)
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Some((
        id,
        row.get("kind"),
        serde_json::from_str(&row.get::<String, _>("payload")).unwrap_or_default(),
        attempts,
    )))
}

impl JobRunner {
    pub fn new(store: Store, max_concurrency: usize) -> Self {
        Self {
            store,
            concurrency: Arc::new(tokio::sync::Semaphore::new(max_concurrency.clamp(1, 16))),
        }
    }

    pub async fn enqueue(&self, kind: &str, payload: Value) -> Result<String> {
        self.store.enqueue_job(kind, &payload).await
    }

    pub async fn run_loop<H: JobHandler + 'static>(
        self: Arc<Self>,
        handler: Arc<H>,
        shutdown: CancellationToken,
    ) {
        loop {
            if shutdown.is_cancelled() {
                break;
            }
            match self.store.claim_job().await {
                Ok(Some((id, kind, payload, attempts))) => {
                    let handler = Arc::clone(&handler);
                    let runner = Arc::clone(&self);
                    let token = shutdown.child_token();
                    if kind != handler.kind() {
                        if let Err(error) = runner
                            .store
                            .finish_job(&id, Some("no handler registered"))
                            .await
                        {
                            warn!(%error, job = %id, "failed to close unhandled job");
                        }
                        continue;
                    }
                    let permit = match Arc::clone(&self.concurrency).acquire_owned().await {
                        Ok(permit) => permit,
                        Err(_) => break,
                    };
                    tokio::spawn(async move {
                        let _permit = permit;
                        info!(job = %id, attempt = attempts, %kind, "job started");
                        let result = tokio::time::timeout(
                            std::time::Duration::from_secs(900),
                            handler.handle(payload, token),
                        )
                        .await;
                        let error = match result {
                            Ok(Ok(())) => None,
                            Ok(Err(error)) => Some(error.to_string()),
                            Err(_) => Some("job timed out".to_string()),
                        };
                        if let Err(store_error) =
                            runner.store.finish_job(&id, error.as_deref()).await
                        {
                            warn!(%store_error, job = %id, "failed to persist job outcome");
                        }
                        if let Some(error) = error {
                            warn!(job = %id, %error, "job failed");
                            if attempts < 3 {
                                let _ = runner.store.retry_job(&id).await;
                            }
                        }
                    });
                }
                Ok(None) => {
                    tokio::time::sleep(std::time::Duration::from_millis(900)).await;
                }
                Err(error) => {
                    warn!(%error, "job queue unavailable");
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                }
            }
        }
        info!("job runner stopped");
    }

    pub async fn cancel(&self, id: &str) -> Result<()> {
        self.store.cancel_job(id).await
    }
}

/// Dispatch all registered handlers from one persisted queue. A job whose kind
/// has no handler is marked failed instead of blocking unrelated work.
pub async fn run_registered(
    store: Store,
    handlers: Vec<Arc<dyn JobHandler>>,
    concurrency: usize,
    shutdown: CancellationToken,
) {
    let runner = Arc::new(JobRunner::new(store, concurrency));
    let in_flight = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    loop {
        if shutdown.is_cancelled() {
            break;
        }
        let kinds: Vec<&str> = handlers.iter().map(|h| h.kind()).collect();
        let claimed = claim_registered(&runner.store, &kinds).await;
        let (id, kind, payload, attempts) = match claimed {
            Ok(Some(job)) => job,
            Ok(None) => {
                tokio::time::sleep(std::time::Duration::from_millis(900)).await;
                continue;
            }
            Err(error) => {
                warn!(%error, "job queue unavailable");
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                continue;
            }
        };
        let Some(handler) = handlers.iter().find(|h| h.kind() == kind).cloned() else {
            let _ = runner
                .store
                .finish_job(&id, Some("no handler registered"))
                .await;
            continue;
        };
        let permit = match Arc::clone(&runner.concurrency).acquire_owned().await {
            Ok(permit) => permit,
            Err(_) => break,
        };
        let runner = Arc::clone(&runner);
        let token = shutdown.child_token();
        let task = tokio::spawn(async move {
            let _permit = permit;
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(900),
                handler.handle(payload, token),
            )
            .await;
            let error = match result {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error.to_string()),
                Err(_) => Some("job timed out".into()),
            };
            if let Err(store_error) = runner.store.finish_job(&id, error.as_deref()).await {
                warn!(%store_error, job = %id, "failed to persist job outcome");
            }
            if error.is_some() && attempts < 3 {
                let _ = runner.store.retry_job(&id).await;
            }
            info!(job = %id, attempt = attempts, %kind, "job completed");
        });
        in_flight.lock().await.push(task);
        // Reap completed tasks so the handles do not grow without bound.
        in_flight.lock().await.retain(|task| !task.is_finished());
    }
    let tasks = in_flight.lock().await.len();
    for handle in in_flight.lock().await.drain(..) {
        let _ = handle.await;
    }
    info!(tasks, "registered job runner stopped after draining");
}

/// A helper for tests and services that need a deterministic cancellation check.
pub fn ensure_not_cancelled(token: &CancellationToken) -> Result<()> {
    if token.is_cancelled() {
        Err(GitPilotError::Cancelled)
    } else {
        Ok(())
    }
}
