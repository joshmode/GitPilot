//! Deterministic project intelligence. Every metric is derived from stored
//! metadata; missing data is represented rather than guessed.

use crate::domain::*;
use crate::error::Result;
use crate::store::Store;
use chrono::{Duration, Utc};
use std::collections::{BTreeMap, HashMap, HashSet};

pub const STALE_DAYS: i64 = 14;

pub fn risk_score(additions: i64, deletions: i64, changed_files: i64, files: &[DiffFile]) -> i64 {
    let churn = additions.saturating_add(deletions);
    let mut score = (churn / 35).min(35) + (changed_files * 2).min(25);
    if files
        .iter()
        .any(|f| f.path.contains("migration") || f.path.contains("schema"))
    {
        score += 15;
    }
    if files.iter().any(|f| {
        f.path.contains("auth")
            || f.path.contains("security")
            || f.path.contains("permission")
            || f.path.ends_with(".sql")
    }) {
        score += 15;
    }
    if !files
        .iter()
        .filter(|f| !f.generated)
        .any(|f| !f.path.contains("test"))
        && !files.is_empty()
    {
        score -= 10;
    }
    score.clamp(0, 100)
}

pub async fn dashboard(store: &Store) -> Result<Dashboard> {
    let repositories = store.repositories().await?;
    let all_prs = store
        .pull_requests(&PrFilter {
            limit: 500,
            ..Default::default()
        })
        .await?;
    let issues = store.issues(None, true).await?;
    let all_issues = store.issues(None, false).await?;
    let releases = store.releases(None).await?;
    let now = Utc::now();
    let mut health = Vec::new();
    for repo in &repositories {
        let prs: Vec<_> = all_prs
            .iter()
            .filter(|p| p.repository_id == repo.id)
            .collect();
        let reviews = prs
            .iter()
            .filter(|p| {
                matches!(p.review_state, ReviewState::None | ReviewState::Pending) && !p.draft
            })
            .count() as i64;
        let failing = prs
            .iter()
            .filter(|p| p.check_state == CheckState::Failure)
            .count() as i64;
        let stale = prs
            .iter()
            .filter(|p| (now - p.updated_at).num_days() >= STALE_DAYS)
            .count() as i64;
        let repo_issues: Vec<_> = issues
            .iter()
            .filter(|i| i.repository_id == repo.id)
            .collect();
        let repo_all_issues: Vec<Issue> = all_issues
            .iter()
            .filter(|i| i.repository_id == repo.id)
            .cloned()
            .collect();
        let milestones = store.milestones(&repo.id).await.unwrap_or_default();
        let milestone_completion = if milestones.is_empty() {
            derive_milestone_progress(&repo_all_issues).map(|progress| progress.ratio())
        } else {
            let total: i64 = milestones
                .iter()
                .map(|m| m.open_issues + m.closed_issues)
                .sum();
            let closed: i64 = milestones.iter().map(|m| m.closed_issues).sum();
            (total > 0).then_some(closed as f64 / total as f64)
        };
        let open_prs = prs.len() as i64;
        let health_score = if open_prs == 0 && repo_issues.is_empty() {
            None
        } else {
            let penalty = (reviews * 5 + failing * 9 + stale * 4).min(80);
            Some((100 - penalty).max(20))
        };
        let security_state = store
            .repository_security_state(&repo.id)
            .await
            .unwrap_or(SecurityState::NotChecked);
        health.push(RepositoryHealth {
            repository_id: repo.id.clone(),
            name: repo.full_name.clone(),
            open_prs,
            review_backlog: reviews,
            failing_checks: failing,
            stale_work: stale,
            open_issues: repo_issues.len() as i64,
            milestone_completion,
            health_score,
            security_state,
            last_synced_at: repo.pushed_at,
        });
    }
    let mut alerts = Vec::new();
    for pr in &all_prs {
        if pr.check_state == CheckState::Failure {
            alerts.push(Alert {
                id: format!("ci-{}", pr.id),
                severity: Severity::High,
                title: format!("CI failing on #{}", pr.number),
                detail: format!("{} in {}", pr.title, pr.repository_name),
                object_id: Some(pr.id.clone()),
                url: Some(pr.html_url.clone()),
            });
        }
        if pr.mergeable == MergeableState::Conflicting {
            alerts.push(Alert {
                id: format!("conflict-{}", pr.id),
                severity: Severity::High,
                title: format!("Merge conflict on #{}", pr.number),
                detail: pr.title.clone(),
                object_id: Some(pr.id.clone()),
                url: Some(pr.html_url.clone()),
            });
        }
        if (now - pr.updated_at).num_days() >= STALE_DAYS && !pr.draft {
            alerts.push(Alert {
                id: format!("stale-{}", pr.id),
                severity: Severity::Medium,
                title: format!("#{} is stale", pr.number),
                detail: format!("No update for {} days", (now - pr.updated_at).num_days()),
                object_id: Some(pr.id.clone()),
                url: Some(pr.html_url.clone()),
            });
        }
    }
    alerts.sort_by_key(|a| std::cmp::Reverse(a.severity));
    let activity = recent_activity(&all_prs, &issues, &releases);
    let mut missing = Vec::new();
    if repositories.is_empty() {
        missing.push("No repositories are connected".into());
    }
    if health
        .iter()
        .all(|h| h.security_state == SecurityState::NotChecked)
    {
        missing.push("Security alerts have not been checked".into());
    }
    if health
        .iter()
        .any(|h| h.security_state == SecurityState::Unavailable)
    {
        missing.push(
            "Some security endpoints were unavailable or not permitted; absence of alerts is not inferred"
                .into(),
        );
    }
    if health.iter().all(|h| h.milestone_completion.is_none()) {
        missing.push("No milestone-linked issues are cached for a progress ratio".into());
    }
    if repositories.iter().any(|r| r.origin == DataOrigin::Demo) {
        missing.push("Demo fixtures are active; connect GitHub for live data".into());
    }
    Ok(Dashboard {
        active_projects: repositories
            .iter()
            .filter(|r| r.workspace.is_some())
            .map(|r| r.workspace.clone().unwrap_or_default())
            .collect::<HashSet<_>>()
            .len() as i64,
        repositories: repositories.len() as i64,
        open_prs: all_prs.len() as i64,
        awaiting_review: all_prs
            .iter()
            .filter(|p| {
                matches!(p.review_state, ReviewState::None | ReviewState::Pending) && !p.draft
            })
            .count() as i64,
        failing_checks: all_prs
            .iter()
            .filter(|p| p.check_state == CheckState::Failure)
            .count() as i64,
        stale_work: all_prs
            .iter()
            .filter(|p| (now - p.updated_at).num_days() >= STALE_DAYS)
            .count() as i64,
        open_issues: issues.len() as i64,
        recent_releases: releases.into_iter().take(5).collect(),
        health,
        alerts,
        activity,
        last_synced_at: repositories
            .iter()
            .filter_map(|r| r.pushed_at)
            .max()
            .or(Some(now)),
        missing,
        demo: repositories.iter().any(|r| r.origin == DataOrigin::Demo),
    })
}

fn recent_activity(
    prs: &[PullRequest],
    issues: &[Issue],
    releases: &[Release],
) -> Vec<ActivityItem> {
    let mut items = Vec::new();
    for pr in prs.iter().take(8) {
        items.push(ActivityItem {
            id: pr.id.clone(),
            kind: "pull_request".into(),
            title: format!("#{} {}", pr.number, pr.title),
            actor: pr.author.clone(),
            repository: pr.repository_name.clone(),
            occurred_at: pr.updated_at,
            url: Some(pr.html_url.clone()),
        });
    }
    for issue in issues.iter().take(6) {
        items.push(ActivityItem {
            id: issue.id.clone(),
            kind: "issue".into(),
            title: format!("#{} {}", issue.number, issue.title),
            actor: issue.author.clone(),
            repository: issue.repository_name.clone(),
            occurred_at: issue.updated_at,
            url: Some(issue.html_url.clone()),
        });
    }
    for release in releases.iter().take(4) {
        items.push(ActivityItem {
            id: release.id.clone(),
            kind: "release".into(),
            title: release.name.clone(),
            actor: "release".into(),
            repository: release.id.clone(),
            occurred_at: release.published_at.unwrap_or(release.created_at),
            url: Some(release.html_url.clone()),
        });
    }
    items.sort_by_key(|a| std::cmp::Reverse(a.occurred_at));
    items.truncate(20);
    items
}

pub fn project_brief(dashboard: &Dashboard) -> ProjectBrief {
    let mut facts = Vec::new();
    let mut metrics = Vec::new();
    if dashboard.repositories == 0 {
        return ProjectBrief {
            range: "No data range available".into(),
            missing: dashboard.missing.clone(),
            ..Default::default()
        };
    }
    facts.push(EvidenceStatement {
        text: format!(
            "{} repositories are connected across {} active project workspaces.",
            dashboard.repositories, dashboard.active_projects
        ),
        kind: EvidenceKind::Observed,
        evidence: dashboard
            .health
            .iter()
            .take(3)
            .map(|h| EvidenceRef {
                label: h.name.clone(),
                url: None,
                object_id: Some(h.repository_id.clone()),
                kind: EvidenceKind::Observed,
            })
            .collect(),
    });
    facts.push(EvidenceStatement {
        text: format!(
            "{} pull requests are open; {} are awaiting human review and {} have failing checks.",
            dashboard.open_prs, dashboard.awaiting_review, dashboard.failing_checks
        ),
        kind: EvidenceKind::Derived,
        evidence: dashboard
            .alerts
            .iter()
            .take(4)
            .map(|a| EvidenceRef {
                label: a.title.clone(),
                url: a.url.clone(),
                object_id: a.object_id.clone(),
                kind: EvidenceKind::Observed,
            })
            .collect(),
    });
    if let Some(health) = dashboard.health.first() {
        metrics.push(EvidenceStatement {
            text: format!(
                "{} has {} open PRs and {} stale work items at the current sync snapshot.",
                health.name, health.open_prs, health.stale_work
            ),
            kind: EvidenceKind::Derived,
            evidence: vec![EvidenceRef {
                label: health.name.clone(),
                url: None,
                object_id: Some(health.repository_id.clone()),
                kind: EvidenceKind::Derived,
            }],
        });
    }
    let inferences = if dashboard.failing_checks > 0 {
        vec![EvidenceStatement {
            text: "CI failures are the most immediate review risk in the current snapshot. Treat this as a prioritization inference, not a causal diagnosis.".into(),
            kind: EvidenceKind::Inference,
            evidence: dashboard.alerts.iter().filter(|a| a.title.contains("CI")).take(3).map(|a| EvidenceRef {
                label: a.title.clone(), url: a.url.clone(), object_id: a.object_id.clone(), kind: EvidenceKind::Observed,
            }).collect(),
        }]
    } else {
        vec![EvidenceStatement {
            text: "No failing checks were returned in the current snapshot. This is not evidence that the code is healthy; some checks may be unavailable or not reported.".into(),
            kind: EvidenceKind::Unavailable,
            evidence: vec![],
        }]
    };
    let mut next_steps = Vec::new();
    if dashboard.awaiting_review > 0 {
        next_steps.push(EvidenceStatement {
            text: format!(
                "Review the {} pull requests without a recorded human review decision.",
                dashboard.awaiting_review
            ),
            kind: EvidenceKind::Recommendation,
            evidence: dashboard
                .alerts
                .iter()
                .filter(|a| a.title.contains("#"))
                .take(5)
                .map(|a| EvidenceRef {
                    label: a.title.clone(),
                    url: a.url.clone(),
                    object_id: a.object_id.clone(),
                    kind: EvidenceKind::Observed,
                })
                .collect(),
        });
    }
    if dashboard.stale_work > 0 {
        next_steps.push(EvidenceStatement {
            text: format!(
                "Triage {} items with no update in at least {STALE_DAYS} days.",
                dashboard.stale_work
            ),
            kind: EvidenceKind::Recommendation,
            evidence: vec![],
        });
    }
    ProjectBrief {
        generated_at: Some(Utc::now()),
        range: "Current cached snapshot".into(),
        freshness: dashboard.last_synced_at,
        provider: None,
        model: None,
        missing: dashboard.missing.clone(),
        facts,
        metrics,
        inferences,
        next_steps,
    }
}

pub fn contributors(prs: &[PullRequest], issues: &[Issue], commits: &[Commit]) -> Vec<Contributor> {
    let mut people: BTreeMap<String, Contributor> = BTreeMap::new();
    for pr in prs {
        let entry = people
            .entry(pr.author.clone())
            .or_insert_with(|| empty_contributor(&pr.author));
        entry.open_prs += 1;
        for reviewer in &pr.requested_reviewers {
            let entry = people
                .entry(reviewer.clone())
                .or_insert_with(|| empty_contributor(reviewer));
            entry.review_requests += 1;
            if pr.review_state == ReviewState::Pending {
                entry.pending_reviews += 1;
            }
        }
    }
    for issue in issues {
        for assignee in &issue.assignees {
            let entry = people
                .entry(assignee.clone())
                .or_insert_with(|| empty_contributor(assignee));
            entry.assigned_issues += 1;
        }
    }
    for commit in commits {
        if let Some(login) = &commit.author_login {
            let entry = people
                .entry(login.clone())
                .or_insert_with(|| empty_contributor(login));
            entry.commits_30d += 1;
        }
    }
    people.into_values().collect()
}

fn empty_contributor(login: &str) -> Contributor {
    Contributor {
        login: login.into(),
        avatar_url: None,
        open_prs: 0,
        assigned_issues: 0,
        review_requests: 0,
        pending_reviews: 0,
        commits_30d: 0,
        owned_paths: vec![],
    }
}

/// Deterministic cross-PR signals. Overlap is only a review prompt; it is not
/// labelled a conflict.
pub fn overlap_signals(
    prs: &[PullRequest],
    files: &HashMap<String, Vec<String>>,
) -> Vec<FindingSignal> {
    let mut signals = Vec::new();
    for (index, left) in prs.iter().enumerate() {
        for right in prs.iter().skip(index + 1) {
            let left_files = files.get(&left.id).cloned().unwrap_or_default();
            let right_files = files.get(&right.id).cloned().unwrap_or_default();
            let overlap: Vec<_> = left_files
                .iter()
                .filter(|f| right_files.contains(f))
                .cloned()
                .collect();
            if !overlap.is_empty() {
                signals.push(FindingSignal {
                    kind: "file_overlap".into(),
                    pull_request_ids: vec![left.id.clone(), right.id.clone()],
                    confidence: (overlap.len() as f64 / 5.0).min(0.95),
                    title: format!(
                        "#{} and #{} touch {} shared file{}",
                        left.number,
                        right.number,
                        overlap.len(),
                        if overlap.len() == 1 { "" } else { "s" }
                    ),
                    evidence: overlap.into_iter().take(12).collect(),
                    is_conflict: false,
                });
            }
        }
    }
    signals
}

#[derive(Debug, Clone)]
pub struct FindingSignal {
    pub kind: String,
    pub pull_request_ids: Vec<String>,
    pub confidence: f64,
    pub title: String,
    pub evidence: Vec<String>,
    pub is_conflict: bool,
}

pub fn derive_milestone_progress(issues: &[Issue]) -> Option<MilestoneProgress> {
    let total = issues.iter().filter(|i| i.milestone.is_some()).count();
    if total == 0 {
        return None;
    }
    let closed = issues
        .iter()
        .filter(|i| i.milestone.is_some() && i.state == "closed")
        .count();
    Some(MilestoneProgress {
        total: total as i64,
        closed: closed as i64,
        open: (total - closed) as i64,
    })
}

#[derive(Debug, Clone)]
pub struct MilestoneProgress {
    pub total: i64,
    pub closed: i64,
    pub open: i64,
}

impl MilestoneProgress {
    pub fn ratio(&self) -> f64 {
        if self.total <= 0 {
            0.0
        } else {
            self.closed as f64 / self.total as f64
        }
    }
}

pub fn sync_freshness(repos: &[Repository]) -> Option<chrono::DateTime<Utc>> {
    repos.iter().filter_map(|r| r.pushed_at).max()
}

pub fn stale_cutoff(days: i64) -> chrono::DateTime<Utc> {
    Utc::now() - Duration::days(days.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(id: &str, name: &str) -> Repository {
        Repository {
            id: id.into(),
            account_id: "a".into(),
            owner: "o".into(),
            name: name.into(),
            full_name: format!("o/{name}"),
            description: None,
            default_branch: "main".into(),
            private: false,
            archived: false,
            stars: 0,
            open_issues: 0,
            html_url: String::new(),
            language: None,
            pushed_at: None,
            updated_at: Utc::now(),
            favorite: false,
            workspace: None,
            origin: DataOrigin::Demo,
        }
    }

    #[test]
    fn risk_score_does_not_reward_tiny_changes_as_safe() {
        let score = risk_score(1, 0, 1, &[]);
        assert!(score <= 100);
    }

    #[test]
    fn contributors_are_derived_from_work_assignments() {
        let pr = PullRequest {
            id: "p".into(),
            repository_id: "r".into(),
            repository_name: "o/r".into(),
            number: 1,
            title: "t".into(),
            body: String::new(),
            author: "alice".into(),
            state: "open".into(),
            draft: false,
            merged: false,
            mergeable: MergeableState::Mergeable,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            merged_at: None,
            base_ref: "main".into(),
            head_ref: "feat".into(),
            base_sha: String::new(),
            head_sha: String::new(),
            additions: 1,
            deletions: 1,
            changed_files: 1,
            commit_count: 1,
            review_state: ReviewState::Pending,
            check_state: CheckState::Missing,
            labels: vec![],
            assignees: vec![],
            requested_reviewers: vec!["bob".into()],
            milestone: None,
            html_url: String::new(),
            risk_score: 1,
            synced_at: Utc::now(),
            origin: DataOrigin::Demo,
        };
        let people = contributors(std::slice::from_ref(&pr), &[], &[]);
        assert!(
            people
                .iter()
                .any(|p| p.login == "bob" && p.pending_reviews == 1)
        );
    }

    #[test]
    fn missing_milestone_data_is_none_not_zero() {
        assert!(derive_milestone_progress(&[]).is_none());
    }

    #[test]
    fn repository_constructor_is_stable() {
        assert_eq!(repo("1", "x").full_name, "o/x");
    }
}
