use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type Id = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DataOrigin {
    GitHub,
    Demo,
    LocalGit,
}

impl DataOrigin {
    pub fn label(&self) -> &'static str {
        match self {
            Self::GitHub => "GitHub",
            Self::Demo => "Demo data",
            Self::LocalGit => "Local Git",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Observed,
    Derived,
    Inference,
    Recommendation,
    Unavailable,
}

impl EvidenceKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Observed => "Observed",
            Self::Derived => "Derived",
            Self::Inference => "AI inference",
            Self::Recommendation => "Recommendation",
            Self::Unavailable => "Unavailable",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRef {
    pub label: String,
    pub url: Option<String>,
    pub object_id: Option<String>,
    pub kind: EvidenceKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceStatement {
    pub text: String,
    pub kind: EvidenceKind,
    pub evidence: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: Id,
    pub login: String,
    pub host: String,
    pub avatar_url: Option<String>,
    pub connected_at: DateTime<Utc>,
    pub last_validated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceFlowStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: i64,
    pub interval: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeviceFlowPoll {
    Pending,
    SlowDown,
    Authorized(Account),
    Denied,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    pub id: Id,
    pub account_id: Id,
    pub owner: String,
    pub name: String,
    pub full_name: String,
    pub description: Option<String>,
    pub default_branch: String,
    pub private: bool,
    pub archived: bool,
    pub stars: i64,
    pub open_issues: i64,
    pub html_url: String,
    pub language: Option<String>,
    pub pushed_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub favorite: bool,
    pub workspace: Option<String>,
    pub origin: DataOrigin,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrFilter {
    pub repository_id: Option<Id>,
    pub search: String,
    pub view: PrView,
    pub author: Option<String>,
    pub reviewer: Option<String>,
    pub label: Option<String>,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PrView {
    #[default]
    All,
    Mine,
    NeedsReview,
    Blocked,
    CiFailing,
    Ready,
    Stale,
    RecentlyMerged,
    HighRisk,
}

impl PrView {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All open",
            Self::Mine => "My PRs",
            Self::NeedsReview => "Needs my review",
            Self::Blocked => "Blocked",
            Self::CiFailing => "CI failing",
            Self::Ready => "Approved & ready",
            Self::Stale => "Stale",
            Self::RecentlyMerged => "Recently merged",
            Self::HighRisk => "High risk",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MergeableState {
    Mergeable,
    Conflicting,
    Unknown,
    Blocked,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    Success,
    Failure,
    Pending,
    Neutral,
    Missing,
}

impl CheckState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Success => "passing",
            Self::Failure => "failing",
            Self::Pending => "pending",
            Self::Neutral => "neutral",
            Self::Missing => "not reported",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    Approved,
    ChangesRequested,
    Pending,
    None,
}

impl ReviewState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::ChangesRequested => "changes requested",
            Self::Pending => "review pending",
            Self::None => "no human review",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub id: Id,
    pub repository_id: Id,
    pub repository_name: String,
    pub number: i64,
    pub title: String,
    pub body: String,
    pub author: String,
    pub state: String,
    pub draft: bool,
    pub merged: bool,
    pub mergeable: MergeableState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub merged_at: Option<DateTime<Utc>>,
    pub base_ref: String,
    pub head_ref: String,
    pub base_sha: String,
    pub head_sha: String,
    pub additions: i64,
    pub deletions: i64,
    pub changed_files: i64,
    pub commit_count: i64,
    pub review_state: ReviewState,
    pub check_state: CheckState,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub requested_reviewers: Vec<String>,
    pub milestone: Option<String>,
    pub html_url: String,
    pub risk_score: i64,
    pub synced_at: DateTime<Utc>,
    pub origin: DataOrigin,
}

impl PullRequest {
    pub fn age_days(&self) -> i64 {
        (Utc::now() - self.created_at).num_days().max(0)
    }

    pub fn is_ready(&self) -> bool {
        self.review_state == ReviewState::Approved
            && self.check_state == CheckState::Success
            && self.mergeable == MergeableState::Mergeable
            && !self.draft
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub id: Id,
    pub repository_id: Id,
    pub repository_name: String,
    pub number: i64,
    pub title: String,
    pub body: String,
    pub author: String,
    pub state: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub milestone: Option<String>,
    pub comments: i64,
    pub html_url: String,
    pub origin: DataOrigin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commit {
    pub id: Id,
    pub repository_id: Id,
    pub repository_name: String,
    pub sha: String,
    pub message: String,
    pub author_name: String,
    pub author_login: Option<String>,
    pub authored_at: DateTime<Utc>,
    pub committed_at: DateTime<Utc>,
    pub parents: Vec<String>,
    pub additions: i64,
    pub deletions: i64,
    pub html_url: String,
    pub pr_number: Option<i64>,
    pub verification: Option<String>,
    pub origin: DataOrigin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckRun {
    pub id: Id,
    pub pull_request_id: Option<Id>,
    pub commit_sha: String,
    pub name: String,
    pub state: CheckState,
    pub conclusion: Option<String>,
    pub details_url: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewComment {
    pub id: Id,
    pub pull_request_id: Id,
    pub author: String,
    pub body: String,
    pub path: Option<String>,
    pub line: Option<i64>,
    pub diff_hunk: Option<String>,
    pub state: Option<String>,
    pub submitted_at: DateTime<Utc>,
    pub html_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    pub id: Id,
    pub repository_id: Id,
    pub tag_name: String,
    pub name: String,
    pub body: String,
    pub draft: bool,
    pub prerelease: bool,
    pub published_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub html_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseReport {
    pub repository_id: Id,
    pub tag_name: String,
    pub generated_at: DateTime<Utc>,
    pub range: String,
    pub merged_prs: Vec<PullRequest>,
    pub groups: BTreeMap<String, Vec<Id>>,
    pub breaking_changes: Vec<EvidenceStatement>,
    pub dependency_changes: Vec<String>,
    pub migration_changes: Vec<String>,
    pub blockers: Vec<EvidenceStatement>,
    pub readiness: Vec<EvidenceStatement>,
    pub draft_notes: String,
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contributor {
    pub login: String,
    pub avatar_url: Option<String>,
    pub open_prs: i64,
    pub assigned_issues: i64,
    pub review_requests: i64,
    pub pending_reviews: i64,
    pub commits_30d: i64,
    pub owned_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryHealth {
    pub repository_id: Id,
    pub name: String,
    pub open_prs: i64,
    pub review_backlog: i64,
    pub failing_checks: i64,
    pub stale_work: i64,
    pub open_issues: i64,
    pub milestone_completion: Option<f64>,
    pub health_score: Option<i64>,
    pub security_state: SecurityState,
    pub last_synced_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MilestoneSnapshot {
    pub id: Id,
    pub repository_id: Id,
    pub title: String,
    pub state: String,
    pub open_issues: i64,
    pub closed_issues: i64,
    pub due_on: Option<DateTime<Utc>>,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityAlert {
    pub id: Id,
    pub repository_id: Id,
    pub kind: String,
    pub severity: Severity,
    pub title: String,
    pub description: String,
    pub state: String,
    pub url: Option<String>,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SecurityState {
    NotChecked,
    Unavailable,
    NoFindingsReturned,
    FindingsPresent,
}

impl SecurityState {
    pub fn label(self) -> &'static str {
        match self {
            Self::NotChecked => "Security not checked",
            Self::Unavailable => "Security data unavailable",
            Self::NoFindingsReturned => "No alerts returned",
            Self::FindingsPresent => "Security findings present",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Dashboard {
    pub active_projects: i64,
    pub repositories: i64,
    pub open_prs: i64,
    pub awaiting_review: i64,
    pub failing_checks: i64,
    pub stale_work: i64,
    pub open_issues: i64,
    pub recent_releases: Vec<Release>,
    pub health: Vec<RepositoryHealth>,
    pub alerts: Vec<Alert>,
    pub activity: Vec<ActivityItem>,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub missing: Vec<String>,
    pub demo: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: Id,
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub object_id: Option<Id>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "Info",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Critical => "Critical",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityItem {
    pub id: Id,
    pub kind: String,
    pub title: String,
    pub actor: String,
    pub repository: String,
    pub occurred_at: DateTime<Utc>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectBrief {
    pub generated_at: Option<DateTime<Utc>>,
    pub range: String,
    pub freshness: Option<DateTime<Utc>>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub missing: Vec<String>,
    pub facts: Vec<EvidenceStatement>,
    pub metrics: Vec<EvidenceStatement>,
    pub inferences: Vec<EvidenceStatement>,
    pub next_steps: Vec<EvidenceStatement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrDetail {
    pub pull_request: PullRequest,
    pub checks: Vec<CheckRun>,
    pub comments: Vec<ReviewComment>,
    pub commits: Vec<Commit>,
    pub files: Vec<DiffFile>,
    pub linked_issues: Vec<Issue>,
    pub related_prs: Vec<PullRequest>,
    pub findings: Vec<ReviewFinding>,
    pub summary: Option<AiExplanation>,
    pub diff_freshness: Option<DateTime<Utc>>,
    pub latest_review_run: Option<ReviewRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AiExplanation {
    pub generated_at: DateTime<Utc>,
    pub provider: String,
    pub model: String,
    pub one_sentence: String,
    pub executive: String,
    pub developer: String,
    pub technical: String,
    pub review_focus: Vec<String>,
    pub uncertainties: Vec<String>,
    pub citations: Vec<EvidenceRef>,
    pub file_notes: Vec<FileExplanation>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub estimated_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileExplanation {
    pub path: String,
    pub summary: String,
    pub status: String,
    pub additions: i64,
    pub deletions: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffFile {
    pub path: String,
    pub old_path: Option<String>,
    pub status: DiffStatus,
    pub additions: i64,
    pub deletions: i64,
    pub binary: bool,
    pub generated: bool,
    pub hunks: Vec<DiffHunk>,
    pub raw: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    pub old_start: i64,
    pub old_lines: i64,
    pub new_start: i64,
    pub new_lines: i64,
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub content: String,
    pub old_line: Option<i64>,
    pub new_line: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineKind {
    Context,
    Added,
    Removed,
    Meta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRun {
    pub id: Id,
    pub pull_request_id: Id,
    pub head_sha: String,
    pub status: ReviewRunStatus,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub deterministic_findings: i64,
    pub ai_findings: i64,
    pub tokens: i64,
    pub cost_usd: f64,
    pub error: Option<String>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewRunStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewFinding {
    pub id: Id,
    pub run_id: Id,
    pub pull_request_id: Id,
    pub fingerprint: String,
    pub category: String,
    pub severity: Severity,
    pub confidence: f64,
    pub title: String,
    pub explanation: String,
    pub why_it_matters: String,
    pub evidence: Vec<EvidenceRef>,
    pub path: Option<String>,
    pub diff_line: Option<i64>,
    pub suggested_remediation: String,
    pub relevant_tests: Vec<String>,
    pub verification_status: String,
    pub lifecycle: FindingLifecycle,
    pub source: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FindingLifecycle {
    Confirmed,
    Unverified,
    Fixed,
    Dismissed,
    FalsePositive,
    Reintroduced,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeRecord {
    pub id: Id,
    pub repository_id: Option<Id>,
    pub kind: String,
    pub title: String,
    pub content: String,
    pub source_object: String,
    pub source_version: Option<String>,
    pub source_url: Option<String>,
    pub confidence: f64,
    pub extraction_method: String,
    pub explicit_fact: bool,
    pub superseded_by: Option<Id>,
    pub extracted_at: DateTime<Utc>,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: Id,
    pub repository_id: Option<Id>,
    pub kind: String,
    pub severity: Severity,
    pub title: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
    pub snoozed_until: Option<DateTime<Utc>>,
    pub dedupe_key: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: Id,
    pub kind: String,
    pub display_name: String,
    pub endpoint: String,
    pub model: String,
    pub enabled: bool,
    pub has_credential: bool,
    pub local_only: bool,
    pub input_cost_per_million: Option<f64>,
    pub output_cost_per_million: Option<f64>,
    pub context_limit: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderUsage {
    pub provider: String,
    pub model: String,
    pub task: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub latency_ms: i64,
    pub retries: i64,
    pub estimated_cost_usd: f64,
    pub fallback_used: bool,
    pub cancelled: bool,
    pub succeeded: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingPolicy {
    pub task: String,
    pub primary_provider_id: String,
    pub fallback_provider_id: Option<String>,
    pub max_cost_usd: f64,
    pub max_input_tokens: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionPolicy {
    pub read_github: bool,
    pub index_repository: bool,
    pub external_ai: bool,
    pub local_execution: bool,
    pub modify_local_files: bool,
    pub modify_github: bool,
    pub publish_reviews: bool,
    pub approve_or_merge: bool,
    pub scheduled_automations: bool,
    pub local_only: bool,
}

impl Default for PermissionPolicy {
    fn default() -> Self {
        Self {
            read_github: true,
            index_repository: false,
            external_ai: false,
            local_execution: false,
            modify_local_files: false,
            modify_github: false,
            publish_reviews: false,
            approve_or_merge: false,
            scheduled_automations: false,
            local_only: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: Id,
    pub occurred_at: DateTime<Utc>,
    pub actor: String,
    pub action: String,
    pub target: String,
    pub outcome: String,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Automation {
    pub id: Id,
    pub name: String,
    pub enabled: bool,
    pub trigger: String,
    pub conditions: Vec<String>,
    pub actions: Vec<String>,
    pub dry_run: bool,
    pub requires_approval: bool,
    pub repository_id: Option<Id>,
    pub policy: PermissionPolicy,
    pub budget_usd: f64,
    pub timeout_seconds: i64,
    pub created_at: DateTime<Utc>,
    pub last_run_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationRun {
    pub id: Id,
    pub automation_id: Id,
    pub event_key: String,
    pub status: String,
    pub summary: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalRepository {
    pub id: Id,
    pub name: String,
    pub path: String,
    pub default_branch: String,
    pub connected_at: DateTime<Utc>,
    pub last_opened_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalStatus {
    pub branch: String,
    pub ahead: i64,
    pub behind: i64,
    pub staged: i64,
    pub modified: i64,
    pub untracked: i64,
    pub conflicted: i64,
    pub entries: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedView {
    pub id: Id,
    pub screen: String,
    pub name: String,
    pub query: String,
    pub filters: BTreeMap<String, String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandSpec {
    pub executable: String,
    pub args: Vec<String>,
    pub working_directory: String,
    pub network: bool,
    pub modifies_files: bool,
    pub privileged: bool,
    pub timeout_seconds: u64,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub command: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: i64,
    pub timed_out: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub kind: String,
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub repository_id: Option<Id>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSnapshot {
    pub accounts: Vec<Account>,
    pub repositories: Vec<Repository>,
    pub open_prs: Vec<PullRequest>,
    pub notifications: Vec<Notification>,
    pub dashboard: Dashboard,
    pub providers: Vec<ProviderConfig>,
    pub permissions: PermissionPolicy,
    pub demo: bool,
    pub last_sync: Option<DateTime<Utc>>,
    pub jobs: Vec<JobRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub id: Id,
    pub kind: String,
    pub state: String,
    pub attempts: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub theme: ThemeMode,
    pub density: String,
    pub font_scale: f32,
    pub demo_enabled: bool,
    pub account_login: Option<String>,
    pub default_provider_id: Option<String>,
    pub notifications_enabled: bool,
    pub quiet_hours: Option<String>,
    pub per_repository_notifications: BTreeMap<String, bool>,
    pub log_retention_days: i64,
    pub sync_interval_minutes: i64,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Dark,
            density: "comfortable".into(),
            font_scale: 1.0,
            demo_enabled: true,
            account_login: None,
            default_provider_id: None,
            notifications_enabled: true,
            quiet_hours: None,
            per_repository_notifications: BTreeMap::new(),
            log_retention_days: 30,
            sync_interval_minutes: 5,
        }
    }
}
