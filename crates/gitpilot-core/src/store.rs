use crate::demo;
use crate::domain::*;
use crate::error::{GitPilotError, Result};
use chrono::{DateTime, Duration, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use uuid::Uuid;

const MIGRATION: &str = include_str!("../../../migrations/0001_init.sql");
const MIGRATION_2: &str = include_str!("../../../migrations/0002_pr_commit_count.sql");
const MIGRATION_3: &str = include_str!("../../../migrations/0003_projects_webhooks.sql");
const MIGRATION_4: &str = include_str!("../../../migrations/0004_hybrid_retrieval.sql");

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
    path: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct StoreStats {
    pub repositories: i64,
    pub pull_requests: i64,
    pub issues: i64,
    pub commits: i64,
    pub knowledge: i64,
}

impl Store {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(std::time::Duration::from_secs(10));
        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .min_connections(1)
            .connect_with(options)
            .await?;
        apply_migrations(&pool).await?;
        backfill_knowledge_hashes(&pool).await?;
        Ok(Self { pool, path })
    }

    pub async fn open_in_memory() -> Result<Self> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")?
            .journal_mode(SqliteJournalMode::Memory)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        apply_migrations(&pool).await?;
        backfill_knowledge_hashes(&pool).await?;
        Ok(Self {
            pool,
            path: PathBuf::from(":memory:"),
        })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let value = sqlx::query_scalar::<_, String>("SELECT value FROM app_settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(value)
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO app_settings(key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn load_settings(&self) -> Result<AppSettings> {
        let Some(raw) = self.get_setting("app_settings").await? else {
            return Ok(AppSettings::default());
        };
        match serde_json::from_str(&raw) {
            Ok(settings) => Ok(settings),
            Err(error) => {
                tracing::warn!(%error, "stored settings are invalid; using safe defaults");
                Ok(AppSettings::default())
            }
        }
    }

    pub async fn save_settings(&self, settings: &AppSettings) -> Result<()> {
        self.set_setting("app_settings", &serde_json::to_string(settings)?)
            .await
    }

    pub async fn set_notification_policy(&self, repository_id: &str, enabled: bool) -> Result<()> {
        let mut settings = self.load_settings().await?;
        settings
            .per_repository_notifications
            .insert(repository_id.into(), enabled);
        self.save_settings(&settings).await
    }

    pub async fn load_permissions(&self) -> Result<PermissionPolicy> {
        let Some(raw) = self.get_setting("permissions").await? else {
            return Ok(PermissionPolicy::default());
        };
        match serde_json::from_str(&raw) {
            Ok(policy) => Ok(policy),
            Err(error) => {
                tracing::warn!(%error, "stored permissions are invalid; using deny-by-default policy");
                Ok(PermissionPolicy::default())
            }
        }
    }

    pub async fn save_permissions(&self, policy: &PermissionPolicy) -> Result<()> {
        self.set_setting("permissions", &serde_json::to_string(policy)?)
            .await
    }

    pub async fn upsert_account(&self, account: &Account) -> Result<()> {
        sqlx::query(
            "INSERT INTO accounts(id, login, host, avatar_url, connected_at, last_validated_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(host, login) DO UPDATE SET
               avatar_url=excluded.avatar_url, last_validated_at=excluded.last_validated_at",
        )
        .bind(&account.id)
        .bind(&account.login)
        .bind(&account.host)
        .bind(&account.avatar_url)
        .bind(account.connected_at.to_rfc3339())
        .bind(account.last_validated_at.map(|v| v.to_rfc3339()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn accounts(&self) -> Result<Vec<Account>> {
        let rows = sqlx::query("SELECT * FROM accounts ORDER BY login")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| Account {
                id: r.get("id"),
                login: r.get("login"),
                host: r.get("host"),
                avatar_url: r.get("avatar_url"),
                connected_at: parse_time(r.get("connected_at")),
                last_validated_at: r
                    .try_get::<Option<String>, _>("last_validated_at")
                    .ok()
                    .flatten()
                    .map(parse_time),
            })
            .collect())
    }

    pub async fn remove_account(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM accounts WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn upsert_repository(&self, repo: &Repository) -> Result<()> {
        sqlx::query(
            "INSERT INTO repositories(
               id, account_id, owner, name, full_name, description, default_branch, private,
               archived, stars, open_issues, html_url, language, pushed_at, updated_at,
               favorite, workspace, origin
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(account_id, full_name) DO UPDATE SET
               owner=excluded.owner, name=excluded.name, full_name=excluded.full_name,
               description=excluded.description, default_branch=excluded.default_branch,
               private=excluded.private, archived=excluded.archived, stars=excluded.stars,
               open_issues=excluded.open_issues, html_url=excluded.html_url,
               language=excluded.language, pushed_at=excluded.pushed_at,
               updated_at=excluded.updated_at, origin=excluded.origin",
        )
        .bind(&repo.id)
        .bind(&repo.account_id)
        .bind(&repo.owner)
        .bind(&repo.name)
        .bind(&repo.full_name)
        .bind(&repo.description)
        .bind(&repo.default_branch)
        .bind(repo.private as i64)
        .bind(repo.archived as i64)
        .bind(repo.stars)
        .bind(repo.open_issues)
        .bind(&repo.html_url)
        .bind(&repo.language)
        .bind(repo.pushed_at.map(|v| v.to_rfc3339()))
        .bind(repo.updated_at.to_rfc3339())
        .bind(repo.favorite as i64)
        .bind(&repo.workspace)
        .bind(origin_str(&repo.origin))
        .execute(&self.pool)
        .await?;
        self.insert_search(
            "repository",
            &repo.id,
            &repo.full_name,
            repo.description.as_deref().unwrap_or(""),
            Some(&repo.id),
        )
        .await?;
        Ok(())
    }

    pub async fn repositories(&self) -> Result<Vec<Repository>> {
        let rows = sqlx::query(
            "SELECT * FROM repositories ORDER BY favorite DESC, full_name COLLATE NOCASE",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(repo_from_row).collect())
    }

    pub async fn repository(&self, id: &str) -> Result<Repository> {
        let row = sqlx::query("SELECT * FROM repositories WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| GitPilotError::NotFound("repository".into()))?;
        Ok(repo_from_row(row))
    }

    pub async fn set_repository_favorite(&self, id: &str, favorite: bool) -> Result<()> {
        sqlx::query("UPDATE repositories SET favorite = ? WHERE id = ?")
            .bind(favorite as i64)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_repository_workspace(&self, id: &str, workspace: Option<&str>) -> Result<()> {
        sqlx::query("UPDATE repositories SET workspace = ? WHERE id = ?")
            .bind(workspace)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn mark_repository_synced(&self, id: &str, at: DateTime<Utc>) -> Result<()> {
        sqlx::query("UPDATE repositories SET last_synced_at = ? WHERE id = ?")
            .bind(at.to_rfc3339())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_repository_etag(
        &self,
        id: &str,
        kind: &str,
        etag: Option<&str>,
    ) -> Result<()> {
        let column = match kind {
            "prs" => "etag_prs",
            "issues" => "etag_issues",
            _ => return Ok(()),
        };
        let sql = format!("UPDATE repositories SET {column} = ? WHERE id = ?");
        sqlx::query(&sql)
            .bind(etag)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn repository_etag(&self, id: &str, kind: &str) -> Result<Option<String>> {
        let column = match kind {
            "prs" => "etag_prs",
            "issues" => "etag_issues",
            _ => return Ok(None),
        };
        let sql = format!("SELECT {column} FROM repositories WHERE id = ?");
        Ok(sqlx::query_scalar(&sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .flatten())
    }

    pub async fn set_security_state(&self, id: &str, state: SecurityState) -> Result<()> {
        let value = match state {
            SecurityState::NotChecked => "not_checked",
            SecurityState::Unavailable => "unavailable",
            SecurityState::NoFindingsReturned => "no_findings",
            SecurityState::FindingsPresent => "findings",
        };
        sqlx::query("UPDATE repositories SET security_state = ? WHERE id = ?")
            .bind(value)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn repository_security_state(&self, id: &str) -> Result<SecurityState> {
        let value: String =
            sqlx::query_scalar("SELECT security_state FROM repositories WHERE id = ?")
                .bind(id)
                .fetch_one(&self.pool)
                .await?;
        Ok(match value.as_str() {
            "unavailable" => SecurityState::Unavailable,
            "no_findings" => SecurityState::NoFindingsReturned,
            "findings" => SecurityState::FindingsPresent,
            _ => SecurityState::NotChecked,
        })
    }

    pub async fn upsert_pull_request(&self, pr: &PullRequest) -> Result<()> {
        sqlx::query(
            "INSERT INTO pull_requests(
               id, repository_id, repository_name, number, title, body, author, state, draft,
               merged, mergeable, created_at, updated_at, merged_at, base_ref, head_ref,
               base_sha, head_sha, additions, deletions, changed_files, commit_count, review_state,
               check_state, labels, assignees, requested_reviewers, milestone, html_url,
               risk_score, synced_at, origin
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(repository_id, number) DO UPDATE SET
               title=excluded.title, body=excluded.body, state=excluded.state, draft=excluded.draft,
               merged=excluded.merged, mergeable=excluded.mergeable, updated_at=excluded.updated_at,
               merged_at=excluded.merged_at, base_ref=excluded.base_ref, head_ref=excluded.head_ref,
               base_sha=excluded.base_sha, head_sha=excluded.head_sha, additions=excluded.additions,
               deletions=excluded.deletions, changed_files=excluded.changed_files,
               commit_count=excluded.commit_count,
               review_state=excluded.review_state, check_state=excluded.check_state,
               labels=excluded.labels, assignees=excluded.assignees,
               requested_reviewers=excluded.requested_reviewers, milestone=excluded.milestone,
               html_url=excluded.html_url, risk_score=excluded.risk_score,
               synced_at=excluded.synced_at, origin=excluded.origin",
        )
        .bind(&pr.id)
        .bind(&pr.repository_id)
        .bind(&pr.repository_name)
        .bind(pr.number)
        .bind(&pr.title)
        .bind(&pr.body)
        .bind(&pr.author)
        .bind(&pr.state)
        .bind(pr.draft as i64)
        .bind(pr.merged as i64)
        .bind(mergeable_str(pr.mergeable))
        .bind(pr.created_at.to_rfc3339())
        .bind(pr.updated_at.to_rfc3339())
        .bind(pr.merged_at.map(|v| v.to_rfc3339()))
        .bind(&pr.base_ref)
        .bind(&pr.head_ref)
        .bind(&pr.base_sha)
        .bind(&pr.head_sha)
        .bind(pr.additions)
        .bind(pr.deletions)
        .bind(pr.changed_files)
        .bind(pr.commit_count)
        .bind(review_str(pr.review_state))
        .bind(check_str(pr.check_state))
        .bind(serde_json::to_string(&pr.labels)?)
        .bind(serde_json::to_string(&pr.assignees)?)
        .bind(serde_json::to_string(&pr.requested_reviewers)?)
        .bind(&pr.milestone)
        .bind(&pr.html_url)
        .bind(pr.risk_score)
        .bind(pr.synced_at.to_rfc3339())
        .bind(origin_str(&pr.origin))
        .execute(&self.pool)
        .await?;
        self.insert_search(
            "pull_request",
            &pr.id,
            &format!("#{} {}", pr.number, pr.title),
            &pr.body,
            Some(&pr.repository_id),
        )
        .await?;
        Ok(())
    }

    pub async fn pull_requests(&self, filter: &PrFilter) -> Result<Vec<PullRequest>> {
        let mut sql = String::from("SELECT * FROM pull_requests WHERE 1=1");
        if filter.view == PrView::RecentlyMerged {
            sql.push_str(" AND merged = 1");
        } else {
            sql.push_str(" AND state = 'open'");
        }
        if filter.repository_id.is_some() {
            sql.push_str(" AND repository_id = ?");
        }
        match filter.view {
            PrView::Blocked => {
                sql.push_str(" AND (mergeable = 'conflicting' OR check_state = 'failure')")
            }
            PrView::CiFailing => sql.push_str(" AND check_state = 'failure'"),
            PrView::Ready => sql.push_str(
                " AND review_state = 'approved' AND check_state = 'success'
                  AND mergeable = 'mergeable' AND draft = 0",
            ),
            PrView::NeedsReview => {
                sql.push_str(" AND review_state IN ('none','pending') AND draft = 0")
            }
            PrView::Stale => sql.push_str(" AND updated_at < ?"),
            PrView::HighRisk => sql.push_str(" AND risk_score >= 60"),
            PrView::Mine => sql.push_str(" AND author = ?"),
            _ => {}
        }
        if filter.view == PrView::NeedsReview && filter.reviewer.is_some() {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM json_each(pull_requests.requested_reviewers)
                              WHERE value = ?)",
            );
        }
        if !filter.search.trim().is_empty() {
            sql.push_str(" AND (title LIKE ? OR body LIKE ? OR author LIKE ?)");
        }
        sql.push_str(" ORDER BY updated_at DESC LIMIT ? OFFSET ?");
        let pattern = format!("%{}%", filter.search.trim());
        let mut q = sqlx::query(&sql);
        if filter.view == PrView::Stale {
            q = q.bind((Utc::now() - Duration::days(crate::intelligence::STALE_DAYS)).to_rfc3339());
        }
        if filter.view == PrView::Mine {
            q = q.bind(filter.author.clone().unwrap_or_default());
        }
        if filter.view == PrView::NeedsReview
            && let Some(reviewer) = &filter.reviewer
        {
            q = q.bind(reviewer);
        }
        if let Some(repo) = &filter.repository_id {
            q = q.bind(repo);
        }
        if !filter.search.trim().is_empty() {
            q = q.bind(&pattern).bind(&pattern).bind(&pattern);
        }
        let rows = q
            .bind(filter.limit.clamp(1, 500) as i64)
            .bind(filter.offset as i64)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(pr_from_row).collect())
    }

    pub async fn count_pull_requests(&self, filter: &PrFilter) -> Result<i64> {
        let mut sql = String::from("SELECT COUNT(*) FROM pull_requests WHERE 1=1");
        if filter.view == PrView::RecentlyMerged {
            sql.push_str(" AND merged = 1");
        } else {
            sql.push_str(" AND state = 'open'");
        }
        if filter.repository_id.is_some() {
            sql.push_str(" AND repository_id = ?");
        }
        match filter.view {
            PrView::Blocked => {
                sql.push_str(" AND (mergeable = 'conflicting' OR check_state = 'failure')")
            }
            PrView::CiFailing => sql.push_str(" AND check_state = 'failure'"),
            PrView::Ready => sql.push_str(
                " AND review_state = 'approved' AND check_state = 'success'
                  AND mergeable = 'mergeable' AND draft = 0",
            ),
            PrView::NeedsReview => {
                sql.push_str(" AND review_state IN ('none','pending') AND draft = 0")
            }
            PrView::Stale => {
                sql.push_str(" AND updated_at < ?");
            }
            PrView::HighRisk => sql.push_str(" AND risk_score >= 60"),
            PrView::Mine => sql.push_str(" AND author = ?"),
            _ => {}
        }
        if filter.view == PrView::NeedsReview && filter.reviewer.is_some() {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM json_each(pull_requests.requested_reviewers)
                              WHERE value = ?)",
            );
        }
        if !filter.search.trim().is_empty() {
            sql.push_str(" AND (title LIKE ? OR body LIKE ? OR author LIKE ?)");
        }
        let pattern = format!("%{}%", filter.search.trim());
        let mut q = sqlx::query_scalar::<_, i64>(&sql);
        if filter.view == PrView::Stale {
            q = q.bind((Utc::now() - Duration::days(crate::intelligence::STALE_DAYS)).to_rfc3339());
        }
        if filter.view == PrView::Mine {
            q = q.bind(filter.author.clone().unwrap_or_default());
        }
        if filter.view == PrView::NeedsReview
            && let Some(reviewer) = &filter.reviewer
        {
            q = q.bind(reviewer);
        }
        if let Some(repo) = &filter.repository_id {
            q = q.bind(repo);
        }
        if !filter.search.trim().is_empty() {
            q = q.bind(&pattern).bind(&pattern).bind(&pattern);
        }
        Ok(q.fetch_one(&self.pool).await?)
    }

    pub async fn pull_request(&self, id: &str) -> Result<PullRequest> {
        let row = sqlx::query("SELECT * FROM pull_requests WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| GitPilotError::NotFound("pull request".into()))?;
        Ok(pr_from_row(row))
    }

    pub async fn pull_request_for_head_sha(
        &self,
        repository_id: &str,
        head_sha: &str,
    ) -> Result<Option<PullRequest>> {
        let row = sqlx::query(
            "SELECT * FROM pull_requests WHERE repository_id = ? AND head_sha = ?
             ORDER BY updated_at DESC LIMIT 1",
        )
        .bind(repository_id)
        .bind(head_sha)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(pr_from_row))
    }

    pub async fn upsert_issue(&self, issue: &Issue) -> Result<()> {
        sqlx::query(
            "INSERT INTO issues(
               id, repository_id, repository_name, number, title, body, author, state,
               created_at, updated_at, closed_at, labels, assignees, milestone, comments,
               html_url, origin
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(repository_id, number) DO UPDATE SET
               title=excluded.title, body=excluded.body, state=excluded.state,
               updated_at=excluded.updated_at, closed_at=excluded.closed_at,
               labels=excluded.labels, assignees=excluded.assignees,
               milestone=excluded.milestone, comments=excluded.comments,
               html_url=excluded.html_url, origin=excluded.origin",
        )
        .bind(&issue.id)
        .bind(&issue.repository_id)
        .bind(&issue.repository_name)
        .bind(issue.number)
        .bind(&issue.title)
        .bind(&issue.body)
        .bind(&issue.author)
        .bind(&issue.state)
        .bind(issue.created_at.to_rfc3339())
        .bind(issue.updated_at.to_rfc3339())
        .bind(issue.closed_at.map(|v| v.to_rfc3339()))
        .bind(serde_json::to_string(&issue.labels)?)
        .bind(serde_json::to_string(&issue.assignees)?)
        .bind(&issue.milestone)
        .bind(issue.comments)
        .bind(&issue.html_url)
        .bind(origin_str(&issue.origin))
        .execute(&self.pool)
        .await?;
        self.insert_search(
            "issue",
            &issue.id,
            &format!("#{} {}", issue.number, issue.title),
            &issue.body,
            Some(&issue.repository_id),
        )
        .await?;
        Ok(())
    }

    pub async fn issues(&self, repository_id: Option<&str>, open_only: bool) -> Result<Vec<Issue>> {
        let rows = if let Some(repo) = repository_id {
            if open_only {
                sqlx::query("SELECT * FROM issues WHERE repository_id = ? AND state = 'open' ORDER BY updated_at DESC")
                    .bind(repo).fetch_all(&self.pool).await?
            } else {
                sqlx::query("SELECT * FROM issues WHERE repository_id = ? ORDER BY updated_at DESC")
                    .bind(repo)
                    .fetch_all(&self.pool)
                    .await?
            }
        } else if open_only {
            sqlx::query("SELECT * FROM issues WHERE state = 'open' ORDER BY updated_at DESC")
                .fetch_all(&self.pool)
                .await?
        } else {
            sqlx::query("SELECT * FROM issues ORDER BY updated_at DESC")
                .fetch_all(&self.pool)
                .await?
        };
        Ok(rows.into_iter().map(issue_from_row).collect())
    }

    pub async fn upsert_milestone(&self, milestone: &MilestoneSnapshot) -> Result<()> {
        sqlx::query(
            "INSERT INTO milestones(id, repository_id, title, state, open_issues,
               closed_issues, due_on, fetched_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(repository_id, title) DO UPDATE SET
               state=excluded.state, open_issues=excluded.open_issues,
               closed_issues=excluded.closed_issues, due_on=excluded.due_on,
               fetched_at=excluded.fetched_at",
        )
        .bind(&milestone.id)
        .bind(&milestone.repository_id)
        .bind(&milestone.title)
        .bind(&milestone.state)
        .bind(milestone.open_issues)
        .bind(milestone.closed_issues)
        .bind(milestone.due_on.map(|v| v.to_rfc3339()))
        .bind(milestone.fetched_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn milestones(&self, repository_id: &str) -> Result<Vec<MilestoneSnapshot>> {
        let rows = sqlx::query(
            "SELECT * FROM milestones WHERE repository_id = ? ORDER BY COALESCE(due_on, fetched_at)",
        )
        .bind(repository_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| MilestoneSnapshot {
                id: row.get("id"),
                repository_id: row.get("repository_id"),
                title: row.get("title"),
                state: row.get("state"),
                open_issues: row.get("open_issues"),
                closed_issues: row.get("closed_issues"),
                due_on: row
                    .try_get::<Option<String>, _>("due_on")
                    .ok()
                    .flatten()
                    .map(parse_time),
                fetched_at: parse_time(row.get("fetched_at")),
            })
            .collect())
    }

    pub async fn save_github_project(&self, project: &GitHubProject) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO github_projects(id, account_id, owner_login, number, title,
               description, url, closed, synced_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
               title=excluded.title, description=excluded.description,
               url=excluded.url, closed=excluded.closed, synced_at=excluded.synced_at",
        )
        .bind(&project.id)
        .bind(&project.account_id)
        .bind(&project.owner_login)
        .bind(project.number)
        .bind(&project.title)
        .bind(&project.description)
        .bind(&project.url)
        .bind(project.closed as i64)
        .bind(project.synced_at.to_rfc3339())
        .execute(&mut *transaction)
        .await?;
        sqlx::query("DELETE FROM github_project_items WHERE project_id = ?")
            .bind(&project.id)
            .execute(&mut *transaction)
            .await?;
        for item in &project.items {
            sqlx::query(
                "INSERT INTO github_project_items(id, project_id, content_type, content_id,
                   title, state, url, repository_name, updated_at, synced_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&item.id)
            .bind(&project.id)
            .bind(&item.content_type)
            .bind(&item.content_id)
            .bind(&item.title)
            .bind(&item.state)
            .bind(&item.url)
            .bind(&item.repository_name)
            .bind(item.updated_at.map(|value| value.to_rfc3339()))
            .bind(project.synced_at.to_rfc3339())
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn codeowners(&self, repository_id: &str) -> Result<Vec<CodeownerRule>> {
        let row = sqlx::query_scalar::<_, String>(
            "SELECT content FROM knowledge_records
             WHERE repository_id = ? AND kind = 'codeowners'
             ORDER BY observed_at DESC LIMIT 1",
        )
        .bind(repository_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(content) = row else {
            return Ok(Vec::new());
        };
        Ok(parse_codeowners(&content))
    }

    pub async fn github_projects(&self, account_id: Option<&str>) -> Result<Vec<GitHubProject>> {
        let rows = if let Some(account_id) = account_id {
            sqlx::query(
                "SELECT * FROM github_projects WHERE account_id = ? ORDER BY synced_at DESC",
            )
            .bind(account_id)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query("SELECT * FROM github_projects ORDER BY synced_at DESC")
                .fetch_all(&self.pool)
                .await?
        };
        let mut projects = Vec::new();
        for row in rows {
            let id: String = row.get("id");
            let item_rows = sqlx::query(
                "SELECT * FROM github_project_items WHERE project_id = ? ORDER BY title",
            )
            .bind(&id)
            .fetch_all(&self.pool)
            .await?;
            projects.push(GitHubProject {
                id,
                account_id: row.get("account_id"),
                owner_login: row.get("owner_login"),
                number: row.get("number"),
                title: row.get("title"),
                description: row.get("description"),
                url: row.get("url"),
                closed: row.get::<i64, _>("closed") != 0,
                synced_at: parse_time(row.get("synced_at")),
                items: item_rows
                    .into_iter()
                    .map(|item| GitHubProjectItem {
                        id: item.get("id"),
                        content_type: item.get("content_type"),
                        content_id: item.get("content_id"),
                        title: item.get("title"),
                        state: item.get("state"),
                        url: item.get("url"),
                        repository_name: item.get("repository_name"),
                        updated_at: item
                            .try_get::<Option<String>, _>("updated_at")
                            .ok()
                            .flatten()
                            .map(parse_time),
                    })
                    .collect(),
            });
        }
        Ok(projects)
    }

    pub async fn replace_repository_branches(
        &self,
        repository_id: &str,
        branches: &[RepositoryBranch],
    ) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM repository_branches WHERE repository_id = ?")
            .bind(repository_id)
            .execute(&mut *transaction)
            .await?;
        for branch in branches.iter().take(200) {
            sqlx::query(
                "INSERT INTO repository_branches(repository_id, name, head_sha, protected, observed_at)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(repository_id)
            .bind(&branch.name)
            .bind(&branch.head_sha)
            .bind(branch.protected as i64)
            .bind(branch.observed_at.to_rfc3339())
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn repository_branches(&self, repository_id: &str) -> Result<Vec<RepositoryBranch>> {
        let rows = sqlx::query(
            "SELECT name, head_sha, protected, observed_at FROM repository_branches
             WHERE repository_id = ? ORDER BY name",
        )
        .bind(repository_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| RepositoryBranch {
                name: row.get("name"),
                head_sha: row.get("head_sha"),
                protected: row.get::<i64, _>("protected") != 0,
                observed_at: parse_time(row.get("observed_at")),
            })
            .collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_webhook_event(
        &self,
        delivery_id: &str,
        event: &str,
        action: Option<&str>,
        repository_full_name: Option<&str>,
        payload_sha256: &str,
        payload: &serde_json::Value,
    ) -> Result<bool> {
        let result = sqlx::query(
            "INSERT OR IGNORE INTO webhook_events(delivery_id, event, action,
               repository_full_name, received_at, payload_sha256, payload_json)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(delivery_id)
        .bind(event)
        .bind(action)
        .bind(repository_full_name)
        .bind(Utc::now().to_rfc3339())
        .bind(payload_sha256)
        .bind(serde_json::to_string(payload)?)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn webhook_events(&self, limit: i64) -> Result<Vec<WebhookEventRecord>> {
        let rows = sqlx::query("SELECT * FROM webhook_events ORDER BY received_at DESC LIMIT ?")
            .bind(limit.clamp(1, 500))
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| WebhookEventRecord {
                delivery_id: row.get("delivery_id"),
                event: row.get("event"),
                action: row.get("action"),
                repository_full_name: row.get("repository_full_name"),
                received_at: parse_time(row.get("received_at")),
                processed_at: row
                    .try_get::<Option<String>, _>("processed_at")
                    .ok()
                    .flatten()
                    .map(parse_time),
                payload: serde_json::from_str(&row.get::<String, _>("payload_json"))
                    .unwrap_or_default(),
            })
            .collect())
    }

    pub async fn mark_webhook_processed(&self, delivery_id: &str) -> Result<()> {
        sqlx::query("UPDATE webhook_events SET processed_at = ? WHERE delivery_id = ?")
            .bind(Utc::now().to_rfc3339())
            .bind(delivery_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn replace_security_alerts(
        &self,
        repository_id: &str,
        alerts: &[SecurityAlert],
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        // A successful endpoint read replaces the cached set for that source;
        // a partial/unavailable endpoint must not erase alert classes that were
        // merely not reachable.
        sqlx::query("DELETE FROM security_alerts WHERE repository_id = ? AND state = 'open'")
            .bind(repository_id)
            .execute(&mut *tx)
            .await?;
        for alert in alerts {
            sqlx::query(
                "INSERT INTO security_alerts(id, repository_id, kind, severity, title,
                  description, state, url, observed_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(repository_id, kind, id) DO UPDATE SET
                   severity=excluded.severity, title=excluded.title,
                   description=excluded.description, state=excluded.state,
                   url=excluded.url, observed_at=excluded.observed_at",
            )
            .bind(&alert.id)
            .bind(&alert.repository_id)
            .bind(&alert.kind)
            .bind(severity_str(alert.severity))
            .bind(&alert.title)
            .bind(&alert.description)
            .bind(&alert.state)
            .bind(&alert.url)
            .bind(alert.observed_at.to_rfc3339())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn security_alerts(&self, repository_id: &str) -> Result<Vec<SecurityAlert>> {
        let rows = sqlx::query(
            "SELECT * FROM security_alerts WHERE repository_id = ? AND state = 'open'
             ORDER BY CASE severity WHEN 'critical' THEN 0 WHEN 'high' THEN 1
               WHEN 'medium' THEN 2 WHEN 'low' THEN 3 ELSE 4 END",
        )
        .bind(repository_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| SecurityAlert {
                id: row.get("id"),
                repository_id: row.get("repository_id"),
                kind: row.get("kind"),
                severity: parse_severity(&row.get::<String, _>("severity")),
                title: row.get("title"),
                description: row.get("description"),
                state: row.get("state"),
                url: row.get("url"),
                observed_at: parse_time(row.get("observed_at")),
            })
            .collect())
    }

    pub async fn set_branch_protection(
        &self,
        repository_id: &str,
        admins_enforced: bool,
        required_checks: &[String],
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO branch_protection(repository_id, admins_enforced, required_checks, observed_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(repository_id) DO UPDATE SET admins_enforced=excluded.admins_enforced,
               required_checks=excluded.required_checks, observed_at=excluded.observed_at",
        )
        .bind(repository_id)
        .bind(admins_enforced as i64)
        .bind(serde_json::to_string(required_checks)?)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn upsert_commit(&self, commit: &Commit) -> Result<()> {
        sqlx::query(
            "INSERT INTO commits(
               id, repository_id, repository_name, sha, message, author_name, author_login,
               authored_at, committed_at, parents, additions, deletions, html_url, pr_number,
               verification, origin
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(repository_id, sha) DO UPDATE SET
               message=excluded.message, author_name=excluded.author_name,
               author_login=excluded.author_login, authored_at=excluded.authored_at,
               committed_at=excluded.committed_at, parents=excluded.parents,
               additions=excluded.additions, deletions=excluded.deletions,
               html_url=excluded.html_url, pr_number=excluded.pr_number,
               verification=excluded.verification, origin=excluded.origin",
        )
        .bind(&commit.id)
        .bind(&commit.repository_id)
        .bind(&commit.repository_name)
        .bind(&commit.sha)
        .bind(&commit.message)
        .bind(&commit.author_name)
        .bind(&commit.author_login)
        .bind(commit.authored_at.to_rfc3339())
        .bind(commit.committed_at.to_rfc3339())
        .bind(serde_json::to_string(&commit.parents)?)
        .bind(commit.additions)
        .bind(commit.deletions)
        .bind(&commit.html_url)
        .bind(commit.pr_number)
        .bind(&commit.verification)
        .bind(origin_str(&commit.origin))
        .execute(&self.pool)
        .await?;
        self.insert_search(
            "commit",
            &commit.id,
            &format!(
                "{} {}",
                &commit.sha[..commit.sha.len().min(8)],
                commit.message.lines().next().unwrap_or("")
            ),
            &commit.message,
            Some(&commit.repository_id),
        )
        .await?;
        Ok(())
    }

    pub async fn commits(&self, repository_id: Option<&str>, limit: usize) -> Result<Vec<Commit>> {
        let rows = if let Some(repo) = repository_id {
            sqlx::query(
                "SELECT * FROM commits WHERE repository_id = ? ORDER BY committed_at DESC LIMIT ?",
            )
            .bind(repo)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query("SELECT * FROM commits ORDER BY committed_at DESC LIMIT ?")
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
        };
        Ok(rows.into_iter().map(commit_from_row).collect())
    }

    pub async fn commit(&self, id: &str) -> Result<Commit> {
        let row = sqlx::query("SELECT * FROM commits WHERE id = ? OR sha = ?")
            .bind(id)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| GitPilotError::NotFound("commit".into()))?;
        Ok(commit_from_row(row))
    }

    pub async fn upsert_check(&self, check: &CheckRun) -> Result<()> {
        sqlx::query(
            "INSERT INTO checks(id, pull_request_id, commit_sha, name, state, conclusion,
              details_url, started_at, completed_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET state=excluded.state, conclusion=excluded.conclusion,
               details_url=excluded.details_url, completed_at=excluded.completed_at",
        )
        .bind(&check.id)
        .bind(&check.pull_request_id)
        .bind(&check.commit_sha)
        .bind(&check.name)
        .bind(check_str(check.state))
        .bind(&check.conclusion)
        .bind(&check.details_url)
        .bind(check.started_at.map(|v| v.to_rfc3339()))
        .bind(check.completed_at.map(|v| v.to_rfc3339()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn checks(&self, pr_id: &str) -> Result<Vec<CheckRun>> {
        let rows = sqlx::query("SELECT * FROM checks WHERE pull_request_id = ? ORDER BY name")
            .bind(pr_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| CheckRun {
                id: r.get("id"),
                pull_request_id: r.get("pull_request_id"),
                commit_sha: r.get("commit_sha"),
                name: r.get("name"),
                state: check_state(&r.get::<String, _>("state")),
                conclusion: r.get("conclusion"),
                details_url: r.get("details_url"),
                started_at: r
                    .try_get::<Option<String>, _>("started_at")
                    .ok()
                    .flatten()
                    .map(parse_time),
                completed_at: r
                    .try_get::<Option<String>, _>("completed_at")
                    .ok()
                    .flatten()
                    .map(parse_time),
            })
            .collect())
    }

    pub async fn upsert_review_comment(&self, comment: &ReviewComment) -> Result<()> {
        sqlx::query(
            "INSERT INTO review_comments(id, pull_request_id, author, body, path, line,
              diff_hunk, state, submitted_at, html_url)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET body=excluded.body, state=excluded.state",
        )
        .bind(&comment.id)
        .bind(&comment.pull_request_id)
        .bind(&comment.author)
        .bind(&comment.body)
        .bind(&comment.path)
        .bind(comment.line)
        .bind(&comment.diff_hunk)
        .bind(&comment.state)
        .bind(comment.submitted_at.to_rfc3339())
        .bind(&comment.html_url)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn review_comments(&self, pr_id: &str) -> Result<Vec<ReviewComment>> {
        let rows = sqlx::query(
            "SELECT * FROM review_comments WHERE pull_request_id = ? ORDER BY submitted_at",
        )
        .bind(pr_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| ReviewComment {
                id: r.get("id"),
                pull_request_id: r.get("pull_request_id"),
                author: r.get("author"),
                body: r.get("body"),
                path: r.get("path"),
                line: r.get("line"),
                diff_hunk: r.get("diff_hunk"),
                state: r.get("state"),
                submitted_at: parse_time(r.get("submitted_at")),
                html_url: r.get("html_url"),
            })
            .collect())
    }

    pub async fn save_pr_files(&self, pr_id: &str, files: &[DiffFile]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM pull_request_files WHERE pull_request_id = ?")
            .bind(pr_id)
            .execute(&mut *tx)
            .await?;
        for file in files {
            let (status, patch) = match file.status {
                DiffStatus::Added => ("added", file.raw.as_deref()),
                DiffStatus::Modified => ("modified", file.raw.as_deref()),
                DiffStatus::Deleted => ("deleted", file.raw.as_deref()),
                DiffStatus::Renamed => ("renamed", file.raw.as_deref()),
            };
            sqlx::query(
                "INSERT INTO pull_request_files(pull_request_id, path, previous_path, status,
                   additions, deletions, generated, patch, fetched_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(pr_id)
            .bind(&file.path)
            .bind(&file.old_path)
            .bind(status)
            .bind(file.additions)
            .bind(file.deletions)
            .bind(file.generated as i64)
            .bind(patch)
            .bind(Utc::now().to_rfc3339())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn pr_file_paths(&self, pr_id: &str) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar::<_, String>(
            "SELECT path FROM pull_request_files WHERE pull_request_id = ? ORDER BY path",
        )
        .bind(pr_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn pr_files(&self, pr_id: &str) -> Result<Vec<DiffFile>> {
        let rows = sqlx::query(
            "SELECT path, previous_path, status, additions, deletions, generated, patch
             FROM pull_request_files WHERE pull_request_id = ? ORDER BY path",
        )
        .bind(pr_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let path: String = r.get("path");
                let status = r.get::<String, _>("status");
                let raw: Option<String> = r.get("patch");
                let hunks = raw
                    .as_deref()
                    .map(|patch| {
                        crate::diff::parse_file_patch(
                            &path,
                            &status,
                            r.get::<Option<String>, _>("previous_path"),
                            patch,
                        )
                    })
                    .and_then(|result| result.ok())
                    .map(|file| file.hunks)
                    .unwrap_or_default();
                DiffFile {
                    path,
                    old_path: r.get("previous_path"),
                    status: match status.as_str() {
                        "added" => DiffStatus::Added,
                        "deleted" => DiffStatus::Deleted,
                        "renamed" => DiffStatus::Renamed,
                        _ => DiffStatus::Modified,
                    },
                    additions: r.get("additions"),
                    deletions: r.get("deletions"),
                    binary: raw.is_none(),
                    generated: r.get::<i64, _>("generated") != 0,
                    hunks,
                    raw,
                }
            })
            .collect())
    }

    pub async fn upsert_release(&self, release: &Release) -> Result<()> {
        sqlx::query(
            "INSERT INTO releases(id, repository_id, tag_name, name, body, draft, prerelease,
              published_at, created_at, html_url)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(repository_id, tag_name) DO UPDATE SET
               name=excluded.name, body=excluded.body, draft=excluded.draft,
               prerelease=excluded.prerelease, published_at=excluded.published_at,
               html_url=excluded.html_url",
        )
        .bind(&release.id)
        .bind(&release.repository_id)
        .bind(&release.tag_name)
        .bind(&release.name)
        .bind(&release.body)
        .bind(release.draft as i64)
        .bind(release.prerelease as i64)
        .bind(release.published_at.map(|v| v.to_rfc3339()))
        .bind(release.created_at.to_rfc3339())
        .bind(&release.html_url)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn releases(&self, repository_id: Option<&str>) -> Result<Vec<Release>> {
        let rows = if let Some(repo) = repository_id {
            sqlx::query("SELECT * FROM releases WHERE repository_id = ? ORDER BY COALESCE(published_at, created_at) DESC")
                .bind(repo).fetch_all(&self.pool).await?
        } else {
            sqlx::query("SELECT * FROM releases ORDER BY COALESCE(published_at, created_at) DESC")
                .fetch_all(&self.pool)
                .await?
        };
        Ok(rows
            .into_iter()
            .map(|r| Release {
                id: r.get("id"),
                repository_id: r.get("repository_id"),
                tag_name: r.get("tag_name"),
                name: r.get("name"),
                body: r.get("body"),
                draft: r.get::<i64, _>("draft") != 0,
                prerelease: r.get::<i64, _>("prerelease") != 0,
                published_at: r
                    .try_get::<Option<String>, _>("published_at")
                    .ok()
                    .flatten()
                    .map(parse_time),
                created_at: parse_time(r.get("created_at")),
                html_url: r.get("html_url"),
            })
            .collect())
    }

    pub async fn save_review_run(&self, run: &ReviewRun) -> Result<String> {
        let mut tx = self.pool.begin().await?;
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT id FROM review_runs WHERE pull_request_id = ? AND head_sha = ?",
        )
        .bind(&run.pull_request_id)
        .bind(&run.head_sha)
        .fetch_optional(&mut *tx)
        .await?;
        let id = existing.clone().unwrap_or_else(|| run.id.clone());
        sqlx::query(
            "INSERT INTO review_runs(id, pull_request_id, head_sha, status, provider, model,
              deterministic_findings, ai_findings, tokens, cost_usd, error, started_at, completed_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(pull_request_id, head_sha) DO UPDATE SET
               status=excluded.status, provider=excluded.provider, model=excluded.model,
               deterministic_findings=excluded.deterministic_findings,
               ai_findings=excluded.ai_findings, tokens=excluded.tokens,
               cost_usd=excluded.cost_usd, error=excluded.error,
               completed_at=excluded.completed_at",
        )
        .bind(&id)
        .bind(&run.pull_request_id)
        .bind(&run.head_sha)
        .bind(run_status(run.status))
        .bind(&run.provider)
        .bind(&run.model)
        .bind(run.deterministic_findings)
        .bind(run.ai_findings)
        .bind(run.tokens)
        .bind(run.cost_usd)
        .bind(&run.error)
        .bind(run.started_at.to_rfc3339())
        .bind(run.completed_at.map(|v| v.to_rfc3339()))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn save_findings(&self, findings: &[ReviewFinding]) -> Result<()> {
        for finding in findings {
            sqlx::query(
                "INSERT INTO review_findings(
                   id, run_id, pull_request_id, fingerprint, category, severity, confidence,
                   title, explanation, why_it_matters, evidence, path, diff_line,
                   suggested_remediation, relevant_tests, verification_status, lifecycle,
                   source, created_at
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(pull_request_id, fingerprint) DO UPDATE SET
                   run_id=excluded.run_id, severity=excluded.severity,
                   confidence=excluded.confidence, explanation=excluded.explanation,
                   evidence=excluded.evidence, verification_status=excluded.verification_status,
                   lifecycle=CASE WHEN review_findings.lifecycle IN ('dismissed','false_positive')
                     THEN review_findings.lifecycle ELSE excluded.lifecycle END",
            )
            .bind(&finding.id)
            .bind(&finding.run_id)
            .bind(&finding.pull_request_id)
            .bind(&finding.fingerprint)
            .bind(&finding.category)
            .bind(severity_str(finding.severity))
            .bind(finding.confidence)
            .bind(&finding.title)
            .bind(&finding.explanation)
            .bind(&finding.why_it_matters)
            .bind(serde_json::to_string(&finding.evidence)?)
            .bind(&finding.path)
            .bind(finding.diff_line)
            .bind(&finding.suggested_remediation)
            .bind(serde_json::to_string(&finding.relevant_tests)?)
            .bind(&finding.verification_status)
            .bind(lifecycle_str(finding.lifecycle))
            .bind(&finding.source)
            .bind(finding.created_at.to_rfc3339())
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    pub async fn findings(&self, pr_id: &str) -> Result<Vec<ReviewFinding>> {
        let rows = sqlx::query(
            "SELECT * FROM review_findings WHERE pull_request_id = ?
             ORDER BY CASE severity WHEN 'critical' THEN 0 WHEN 'high' THEN 1 WHEN 'medium' THEN 2 WHEN 'low' THEN 3 ELSE 4 END,
             created_at DESC",
        )
        .bind(pr_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(finding_from_row).collect())
    }

    pub async fn update_finding_lifecycle(
        &self,
        id: &str,
        lifecycle: FindingLifecycle,
    ) -> Result<()> {
        sqlx::query("UPDATE review_findings SET lifecycle = ? WHERE id = ?")
            .bind(lifecycle_str(lifecycle))
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn review_runs(&self, pr_id: &str) -> Result<Vec<ReviewRun>> {
        let rows = sqlx::query(
            "SELECT * FROM review_runs WHERE pull_request_id = ? ORDER BY started_at DESC",
        )
        .bind(pr_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| ReviewRun {
                id: r.get("id"),
                pull_request_id: r.get("pull_request_id"),
                head_sha: r.get("head_sha"),
                status: parse_run_status(&r.get::<String, _>("status")),
                provider: r.get("provider"),
                model: r.get("model"),
                deterministic_findings: r.get("deterministic_findings"),
                ai_findings: r.get("ai_findings"),
                tokens: r.get("tokens"),
                cost_usd: r.get("cost_usd"),
                error: r.get("error"),
                started_at: parse_time(r.get("started_at")),
                completed_at: r
                    .try_get::<Option<String>, _>("completed_at")
                    .ok()
                    .flatten()
                    .map(parse_time),
            })
            .collect())
    }

    pub async fn review_attempts(&self, pr_id: &str) -> Result<i64> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM review_runs WHERE pull_request_id = ?",
        )
        .bind(pr_id)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn review_for_head(&self, pr_id: &str, head_sha: &str) -> Result<Option<ReviewRun>> {
        let row = sqlx::query(
            "SELECT * FROM review_runs WHERE pull_request_id = ? AND head_sha = ?
             ORDER BY started_at DESC LIMIT 1",
        )
        .bind(pr_id)
        .bind(head_sha)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| ReviewRun {
            id: r.get("id"),
            pull_request_id: r.get("pull_request_id"),
            head_sha: r.get("head_sha"),
            status: parse_run_status(&r.get::<String, _>("status")),
            provider: r.get("provider"),
            model: r.get("model"),
            deterministic_findings: r.get("deterministic_findings"),
            ai_findings: r.get("ai_findings"),
            tokens: r.get("tokens"),
            cost_usd: r.get("cost_usd"),
            error: r.get("error"),
            started_at: parse_time(r.get("started_at")),
            completed_at: r
                .try_get::<Option<String>, _>("completed_at")
                .ok()
                .flatten()
                .map(parse_time),
        }))
    }

    pub async fn save_knowledge(&self, record: &KnowledgeRecord) -> Result<()> {
        let content_hash = knowledge_content_hash(&record.title, &record.content);
        let existing_hash: Option<String> = sqlx::query_scalar(
            "SELECT content_hash FROM knowledge_records WHERE repository_id IS ?
             AND source_object = ? AND source_version IS ? AND kind = ?",
        )
        .bind(&record.repository_id)
        .bind(&record.source_object)
        .bind(&record.source_version)
        .bind(&record.kind)
        .fetch_optional(&self.pool)
        .await?;
        sqlx::query(
            "INSERT INTO knowledge_records(id, repository_id, kind, title, content,
              source_object, source_version, source_url, confidence, extraction_method,
              explicit_fact, superseded_by, extracted_at, observed_at, content_hash)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(repository_id, source_object, source_version, kind) DO UPDATE SET
               title=excluded.title, content=excluded.content, source_url=excluded.source_url,
               confidence=excluded.confidence, extraction_method=excluded.extraction_method,
               explicit_fact=excluded.explicit_fact, extracted_at=excluded.extracted_at,
               observed_at=excluded.observed_at, content_hash=excluded.content_hash",
        )
        .bind(&record.id)
        .bind(&record.repository_id)
        .bind(&record.kind)
        .bind(&record.title)
        .bind(&record.content)
        .bind(&record.source_object)
        .bind(&record.source_version)
        .bind(&record.source_url)
        .bind(record.confidence)
        .bind(&record.extraction_method)
        .bind(record.explicit_fact as i64)
        .bind(&record.superseded_by)
        .bind(record.extracted_at.to_rfc3339())
        .bind(record.observed_at.to_rfc3339())
        .bind(&content_hash)
        .execute(&self.pool)
        .await?;
        let persisted_id: Option<String> = sqlx::query_scalar(
            "SELECT id FROM knowledge_records
             WHERE repository_id IS ? AND source_object = ? AND source_version IS ?
               AND kind = ? LIMIT 1",
        )
        .bind(&record.repository_id)
        .bind(&record.source_object)
        .bind(&record.source_version)
        .bind(&record.kind)
        .fetch_optional(&self.pool)
        .await?;
        if existing_hash.as_deref() != Some(content_hash.as_str())
            && let Some(id) = persisted_id.as_deref()
        {
            sqlx::query("DELETE FROM knowledge_embeddings WHERE record_id = ?")
                .bind(id)
                .execute(&self.pool)
                .await?;
        }
        self.insert_search(
            "knowledge",
            persisted_id.as_deref().unwrap_or(&record.id),
            &record.title,
            &record.content,
            record.repository_id.as_deref(),
        )
        .await?;
        Ok(())
    }

    pub async fn knowledge(
        &self,
        repository_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<KnowledgeRecord>> {
        let rows = if let Some(repo) = repository_id {
            sqlx::query("SELECT * FROM knowledge_records WHERE repository_id = ? AND superseded_by IS NULL ORDER BY observed_at DESC LIMIT ?")
                .bind(repo).bind(limit as i64).fetch_all(&self.pool).await?
        } else {
            sqlx::query("SELECT * FROM knowledge_records WHERE superseded_by IS NULL ORDER BY observed_at DESC LIMIT ?")
                .bind(limit as i64).fetch_all(&self.pool).await?
        };
        Ok(rows.into_iter().map(knowledge_from_row).collect())
    }

    pub async fn knowledge_by_id(&self, id: &str) -> Result<Option<KnowledgeRecord>> {
        let row = sqlx::query("SELECT * FROM knowledge_records WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(knowledge_from_row))
    }

    pub async fn knowledge_for_embedding(
        &self,
        repository_id: Option<&str>,
        provider_id: &str,
        model: &str,
        limit: usize,
    ) -> Result<Vec<KnowledgeRecord>> {
        let rows = if let Some(repository_id) = repository_id {
            sqlx::query(
                "SELECT k.* FROM knowledge_records k
                 WHERE k.repository_id = ? AND k.superseded_by IS NULL
                   AND NOT EXISTS (
                     SELECT 1 FROM knowledge_embeddings e
                     WHERE e.record_id = k.id AND e.provider_id = ? AND e.model = ?
                       AND e.content_hash = k.content_hash
                   )
                 ORDER BY k.observed_at DESC LIMIT ?",
            )
            .bind(repository_id)
            .bind(provider_id)
            .bind(model)
            .bind(limit.clamp(1, 512) as i64)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT k.* FROM knowledge_records k
                 WHERE k.superseded_by IS NULL
                   AND NOT EXISTS (
                     SELECT 1 FROM knowledge_embeddings e
                     WHERE e.record_id = k.id AND e.provider_id = ? AND e.model = ?
                       AND e.content_hash = k.content_hash
                   )
                 ORDER BY k.observed_at DESC LIMIT ?",
            )
            .bind(provider_id)
            .bind(model)
            .bind(limit.clamp(1, 512) as i64)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows.into_iter().map(knowledge_from_row).collect())
    }

    pub async fn save_knowledge_embedding(
        &self,
        record_id: &str,
        provider_id: &str,
        model: &str,
        vector: &[f32],
        content_hash: &str,
    ) -> Result<()> {
        if vector.is_empty() || vector.iter().any(|value| !value.is_finite()) {
            return Err(GitPilotError::Validation(
                "embedding vector is empty or non-finite".into(),
            ));
        }
        let mut bytes = Vec::with_capacity(vector.len() * 4);
        for value in vector {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        sqlx::query(
            "INSERT INTO knowledge_embeddings(record_id, provider_id, model, dimensions,
               vector, content_hash, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(record_id, provider_id, model) DO UPDATE SET
               dimensions=excluded.dimensions, vector=excluded.vector,
               content_hash=excluded.content_hash, created_at=excluded.created_at",
        )
        .bind(record_id)
        .bind(provider_id)
        .bind(model)
        .bind(vector.len() as i64)
        .bind(bytes)
        .bind(content_hash)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn knowledge_embeddings(
        &self,
        repository_id: Option<&str>,
        provider_id: &str,
        model: &str,
    ) -> Result<Vec<(KnowledgeRecord, Vec<f32>)>> {
        let rows = if let Some(repository_id) = repository_id {
            sqlx::query(
                "SELECT k.*, e.vector, e.dimensions, e.content_hash AS embedding_hash
                 FROM knowledge_records k
                 JOIN knowledge_embeddings e ON e.record_id = k.id
                 WHERE k.repository_id = ? AND k.superseded_by IS NULL
                   AND e.provider_id = ? AND e.model = ?
                 LIMIT 2000",
            )
            .bind(repository_id)
            .bind(provider_id)
            .bind(model)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT k.*, e.vector, e.dimensions, e.content_hash AS embedding_hash
                 FROM knowledge_records k
                 JOIN knowledge_embeddings e ON e.record_id = k.id
                 WHERE k.superseded_by IS NULL
                   AND e.provider_id = ? AND e.model = ?
                 LIMIT 2000",
            )
            .bind(provider_id)
            .bind(model)
            .fetch_all(&self.pool)
            .await?
        };
        let mut output = Vec::new();
        for row in rows {
            let content_hash: String = row.get("embedding_hash");
            let id: String = row.get("id");
            let repository_id: Option<String> = row.get("repository_id");
            let kind: String = row.get("kind");
            let title: String = row.get("title");
            let content: String = row.get("content");
            let source_object: String = row.get("source_object");
            let source_version: Option<String> = row.get("source_version");
            let source_url: Option<String> = row.get("source_url");
            let confidence: f64 = row.get("confidence");
            let extraction_method: String = row.get("extraction_method");
            let explicit_fact: i64 = row.get("explicit_fact");
            let superseded_by: Option<String> = row.get("superseded_by");
            let extracted_at: String = row.get("extracted_at");
            let observed_at: String = row.get("observed_at");
            let record = KnowledgeRecord {
                id,
                repository_id,
                kind,
                title,
                content,
                source_object,
                source_version,
                source_url,
                confidence,
                extraction_method,
                explicit_fact: explicit_fact != 0,
                superseded_by,
                extracted_at: parse_time(extracted_at),
                observed_at: parse_time(observed_at),
            };
            if content_hash != knowledge_content_hash(&record.title, &record.content) {
                continue;
            }
            let stored: Vec<u8> = row.get("vector");
            let dimensions: i64 = row.get("dimensions");
            if stored.len() != dimensions as usize * 4 {
                continue;
            }
            let vector = stored
                .as_chunks::<4>()
                .0
                .iter()
                .map(|bytes| f32::from_le_bytes(*bytes))
                .collect::<Vec<_>>();
            output.push((record, vector));
        }
        Ok(output)
    }

    pub async fn knowledge_embedding_count(&self) -> Result<i64> {
        Ok(
            sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_embeddings")
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn semantic_knowledge_search(
        &self,
        query_vector: &[f32],
        repository_id: Option<&str>,
        provider_id: &str,
        model: &str,
        limit: usize,
    ) -> Result<Vec<(KnowledgeRecord, f64)>> {
        let embeddings = self
            .knowledge_embeddings(repository_id, provider_id, model)
            .await?;
        let mut results = embeddings
            .into_iter()
            .map(|(record, vector)| {
                let similarity = cosine_similarity(query_vector, &vector);
                (record, similarity)
            })
            .filter(|(_, similarity)| *similarity > 0.0)
            .collect::<Vec<_>>();
        results.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit.clamp(1, 100));
        Ok(results)
    }

    pub async fn search(
        &self,
        query: &str,
        repository_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        if query.trim().is_empty() {
            return Ok(vec![]);
        }
        let safe_query = query
            .split_whitespace()
            .map(|word| format!("\"{}\"*", word.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" ");
        let rows = if let Some(repo) = repository_id {
            sqlx::query(
                "SELECT object_kind, object_id, title, body, repository_id FROM search_index
                 WHERE search_index MATCH ? AND repository_id = ? ORDER BY rank LIMIT ?",
            )
            .bind(&safe_query)
            .bind(repo)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT object_kind, object_id, title, body, repository_id FROM search_index
                 WHERE search_index MATCH ? ORDER BY rank LIMIT ?",
            )
            .bind(&safe_query)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(|r| SearchHit {
                kind: r.get("object_kind"),
                id: r.get("object_id"),
                title: r.get("title"),
                subtitle: r
                    .get::<String, _>("body")
                    .chars()
                    .take(120)
                    .collect::<String>(),
                repository_id: r.get("repository_id"),
            })
            .collect())
    }

    async fn insert_search(
        &self,
        kind: &str,
        id: &str,
        title: &str,
        body: &str,
        repository_id: Option<&str>,
    ) -> Result<()> {
        // Keep a relational source row and let the FTS table mirror it. The
        // primary key makes repeated ingestion idempotent while `rebuild_search`
        // can reconstruct the index after a migration or interrupted write.
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO search_documents(object_kind, object_id, title, body, repository_id)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(object_kind, object_id) DO UPDATE SET
               title=excluded.title, body=excluded.body, repository_id=excluded.repository_id",
        )
        .bind(kind)
        .bind(id)
        .bind(title)
        .bind(body)
        .bind(repository_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM search_index WHERE object_kind = ? AND object_id = ?")
            .bind(kind)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO search_index(object_kind, object_id, title, body, repository_id)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(kind)
        .bind(id)
        .bind(title)
        .bind(body)
        .bind(repository_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn rebuild_search(&self) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM search_index")
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO search_index(object_kind, object_id, title, body, repository_id)
             SELECT object_kind, object_id, title, body, repository_id FROM search_documents",
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn search_diagnostic(&self) -> Result<String> {
        let row = sqlx::query(
            "SELECT COUNT(*) AS documents,
                    (SELECT COUNT(*) FROM search_index) AS indexed",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(format!(
            "{} source documents, {} indexed rows",
            row.get::<i64, _>("documents"),
            row.get::<i64, _>("indexed")
        ))
    }

    pub async fn audit(&self, event: &AuditEvent) -> Result<()> {
        sqlx::query(
            "INSERT INTO audit_events(id, occurred_at, actor, action, target, outcome, metadata)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&event.id)
        .bind(event.occurred_at.to_rfc3339())
        .bind(&event.actor)
        .bind(&event.action)
        .bind(&event.target)
        .bind(&event.outcome)
        .bind(serde_json::to_string(&event.metadata)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn audit_events(&self, limit: usize) -> Result<Vec<AuditEvent>> {
        let rows = sqlx::query("SELECT * FROM audit_events ORDER BY occurred_at DESC LIMIT ?")
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| AuditEvent {
                id: r.get("id"),
                occurred_at: parse_time(r.get("occurred_at")),
                actor: r.get("actor"),
                action: r.get("action"),
                target: r.get("target"),
                outcome: r.get("outcome"),
                metadata: serde_json::from_str(&r.get::<String, _>("metadata"))
                    .unwrap_or(serde_json::Value::Null),
            })
            .collect())
    }

    pub async fn add_notification(&self, notification: &Notification) -> Result<bool> {
        let result = sqlx::query(
            "INSERT OR IGNORE INTO notifications(id, repository_id, kind, severity, title, body,
              created_at, read_at, snoozed_until, dedupe_key, url)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&notification.id)
        .bind(&notification.repository_id)
        .bind(&notification.kind)
        .bind(severity_str(notification.severity))
        .bind(&notification.title)
        .bind(&notification.body)
        .bind(notification.created_at.to_rfc3339())
        .bind(notification.read_at.map(|v| v.to_rfc3339()))
        .bind(notification.snoozed_until.map(|v| v.to_rfc3339()))
        .bind(&notification.dedupe_key)
        .bind(&notification.url)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn notifications(&self, unread_only: bool) -> Result<Vec<Notification>> {
        let rows = if unread_only {
            sqlx::query("SELECT * FROM notifications WHERE read_at IS NULL AND (snoozed_until IS NULL OR snoozed_until <= ?) ORDER BY created_at DESC LIMIT 200")
                .bind(Utc::now().to_rfc3339())
                .fetch_all(&self.pool).await?
        } else {
            sqlx::query("SELECT * FROM notifications ORDER BY created_at DESC LIMIT 200")
                .fetch_all(&self.pool)
                .await?
        };
        Ok(rows
            .into_iter()
            .map(|r| Notification {
                id: r.get("id"),
                repository_id: r.get("repository_id"),
                kind: r.get("kind"),
                severity: parse_severity(&r.get::<String, _>("severity")),
                title: r.get("title"),
                body: r.get("body"),
                created_at: parse_time(r.get("created_at")),
                read_at: r
                    .try_get::<Option<String>, _>("read_at")
                    .ok()
                    .flatten()
                    .map(parse_time),
                snoozed_until: r
                    .try_get::<Option<String>, _>("snoozed_until")
                    .ok()
                    .flatten()
                    .map(parse_time),
                dedupe_key: r.get("dedupe_key"),
                url: r.get("url"),
            })
            .collect())
    }

    pub async fn mark_notification_read(&self, id: &str) -> Result<()> {
        sqlx::query("UPDATE notifications SET read_at = ? WHERE id = ?")
            .bind(Utc::now().to_rfc3339())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn snooze_notification(&self, id: &str, until: DateTime<Utc>) -> Result<()> {
        sqlx::query("UPDATE notifications SET snoozed_until = ? WHERE id = ?")
            .bind(until.to_rfc3339())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn providers(&self) -> Result<Vec<ProviderConfig>> {
        let rows = sqlx::query("SELECT * FROM provider_configs ORDER BY display_name")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| ProviderConfig {
                id: r.get("id"),
                kind: r.get("kind"),
                display_name: r.get("display_name"),
                endpoint: r.get("endpoint"),
                model: r.get("model"),
                enabled: r.get::<i64, _>("enabled") != 0,
                has_credential: false,
                local_only: r.get::<i64, _>("local_only") != 0,
                input_cost_per_million: r.get("input_cost_per_million"),
                output_cost_per_million: r.get("output_cost_per_million"),
                context_limit: r.get("context_limit"),
                embedding_model: r
                    .try_get::<Option<String>, _>("embedding_model")
                    .ok()
                    .flatten(),
            })
            .collect())
    }

    pub async fn save_provider(&self, provider: &ProviderConfig) -> Result<()> {
        sqlx::query(
            "INSERT INTO provider_configs(id, kind, display_name, endpoint, model, enabled,
              local_only, input_cost_per_million, output_cost_per_million, context_limit,
              embedding_model, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET kind=excluded.kind, display_name=excluded.display_name,
               endpoint=excluded.endpoint, model=excluded.model, enabled=excluded.enabled,
               local_only=excluded.local_only, input_cost_per_million=excluded.input_cost_per_million,
               output_cost_per_million=excluded.output_cost_per_million,
               context_limit=excluded.context_limit,
               embedding_model=excluded.embedding_model",
        )
        .bind(&provider.id)
        .bind(&provider.kind)
        .bind(&provider.display_name)
        .bind(&provider.endpoint)
        .bind(&provider.model)
        .bind(provider.enabled as i64)
        .bind(provider.local_only as i64)
        .bind(provider.input_cost_per_million)
        .bind(provider.output_cost_per_million)
        .bind(provider.context_limit)
        .bind(&provider.embedding_model)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn routing_policies(&self) -> Result<Vec<RoutingPolicy>> {
        let rows = sqlx::query("SELECT * FROM routing_policies ORDER BY task")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| RoutingPolicy {
                task: r.get("task"),
                primary_provider_id: r.get("primary_provider_id"),
                fallback_provider_id: r.get("fallback_provider_id"),
                max_cost_usd: r.get("max_cost_usd"),
                max_input_tokens: r.get("max_input_tokens"),
            })
            .collect())
    }

    pub async fn save_routing_policy(&self, policy: &RoutingPolicy) -> Result<()> {
        sqlx::query(
            "INSERT INTO routing_policies(task, primary_provider_id, fallback_provider_id,
              max_cost_usd, max_input_tokens) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(task) DO UPDATE SET primary_provider_id=excluded.primary_provider_id,
               fallback_provider_id=excluded.fallback_provider_id,
               max_cost_usd=excluded.max_cost_usd, max_input_tokens=excluded.max_input_tokens",
        )
        .bind(&policy.task)
        .bind(&policy.primary_provider_id)
        .bind(&policy.fallback_provider_id)
        .bind(policy.max_cost_usd)
        .bind(policy.max_input_tokens)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn record_usage(&self, usage: &ProviderUsage) -> Result<()> {
        sqlx::query(
            "INSERT INTO provider_usage(id, provider, model, task, input_tokens, output_tokens,
              latency_ms, retries, estimated_cost_usd, fallback_used, cancelled, succeeded, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&usage.provider)
        .bind(&usage.model)
        .bind(&usage.task)
        .bind(usage.input_tokens)
        .bind(usage.output_tokens)
        .bind(usage.latency_ms)
        .bind(usage.retries)
        .bind(usage.estimated_cost_usd)
        .bind(usage.fallback_used as i64)
        .bind(usage.cancelled as i64)
        .bind(usage.succeeded as i64)
        .bind(usage.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn usage_summary(&self) -> Result<(i64, i64, f64)> {
        let row = sqlx::query(
            "SELECT COALESCE(SUM(input_tokens+output_tokens),0) as tokens,
             COUNT(*) as requests, CAST(COALESCE(SUM(estimated_cost_usd),0) AS REAL) as cost
             FROM provider_usage",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok((
            row.get::<i64, _>("tokens"),
            row.get("requests"),
            row.get::<f64, _>("cost"),
        ))
    }

    pub async fn save_automation(&self, automation: &Automation) -> Result<()> {
        sqlx::query(
            "INSERT INTO automations(id, name, enabled, trigger, conditions, actions, dry_run,
              requires_approval, repository_id, policy, budget_usd, timeout_seconds, created_at, last_run_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, enabled=excluded.enabled,
               trigger=excluded.trigger, conditions=excluded.conditions, actions=excluded.actions,
               dry_run=excluded.dry_run, requires_approval=excluded.requires_approval,
               repository_id=excluded.repository_id, policy=excluded.policy,
               budget_usd=excluded.budget_usd, timeout_seconds=excluded.timeout_seconds,
               last_run_at=excluded.last_run_at",
        )
        .bind(&automation.id)
        .bind(&automation.name)
        .bind(automation.enabled as i64)
        .bind(&automation.trigger)
        .bind(serde_json::to_string(&automation.conditions)?)
        .bind(serde_json::to_string(&automation.actions)?)
        .bind(automation.dry_run as i64)
        .bind(automation.requires_approval as i64)
        .bind(&automation.repository_id)
        .bind(serde_json::to_string(&automation.policy)?)
        .bind(automation.budget_usd)
        .bind(automation.timeout_seconds)
        .bind(automation.created_at.to_rfc3339())
        .bind(automation.last_run_at.map(|v| v.to_rfc3339()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn automations(&self) -> Result<Vec<Automation>> {
        let rows = sqlx::query("SELECT * FROM automations ORDER BY name")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| Automation {
                id: r.get("id"),
                name: r.get("name"),
                enabled: r.get::<i64, _>("enabled") != 0,
                trigger: r.get("trigger"),
                conditions: serde_json::from_str(&r.get::<String, _>("conditions"))
                    .unwrap_or_default(),
                actions: serde_json::from_str(&r.get::<String, _>("actions")).unwrap_or_default(),
                dry_run: r.get::<i64, _>("dry_run") != 0,
                requires_approval: r.get::<i64, _>("requires_approval") != 0,
                repository_id: r.get("repository_id"),
                policy: serde_json::from_str(&r.get::<String, _>("policy")).unwrap_or_default(),
                budget_usd: r.get("budget_usd"),
                timeout_seconds: r.get("timeout_seconds"),
                created_at: parse_time(r.get("created_at")),
                last_run_at: r
                    .try_get::<Option<String>, _>("last_run_at")
                    .ok()
                    .flatten()
                    .map(parse_time),
            })
            .collect())
    }

    pub async fn record_automation_run(&self, run: &AutomationRun) -> Result<bool> {
        let result = sqlx::query(
            "INSERT OR IGNORE INTO automation_runs(id, automation_id, event_key, status,
              summary, started_at, completed_at, error) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&run.id)
        .bind(&run.automation_id)
        .bind(&run.event_key)
        .bind(&run.status)
        .bind(&run.summary)
        .bind(run.started_at.to_rfc3339())
        .bind(run.completed_at.map(|v| v.to_rfc3339()))
        .bind(&run.error)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn automation_runs(&self) -> Result<Vec<AutomationRun>> {
        let rows = sqlx::query("SELECT * FROM automation_runs ORDER BY started_at DESC LIMIT 200")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| AutomationRun {
                id: r.get("id"),
                automation_id: r.get("automation_id"),
                event_key: r.get("event_key"),
                status: r.get("status"),
                summary: r.get("summary"),
                started_at: parse_time(r.get("started_at")),
                completed_at: r
                    .try_get::<Option<String>, _>("completed_at")
                    .ok()
                    .flatten()
                    .map(parse_time),
                error: r.get("error"),
            })
            .collect())
    }

    pub async fn save_local_repository(&self, repo: &LocalRepository) -> Result<()> {
        sqlx::query(
            "INSERT INTO local_repositories(id, name, path, default_branch, connected_at, last_opened_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(path) DO UPDATE SET name=excluded.name, default_branch=excluded.default_branch,
               last_opened_at=excluded.last_opened_at",
        )
        .bind(&repo.id)
        .bind(&repo.name)
        .bind(&repo.path)
        .bind(&repo.default_branch)
        .bind(repo.connected_at.to_rfc3339())
        .bind(repo.last_opened_at.map(|v| v.to_rfc3339()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn local_repositories(&self) -> Result<Vec<LocalRepository>> {
        let rows = sqlx::query("SELECT * FROM local_repositories ORDER BY last_opened_at DESC")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| LocalRepository {
                id: r.get("id"),
                name: r.get("name"),
                path: r.get("path"),
                default_branch: r.get("default_branch"),
                connected_at: parse_time(r.get("connected_at")),
                last_opened_at: r
                    .try_get::<Option<String>, _>("last_opened_at")
                    .ok()
                    .flatten()
                    .map(parse_time),
            })
            .collect())
    }

    pub async fn save_view(&self, view: &SavedView) -> Result<()> {
        sqlx::query(
            "INSERT INTO saved_views(id, screen, name, query, filters, created_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(screen, name) DO UPDATE SET query=excluded.query, filters=excluded.filters",
        )
        .bind(&view.id)
        .bind(&view.screen)
        .bind(&view.name)
        .bind(&view.query)
        .bind(serde_json::to_string(&view.filters)?)
        .bind(view.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn saved_views(&self, screen: &str) -> Result<Vec<SavedView>> {
        let rows = sqlx::query("SELECT * FROM saved_views WHERE screen = ? ORDER BY name")
            .bind(screen)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| SavedView {
                id: r.get("id"),
                screen: r.get("screen"),
                name: r.get("name"),
                query: r.get("query"),
                filters: serde_json::from_str(&r.get::<String, _>("filters")).unwrap_or_default(),
                created_at: parse_time(r.get("created_at")),
            })
            .collect())
    }

    #[allow(clippy::too_many_arguments)] // This is a persistence projection, not public domain behavior.
    pub async fn save_ai_artifact(
        &self,
        object_kind: &str,
        object_id: &str,
        task: &str,
        provider: &str,
        model: &str,
        content: &str,
        evidence: &[EvidenceRef],
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO ai_artifacts(id, object_kind, object_id, task, provider, model, content, evidence, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(object_kind, object_id, task, provider, model) DO UPDATE SET
               content=excluded.content, evidence=excluded.evidence, created_at=excluded.created_at",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(object_kind)
        .bind(object_id)
        .bind(task)
        .bind(provider)
        .bind(model)
        .bind(content)
        .bind(serde_json::to_string(evidence)?)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn ai_artifact(
        &self,
        object_kind: &str,
        object_id: &str,
        task: &str,
    ) -> Result<Option<String>> {
        Ok(sqlx::query_scalar::<_, String>(
            "SELECT content FROM ai_artifacts WHERE object_kind = ? AND object_id = ? AND task = ?
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(object_kind)
        .bind(object_id)
        .bind(task)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn stats(&self) -> Result<StoreStats> {
        let row = sqlx::query(
            "SELECT
               (SELECT COUNT(*) FROM repositories) repositories,
               (SELECT COUNT(*) FROM pull_requests) pull_requests,
               (SELECT COUNT(*) FROM issues) issues,
               (SELECT COUNT(*) FROM commits) commits,
               (SELECT COUNT(*) FROM knowledge_records) knowledge",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(StoreStats {
            repositories: row.get("repositories"),
            pull_requests: row.get("pull_requests"),
            issues: row.get("issues"),
            commits: row.get("commits"),
            knowledge: row.get("knowledge"),
        })
    }

    pub async fn database_size_bytes(&self) -> Result<u64> {
        if self.path == Path::new(":memory:") {
            return Ok(0);
        }
        Ok(tokio::fs::metadata(&self.path)
            .await
            .map(|m| m.len())
            .unwrap_or(0))
    }

    pub async fn backup_to(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        sqlx::query("VACUUM INTO ?")
            .bind(path.to_string_lossy().to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn integrity_check(&self) -> Result<String> {
        Ok(sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_one(&self.pool)
            .await?)
    }

    pub async fn delete_all_user_data(&self) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for table in [
            "notifications",
            "webhook_events",
            "github_project_items",
            "github_projects",
            "repository_branches",
            "audit_events",
            "provider_usage",
            "ai_artifacts",
            "automation_runs",
            "automations",
            "jobs",
            "saved_views",
            "local_repositories",
            "conversation_messages",
            "conversations",
            "knowledge_records",
            "knowledge_embeddings",
            "review_findings",
            "review_runs",
            "review_comments",
            "checks",
            "commits",
            "releases",
            "issues",
            "milestones",
            "security_alerts",
            "branch_protection",
            "pull_requests",
            "repositories",
            "provider_configs",
            "routing_policies",
            "accounts",
            "app_settings",
            "search_documents",
            "search_index",
        ] {
            sqlx::query(&format!("DELETE FROM {table}"))
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn seed_demo_if_empty(&self) -> Result<bool> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM repositories")
            .fetch_one(&self.pool)
            .await?;
        if count > 0 {
            return Ok(false);
        }
        demo::seed(self).await?;
        self.set_setting("demo_seeded", "true").await?;
        Ok(true)
    }

    pub async fn recent_items(&self, limit: usize) -> Result<Vec<SearchHit>> {
        let rows = sqlx::query(
            "SELECT object_kind, object_id, title, body, repository_id
             FROM search_documents ORDER BY rowid DESC LIMIT ?",
        )
        .bind(limit.clamp(1, 50) as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| SearchHit {
                kind: row.get("object_kind"),
                id: row.get("object_id"),
                title: row.get("title"),
                subtitle: row.get::<String, _>("body").chars().take(100).collect(),
                repository_id: row.get("repository_id"),
            })
            .collect())
    }

    pub async fn prune_old_data(&self, older_than_days: i64) -> Result<()> {
        let cutoff = (Utc::now() - Duration::days(older_than_days.max(1))).to_rfc3339();
        sqlx::query("DELETE FROM audit_events WHERE occurred_at < ?")
            .bind(&cutoff)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM provider_usage WHERE created_at < ?")
            .bind(&cutoff)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM notifications WHERE created_at < ? AND read_at IS NOT NULL")
            .bind(&cutoff)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn enqueue_job(&self, kind: &str, payload: &serde_json::Value) -> Result<String> {
        // Collapse repeated synchronization requests for the same payload while
        // one is already queued or running. This prevents double-click and
        // retry storms from multiplying remote calls.
        let payload_json = serde_json::to_string(payload)?;
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT id FROM jobs WHERE kind = ? AND payload = ?
             AND state IN ('queued','running') LIMIT 1",
        )
        .bind(kind)
        .bind(&payload_json)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(existing) = existing {
            return Ok(existing);
        }
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO jobs(id, kind, payload, state, attempts, run_after, created_at, updated_at)
             VALUES (?, ?, ?, 'queued', 0, ?, ?, ?)",
        )
        .bind(&id)
        .bind(kind)
        .bind(payload_json)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn claim_job(&self) -> Result<Option<(String, String, serde_json::Value, i64)>> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT id, kind, payload, attempts FROM jobs
             WHERE state = 'queued' AND run_after <= ? ORDER BY created_at LIMIT 1",
        )
        .bind(Utc::now().to_rfc3339())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        let id: String = row.get("id");
        let attempts: i64 = row.get::<i64, _>("attempts") + 1;
        sqlx::query("UPDATE jobs SET state = 'running', attempts = ?, updated_at = ? WHERE id = ?")
            .bind(attempts)
            .bind(Utc::now().to_rfc3339())
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

    pub async fn finish_job(&self, id: &str, error: Option<&str>) -> Result<()> {
        sqlx::query("UPDATE jobs SET state = ?, error = ?, updated_at = ? WHERE id = ?")
            .bind(if error.is_some() {
                "failed"
            } else {
                "completed"
            })
            .bind(error)
            .bind(Utc::now().to_rfc3339())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn retry_job(&self, id: &str) -> Result<()> {
        sqlx::query("UPDATE jobs SET state = 'queued', run_after = ?, updated_at = ? WHERE id = ?")
            .bind((Utc::now() + Duration::seconds(5)).to_rfc3339())
            .bind(Utc::now().to_rfc3339())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn recover_jobs(&self) -> Result<()> {
        sqlx::query("UPDATE jobs SET state = 'queued', updated_at = ? WHERE state = 'running'")
            .bind(Utc::now().to_rfc3339())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn prune_old_jobs(&self, older_than_days: i64) -> Result<()> {
        let cutoff = (Utc::now() - Duration::days(older_than_days.max(1))).to_rfc3339();
        sqlx::query(
            "DELETE FROM jobs WHERE state IN ('completed','failed','cancelled')
             AND updated_at < ?",
        )
        .bind(cutoff)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn jobs(&self, limit: usize) -> Result<Vec<JobRecord>> {
        let rows = sqlx::query("SELECT * FROM jobs ORDER BY created_at DESC LIMIT ?")
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| JobRecord {
                id: r.get("id"),
                kind: r.get("kind"),
                state: r.get("state"),
                attempts: r.get("attempts"),
                created_at: parse_time(r.get("created_at")),
                updated_at: parse_time(r.get("updated_at")),
                error: r.get("error"),
            })
            .collect())
    }

    pub async fn cancel_job(&self, id: &str) -> Result<()> {
        sqlx::query("UPDATE jobs SET state = 'cancelled', updated_at = ? WHERE id = ?")
            .bind(Utc::now().to_rfc3339())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

async fn apply_migrations(pool: &SqlitePool) -> Result<()> {
    // Tests open many isolated in-memory databases. A process-local guard keeps
    // global migration setup deterministic without sharing a connection.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
           version INTEGER PRIMARY KEY,
           applied_at TEXT NOT NULL
         )",
    )
    .execute(pool)
    .await?;
    let version: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM schema_migrations")
        .fetch_one(pool)
        .await?;
    if version.unwrap_or(0) < 1 {
        sqlx::query(MIGRATION).execute(pool).await?;
    }
    let version: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM schema_migrations")
        .fetch_one(pool)
        .await?;
    if version.unwrap_or(0) < 2 {
        let column_exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('pull_requests') WHERE name = 'commit_count'",
        )
        .fetch_one(pool)
        .await?;
        if column_exists == 0 {
            sqlx::query(MIGRATION_2).execute(pool).await?;
        } else {
            sqlx::query(
                "INSERT OR IGNORE INTO schema_migrations(version, applied_at)
                 VALUES (2, datetime('now'))",
            )
            .execute(pool)
            .await?;
        }
    }
    let version: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM schema_migrations")
        .fetch_one(pool)
        .await?;
    if version.unwrap_or(0) < 3 {
        sqlx::query(MIGRATION_3).execute(pool).await?;
    }
    let version: Option<i64> = sqlx::query_scalar("SELECT MAX(version) FROM schema_migrations")
        .fetch_one(pool)
        .await?;
    if version.unwrap_or(0) < 4 {
        let knowledge_hash_exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('knowledge_records')
             WHERE name = 'content_hash'",
        )
        .fetch_one(pool)
        .await?;
        if knowledge_hash_exists == 0 {
            sqlx::query(
                "ALTER TABLE knowledge_records
                 ADD COLUMN content_hash TEXT NOT NULL DEFAULT ''",
            )
            .execute(pool)
            .await?;
        }
        let embedding_model_exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('provider_configs')
             WHERE name = 'embedding_model'",
        )
        .fetch_one(pool)
        .await?;
        if embedding_model_exists == 0 {
            sqlx::query("ALTER TABLE provider_configs ADD COLUMN embedding_model TEXT")
                .execute(pool)
                .await?;
        }
        sqlx::query(MIGRATION_4).execute(pool).await?;
    }
    Ok(())
}

/// Existing knowledge rows predate `content_hash`. Backfill them once so
/// embeddings are only trusted for the exact source content that produced them.
async fn backfill_knowledge_hashes(pool: &SqlitePool) -> Result<()> {
    let rows = sqlx::query(
        "SELECT id, title, content FROM knowledge_records
         WHERE content_hash = '' LIMIT 5000",
    )
    .fetch_all(pool)
    .await?;
    for row in rows {
        let id: String = row.get("id");
        let title: String = row.get("title");
        let content: String = row.get("content");
        sqlx::query("UPDATE knowledge_records SET content_hash = ? WHERE id = ?")
            .bind(knowledge_content_hash(&title, &content))
            .bind(id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

fn repo_from_row(r: sqlx::sqlite::SqliteRow) -> Repository {
    Repository {
        id: r.get("id"),
        account_id: r.get("account_id"),
        owner: r.get("owner"),
        name: r.get("name"),
        full_name: r.get("full_name"),
        description: r.get("description"),
        default_branch: r.get("default_branch"),
        private: r.get::<i64, _>("private") != 0,
        archived: r.get::<i64, _>("archived") != 0,
        stars: r.get("stars"),
        open_issues: r.get("open_issues"),
        html_url: r.get("html_url"),
        language: r.get("language"),
        pushed_at: r
            .try_get::<Option<String>, _>("pushed_at")
            .ok()
            .flatten()
            .map(parse_time),
        updated_at: parse_time(r.get("updated_at")),
        favorite: r.get::<i64, _>("favorite") != 0,
        workspace: r.get("workspace"),
        origin: parse_origin(&r.get::<String, _>("origin")),
    }
}

fn pr_from_row(r: sqlx::sqlite::SqliteRow) -> PullRequest {
    PullRequest {
        id: r.get("id"),
        repository_id: r.get("repository_id"),
        repository_name: r.get("repository_name"),
        number: r.get("number"),
        title: r.get("title"),
        body: r.get("body"),
        author: r.get("author"),
        state: r.get("state"),
        draft: r.get::<i64, _>("draft") != 0,
        merged: r.get::<i64, _>("merged") != 0,
        mergeable: parse_mergeable(&r.get::<String, _>("mergeable")),
        created_at: parse_time(r.get("created_at")),
        updated_at: parse_time(r.get("updated_at")),
        merged_at: r
            .try_get::<Option<String>, _>("merged_at")
            .ok()
            .flatten()
            .map(parse_time),
        base_ref: r.get("base_ref"),
        head_ref: r.get("head_ref"),
        base_sha: r.get("base_sha"),
        head_sha: r.get("head_sha"),
        additions: r.get("additions"),
        deletions: r.get("deletions"),
        changed_files: r.get("changed_files"),
        commit_count: r.get("commit_count"),
        review_state: parse_review(&r.get::<String, _>("review_state")),
        check_state: check_state(&r.get::<String, _>("check_state")),
        labels: serde_json::from_str(&r.get::<String, _>("labels")).unwrap_or_default(),
        assignees: serde_json::from_str(&r.get::<String, _>("assignees")).unwrap_or_default(),
        requested_reviewers: serde_json::from_str(&r.get::<String, _>("requested_reviewers"))
            .unwrap_or_default(),
        milestone: r.get("milestone"),
        html_url: r.get("html_url"),
        risk_score: r.get("risk_score"),
        synced_at: parse_time(r.get("synced_at")),
        origin: parse_origin(&r.get::<String, _>("origin")),
    }
}

fn issue_from_row(r: sqlx::sqlite::SqliteRow) -> Issue {
    Issue {
        id: r.get("id"),
        repository_id: r.get("repository_id"),
        repository_name: r.get("repository_name"),
        number: r.get("number"),
        title: r.get("title"),
        body: r.get("body"),
        author: r.get("author"),
        state: r.get("state"),
        created_at: parse_time(r.get("created_at")),
        updated_at: parse_time(r.get("updated_at")),
        closed_at: r
            .try_get::<Option<String>, _>("closed_at")
            .ok()
            .flatten()
            .map(parse_time),
        labels: serde_json::from_str(&r.get::<String, _>("labels")).unwrap_or_default(),
        assignees: serde_json::from_str(&r.get::<String, _>("assignees")).unwrap_or_default(),
        milestone: r.get("milestone"),
        comments: r.get("comments"),
        html_url: r.get("html_url"),
        origin: parse_origin(&r.get::<String, _>("origin")),
    }
}

fn commit_from_row(r: sqlx::sqlite::SqliteRow) -> Commit {
    Commit {
        id: r.get("id"),
        repository_id: r.get("repository_id"),
        repository_name: r.get("repository_name"),
        sha: r.get("sha"),
        message: r.get("message"),
        author_name: r.get("author_name"),
        author_login: r.get("author_login"),
        authored_at: parse_time(r.get("authored_at")),
        committed_at: parse_time(r.get("committed_at")),
        parents: serde_json::from_str(&r.get::<String, _>("parents")).unwrap_or_default(),
        additions: r.get("additions"),
        deletions: r.get("deletions"),
        html_url: r.get("html_url"),
        pr_number: r.get("pr_number"),
        verification: r.get("verification"),
        origin: parse_origin(&r.get::<String, _>("origin")),
    }
}

fn knowledge_from_row(r: sqlx::sqlite::SqliteRow) -> KnowledgeRecord {
    KnowledgeRecord {
        id: r.get("id"),
        repository_id: r.get("repository_id"),
        kind: r.get("kind"),
        title: r.get("title"),
        content: r.get("content"),
        source_object: r.get("source_object"),
        source_version: r.get("source_version"),
        source_url: r.get("source_url"),
        confidence: r.get("confidence"),
        extraction_method: r.get("extraction_method"),
        explicit_fact: r.get::<i64, _>("explicit_fact") != 0,
        superseded_by: r.get("superseded_by"),
        extracted_at: parse_time(r.get("extracted_at")),
        observed_at: parse_time(r.get("observed_at")),
    }
}

fn finding_from_row(r: sqlx::sqlite::SqliteRow) -> ReviewFinding {
    ReviewFinding {
        id: r.get("id"),
        run_id: r.get("run_id"),
        pull_request_id: r.get("pull_request_id"),
        fingerprint: r.get("fingerprint"),
        category: r.get("category"),
        severity: parse_severity(&r.get::<String, _>("severity")),
        confidence: r.get("confidence"),
        title: r.get("title"),
        explanation: r.get("explanation"),
        why_it_matters: r.get("why_it_matters"),
        evidence: serde_json::from_str(&r.get::<String, _>("evidence")).unwrap_or_default(),
        path: r.get("path"),
        diff_line: r.get("diff_line"),
        suggested_remediation: r.get("suggested_remediation"),
        relevant_tests: serde_json::from_str(&r.get::<String, _>("relevant_tests"))
            .unwrap_or_default(),
        verification_status: r.get("verification_status"),
        lifecycle: parse_lifecycle(&r.get::<String, _>("lifecycle")),
        source: r.get("source"),
        created_at: parse_time(r.get("created_at")),
    }
}

fn parse_time(value: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&value)
        .map(|v| v.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

/// Content hash used to invalidate embeddings when a knowledge source changes.
pub fn knowledge_content_hash(title: &str, content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    digest.update(title.as_bytes());
    digest.update([0]);
    digest.update(content.as_bytes());
    hex::encode(digest.finalize())
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> f64 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut left_norm = 0.0f64;
    let mut right_norm = 0.0f64;
    for (a, b) in left.iter().zip(right.iter()) {
        let a = f64::from(*a);
        let b = f64::from(*b);
        dot += a * b;
        left_norm += a * a;
        right_norm += b * b;
    }
    if left_norm == 0.0 || right_norm == 0.0 {
        return 0.0;
    }
    (dot / (left_norm.sqrt() * right_norm.sqrt())).clamp(-1.0, 1.0)
}

fn origin_str(origin: &DataOrigin) -> &'static str {
    match origin {
        DataOrigin::GitHub => "github",
        DataOrigin::Demo => "demo",
        DataOrigin::LocalGit => "local_git",
    }
}

fn parse_origin(value: &str) -> DataOrigin {
    match value {
        "demo" => DataOrigin::Demo,
        "local_git" => DataOrigin::LocalGit,
        _ => DataOrigin::GitHub,
    }
}

fn mergeable_str(value: MergeableState) -> &'static str {
    match value {
        MergeableState::Mergeable => "mergeable",
        MergeableState::Conflicting => "conflicting",
        MergeableState::Unknown => "unknown",
        MergeableState::Blocked => "blocked",
    }
}

fn parse_mergeable(value: &str) -> MergeableState {
    match value {
        "mergeable" => MergeableState::Mergeable,
        "conflicting" => MergeableState::Conflicting,
        "blocked" => MergeableState::Blocked,
        _ => MergeableState::Unknown,
    }
}

fn check_str(value: CheckState) -> &'static str {
    match value {
        CheckState::Success => "success",
        CheckState::Failure => "failure",
        CheckState::Pending => "pending",
        CheckState::Neutral => "neutral",
        CheckState::Missing => "missing",
    }
}

fn check_state(value: &str) -> CheckState {
    match value {
        "success" => CheckState::Success,
        "failure" => CheckState::Failure,
        "pending" => CheckState::Pending,
        "neutral" => CheckState::Neutral,
        _ => CheckState::Missing,
    }
}

fn review_str(value: ReviewState) -> &'static str {
    match value {
        ReviewState::Approved => "approved",
        ReviewState::ChangesRequested => "changes_requested",
        ReviewState::Pending => "pending",
        ReviewState::None => "none",
    }
}

fn parse_review(value: &str) -> ReviewState {
    match value {
        "approved" => ReviewState::Approved,
        "changes_requested" => ReviewState::ChangesRequested,
        "pending" => ReviewState::Pending,
        _ => ReviewState::None,
    }
}

fn severity_str(value: Severity) -> &'static str {
    match value {
        Severity::Info => "info",
        Severity::Low => "low",
        Severity::Medium => "medium",
        Severity::High => "high",
        Severity::Critical => "critical",
    }
}

fn parse_severity(value: &str) -> Severity {
    match value {
        "critical" => Severity::Critical,
        "high" => Severity::High,
        "medium" => Severity::Medium,
        "low" => Severity::Low,
        _ => Severity::Info,
    }
}

fn lifecycle_str(value: FindingLifecycle) -> &'static str {
    match value {
        FindingLifecycle::Confirmed => "confirmed",
        FindingLifecycle::Unverified => "unverified",
        FindingLifecycle::Fixed => "fixed",
        FindingLifecycle::Dismissed => "dismissed",
        FindingLifecycle::FalsePositive => "false_positive",
        FindingLifecycle::Reintroduced => "reintroduced",
    }
}

fn parse_lifecycle(value: &str) -> FindingLifecycle {
    match value {
        "confirmed" => FindingLifecycle::Confirmed,
        "fixed" => FindingLifecycle::Fixed,
        "dismissed" => FindingLifecycle::Dismissed,
        "false_positive" => FindingLifecycle::FalsePositive,
        "reintroduced" => FindingLifecycle::Reintroduced,
        _ => FindingLifecycle::Unverified,
    }
}

fn run_status(value: ReviewRunStatus) -> &'static str {
    match value {
        ReviewRunStatus::Queued => "queued",
        ReviewRunStatus::Running => "running",
        ReviewRunStatus::Completed => "completed",
        ReviewRunStatus::Failed => "failed",
        ReviewRunStatus::Cancelled => "cancelled",
    }
}

fn parse_run_status(value: &str) -> ReviewRunStatus {
    match value {
        "running" => ReviewRunStatus::Running,
        "completed" => ReviewRunStatus::Completed,
        "failed" => ReviewRunStatus::Failed,
        "cancelled" => ReviewRunStatus::Cancelled,
        _ => ReviewRunStatus::Queued,
    }
}

/// Parse GitHub's CODEOWNERS subset. Comments are ignored; the last matching
/// rule is the effective rule, matching GitHub's documented precedence.
pub fn parse_codeowners(content: &str) -> Vec<CodeownerRule> {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pattern = parts.next()?;
            let owners = parts
                .filter(|owner| owner.starts_with('@'))
                .map(|owner| owner.trim_start_matches('@').to_owned())
                .collect::<Vec<_>>();
            (!owners.is_empty()).then(|| CodeownerRule {
                pattern: pattern.into(),
                owners,
            })
        })
        .collect()
}

#[cfg(test)]
mod codeowner_tests {
    use super::*;

    #[test]
    fn codeowners_parses_rules_and_ignores_comments() {
        let rules =
            parse_codeowners("# owners\n/src/auth/** @alice @acme/security\n*.md @docs\ninvalid\n");
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].pattern, "/src/auth/**");
        assert_eq!(rules[0].owners, vec!["alice", "acme/security"]);
        assert_eq!(rules[1].owners, vec!["docs"]);
    }
}
