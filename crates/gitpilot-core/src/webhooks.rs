//! Optional GitHub webhook ingestion.
//!
//! A desktop process must not be assumed to receive public webhooks. This
//! module provides a separately deployable receiver with strict signature
//! verification, bounded payloads, replay-safe delivery IDs and durable
//! persistence. It never executes repository content or privileged actions.

use crate::domain::WebhookEventRecord;
use crate::error::{GitPilotError, Result};
use crate::store::Store;
use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tracing::info;

pub const MAX_WEBHOOK_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub struct WebhookReceiver {
    store: Store,
    secret: Arc<[u8]>,
}

impl WebhookReceiver {
    pub fn new(store: Store, secret: impl AsRef<[u8]>) -> Result<Self> {
        let secret = secret.as_ref();
        if secret.len() < 32 {
            return Err(GitPilotError::Validation(
                "webhook secret must be at least 32 bytes".into(),
            ));
        }
        Ok(Self {
            store,
            secret: Arc::from(secret),
        })
    }

    pub fn router(self) -> Router {
        Router::new()
            .route("/github/webhook", post(handle_webhook))
            .with_state(Arc::new(self))
    }

    /// Verify `X-Hub-Signature-256` against the raw request bytes. Constant-time
    /// comparison is provided by `hmac::Mac::verify_slice`.
    pub fn verify_signature(&self, body: &[u8], signature: &str) -> bool {
        let Some(hex_signature) = signature.strip_prefix("sha256=") else {
            return false;
        };
        let Ok(decoded) = hex::decode(hex_signature) else {
            return false;
        };
        let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(&self.secret) else {
            return false;
        };
        mac.update(body);
        mac.verify_slice(&decoded).is_ok()
    }

    pub async fn ingest(
        &self,
        event: &str,
        delivery_id: &str,
        body: &[u8],
    ) -> Result<(WebhookEventRecord, bool)> {
        if body.len() > MAX_WEBHOOK_BYTES {
            return Err(GitPilotError::Validation(
                "webhook payload exceeds the accepted size".into(),
            ));
        }
        if event.is_empty()
            || event.len() > 80
            || delivery_id.is_empty()
            || delivery_id.len() > 128
            || !delivery_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(GitPilotError::Validation(
                "invalid webhook event or delivery identifier".into(),
            ));
        }
        let payload: serde_json::Value = serde_json::from_slice(body)
            .map_err(|error| GitPilotError::Validation(format!("invalid JSON payload: {error}")))?;
        let action = payload
            .get("action")
            .and_then(|value| value.as_str())
            .map(str::to_owned);
        let repository_full_name = payload
            .pointer("/repository/full_name")
            .and_then(|value| value.as_str())
            .map(str::to_owned);
        let payload_sha256 = hex::encode(Sha256::digest(body));
        let record = WebhookEventRecord {
            delivery_id: delivery_id.into(),
            event: event.into(),
            action: action.clone(),
            repository_full_name: repository_full_name.clone(),
            received_at: Utc::now(),
            processed_at: None,
            payload: payload.clone(),
        };
        let inserted = self
            .store
            .insert_webhook_event(
                delivery_id,
                event,
                action.as_deref(),
                repository_full_name.as_deref(),
                &payload_sha256,
                &payload,
            )
            .await?;
        info!(
            event,
            delivery_id, inserted, "verified GitHub webhook received"
        );
        Ok((record, inserted))
    }
}

async fn handle_webhook(
    State(receiver): State<Arc<WebhookReceiver>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if body.len() > MAX_WEBHOOK_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, "payload too large").into_response();
    }
    let signature = headers
        .get("x-hub-signature-256")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !receiver.verify_signature(&body, signature) {
        return (StatusCode::UNAUTHORIZED, "invalid webhook signature").into_response();
    }
    let event = headers
        .get("x-github-event")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let delivery_id = headers
        .get("x-github-delivery")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    match receiver.ingest(event, delivery_id, &body).await {
        Ok((_record, true)) => (StatusCode::OK, "accepted").into_response(),
        Ok((_record, false)) => (StatusCode::OK, "duplicate delivery").into_response(),
        Err(GitPilotError::Validation(message)) => {
            (StatusCode::BAD_REQUEST, message).into_response()
        }
        Err(error) => {
            tracing::warn!(%error, "webhook persistence failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "persistence failed").into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn signatures_are_verified_and_rejected() {
        let receiver = WebhookReceiver::new(
            Store::open_in_memory().await.expect("store"),
            b"0123456789abcdef0123456789abcdef",
        )
        .expect("receiver");
        let body = br#"{"action":"opened"}"#;
        assert!(!receiver.verify_signature(body, "sha256=deadbeef"));
        assert!(!receiver.verify_signature(body, "sha256=zzzz"));
        assert!(!receiver.verify_signature(body, ""));
    }

    #[tokio::test]
    async fn signed_events_are_persisted_and_deduplicated() {
        let receiver = WebhookReceiver::new(
            Store::open_in_memory().await.expect("store"),
            b"0123456789abcdef0123456789abcdef",
        )
        .expect("receiver");
        let body = br#"{"action":"opened","repository":{"full_name":"octo/repo"}}"#;
        let (_, inserted) = receiver
            .ingest("pull_request", "delivery-1", body)
            .await
            .expect("ingest");
        assert!(inserted);
        let (_, duplicate) = receiver
            .ingest("pull_request", "delivery-1", body)
            .await
            .expect("duplicate");
        assert!(!duplicate);
        assert!(
            receiver
                .ingest("pull_request", "../bad", body)
                .await
                .is_err()
        );
    }
}
