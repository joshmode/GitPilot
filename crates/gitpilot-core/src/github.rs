//! GitHub REST integration, transport policy and synchronization.

use crate::domain::*;
use crate::error::{GitPilotError, Result, redact_secrets};
use crate::store::Store;
use chrono::{DateTime, Utc};
use reqwest::header::{ACCEPT, AUTHORIZATION, ETAG, IF_NONE_MATCH, LINK, USER_AGENT};
use reqwest::{Client, Method, Response, StatusCode};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::info;
use url::Url;

pub const API_VERSION: &str = "2022-11-28";
pub const MAX_PAGE_SIZE: usize = 100;
pub const MAX_PAGES: usize = 30;

#[derive(Debug, Clone)]
pub struct RateLimit {
    pub remaining: Option<u64>,
    pub reset_at: Option<DateTime<Utc>>,
    pub retry_after: Option<Duration>,
}

#[derive(Debug, Clone)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next: Option<String>,
    pub etag: Option<String>,
    pub not_modified: bool,
    pub rate_limit: RateLimit,
}

#[derive(Clone)]
pub struct GitHubClient {
    http: Client,
    base_url: String,
    token: Arc<str>,
}

impl GitHubClient {
    pub fn new(token: impl Into<String>, base_url: Option<String>) -> Result<Self> {
        let base_url = base_url.unwrap_or_else(|| "https://api.github.com".into());
        let parsed = Url::parse(&base_url)
            .map_err(|error| GitPilotError::InvalidInput(format!("invalid GitHub URL: {error}")))?;
        if parsed.scheme() != "https" {
            return Err(GitPilotError::InvalidInput(
                "GitHub API endpoints must use HTTPS".into(),
            ));
        }
        let http = Client::builder()
            .user_agent(format!("GitPilot/{}", crate::APP_VERSION))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(90))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| GitPilotError::Network(redact_secrets(&error.to_string())))?;
        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').into(),
            token: token.into().into(),
        })
    }

    pub async fn current_user(&self) -> Result<WireUser> {
        self.get_json::<WireUser>("/user").await
    }

    pub async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let response = self
            .request::<serde_json::Value>(Method::GET, path, None)
            .send()
            .await
            .map_err(network_error)?;
        let response = self.check_response(response).await?;
        response
            .json()
            .await
            .map_err(|error| GitPilotError::GitHub {
                status: 200,
                message: format!("invalid JSON response: {error}"),
            })
    }

    pub async fn get_page<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        etag: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<Page<T>> {
        if cancel.is_cancelled() {
            return Err(GitPilotError::Cancelled);
        }
        let mut request = self.request::<serde_json::Value>(Method::GET, path, None);
        if let Some(etag) = etag {
            request = request.header(IF_NONE_MATCH, etag);
        }
        let response = request.send().await.map_err(network_error)?;
        let rate_limit = parse_rate_limit(&response);
        if response.status() == StatusCode::NOT_MODIFIED {
            return Ok(Page {
                items: Vec::new(),
                next: None,
                etag: response
                    .headers()
                    .get(ETAG)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned),
                not_modified: true,
                rate_limit,
            });
        }
        let response = self.check_response(response).await?;
        let etag = response
            .headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let next = parse_next_link(response.headers().get(LINK).and_then(|v| v.to_str().ok()));
        let items = response
            .json()
            .await
            .map_err(|error| GitPilotError::GitHub {
                status: 200,
                message: format!("invalid JSON response: {error}"),
            })?;
        Ok(Page {
            items,
            next,
            etag,
            not_modified: false,
            rate_limit,
        })
    }

    pub async fn get_paged<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        max_pages: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<T>> {
        let mut url = self.resolve(path)?;
        let mut output = Vec::new();
        for _ in 0..max_pages.clamp(1, MAX_PAGES) {
            if cancel.is_cancelled() {
                return Err(GitPilotError::Cancelled);
            }
            let response = self
                .request_url(Method::GET, url.clone())
                .send()
                .await
                .map_err(network_error)?;
            let response = self.check_response(response).await?;
            let next = parse_next_link(response.headers().get(LINK).and_then(|v| v.to_str().ok()));
            let items: Vec<T> = response
                .json()
                .await
                .map_err(|error| GitPilotError::GitHub {
                    status: 200,
                    message: format!("invalid JSON response: {error}"),
                })?;
            output.extend(items);
            let Some(next) = next else { break };
            url = self.resolve(&next)?;
        }
        Ok(output)
    }

    /// Fetch every page of a collection while retaining the first-page ETag for
    /// conditional synchronization. The caller supplies an explicit page cap.
    #[allow(clippy::type_complexity)]
    pub async fn get_all_pages<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        max_pages: usize,
        etag: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<(Vec<T>, Option<String>, bool)> {
        let first = self.get_page::<T>(path, etag, cancel).await?;
        if first.not_modified {
            return Ok((Vec::new(), first.etag, true));
        }
        let mut items = first.items;
        let mut next = first.next;
        let new_etag = first.etag;
        for _ in 1..max_pages.clamp(1, MAX_PAGES) {
            let Some(page_url) = next.take() else {
                break;
            };
            if cancel.is_cancelled() {
                return Err(GitPilotError::Cancelled);
            }
            let response = self
                .request_url(Method::GET, self.resolve(&page_url)?)
                .send()
                .await
                .map_err(network_error)?;
            let response = self.check_response(response).await?;
            next = parse_next_link(response.headers().get(LINK).and_then(|v| v.to_str().ok()));
            let page: Vec<T> = response
                .json()
                .await
                .map_err(|error| GitPilotError::GitHub {
                    status: 200,
                    message: format!("invalid JSON response: {error}"),
                })?;
            items.extend(page);
        }
        Ok((items, new_etag, false))
    }

    pub async fn get_optional_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<Option<T>> {
        let response = self
            .request::<serde_json::Value>(Method::GET, path, None)
            .send()
            .await
            .map_err(network_error)?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = self.check_response(response).await?;
        response
            .json()
            .await
            .map(Some)
            .map_err(|error| GitPilotError::GitHub {
                status: 200,
                message: format!("invalid JSON response: {error}"),
            })
    }

    pub async fn post_json<T: serde::de::DeserializeOwned, B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let response = self
            .request::<B>(Method::POST, path, Some(body))
            .send()
            .await
            .map_err(network_error)?;
        let response = self.check_response(response).await?;
        response
            .json()
            .await
            .map_err(|error| GitPilotError::GitHub {
                status: 200,
                message: format!("invalid JSON response: {error}"),
            })
    }

    /// Execute a GitHub GraphQL query. GraphQL is only used for schema surfaces
    /// such as Projects v2 where there is no equivalent REST endpoint.
    pub async fn graphql<T: serde::de::DeserializeOwned, V: serde::Serialize>(
        &self,
        query: &str,
        variables: V,
    ) -> Result<T> {
        let response = self
            .http
            .post(self.graphql_endpoint())
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
            .header(USER_AGENT, format!("GitPilot/{}", crate::APP_VERSION))
            .json(&serde_json::json!({ "query": query, "variables": variables }))
            .send()
            .await
            .map_err(network_error)?;
        let response = self.check_response(response).await?;
        let envelope: GraphqlEnvelope<T> =
            response
                .json()
                .await
                .map_err(|error| GitPilotError::GitHub {
                    status: 200,
                    message: format!("invalid GraphQL response: {error}"),
                })?;
        if let Some(errors) = envelope.errors
            && !errors.is_empty()
        {
            let message = errors
                .into_iter()
                .filter_map(|error| error.message)
                .collect::<Vec<_>>()
                .join("; ");
            return Err(GitPilotError::GitHub {
                status: 200,
                message: redact_secrets(&format!("GraphQL error: {message}"))
                    .chars()
                    .take(600)
                    .collect(),
            });
        }
        envelope.data.ok_or_else(|| GitPilotError::GitHub {
            status: 200,
            message: "GraphQL response contained no data".into(),
        })
    }

    fn graphql_endpoint(&self) -> String {
        if self.base_url == "https://api.github.com" {
            "https://api.github.com/graphql".into()
        } else {
            // Enterprise API base is https://host/api/v3 and its GraphQL
            // endpoint is https://host/api/graphql.
            self.base_url
                .trim_end_matches("/api/v3")
                .trim_end_matches('/')
                .to_owned()
                + "/api/graphql"
        }
    }

    pub async fn get_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let response = self
            .request::<serde_json::Value>(Method::GET, path, None)
            .send()
            .await
            .map_err(network_error)?;
        let response = self.check_response(response).await?;
        if response.content_length().unwrap_or(0) > 2_000_000 {
            return Err(GitPilotError::InvalidInput(
                "file exceeds the 2 MB content limit".into(),
            ));
        }
        let bytes = response.bytes().await.map_err(network_error)?;
        Ok(bytes.to_vec())
    }

    /// Download a bounded raw file from a repository at a branch/ref. This is
    /// used for local-context retrieval and never for automatic execution.
    pub async fn get_repository_file(
        &self,
        owner: &str,
        name: &str,
        path: &str,
        git_ref: &str,
        max_bytes: usize,
    ) -> Result<String> {
        if path.is_empty()
            || path.starts_with('/')
            || path.contains("..")
            || path.contains('\0')
            || git_ref.contains("..")
        {
            return Err(GitPilotError::Validation(
                "invalid repository file path or ref".into(),
            ));
        }
        let encoded = path
            .split('/')
            .map(|segment| {
                url::form_urlencoded::byte_serialize(segment.as_bytes()).collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("/");
        self.get_text(
            &format!("/repos/{owner}/{name}/contents/{encoded}?ref={}", git_ref),
            max_bytes,
        )
        .await
    }

    pub async fn repository_compare(
        &self,
        owner: &str,
        name: &str,
        base: &str,
        head: &str,
    ) -> Result<serde_json::Value> {
        if base.is_empty()
            || head.is_empty()
            || base.starts_with('-')
            || head.starts_with('-')
            || base.contains("..")
            || head.contains("..")
        {
            return Err(GitPilotError::Validation(
                "invalid comparison revision".into(),
            ));
        }
        let base = url::form_urlencoded::byte_serialize(base.as_bytes()).collect::<String>();
        let head = url::form_urlencoded::byte_serialize(head.as_bytes()).collect::<String>();
        self.get_json::<serde_json::Value>(&format!(
            "/repos/{owner}/{name}/compare/{base}...{head}"
        ))
        .await
    }

    pub async fn get_text(&self, path: &str, max_bytes: usize) -> Result<String> {
        let response = self
            .request::<serde_json::Value>(Method::GET, path, None)
            .send()
            .await
            .map_err(network_error)?;
        let response = self.check_response(response).await?;
        if response.content_length().unwrap_or(0) > max_bytes as u64 {
            return Err(GitPilotError::InvalidInput(
                "remote file exceeds the configured content limit".into(),
            ));
        }
        let value: serde_json::Value =
            response
                .json()
                .await
                .map_err(|error| GitPilotError::GitHub {
                    status: 200,
                    message: format!("invalid contents response: {error}"),
                })?;
        let encoded = value
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| GitPilotError::GitHub {
                status: 200,
                message: "GitHub returned no file content".into(),
            })?;
        if value.get("size").and_then(|v| v.as_u64()).unwrap_or(0) > max_bytes as u64 {
            return Err(GitPilotError::InvalidInput(
                "remote file exceeds the configured content limit".into(),
            ));
        }
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded.replace('\n', ""))
            .map_err(|_| GitPilotError::Validation("invalid base64 file content".into()))?;
        Ok(String::from_utf8_lossy(&bytes[..bytes.len().min(max_bytes)]).into_owned())
    }

    fn request<B: serde::Serialize>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> reqwest::RequestBuilder {
        let request = self
            .request_url(
                method,
                self.resolve(path).unwrap_or_else(|_| {
                    Url::parse(&self.base_url).expect("validated GitHub base URL")
                }),
            )
            .query(&[("per_page", MAX_PAGE_SIZE.to_string())]);
        match body {
            Some(body) => request.json(body),
            None => request,
        }
    }

    fn request_url(&self, method: Method, url: Url) -> reqwest::RequestBuilder {
        self.http
            .request(method, url)
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
            .header(ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .header(USER_AGENT, format!("GitPilot/{}", crate::APP_VERSION))
    }

    fn resolve(&self, path: &str) -> Result<Url> {
        if path.starts_with("http://") || path.starts_with("https://") {
            let url =
                Url::parse(path).map_err(|error| GitPilotError::InvalidInput(error.to_string()))?;
            if !url.as_str().starts_with(&self.base_url)
                && !url.host_str().is_some_and(|h| h.contains("github"))
            {
                return Err(GitPilotError::InvalidInput(
                    "pagination link escaped the configured GitHub host".into(),
                ));
            }
            return Ok(url);
        }
        Url::parse(&format!(
            "{}/{}",
            self.base_url,
            path.trim_start_matches('/')
        ))
        .map_err(|error| GitPilotError::InvalidInput(error.to_string()))
    }

    async fn check_response(&self, response: Response) -> Result<Response> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());
        let remaining = response
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());
        let body = response.text().await.unwrap_or_default();
        let message = serde_json::from_str::<WireError>(&body)
            .ok()
            .and_then(|w| w.message)
            .unwrap_or(body)
            .chars()
            .take(600)
            .collect::<String>();
        if status == StatusCode::UNAUTHORIZED {
            return Err(GitPilotError::AuthRequired(redact_secrets(&message)));
        }
        if status == StatusCode::FORBIDDEN && remaining == Some(0) {
            return Err(GitPilotError::GitHub {
                status: 403,
                message: format!(
                    "rate limit exhausted; retry after {} seconds",
                    retry_after.unwrap_or(60)
                ),
            });
        }
        Err(GitPilotError::GitHub {
            status: status.as_u16(),
            message: redact_secrets(&message),
        })
    }
}

#[derive(Debug, serde::Deserialize)]
struct GraphqlEnvelope<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Option<Vec<GraphqlError>>,
}

#[derive(Debug, serde::Deserialize)]
struct GraphqlError {
    message: Option<String>,
}

fn parse_next_link(header: Option<&str>) -> Option<String> {
    let header = header?;
    for part in header.split(',') {
        let mut sections = part.trim().split(';');
        let url = sections
            .next()?
            .trim()
            .strip_prefix('<')?
            .strip_suffix('>')?;
        if sections.any(|v| v.trim() == r#"rel="next""#) {
            return Some(url.into());
        }
    }
    None
}

fn parse_rate_limit(response: &Response) -> RateLimit {
    let remaining = response
        .headers()
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());
    let reset_at = response
        .headers()
        .get("x-ratelimit-reset")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<i64>().ok())
        .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
        .map(|v| v.with_timezone(&Utc));
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .map(Duration::from_secs);
    RateLimit {
        remaining,
        reset_at,
        retry_after,
    }
}

fn network_error(error: reqwest::Error) -> GitPilotError {
    GitPilotError::Network(redact_secrets(&error.to_string()))
}

fn iso(value: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&value)
        .map(|v| v.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn opt_iso(value: Option<String>) -> Option<DateTime<Utc>> {
    value.map(iso)
}

#[derive(Debug, Deserialize)]
pub struct WireUser {
    pub login: String,
    pub id: u64,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WireError {
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WireRepo {
    pub id: u64,
    pub name: String,
    pub full_name: String,
    pub description: Option<String>,
    pub private: bool,
    pub archived: bool,
    pub default_branch: Option<String>,
    pub stargazers_count: i64,
    pub open_issues_count: i64,
    pub html_url: String,
    pub language: Option<String>,
    pub pushed_at: Option<String>,
    pub updated_at: String,
    pub owner: WireOwner,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireOwner {
    pub login: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WirePr {
    pub id: u64,
    pub number: i64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub draft: Option<bool>,
    pub merged: Option<bool>,
    pub mergeable: Option<bool>,
    pub mergeable_state: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub merged_at: Option<String>,
    pub additions: Option<i64>,
    pub deletions: Option<i64>,
    pub changed_files: Option<i64>,
    pub html_url: String,
    pub user: Option<WireOwner>,
    pub base: WireBranch,
    pub head: WireBranch,
    pub labels: Vec<WireLabel>,
    pub assignees: Option<Vec<WireOwner>>,
    pub requested_reviewers: Option<Vec<WireOwner>>,
    pub milestone: Option<WireMilestone>,
    pub commits: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireBranch {
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub sha: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireLabel {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireMilestone {
    pub title: String,
}

#[derive(Debug, Deserialize)]
pub struct WireIssue {
    pub id: u64,
    pub number: i64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub created_at: String,
    pub updated_at: String,
    pub closed_at: Option<String>,
    pub comments: i64,
    pub html_url: String,
    pub user: Option<WireOwner>,
    pub labels: Vec<WireLabel>,
    pub assignees: Option<Vec<WireOwner>>,
    pub milestone: Option<WireMilestone>,
    pub pull_request: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireMilestoneDetail {
    pub id: u64,
    pub title: String,
    pub state: String,
    pub open_issues: i64,
    pub closed_issues: i64,
    pub due_on: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireContent {
    pub encoding: Option<String>,
    pub content: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct WireCodeScanningAlert {
    number: u64,
    state: String,
    html_url: Option<String>,
    rule: Option<WireSecurityRule>,
    most_recent_instance: Option<WireAlertInstance>,
}

#[derive(Debug, Clone, Deserialize)]
struct WireSecurityRule {
    security_severity_level: Option<String>,
    description: Option<String>,
    id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct WireAlertInstance {
    message: Option<WireAlertMessage>,
}

#[derive(Debug, Clone, Deserialize)]
struct WireAlertMessage {
    text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct WireDependabotAlert {
    number: u64,
    state: String,
    html_url: Option<String>,
    security_advisory: Option<WireAdvisory>,
    dependency: Option<WireDependency>,
}

#[derive(Debug, Clone, Deserialize)]
struct WireAdvisory {
    summary: Option<String>,
    description: Option<String>,
    severity: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct WireDependency {
    package: Option<WirePackage>,
    manifest_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct WirePackage {
    name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct WireSecretScanningAlert {
    number: u64,
    state: String,
    html_url: Option<String>,
    secret_type_display_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct WireBranchProtection {
    required_status_checks: Option<serde_json::Value>,
    enforce_admins: Option<WireEnforceAdmins>,
}

#[derive(Debug, Clone, Deserialize)]
struct WireEnforceAdmins {
    enabled: Option<bool>,
}

impl WireContent {
    pub fn decode(&self) -> Option<String> {
        if self.size.unwrap_or(0) > 2 * 1024 * 1024 || self.encoding.as_deref() != Some("base64") {
            return None;
        }
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(self.content.as_deref()?.replace('\n', ""))
            .ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }
}

#[derive(Debug, Deserialize)]
pub struct WireCommit {
    pub sha: String,
    pub html_url: String,
    pub commit: WireCommitInner,
    pub author: Option<WireOwner>,
    pub parents: Vec<WireParent>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireCommitDetail {
    pub sha: String,
    pub html_url: String,
    pub commit: WireCommitInner,
    pub author: Option<WireOwner>,
    pub parents: Vec<WireParent>,
    pub files: Option<Vec<WireFile>>,
    pub stats: Option<WireCommitStats>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireCommitStats {
    pub additions: Option<i64>,
    pub deletions: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireCommitInner {
    pub message: String,
    pub author: WireGitActor,
    pub committer: WireGitActor,
    pub verification: Option<WireVerification>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireGitActor {
    pub name: String,
    pub date: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireParent {
    pub sha: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireVerification {
    pub verified: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WireCheck {
    pub id: u64,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub details_url: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WireCheckResponse {
    pub total_count: i64,
    pub check_runs: Vec<WireCheck>,
}

#[derive(Debug, Deserialize)]
pub struct WireReview {
    pub id: u64,
    pub user: Option<WireOwner>,
    pub state: String,
    pub body: Option<String>,
    pub submitted_at: Option<String>,
    pub html_url: Option<String>,
    pub commit_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    Pending,
    None,
}

/// Derive a current review decision from the newest decisive review. Reviews
/// attached to a previous head SHA do not approve a new commit when GitHub
/// supplies the commit ID.
pub fn review_decision(reviews: &[(String, Option<String>)], head_sha: &str) -> ReviewDecision {
    let mut latest: Option<(usize, &str)> = None;
    for (index, (state, commit_id)) in reviews.iter().enumerate() {
        let state = state.to_ascii_uppercase();
        if state == "DISMISSED" || state == "COMMENTED" {
            continue;
        }
        if commit_id
            .as_deref()
            .is_some_and(|commit| !head_sha.is_empty() && commit != head_sha)
        {
            continue;
        }
        if latest.is_none_or(|(latest_index, _)| index > latest_index) {
            latest = Some((
                index,
                match state.as_str() {
                    "APPROVED" => "APPROVED",
                    "CHANGES_REQUESTED" => "CHANGES_REQUESTED",
                    "PENDING" => "PENDING",
                    _ => "NONE",
                },
            ));
        }
    }
    match latest.map(|(_, state)| state) {
        Some("APPROVED") => ReviewDecision::Approved,
        Some("CHANGES_REQUESTED") => ReviewDecision::ChangesRequested,
        Some("PENDING") => ReviewDecision::Pending,
        _ => ReviewDecision::None,
    }
}

fn parse_security_severity(value: Option<&str>) -> Severity {
    match value.unwrap_or("").to_ascii_lowercase().as_str() {
        "critical" => Severity::Critical,
        "high" => Severity::High,
        "medium" => Severity::Medium,
        "low" => Severity::Low,
        _ => Severity::Info,
    }
}

fn validate_github_host(host: &str) -> Result<String> {
    let host = host.trim().to_ascii_lowercase();
    if host.is_empty()
        || host.contains('/')
        || host.contains('\\')
        || host.contains('@')
        || host.contains(':')
        || host.contains("..")
        || host.split('.').any(|part| part.is_empty())
        || !host.contains('.')
    {
        return Err(GitPilotError::Validation(
            "host must be a bare fully-qualified DNS name such as github.example.com".into(),
        ));
    }
    Ok(host)
}

#[derive(Debug, Deserialize)]
pub struct WireReviewComment {
    pub id: u64,
    pub user: Option<WireOwner>,
    pub body: Option<String>,
    pub path: Option<String>,
    pub line: Option<i64>,
    pub original_line: Option<i64>,
    pub diff_hunk: Option<String>,
    pub created_at: String,
    pub html_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireFile {
    pub filename: String,
    pub previous_filename: Option<String>,
    pub status: String,
    pub additions: i64,
    pub deletions: i64,
    pub patch: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WireRelease {
    pub id: u64,
    pub tag_name: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub draft: bool,
    pub prerelease: bool,
    pub published_at: Option<String>,
    pub created_at: String,
    pub html_url: String,
}

#[derive(Debug, Deserialize)]
pub struct WireAlert {
    pub number: Option<i64>,
    pub state: Option<String>,
    pub html_url: Option<String>,
}

#[derive(Clone)]
pub struct GitHubService {
    store: Store,
    client_cache: Arc<parking_lot::Mutex<HashMap<String, GitHubClient>>>,
    credentials: Arc<dyn crate::credentials::CredentialStore>,
}

#[derive(Debug, Clone)]
pub struct CreateIssueRequest {
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct CreateIssueBody {
    title: String,
    body: String,
    labels: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreatedIssue {
    pub number: i64,
    pub html_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireCreatedRelease {
    pub id: u64,
    pub tag_name: String,
    pub html_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireRepositoryPermission {
    pub permission: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireBranchSummary {
    pub name: String,
    pub protected: Option<bool>,
    pub commit: WireBranchCommit,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WireBranchCommit {
    pub sha: String,
}

const PROJECTS_QUERY: &str = r#"
query GitPilotProjects($owner: String!, $cursor: String) {
  repositoryOwner(login: $owner) {
    ... on Organization {
      login
      projectsV2(first: 25, after: $cursor, orderBy: {field: UPDATED_AT, direction: DESC}) {
        ...ProjectConnection
      }
    }
    ... on User {
      login
      projectsV2(first: 25, after: $cursor, orderBy: {field: UPDATED_AT, direction: DESC}) {
        ...ProjectConnection
      }
    }
  }
}

fragment ProjectConnection on ProjectV2Connection {
  pageInfo { hasNextPage endCursor }
  nodes {
    id
    number
    title
    shortDescription
    url
    closed
    items(first: 100) {
      nodes {
        id
        updatedAt
        fieldValues(first: 20) {
          nodes {
            ... on ProjectV2ItemFieldSingleSelectValue {
              name
              field { ... on ProjectV2SingleSelectField { name } }
            }
            ... on ProjectV2ItemFieldTextValue {
              text
              field { ... on ProjectV2FieldCommon { name } }
            }
          }
        }
        content {
          __typename
          ... on Issue {
            id
            title
            state
            url
            repository { nameWithOwner }
          }
          ... on PullRequest {
            id
            title
            state
            merged
            url
            repository { nameWithOwner }
          }
          ... on DraftIssue {
            title
          }
        }
      }
    }
  }
}
"#;

#[derive(Debug, Deserialize)]
struct ProjectsQueryData {
    #[serde(rename = "repositoryOwner")]
    owner: Option<ProjectsOwner>,
}

#[derive(Debug, Deserialize)]
struct ProjectsOwner {
    #[serde(rename = "projectsV2")]
    projects_v2: Option<ProjectsConnection>,
}

#[derive(Debug, Deserialize)]
struct ProjectsConnection {
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
    nodes: Option<Vec<WireProject>>,
}

#[derive(Debug, Deserialize)]
struct PageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WireProject {
    id: String,
    number: i64,
    title: String,
    #[serde(rename = "shortDescription")]
    short_description: Option<String>,
    url: String,
    closed: bool,
    items: ProjectItems,
}

#[derive(Debug, Deserialize)]
struct ProjectItems {
    nodes: Option<Vec<WireProjectItem>>,
}

#[derive(Debug, Deserialize)]
struct WireProjectItem {
    id: String,
    #[serde(rename = "updatedAt")]
    updated_at: Option<String>,
    #[serde(rename = "fieldValues")]
    field_values: Option<ProjectFieldValues>,
    content: Option<WireProjectContent>,
}

#[derive(Debug, Deserialize)]
struct ProjectFieldValues {
    nodes: Option<Vec<ProjectFieldValue>>,
}

#[derive(Debug, Deserialize)]
struct ProjectFieldValue {
    name: Option<String>,
    text: Option<String>,
    field: Option<ProjectField>,
}

#[derive(Debug, Deserialize)]
struct ProjectField {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WireProjectContent {
    #[serde(rename = "__typename")]
    typename: String,
    id: Option<String>,
    title: Option<String>,
    state: Option<String>,
    merged: Option<bool>,
    url: Option<String>,
    repository: Option<WireProjectRepo>,
}

#[derive(Debug, Deserialize)]
struct WireProjectRepo {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
}

fn normalize_project(account_id: &str, owner: &str, wire: WireProject) -> GitHubProject {
    let items = wire
        .items
        .nodes
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            let content = item.content?;
            let (title, url, repository) = match content.typename.as_str() {
                "Issue" | "PullRequest" => (
                    content.title?,
                    content.url,
                    content
                        .repository
                        .map(|repository| repository.name_with_owner),
                ),
                "DraftIssue" => (
                    content.title.unwrap_or_else(|| "Draft issue".into()),
                    None,
                    None,
                ),
                _ => return None,
            };
            let mut state = content.state.unwrap_or_else(|| "unknown".into());
            if content.merged == Some(true) {
                state = "merged".into();
            }
            if let Some(status) = item
                .field_values
                .and_then(|values| values.nodes)
                .unwrap_or_default()
                .into_iter()
                .find_map(|value| {
                    let field_name = value.field.and_then(|field| field.name);
                    let selected = value.name.or(value.text);
                    (field_name.as_deref() == Some("Status"))
                        .then_some(selected)
                        .flatten()
                })
            {
                state = status;
            }
            Some(GitHubProjectItem {
                id: item.id,
                content_type: content.typename,
                content_id: content.id,
                title,
                state,
                url,
                repository_name: repository,
                updated_at: item.updated_at.map(iso),
            })
        })
        .collect();
    GitHubProject {
        id: wire.id,
        account_id: account_id.into(),
        owner_login: owner.into(),
        number: wire.number,
        title: wire.title,
        description: wire.short_description.unwrap_or_default(),
        url: wire.url,
        closed: wire.closed,
        synced_at: Utc::now(),
        items,
    }
}

#[cfg(test)]
fn normalize_projects_fixture(value: serde_json::Value) -> Vec<GitHubProject> {
    let data: ProjectsQueryData = serde_json::from_value(value).expect("project fixture");
    let projects = data.owner.and_then(|owner| owner.projects_v2);
    projects
        .and_then(|projects| projects.nodes)
        .unwrap_or_default()
        .into_iter()
        .map(|project| normalize_project("acct", "octo", project))
        .collect()
}

impl GitHubService {
    pub fn new(store: Store, credentials: Arc<dyn crate::credentials::CredentialStore>) -> Self {
        Self {
            store,
            client_cache: Arc::new(parking_lot::Mutex::new(HashMap::new())),
            credentials,
        }
    }

    pub fn client_for(&self, account_id: &str, host: &str) -> Result<GitHubClient> {
        let cache_key = format!("{account_id}:{host}");
        if let Some(client) = self.client_cache.lock().get(&cache_key) {
            return Ok(client.clone());
        }
        let token = self
            .credentials
            .get(crate::credentials::CredentialKind::GitHub, account_id)?
            .ok_or_else(|| {
                GitPilotError::AuthRequired(
                    "No GitHub credential is stored for this account".into(),
                )
            })?;
        let base_url = if host == "github.com" {
            None
        } else {
            Some(format!("https://{host}/api/v3"))
        };
        let client = GitHubClient::new(token, base_url)?;
        self.client_cache.lock().insert(cache_key, client.clone());
        Ok(client)
    }

    pub async fn connect(&self, token: &str, host: &str) -> Result<Account> {
        if token.trim().is_empty() {
            return Err(GitPilotError::Validation("token is empty".into()));
        }
        let host = validate_github_host(host)?;
        let base_url = if host == "github.com" {
            None
        } else {
            Some(format!("https://{host}/api/v3"))
        };
        let client = GitHubClient::new(token, base_url)?;
        let user = client.current_user().await?;
        let account = Account {
            id: format!("gh-{host}-{}", user.id),
            login: user.login,
            host: host.clone(),
            avatar_url: user.avatar_url,
            connected_at: Utc::now(),
            last_validated_at: Some(Utc::now()),
        };
        self.credentials.set(
            crate::credentials::CredentialKind::GitHub,
            &account.id,
            token,
        )?;
        self.store.upsert_account(&account).await?;
        self.client_cache
            .lock()
            .insert(format!("{}:{}", account.id, account.host), client);
        self.store
            .audit(&AuditEvent {
                id: uuid::Uuid::new_v4().to_string(),
                occurred_at: Utc::now(),
                actor: account.login.clone(),
                action: "github.connect".into(),
                target: host,
                outcome: "success".into(),
                metadata: serde_json::json!({"account_id": account.id}),
            })
            .await?;
        Ok(account)
    }

    #[allow(clippy::too_many_arguments)] // Device flow protocol fields stay explicit.
    pub async fn start_device_flow(
        &self,
        client_id: &str,
        host: &str,
        scope: &str,
    ) -> Result<DeviceFlowStart> {
        let host = validate_github_host(host)?;
        if client_id.trim().is_empty() || client_id.len() > 200 {
            return Err(GitPilotError::Validation(
                "GitHub OAuth client ID is required".into(),
            ));
        }
        let endpoint = if host == "github.com" {
            "https://github.com/login/device/code".to_string()
        } else {
            format!("https://{host}/login/device/code")
        };
        let form = [("client_id", client_id), ("scope", scope)]
            .iter()
            .map(|(key, value)| {
                format!(
                    "{}={}",
                    key,
                    url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>()
                )
            })
            .collect::<Vec<_>>()
            .join("&");
        let response = reqwest::Client::new()
            .post(endpoint)
            .header(ACCEPT, "application/json")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form)
            .send()
            .await
            .map_err(network_error)?;
        let value: serde_json::Value = response
            .error_for_status()
            .map_err(network_error)?
            .json()
            .await
            .map_err(network_error)?;
        Ok(DeviceFlowStart {
            device_code: value["device_code"]
                .as_str()
                .ok_or_else(|| GitPilotError::Provider("missing device code".into()))?
                .into(),
            user_code: value["user_code"].as_str().unwrap_or("").into(),
            verification_uri: value["verification_uri"]
                .as_str()
                .unwrap_or("https://github.com/login/device")
                .into(),
            expires_in: value["expires_in"].as_i64().unwrap_or(900),
            interval: value["interval"].as_i64().unwrap_or(5),
        })
    }

    pub async fn poll_device_flow(
        &self,
        client_id: &str,
        host: &str,
        device_code: &str,
    ) -> Result<DeviceFlowPoll> {
        let host = validate_github_host(host)?;
        let endpoint = if host == "github.com" {
            "https://github.com/login/oauth/access_token".to_string()
        } else {
            format!("https://{host}/login/oauth/access_token")
        };
        let form = [
            ("client_id", client_id),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ]
        .iter()
        .map(|(key, value)| {
            format!(
                "{}={}",
                key,
                url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("&");
        let response = reqwest::Client::new()
            .post(endpoint)
            .header(ACCEPT, "application/json")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form)
            .send()
            .await
            .map_err(network_error)?;
        let value: serde_json::Value = response.json().await.map_err(network_error)?;
        if let Some(token) = value["access_token"].as_str() {
            let account = self.connect(token, &host).await?;
            return Ok(DeviceFlowPoll::Authorized(account));
        }
        match value["error"].as_str().unwrap_or("") {
            "authorization_pending" => Ok(DeviceFlowPoll::Pending),
            "slow_down" => Ok(DeviceFlowPoll::SlowDown),
            "expired_token" => Ok(DeviceFlowPoll::Expired),
            "access_denied" => Ok(DeviceFlowPoll::Denied),
            other => Err(GitPilotError::AuthRequired(format!(
                "GitHub device authorization failed: {}",
                if other.is_empty() {
                    "unknown response"
                } else {
                    other
                }
            ))),
        }
    }

    pub async fn disconnect(&self, account_id: &str) -> Result<()> {
        self.credentials
            .delete(crate::credentials::CredentialKind::GitHub, account_id)?;
        self.store.remove_account(account_id).await?;
        self.client_cache
            .lock()
            .retain(|key, _| !key.starts_with(account_id));
        Ok(())
    }

    async fn sync_security(
        &self,
        repo: &Repository,
        client: &GitHubClient,
        cancel: &CancellationToken,
    ) -> Result<()> {
        let mut alerts = Vec::new();
        let mut required_checks: Vec<String> = Vec::new();
        let mut admins_enforced = false;
        if let Ok(Some(protection)) = client
            .get_optional_json::<WireBranchProtection>(&format!(
                "/repos/{}/{}/branches/{}/protection",
                repo.owner, repo.name, repo.default_branch
            ))
            .await
        {
            admins_enforced = protection
                .enforce_admins
                .and_then(|e| e.enabled)
                .unwrap_or(false);
            required_checks = protection
                .required_status_checks
                .and_then(|v| v.get("contexts").cloned())
                .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
                .unwrap_or_default();
        }
        self.store
            .set_branch_protection(&repo.id, admins_enforced, &required_checks)
            .await?;
        let mut checked = 0;
        let mut unavailable = 0;
        let code_scanning = client
            .get_paged::<WireCodeScanningAlert>(
                &format!(
                    "/repos/{}/{}/code-scanning/alerts?state=open&per_page=100",
                    repo.owner, repo.name
                ),
                3,
                cancel,
            )
            .await;
        match code_scanning {
            Ok(items) => {
                checked += 1;
                for alert in items {
                    let rule = alert.rule.unwrap_or(WireSecurityRule {
                        security_severity_level: None,
                        description: None,
                        id: None,
                    });
                    alerts.push(SecurityAlert {
                        id: format!("gh-code-{}", alert.number),
                        repository_id: repo.id.clone(),
                        kind: "code_scanning".into(),
                        severity: parse_security_severity(rule.security_severity_level.as_deref()),
                        title: rule
                            .description
                            .or(rule.id)
                            .unwrap_or_else(|| format!("Code scanning alert {}", alert.number)),
                        description: alert
                            .most_recent_instance
                            .and_then(|i| i.message)
                            .and_then(|m| m.text)
                            .unwrap_or_default(),
                        state: alert.state,
                        url: alert.html_url,
                        observed_at: Utc::now(),
                    });
                }
            }
            Err(GitPilotError::GitHub { status: 404, .. })
            | Err(GitPilotError::GitHub { status: 403, .. }) => unavailable += 1,
            Err(GitPilotError::Cancelled) => return Err(GitPilotError::Cancelled),
            Err(_) => unavailable += 1,
        }
        let dependabot = client
            .get_paged::<WireDependabotAlert>(
                &format!(
                    "/repos/{}/{}/dependabot/alerts?state=open&per_page=100",
                    repo.owner, repo.name
                ),
                3,
                cancel,
            )
            .await;
        match dependabot {
            Ok(items) => {
                checked += 1;
                for alert in items {
                    let advisory = alert.security_advisory.unwrap_or(WireAdvisory {
                        summary: None,
                        description: None,
                        severity: None,
                    });
                    let package = alert
                        .dependency
                        .as_ref()
                        .and_then(|d| d.package.as_ref())
                        .and_then(|p| p.name.clone())
                        .unwrap_or_else(|| "dependency".into());
                    let manifest = alert
                        .dependency
                        .and_then(|d| d.manifest_path)
                        .unwrap_or_default();
                    alerts.push(SecurityAlert {
                        id: format!("gh-dependabot-{}", alert.number),
                        repository_id: repo.id.clone(),
                        kind: "dependabot".into(),
                        severity: parse_security_severity(advisory.severity.as_deref()),
                        title: advisory
                            .summary
                            .unwrap_or_else(|| format!("Vulnerable dependency {package}")),
                        description: format!(
                            "{} in {}",
                            advisory.description.unwrap_or_default(),
                            if manifest.is_empty() {
                                package
                            } else {
                                manifest
                            }
                        ),
                        state: alert.state,
                        url: alert.html_url,
                        observed_at: Utc::now(),
                    });
                }
            }
            Err(GitPilotError::GitHub { status: 404, .. })
            | Err(GitPilotError::GitHub { status: 403, .. }) => unavailable += 1,
            Err(GitPilotError::Cancelled) => return Err(GitPilotError::Cancelled),
            Err(_) => unavailable += 1,
        }
        let secrets = client
            .get_paged::<WireSecretScanningAlert>(
                &format!(
                    "/repos/{}/{}/secret-scanning/alerts?state=open&per_page=100",
                    repo.owner, repo.name
                ),
                3,
                cancel,
            )
            .await;
        match secrets {
            Ok(items) => {
                checked += 1;
                for alert in items {
                    alerts.push(SecurityAlert {
                        id: format!("gh-secret-{}", alert.number),
                        repository_id: repo.id.clone(),
                        kind: "secret_scanning".into(),
                        severity: Severity::Critical,
                        title: alert
                            .secret_type_display_name
                            .unwrap_or_else(|| "Exposed secret".into()),
                        description: "A secret-scanning alert is open. Rotate the credential and review exposure.".into(),
                        state: alert.state,
                        url: alert.html_url,
                        observed_at: Utc::now(),
                    });
                }
            }
            Err(GitPilotError::GitHub { status: 404, .. })
            | Err(GitPilotError::GitHub { status: 403, .. }) => unavailable += 1,
            Err(GitPilotError::Cancelled) => return Err(GitPilotError::Cancelled),
            Err(_) => unavailable += 1,
        }
        // Replace only after all three reads completed; a partial response must
        // not erase previously cached alerts for an unreachable source.
        if unavailable == 0 {
            self.store
                .replace_security_alerts(&repo.id, &alerts)
                .await?;
        }
        let state = if !alerts.is_empty() {
            SecurityState::FindingsPresent
        } else if checked > 0 && unavailable == 0 {
            SecurityState::NoFindingsReturned
        } else {
            SecurityState::Unavailable
        };
        self.store.set_security_state(&repo.id, state).await?;
        Ok(())
    }

    pub async fn create_issue(
        &self,
        repo: &Repository,
        request: &CreateIssueRequest,
    ) -> Result<CreatedIssue> {
        let account = self
            .store
            .accounts()
            .await?
            .into_iter()
            .find(|a| a.id == repo.account_id)
            .ok_or_else(|| {
                GitPilotError::AuthRequired("repository account is not connected".into())
            })?;
        let client = self.client_for(&account.id, &account.host)?;
        let body = CreateIssueBody {
            title: request.title.clone(),
            body: request.body.clone(),
            labels: request.labels.clone(),
        };
        let issue = client
            .post_json::<CreatedIssue, _>(
                &format!("/repos/{}/{}/issues", repo.owner, repo.name),
                &body,
            )
            .await?;
        Ok(issue)
    }

    pub async fn create_release(
        &self,
        repo: &Repository,
        tag_name: &str,
        name: &str,
        body: &str,
        draft: bool,
        prerelease: bool,
    ) -> Result<WireCreatedRelease> {
        if tag_name.trim().is_empty() || tag_name.len() > 200 || body.len() > 120_000 {
            return Err(GitPilotError::Validation(
                "release tag or body is outside the accepted size".into(),
            ));
        }
        let account = self
            .store
            .accounts()
            .await?
            .into_iter()
            .find(|a| a.id == repo.account_id)
            .ok_or_else(|| {
                GitPilotError::AuthRequired("repository account is not connected".into())
            })?;
        let client = self.client_for(&account.id, &account.host)?;
        client
            .post_json::<WireCreatedRelease, _>(
                &format!("/repos/{}/{}/releases", repo.owner, repo.name),
                &serde_json::json!({
                    "tag_name": tag_name,
                    "target_commitish": repo.default_branch,
                    "name": name,
                    "body": body,
                    "draft": draft,
                    "prerelease": prerelease,
                }),
            )
            .await
    }

    pub async fn review_pull_request(
        &self,
        repo: &Repository,
        number: i64,
        head_sha: &str,
        event: &str,
        body: &str,
    ) -> Result<()> {
        if !matches!(event, "COMMENT" | "REQUEST_CHANGES" | "APPROVE") {
            return Err(GitPilotError::Validation("unsupported review event".into()));
        }
        let account = self
            .store
            .accounts()
            .await?
            .into_iter()
            .find(|a| a.id == repo.account_id)
            .ok_or_else(|| {
                GitPilotError::AuthRequired("repository account is not connected".into())
            })?;
        let client = self.client_for(&account.id, &account.host)?;
        let _: serde_json::Value = client
            .post_json(
                &format!(
                    "/repos/{}/{}/pulls/{}/reviews",
                    repo.owner, repo.name, number
                ),
                &serde_json::json!({
                    "commit_id": head_sha,
                    "event": event,
                    "body": body,
                }),
            )
            .await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn review_pull_request_with_comments(
        &self,
        repo: &Repository,
        number: i64,
        head_sha: &str,
        event: &str,
        body: &str,
        comments: &[serde_json::Value],
    ) -> Result<()> {
        if !matches!(event, "COMMENT" | "REQUEST_CHANGES" | "APPROVE") {
            return Err(GitPilotError::Validation("unsupported review event".into()));
        }
        if comments.len() > 50 {
            return Err(GitPilotError::Validation(
                "a review may contain at most 50 line comments".into(),
            ));
        }
        let account = self
            .store
            .accounts()
            .await?
            .into_iter()
            .find(|a| a.id == repo.account_id)
            .ok_or_else(|| {
                GitPilotError::AuthRequired("repository account is not connected".into())
            })?;
        let client = self.client_for(&account.id, &account.host)?;
        let _: serde_json::Value = client
            .post_json(
                &format!(
                    "/repos/{}/{}/pulls/{}/reviews",
                    repo.owner, repo.name, number
                ),
                &serde_json::json!({
                    "commit_id": head_sha,
                    "event": event,
                    "body": body,
                    "comments": comments,
                }),
            )
            .await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create_review_comment(
        &self,
        repo: &Repository,
        number: i64,
        body: &str,
        commit_id: &str,
        path: &str,
        line: i64,
        side: &str,
    ) -> Result<()> {
        if body.trim().is_empty()
            || body.len() > 60_000
            || path.is_empty()
            || path.contains("..")
            || line <= 0
            || !matches!(side, "LEFT" | "RIGHT")
        {
            return Err(GitPilotError::Validation(
                "invalid review comment location or body".into(),
            ));
        }
        let account = self
            .store
            .accounts()
            .await?
            .into_iter()
            .find(|a| a.id == repo.account_id)
            .ok_or_else(|| {
                GitPilotError::AuthRequired("repository account is not connected".into())
            })?;
        let client = self.client_for(&account.id, &account.host)?;
        let _: serde_json::Value = client
            .post_json(
                &format!(
                    "/repos/{}/{}/pulls/{}/comments",
                    repo.owner, repo.name, number
                ),
                &serde_json::json!({
                    "body": body,
                    "commit_id": commit_id,
                    "path": path,
                    "line": line,
                    "side": side,
                }),
            )
            .await?;
        Ok(())
    }

    pub async fn discover_repositories(
        &self,
        account: &Account,
        cancel: &CancellationToken,
    ) -> Result<Vec<Repository>> {
        crate::permissions::require(
            &self.store.load_permissions().await?,
            crate::permissions::Permission::ReadGitHub,
        )?;
        let client = self.client_for(&account.id, &account.host)?;
        let repos: Vec<WireRepo> = client
            .get_paged(
                "/user/repos?sort=updated&affiliation=owner,collaborator,organization_member",
                10,
                cancel,
            )
            .await?;
        let now = Utc::now();
        let mut output = Vec::new();
        for wire in repos {
            let repo = Repository {
                id: format!("gh-repo-{}", wire.id),
                account_id: account.id.clone(),
                owner: wire.owner.login,
                name: wire.name,
                full_name: wire.full_name,
                description: wire.description,
                default_branch: wire.default_branch.unwrap_or_else(|| "main".into()),
                private: wire.private,
                archived: wire.archived,
                stars: wire.stargazers_count,
                open_issues: wire.open_issues_count,
                html_url: wire.html_url,
                language: wire.language,
                pushed_at: opt_iso(wire.pushed_at),
                updated_at: iso(wire.updated_at),
                favorite: false,
                workspace: None,
                origin: DataOrigin::GitHub,
            };
            self.store.upsert_repository(&repo).await?;
            output.push(repo);
        }
        let _ = now;
        Ok(output)
    }

    pub async fn sync_repository(
        &self,
        repo: &Repository,
        cancel: &CancellationToken,
    ) -> Result<SyncSummary> {
        let account = self
            .store
            .accounts()
            .await?
            .into_iter()
            .find(|a| a.id == repo.account_id)
            .ok_or_else(|| {
                GitPilotError::AuthRequired("repository account is not connected".into())
            })?;
        let client = self.client_for(&account.id, &account.host)?;
        let mut summary = SyncSummary::default();
        // Index explicit documentation before PRs so project questions have
        // authoritative source material even when no PR body exists. This is
        // read-only and permission-gated by the caller.
        for (path, kind) in [
            ("README.md", "readme"),
            ("CONTRIBUTING.md", "contributing"),
            ("docs/architecture.md", "architecture"),
            ("ARCHITECTURE.md", "architecture"),
            ("CODEOWNERS", "codeowners"),
            (".github/CODEOWNERS", "codeowners"),
            ("docs/CODEOWNERS", "codeowners"),
        ] {
            let content = client
                .get_optional_json::<WireContent>(&format!(
                    "/repos/{}/{}/contents/{}?ref={}",
                    repo.owner, repo.name, path, repo.default_branch
                ))
                .await
                .ok()
                .flatten()
                .and_then(|wire| wire.decode());
            if let Some(content) = content {
                self.store
                    .save_knowledge(&KnowledgeRecord {
                        id: format!("gh-doc-{}-{}", repo.id, path.replace('/', "-")),
                        repository_id: Some(repo.id.clone()),
                        kind: if kind == "codeowners" {
                            "codeowners".into()
                        } else {
                            "documentation".into()
                        },
                        title: "CODEOWNERS".into(),
                        content: content.chars().take(120_000).collect(),
                        source_object: path.into(),
                        source_version: Some(
                            repo.pushed_at
                                .map(|t| t.to_rfc3339())
                                .unwrap_or_else(|| repo.updated_at.to_rfc3339()),
                        ),
                        source_url: Some(format!(
                            "{}/blob/{}/{}",
                            repo.html_url, repo.default_branch, path
                        )),
                        confidence: 0.95,
                        extraction_method: format!("GitHub Contents API ({kind})"),
                        explicit_fact: true,
                        superseded_by: None,
                        extracted_at: Utc::now(),
                        observed_at: repo.updated_at,
                    })
                    .await?;
            }
        }
        let etag = self.store.repository_etag(&repo.id, "prs").await?;
        let (pr_items, pr_etag, prs_not_modified): (Vec<WirePr>, Option<String>, bool) = client
            .get_all_pages(
                &format!(
                    "/repos/{}/{}/pulls?state=open&per_page=100&sort=updated&direction=desc",
                    repo.owner, repo.name
                ),
                5,
                etag.as_deref(),
                cancel,
            )
            .await?;
        if prs_not_modified {
            summary.prs_not_modified = true;
        } else {
            for wire in pr_items {
                let pr = self.normalize_pr(repo, &wire, &client, cancel).await?;
                self.store.upsert_pull_request(&pr).await?;
                summary.prs += 1;
            }
            self.store
                .set_repository_etag(&repo.id, "prs", pr_etag.as_deref())
                .await?;
        }
        let etag = self.store.repository_etag(&repo.id, "issues").await?;
        let (issue_items, issue_etag, issues_not_modified): (Vec<WireIssue>, Option<String>, bool) =
            client
                .get_all_pages(
                    &format!(
                        "/repos/{}/{}/issues?state=open&per_page=100&sort=updated&direction=desc",
                        repo.owner, repo.name
                    ),
                    5,
                    etag.as_deref(),
                    cancel,
                )
                .await?;
        if !issues_not_modified {
            for wire in issue_items {
                if wire.pull_request.is_some() {
                    continue;
                }
                self.store
                    .upsert_issue(&normalize_issue(repo, wire))
                    .await?;
                summary.issues += 1;
            }
            self.store
                .set_repository_etag(&repo.id, "issues", issue_etag.as_deref())
                .await?;
        }
        // Recently merged work is a first-class PR view. Fetch a bounded,
        // recently-updated closed window rather than fabricating merged rows.
        let merged: Vec<WirePr> = client
            .get_paged(
                &format!(
                    "/repos/{}/{}/pulls?state=closed&per_page=50&sort=updated&direction=desc",
                    repo.owner, repo.name
                ),
                1,
                cancel,
            )
            .await
            .unwrap_or_default();
        for wire in merged.into_iter().filter(|pr| pr.merged_at.is_some()) {
            if let Ok(pr) = self.normalize_pr(repo, &wire, &client, cancel).await {
                let mut pr = pr;
                pr.state = "closed".into();
                pr.merged = true;
                self.store.upsert_pull_request(&pr).await?;
                summary.prs += 1;
            }
        }
        // Closed issues are needed to calculate milestone completion and
        // accurate historical progress. Open issues above remain the active
        // queue; this bounded window supplies only recent closure evidence.
        let closed_issues: Vec<WireIssue> = client
            .get_paged(
                &format!(
                    "/repos/{}/{}/issues?state=closed&per_page=50&sort=updated&direction=desc",
                    repo.owner, repo.name
                ),
                1,
                cancel,
            )
            .await
            .unwrap_or_default();
        for wire in closed_issues {
            if wire.pull_request.is_some() {
                continue;
            }
            self.store
                .upsert_issue(&normalize_issue(repo, wire))
                .await?;
            summary.issues += 1;
        }
        let milestones: Vec<WireMilestoneDetail> = client
            .get_paged(
                &format!(
                    "/repos/{}/{}/milestones?state=open&per_page=100&sort=due_on&direction=asc",
                    repo.owner, repo.name
                ),
                2,
                cancel,
            )
            .await
            .unwrap_or_default();
        for milestone in milestones {
            self.store
                .upsert_milestone(&MilestoneSnapshot {
                    id: format!("gh-milestone-{}", milestone.id),
                    repository_id: repo.id.clone(),
                    title: milestone.title,
                    state: milestone.state,
                    open_issues: milestone.open_issues,
                    closed_issues: milestone.closed_issues,
                    due_on: milestone.due_on.map(iso),
                    fetched_at: Utc::now(),
                })
                .await?;
        }
        self.sync_security(repo, &client, cancel).await?;
        let branches: Vec<WireBranchSummary> = client
            .get_paged(
                &format!("/repos/{}/{}/branches?per_page=100", repo.owner, repo.name),
                2,
                cancel,
            )
            .await
            .unwrap_or_default();
        self.store
            .replace_repository_branches(
                &repo.id,
                &branches
                    .into_iter()
                    .map(|branch| RepositoryBranch {
                        name: branch.name,
                        head_sha: branch.commit.sha,
                        protected: branch.protected.unwrap_or(false),
                        observed_at: Utc::now(),
                    })
                    .collect::<Vec<_>>(),
            )
            .await?;
        let releases: Vec<WireRelease> = client
            .get_paged(
                &format!("/repos/{}/{}/releases?per_page=15", repo.owner, repo.name),
                1,
                cancel,
            )
            .await
            .unwrap_or_default();
        for wire in releases {
            self.store
                .upsert_release(&Release {
                    id: format!("gh-release-{}", wire.id),
                    repository_id: repo.id.clone(),
                    tag_name: wire.tag_name,
                    name: wire.name.unwrap_or_default(),
                    body: wire.body.unwrap_or_default(),
                    draft: wire.draft,
                    prerelease: wire.prerelease,
                    published_at: opt_iso(wire.published_at),
                    created_at: iso(wire.created_at),
                    html_url: wire.html_url,
                })
                .await?;
            summary.releases += 1;
        }
        let commits: Vec<WireCommit> = client
            .get_paged(
                &format!(
                    "/repos/{}/{}/commits?sha={}&per_page=100",
                    repo.owner, repo.name, repo.default_branch
                ),
                3,
                cancel,
            )
            .await
            .unwrap_or_default();
        for wire in commits {
            let sha = wire.sha.clone();
            let parents: Vec<String> = wire.parents.iter().map(|p| p.sha.clone()).collect();
            let pr_number = self
                .store
                .pull_request_for_head_sha(&repo.id, &sha)
                .await
                .ok()
                .flatten()
                .map(|pr| pr.number);
            self.store
                .upsert_commit(&Commit {
                    id: format!("gh-commit-{}", sha),
                    repository_id: repo.id.clone(),
                    repository_name: repo.full_name.clone(),
                    sha: sha.clone(),
                    message: wire.commit.message,
                    author_name: wire.commit.author.name,
                    author_login: wire.author.map(|a| a.login),
                    authored_at: iso(wire.commit.author.date),
                    committed_at: iso(wire.commit.committer.date),
                    parents,
                    additions: 0,
                    deletions: 0,
                    html_url: wire.html_url,
                    pr_number,
                    verification: wire.commit.verification.map(|v| {
                        if v.verified {
                            "verified".into()
                        } else {
                            v.reason.unwrap_or_else(|| "unverified".into())
                        }
                    }),
                    origin: DataOrigin::GitHub,
                })
                .await?;
            summary.commits += 1;
        }
        if let Err(error) = self.sync_projects_v2(repo, cancel).await {
            tracing::warn!(repo = %repo.full_name, %error, "Projects v2 sync unavailable");
        }
        self.store
            .mark_repository_synced(&repo.id, Utc::now())
            .await?;
        info!(repo = %repo.full_name, prs = summary.prs, issues = summary.issues, "repository synced");
        Ok(summary)
    }

    /// Synchronize organization/user Projects v2 metadata and project items
    /// through GitHub's GraphQL API. This is a bounded read-only operation.
    pub async fn sync_projects_v2(
        &self,
        repo: &Repository,
        cancel: &CancellationToken,
    ) -> Result<usize> {
        let account = self
            .store
            .accounts()
            .await?
            .into_iter()
            .find(|a| a.id == repo.account_id)
            .ok_or_else(|| {
                GitPilotError::AuthRequired("repository account is not connected".into())
            })?;
        let client = self.client_for(&account.id, &account.host)?;
        let mut total = 0;
        let mut cursor: Option<String> = None;
        loop {
            if cancel.is_cancelled() {
                return Err(GitPilotError::Cancelled);
            }
            let data: ProjectsQueryData = client
                .graphql(
                    PROJECTS_QUERY,
                    serde_json::json!({
                        "owner": repo.owner,
                        "cursor": cursor,
                    }),
                )
                .await?;
            let Some(projects) = data.owner.and_then(|owner| owner.projects_v2) else {
                break;
            };
            for node in projects.nodes.unwrap_or_default() {
                let project = normalize_project(&account.id, &repo.owner, node);
                total += project.items.len();
                self.store.save_github_project(&project).await?;
            }
            let page_info = projects.page_info;
            if !page_info.has_next_page {
                break;
            }
            cursor = page_info.end_cursor;
            if cursor.is_none() {
                break;
            }
        }
        Ok(total)
    }

    pub async fn fetch_commit_diff(&self, repository_id: &str, sha: &str) -> Result<Vec<DiffFile>> {
        if !sha.chars().all(|c| c.is_ascii_hexdigit()) || sha.len() > 64 {
            return Err(GitPilotError::Validation("invalid commit SHA".into()));
        }
        let repo = self.store.repository(repository_id).await?;
        let account = self
            .store
            .accounts()
            .await?
            .into_iter()
            .find(|a| a.id == repo.account_id)
            .ok_or_else(|| {
                GitPilotError::AuthRequired("repository account is not connected".into())
            })?;
        let client = self.client_for(&account.id, &account.host)?;
        let detail = client
            .get_json::<WireCommitDetail>(&format!(
                "/repos/{}/{}/commits/{}",
                repo.owner, repo.name, sha
            ))
            .await?;
        let files = detail.files.unwrap_or_default();
        Ok(files
            .into_iter()
            .map(|file| {
                file.patch
                    .as_deref()
                    .and_then(|patch| {
                        crate::diff::parse_file_patch(
                            &file.filename,
                            &file.status,
                            file.previous_filename.clone(),
                            patch,
                        )
                        .ok()
                    })
                    .unwrap_or_else(|| DiffFile {
                        path: file.filename,
                        old_path: file.previous_filename,
                        status: match file.status.as_str() {
                            "added" => DiffStatus::Added,
                            "deleted" => DiffStatus::Deleted,
                            "renamed" => DiffStatus::Renamed,
                            _ => DiffStatus::Modified,
                        },
                        additions: file.additions,
                        deletions: file.deletions,
                        binary: file.patch.is_none(),
                        generated: false,
                        hunks: vec![],
                        raw: file.patch,
                    })
            })
            .collect())
    }

    async fn normalize_pr(
        &self,
        repo: &Repository,
        wire: &WirePr,
        client: &GitHubClient,
        cancel: &CancellationToken,
    ) -> Result<PullRequest> {
        // List responses omit diff size and can have stale mergeability. Fetch
        // the authoritative PR detail before persisting cockpit metrics.
        let detail = client
            .get_optional_json::<WirePr>(&format!(
                "/repos/{}/{}/pulls/{}",
                repo.owner, repo.name, wire.number
            ))
            .await
            .ok()
            .flatten();
        let wire = detail.as_ref().unwrap_or(wire);
        let checks = client
            .get_optional_json::<WireCheckResponse>(&format!(
                "/repos/{}/{}/commits/{}/check-runs",
                repo.owner, repo.name, wire.head.sha
            ))
            .await
            .unwrap_or(None);
        let check_state = checks
            .as_ref()
            .map(|response| {
                if response.check_runs.iter().any(|c| {
                    c.conclusion.as_deref() == Some("failure")
                        || c.conclusion.as_deref() == Some("timed_out")
                }) {
                    CheckState::Failure
                } else if response.check_runs.iter().any(|c| c.status != "completed") {
                    CheckState::Pending
                } else if response.check_runs.is_empty() {
                    CheckState::Missing
                } else {
                    CheckState::Success
                }
            })
            .unwrap_or(CheckState::Missing);
        let reviews = client
            .get_paged::<WireReview>(
                &format!(
                    "/repos/{}/{}/pulls/{}/reviews?per_page=100",
                    repo.owner, repo.name, wire.number
                ),
                4,
                cancel,
            )
            .await
            .unwrap_or_default();
        let review_pairs: Vec<_> = reviews
            .iter()
            .map(|review| (review.state.clone(), review.commit_id.clone()))
            .collect();
        let review_state = match review_decision(&review_pairs, &wire.head.sha) {
            ReviewDecision::Approved => ReviewState::Approved,
            ReviewDecision::ChangesRequested => ReviewState::ChangesRequested,
            ReviewDecision::Pending => ReviewState::Pending,
            ReviewDecision::None => ReviewState::None,
        };
        let pr_commits = client
            .get_paged::<WireCommit>(
                &format!(
                    "/repos/{}/{}/pulls/{}/commits?per_page=100",
                    repo.owner, repo.name, wire.number
                ),
                4,
                cancel,
            )
            .await
            .unwrap_or_default();
        for commit in pr_commits {
            self.store
                .upsert_commit(&Commit {
                    id: format!("gh-commit-{}", commit.sha),
                    repository_id: repo.id.clone(),
                    repository_name: repo.full_name.clone(),
                    sha: commit.sha.clone(),
                    message: commit.commit.message,
                    author_name: commit.commit.author.name,
                    author_login: commit.author.map(|a| a.login),
                    authored_at: iso(commit.commit.author.date),
                    committed_at: iso(commit.commit.committer.date),
                    parents: commit.parents.into_iter().map(|p| p.sha).collect(),
                    additions: 0,
                    deletions: 0,
                    html_url: commit.html_url,
                    pr_number: Some(wire.number),
                    verification: commit.commit.verification.map(|v| {
                        if v.verified {
                            "verified".into()
                        } else {
                            v.reason.unwrap_or_else(|| "unverified".into())
                        }
                    }),
                    origin: DataOrigin::GitHub,
                })
                .await?;
        }
        let mergeable = match wire.mergeable_state.as_deref() {
            Some("clean") | Some("unstable") => MergeableState::Mergeable,
            Some("dirty") => MergeableState::Conflicting,
            Some("blocked") | Some("behind") => MergeableState::Blocked,
            _ if wire.mergeable == Some(false) => MergeableState::Conflicting,
            _ => MergeableState::Unknown,
        };
        let pr_id = format!("gh-pr-{}", wire.id);
        if let Some(checks) = checks {
            for check in checks.check_runs {
                let state = match check.conclusion.as_deref() {
                    Some("success") => CheckState::Success,
                    Some("failure") | Some("timed_out") | Some("cancelled") => CheckState::Failure,
                    Some("neutral") | Some("skipped") => CheckState::Neutral,
                    _ if check.status == "completed" => CheckState::Neutral,
                    _ => CheckState::Pending,
                };
                self.store
                    .upsert_check(&CheckRun {
                        id: format!("gh-check-{}", check.id),
                        pull_request_id: Some(pr_id.clone()),
                        commit_sha: wire.head.sha.clone(),
                        name: check.name,
                        state,
                        conclusion: check.conclusion,
                        details_url: check.details_url,
                        started_at: check.started_at.map(iso),
                        completed_at: check.completed_at.map(iso),
                    })
                    .await?;
            }
        }
        let comments = client
            .get_paged::<WireReviewComment>(
                &format!(
                    "/repos/{}/{}/pulls/{}/comments?per_page=100",
                    repo.owner, repo.name, wire.number
                ),
                4,
                cancel,
            )
            .await
            .unwrap_or_default();
        for comment in comments {
            self.store
                .upsert_review_comment(&ReviewComment {
                    id: format!("gh-review-comment-{}", comment.id),
                    pull_request_id: pr_id.clone(),
                    author: comment
                        .user
                        .map(|u| u.login)
                        .unwrap_or_else(|| "ghost".into()),
                    body: comment.body.unwrap_or_default(),
                    path: comment.path,
                    line: comment.line.or(comment.original_line),
                    diff_hunk: comment.diff_hunk,
                    state: None,
                    submitted_at: iso(comment.created_at),
                    html_url: comment.html_url,
                })
                .await?;
        }
        let reviews_comments: Vec<ReviewComment> = reviews
            .into_iter()
            .filter(|review| review.body.as_deref().is_some_and(|b| !b.trim().is_empty()))
            .map(|review| ReviewComment {
                id: format!("gh-review-{}", review.id),
                pull_request_id: pr_id.clone(),
                author: review
                    .user
                    .map(|u| u.login)
                    .unwrap_or_else(|| "ghost".into()),
                body: review.body.unwrap_or_default(),
                path: None,
                line: None,
                diff_hunk: None,
                state: Some(review.state),
                submitted_at: review.submitted_at.map(iso).unwrap_or_else(Utc::now),
                html_url: review.html_url,
            })
            .collect();
        for comment in reviews_comments {
            self.store.upsert_review_comment(&comment).await?;
        }
        let files = client
            .get_paged::<WireFile>(
                &format!(
                    "/repos/{}/{}/pulls/{}/files?per_page=100",
                    repo.owner, repo.name, wire.number
                ),
                8,
                cancel,
            )
            .await
            .unwrap_or_default();
        let diff_files: Vec<DiffFile> = files
            .into_iter()
            .map(|file| {
                file.patch
                    .as_deref()
                    .and_then(|patch| {
                        crate::diff::parse_file_patch(
                            &file.filename,
                            &file.status,
                            file.previous_filename.clone(),
                            patch,
                        )
                        .ok()
                    })
                    .unwrap_or_else(|| DiffFile {
                        path: file.filename,
                        old_path: file.previous_filename,
                        status: match file.status.as_str() {
                            "added" => DiffStatus::Added,
                            "deleted" => DiffStatus::Deleted,
                            "renamed" => DiffStatus::Renamed,
                            _ => DiffStatus::Modified,
                        },
                        additions: file.additions,
                        deletions: file.deletions,
                        binary: file.patch.is_none(),
                        generated: false,
                        hunks: vec![],
                        raw: file.patch,
                    })
            })
            .collect();
        self.store.save_pr_files(&pr_id, &diff_files).await?;
        let _ = cancel;
        let risk = crate::intelligence::risk_score(
            wire.additions.unwrap_or(0),
            wire.deletions.unwrap_or(0),
            wire.changed_files.unwrap_or(0),
            &diff_files,
        );
        Ok(PullRequest {
            id: pr_id,
            repository_id: repo.id.clone(),
            repository_name: repo.full_name.clone(),
            number: wire.number,
            title: wire.title.clone(),
            body: wire.body.clone().unwrap_or_default(),
            author: wire
                .user
                .as_ref()
                .map(|u| u.login.clone())
                .unwrap_or_else(|| "ghost".into()),
            state: wire.state.clone(),
            draft: wire.draft.unwrap_or(false),
            merged: wire.merged.unwrap_or(false) || wire.merged_at.is_some(),
            mergeable,
            created_at: iso(wire.created_at.clone()),
            updated_at: iso(wire.updated_at.clone()),
            merged_at: wire.merged_at.clone().map(iso),
            base_ref: wire.base.git_ref.clone(),
            head_ref: wire.head.git_ref.clone(),
            base_sha: wire.base.sha.clone(),
            head_sha: wire.head.sha.clone(),
            additions: wire.additions.unwrap_or(0),
            deletions: wire.deletions.unwrap_or(0),
            changed_files: wire.changed_files.unwrap_or(diff_files.len() as i64),
            commit_count: wire.commits.unwrap_or(0),
            review_state,
            check_state,
            labels: wire.labels.iter().map(|l| l.name.clone()).collect(),
            assignees: wire
                .assignees
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|u| u.login)
                .collect(),
            requested_reviewers: wire
                .requested_reviewers
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|u| u.login)
                .collect(),
            milestone: wire.milestone.as_ref().map(|m| m.title.clone()),
            html_url: wire.html_url.clone(),
            risk_score: risk,
            synced_at: Utc::now(),
            origin: DataOrigin::GitHub,
        })
    }
}

fn normalize_issue(repo: &Repository, wire: WireIssue) -> Issue {
    Issue {
        id: format!("gh-issue-{}", wire.id),
        repository_id: repo.id.clone(),
        repository_name: repo.full_name.clone(),
        number: wire.number,
        title: wire.title,
        body: wire.body.unwrap_or_default(),
        author: wire.user.map(|u| u.login).unwrap_or_else(|| "ghost".into()),
        state: wire.state,
        created_at: iso(wire.created_at),
        updated_at: iso(wire.updated_at),
        closed_at: wire.closed_at.map(iso),
        labels: wire.labels.into_iter().map(|l| l.name).collect(),
        assignees: wire
            .assignees
            .unwrap_or_default()
            .into_iter()
            .map(|u| u.login)
            .collect(),
        milestone: wire.milestone.map(|m| m.title),
        comments: wire.comments,
        html_url: wire.html_url,
        origin: DataOrigin::GitHub,
    }
}

#[derive(Debug, Clone, Default)]
pub struct SyncSummary {
    pub prs: usize,
    pub issues: usize,
    pub commits: usize,
    pub releases: usize,
    pub prs_not_modified: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_github_pagination_links() {
        let header = r#"<https://api.github.com/repos/a/b/issues?page=2>; rel="next", <https://api.github.com/repos/a/b/issues?page=4>; rel="last""#;
        assert_eq!(
            parse_next_link(Some(header)).as_deref(),
            Some("https://api.github.com/repos/a/b/issues?page=2")
        );
        assert_eq!(parse_next_link(None), None);
    }

    #[test]
    fn projects_v2_fixture_is_typed_and_status_is_preserved() {
        let fixture = serde_json::json!({
            "repositoryOwner": {
                "projectsV2": {
                        "pageInfo": { "hasNextPage": false, "endCursor": null },
                        "nodes": [{
                            "id": "PVT_1",
                            "number": 4,
                            "title": "Release 1.0",
                            "shortDescription": "Ship the release",
                            "url": "https://github.com/orgs/octo/projects/4",
                            "closed": false,
                            "items": {
                                "nodes": [{
                                    "id": "PVTI_1",
                                    "updatedAt": "2026-10-29T00:00:00Z",
                                    "fieldValues": { "nodes": [{
                                        "name": "Done",
                                        "field": { "name": "Status" }
                                    }]},
                                    "content": {
                                        "__typename": "PullRequest",
                                        "id": "PR_1",
                                        "title": "Ship it",
                                        "state": "OPEN",
                                        "merged": false,
                                        "url": "https://github.com/octo/repo/pull/1",
                                        "repository": { "nameWithOwner": "octo/repo" }
                                    }
                                }]
                            }
                        }]
                }
            }
        });
        let projects = normalize_projects_fixture(fixture);
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].items.len(), 1);
        assert_eq!(projects[0].items[0].state, "Done");
        assert_eq!(
            projects[0].items[0].repository_name.as_deref(),
            Some("octo/repo")
        );
    }

    #[test]
    fn refuses_plain_http_endpoint() {
        assert!(GitHubClient::new("token", Some("http://example.test".into())).is_err());
    }

    #[test]
    fn approval_on_previous_head_is_not_current_approval() {
        let reviews = vec![("APPROVED".to_string(), Some("old".to_string()))];
        assert_eq!(review_decision(&reviews, "new"), ReviewDecision::None);
    }

    #[test]
    fn newest_decisive_review_wins() {
        let reviews = vec![
            ("CHANGES_REQUESTED".to_string(), None),
            ("APPROVED".to_string(), None),
        ];
        assert_eq!(review_decision(&reviews, "head"), ReviewDecision::Approved);
    }

    #[test]
    fn base64_content_decoding_is_bounded_and_lossy_for_utf8() {
        use base64::Engine;
        let content = base64::engine::general_purpose::STANDARD.encode(b"hello\nworld");
        let wire = WireContent {
            encoding: Some("base64".into()),
            content: Some(content),
            size: Some(11),
        };
        assert_eq!(wire.decode().as_deref(), Some("hello\nworld"));
        let too_large = WireContent {
            encoding: Some("base64".into()),
            content: Some(String::new()),
            size: Some(3 * 1024 * 1024),
        };
        assert!(too_large.decode().is_none());
    }

    #[test]
    fn enterprise_hosts_must_not_include_paths_or_ports() {
        let invalid = [
            "https://github.com",
            "github.com/api/v3",
            "host:443",
            "a..b",
        ];
        for host in invalid {
            assert!(validate_github_host(host).is_err(), "{host}");
        }
        assert_eq!(
            validate_github_host("GITHUB.EXAMPLE.COM").unwrap(),
            "github.example.com"
        );
    }
}
