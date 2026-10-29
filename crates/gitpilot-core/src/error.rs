use thiserror::Error;

pub type Result<T> = std::result::Result<T, GitPilotError>;

#[derive(Debug, Error)]
pub enum GitPilotError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("network request failed: {0}")]
    Network(String),
    #[error("GitHub API error ({status}): {message}")]
    GitHub { status: u16, message: String },
    #[error("authentication required: {0}")]
    AuthRequired(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("operation cancelled")]
    Cancelled,
    #[error("AI provider error: {0}")]
    Provider(String),
    #[error("budget exceeded: {0}")]
    BudgetExceeded(String),
    #[error("capability not supported: {0}")]
    Unsupported(String),
    #[error("local Git error: {0}")]
    Git(String),
    #[error("credential store error: {0}")]
    Credential(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("data unavailable: {0}")]
    Unavailable(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl GitPilotError {
    pub fn user_message(&self) -> String {
        match self {
            Self::Network(message) => {
                format!(
                    "Could not reach the remote service. Check your connection and retry. ({message})"
                )
            }
            Self::GitHub { status, message } => match status {
                401 => "GitHub rejected the credential. Reconnect your account.".into(),
                403 => {
                    format!("GitHub denied this request or the rate limit was reached. {message}")
                }
                404 => {
                    "The repository or item is unavailable or not visible to this account.".into()
                }
                _ => format!("GitHub request failed ({status}). {message}"),
            },
            other => other.to_string(),
        }
    }
}

/// Remove common credentials from text before it enters logs, audit events or UI
/// error notices.
pub fn redact_secrets(input: &str) -> String {
    static PATTERNS: std::sync::OnceLock<Vec<regex::Regex>> = std::sync::OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        [
            r#"(?i)(authorization:\s*(?:bearer|token)\s+)\S+"#,
            r#"(?i)(api[_-]?key["']?\s*[:=]\s*["']?)[A-Za-z0-9_\-]{12,}"#,
            r"\bgh[pousr]_[A-Za-z0-9_]{20,}\b",
            r"\bsk-[A-Za-z0-9_\-]{16,}\b",
            r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
        ]
        .into_iter()
        .filter_map(|p| regex::Regex::new(p).ok())
        .collect()
    });
    let mut output = input.to_owned();
    for (index, pattern) in patterns.iter().enumerate() {
        output = if index <= 1 {
            pattern.replace_all(&output, "$1[REDACTED]").into_owned()
        } else {
            pattern.replace_all(&output, "[REDACTED]").into_owned()
        };
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_tokens_and_private_keys() {
        let input =
            "Authorization: Bearer ghp_abcdefghijklmnopqrstuvwxyz\napi_key=sk-abcdefghijklmnop";
        let output = redact_secrets(input);
        assert!(!output.contains("ghp_abcdefghijklmnopqrstuvwxyz"));
        assert!(!output.contains("sk-abcdefghijklmnop"));
        assert!(output.contains("[REDACTED]"));
    }

    #[test]
    fn authorization_errors_have_actionable_messages() {
        let error = GitPilotError::GitHub {
            status: 401,
            message: "bad credentials".into(),
        };
        assert!(error.user_message().contains("Reconnect"));
    }
}
