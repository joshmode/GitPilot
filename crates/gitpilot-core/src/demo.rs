//! Deterministic, visibly synthetic fixtures used by the desktop demo profile.

use crate::domain::*;
use crate::error::Result;
use crate::store::Store;
use chrono::{Duration, Utc};
use uuid::Uuid;

fn id(prefix: &str, n: usize) -> String {
    format!("{prefix}-{n:04}")
}

pub async fn seed(store: &Store) -> Result<()> {
    let now = Utc::now();
    let account_id = id("account", 1);
    store
        .upsert_account(&Account {
            id: account_id.clone(),
            login: "maya".into(),
            host: "github.com".into(),
            avatar_url: None,
            connected_at: now - Duration::days(180),
            last_validated_at: Some(now),
        })
        .await?;

    let repos = [
        (
            "acme/control-plane",
            "Control Plane",
            "Authentication, tenancy and policy APIs for the platform.",
            "Rust",
            148,
        ),
        (
            "acme/web-console",
            "Web Console",
            "Operator-facing web console and shared design system.",
            "TypeScript",
            62,
        ),
        (
            "acme/telemetry",
            "Telemetry Pipeline",
            "Ingestion, enrichment and retention for product telemetry.",
            "Go",
            91,
        ),
    ];
    for (index, (full_name, name, description, language, stars)) in repos.iter().enumerate() {
        let repo_id = id("repo", index + 1);
        let (owner, repo_name) = full_name.split_once('/').unwrap();
        store
            .upsert_repository(&Repository {
                id: repo_id.clone(),
                account_id: account_id.clone(),
                owner: owner.into(),
                name: repo_name.into(),
                full_name: (*full_name).into(),
                description: Some((*description).into()),
                default_branch: "main".into(),
                private: index == 0,
                archived: false,
                stars: *stars,
                open_issues: 11 + index as i64 * 5,
                html_url: format!("https://github.com/{full_name}"),
                language: Some((*language).into()),
                pushed_at: Some(now - Duration::hours(index as i64 * 9 + 1)),
                updated_at: now - Duration::minutes(index as i64 * 33),
                favorite: index == 0,
                workspace: Some(if index < 2 {
                    "Platform".into()
                } else {
                    "Observability".into()
                }),
                origin: DataOrigin::Demo,
            })
            .await?;
        store
            .mark_repository_synced(&repo_id, now - Duration::minutes(4))
            .await?;
        store
            .set_security_state(
                &repo_id,
                if index == 2 {
                    SecurityState::FindingsPresent
                } else {
                    SecurityState::Unavailable
                },
            )
            .await?;
        let _ = name;
    }

    let pr_fixtures = [
        (
            "repo-0001",
            "acme/control-plane",
            412,
            "Harden tenant boundary checks for workspace invites",
            "Invitations can currently be created for a workspace the caller only has read access to. This change resolves the effective member role before creating the invite.",
            "maya",
            CheckState::Failure,
            ReviewState::ChangesRequested,
            MergeableState::Mergeable,
            false,
            3,
            182,
            44,
            vec!["security", "auth"],
            vec!["jon"],
            83,
        ),
        (
            "repo-0001",
            "acme/control-plane",
            419,
            "Add idempotency keys to policy updates",
            "Policy writes now accept an idempotency key stored with the revision. A duplicate request returns the first result instead of creating a second revision.",
            "devin",
            CheckState::Success,
            ReviewState::Approved,
            MergeableState::Mergeable,
            false,
            6,
            96,
            21,
            vec!["api", "reliability"],
            vec![],
            34,
        ),
        (
            "repo-0002",
            "acme/web-console",
            288,
            "Fix focus loss when refreshing the diff panel",
            "The diff pane remounts on every polling refresh, which drops keyboard focus and collapses expanded files. Preserve component identity and diff state during background refresh.",
            "sora",
            CheckState::Success,
            ReviewState::Pending,
            MergeableState::Mergeable,
            false,
            2,
            64,
            12,
            vec!["accessibility", "frontend"],
            vec!["maya"],
            18,
        ),
        (
            "repo-0002",
            "acme/web-console",
            291,
            "Replace generated status cards with compact activity rows",
            "Implements the new activity-row layout and keeps the high-density mode on laptop widths.",
            "maya",
            CheckState::Pending,
            ReviewState::Pending,
            MergeableState::Unknown,
            true,
            9,
            244,
            76,
            vec!["design-system"],
            vec!["sora"],
            26,
        ),
        (
            "repo-0003",
            "acme/telemetry",
            176,
            "Backpressure ingestion during partition reassignment",
            "Ingestion workers continue accepting batches while partitions are moving, causing unbounded queue growth. Apply bounded admission and retry the batch after metadata convergence.",
            "jon",
            CheckState::Failure,
            ReviewState::Pending,
            MergeableState::Conflicting,
            false,
            13,
            310,
            88,
            vec!["reliability", "kafka"],
            vec!["devin"],
            78,
        ),
        (
            "repo-0003",
            "acme/telemetry",
            168,
            "Add retention policy audit events",
            "Retention changes now emit a structured audit record with actor and prior policy.",
            "sora",
            CheckState::Success,
            ReviewState::Approved,
            MergeableState::Mergeable,
            false,
            5,
            120,
            33,
            vec!["observability"],
            vec![],
            29,
        ),
    ];

    for (index, fixture) in pr_fixtures.iter().enumerate() {
        let repo_id = fixture.0;
        let repo_name = fixture.1;
        let number = fixture.2;
        let created = now - Duration::days(1 + index as i64 * 2);
        let pr = PullRequest {
            id: format!("pr-{number}"),
            repository_id: repo_id.into(),
            repository_name: repo_name.into(),
            number,
            title: fixture.3.into(),
            body: fixture.4.into(),
            author: fixture.5.into(),
            state: "open".into(),
            draft: fixture.9,
            merged: false,
            mergeable: fixture.8,
            created_at: created,
            updated_at: now - Duration::hours(index as i64 * 5 + 1),
            merged_at: None,
            base_ref: "main".into(),
            head_ref: format!("feature/pr-{number}"),
            base_sha: format!("demo-base-{index:02}"),
            head_sha: format!("demo-head-{index:02}"),
            additions: fixture.11,
            deletions: fixture.12,
            changed_files: fixture.11 / 18 + 1,
            commit_count: 3,
            review_state: fixture.7,
            check_state: fixture.6,
            labels: fixture.13.iter().map(|v| (*v).to_string()).collect(),
            assignees: fixture.14.iter().map(|v| (*v).to_string()).collect(),
            requested_reviewers: fixture.14.iter().map(|v| (*v).to_string()).collect(),
            milestone: Some("Q4 reliability".into()),
            html_url: format!("https://github.com/{repo_name}/pull/{number}"),
            risk_score: fixture.15,
            synced_at: now - Duration::minutes(4),
            origin: DataOrigin::Demo,
        };
        store.upsert_pull_request(&pr).await?;
        for (ci, name, state) in [
            ("ci-lint", "Rustfmt & Clippy", CheckState::Success),
            ("ci-unit", "Unit tests", fixture.6),
            ("ci-security", "Dependency audit", CheckState::Success),
        ] {
            store
                .upsert_check(&CheckRun {
                    id: format!("{}-{number}-{ci}", repo_id),
                    pull_request_id: Some(pr.id.clone()),
                    commit_sha: pr.head_sha.clone(),
                    name: name.into(),
                    state,
                    conclusion: Some(
                        if state == CheckState::Failure {
                            "failure"
                        } else {
                            "success"
                        }
                        .into(),
                    ),
                    details_url: Some(format!("https://github.com/{repo_name}/actions")),
                    started_at: Some(created),
                    completed_at: Some(created + Duration::minutes(9)),
                })
                .await?;
        }
        let files = fixture.files();
        store.save_pr_files(&pr.id, &files).await?;
        for (comment_index, (author, body)) in [
            (
                "jon",
                "The boundary check now uses the effective role, but we should add a regression test for a suspended member.",
            ),
            (
                "devin",
                "Please confirm the invite lookup is scoped to the workspace ID before authorization.",
            ),
        ]
        .iter()
        .enumerate()
        {
            store
                .upsert_review_comment(&ReviewComment {
                    id: format!("review-{number}-{comment_index}"),
                    pull_request_id: pr.id.clone(),
                    author: (*author).into(),
                    body: (*body).into(),
                    path: files.first().map(|f| f.path.clone()),
                    line: files
                        .first()
                        .and_then(|f| f.hunks.first())
                        .and_then(|h| h.lines.iter().find(|l| l.kind == DiffLineKind::Added))
                        .and_then(|l| l.new_line),
                    diff_hunk: files
                        .first()
                        .and_then(|f| f.hunks.first())
                        .map(|h| h.header.clone()),
                    state: Some("COMMENTED".into()),
                    submitted_at: created + Duration::hours(3 + comment_index as i64),
                    html_url: Some(format!("https://github.com/{repo_name}/pull/{number}")),
                })
                .await?;
        }
        for commit_index in 0..3 {
            store
                .upsert_commit(&Commit {
                    id: format!("commit-{number}-{commit_index}"),
                    repository_id: repo_id.into(),
                    repository_name: repo_name.into(),
                    sha: format!("{:040x}", number * 100 + commit_index),
                    message: if commit_index == 0 {
                        "Resolve effective member role before invite creation"
                    } else if commit_index == 1 {
                        "Cover suspended-member regression case"
                    } else {
                        "Clarify authorization error handling"
                    }
                    .into(),
                    author_name: fixture.5.into(),
                    author_login: Some(fixture.5.into()),
                    authored_at: created + Duration::hours(commit_index * 7),
                    committed_at: created + Duration::hours(commit_index * 7),
                    parents: if commit_index == 0 {
                        vec![format!("{:040x}", number * 100 - 1)]
                    } else {
                        vec![format!("{:040x}", number * 100 + commit_index - 1)]
                    },
                    additions: 34 + commit_index * 8,
                    deletions: 12 + commit_index * 2,
                    html_url: format!("https://github.com/{repo_name}/commit/demo"),
                    pr_number: Some(number),
                    verification: Some("verified".into()),
                    origin: DataOrigin::Demo,
                })
                .await?;
        }
    }

    let issue_fixtures = [
        (
            "repo-0001",
            "acme/control-plane",
            701,
            "Invite acceptance does not revoke suspended sessions",
            "Suspended members can retain a session after accepting a workspace invite.",
            "jon",
            vec!["security"],
            vec!["maya"],
            Some("Q4 reliability"),
        ),
        (
            "repo-0001",
            "acme/control-plane",
            714,
            "Policy revisions should expose an optimistic concurrency token",
            "Clients need a stable revision token to avoid lost updates.",
            "devin",
            vec!["api"],
            vec!["devin"],
            Some("Q4 reliability"),
        ),
        (
            "repo-0002",
            "acme/web-console",
            642,
            "Diff focus is lost after background refresh",
            "Keyboard users lose their place when polling completes.",
            "sora",
            vec!["accessibility"],
            vec!["sora"],
            Some("Editorial polish"),
        ),
        (
            "repo-0002",
            "acme/web-console",
            651,
            "High density mode clips command palette hints",
            "The shortcut column is truncated at 1280px.",
            "maya",
            vec!["design-system"],
            vec![],
            Some("Editorial polish"),
        ),
        (
            "repo-0003",
            "acme/telemetry",
            590,
            "Retention worker can double-delete archived partitions",
            "A retry after metadata convergence may schedule deletion twice.",
            "jon",
            vec!["reliability", "bug"],
            vec!["jon"],
            Some("Q4 reliability"),
        ),
        (
            "repo-0003",
            "acme/telemetry",
            604,
            "Document replay limits for incident tooling",
            "Operators need to know the maximum supported replay window.",
            "devin",
            vec!["documentation"],
            vec!["devin"],
            None,
        ),
    ];
    for (index, fixture) in issue_fixtures.iter().enumerate() {
        let created = now - Duration::days(10 + index as i64 * 3);
        let closed = (index == 5).then_some(now - Duration::days(2));
        store
            .upsert_issue(&Issue {
                id: format!("issue-{}", fixture.2),
                repository_id: fixture.0.into(),
                repository_name: fixture.1.into(),
                number: fixture.2,
                title: fixture.3.into(),
                body: fixture.4.into(),
                author: fixture.5.into(),
                state: if closed.is_some() { "closed" } else { "open" }.into(),
                created_at: created,
                updated_at: closed.unwrap_or(created + Duration::hours(8)),
                closed_at: closed,
                labels: fixture.6.iter().map(|v| (*v).to_string()).collect(),
                assignees: fixture.7.iter().map(|v| (*v).to_string()).collect(),
                milestone: fixture.8.map(str::to_string),
                comments: 2 + index as i64,
                html_url: format!("https://github.com/{}/issues/{}", fixture.1, fixture.2),
                origin: DataOrigin::Demo,
            })
            .await?;
    }

    store
        .upsert_release(&Release {
            id: "release-1".into(),
            repository_id: "repo-0001".into(),
            tag_name: "v2.8.0".into(),
            name: "Control Plane 2.8".into(),
            body: "Workspace policy revisions now support idempotency keys.\n\n### Breaking changes\nNone identified in source metadata.".into(),
            draft: false,
            prerelease: false,
            published_at: Some(now - Duration::days(6)),
            created_at: now - Duration::days(7),
            html_url: "https://github.com/acme/control-plane/releases/tag/v2.8.0".into(),
        })
        .await?;

    let knowledge = [
        (
            "architecture",
            "Tenant authorization boundary",
            "Workspace authorization resolves an effective member role before resource access. Invite creation is an administrative workspace operation and must enforce the same boundary.",
            "docs/architecture/tenancy.md",
            "README",
            true,
        ),
        (
            "convention",
            "API error contract",
            "API errors use a stable machine-readable code plus a human-readable message. Authorization failures must not leak resource existence across tenants.",
            "CONTRIBUTING.md",
            "CONTRIBUTING",
            true,
        ),
        (
            "risk",
            "Session revocation gap",
            "Issue #701 reports that suspended members may retain sessions after accepting an invite. This is a candidate security risk, not a confirmed exploit.",
            "issue:701",
            "Issue ingestion",
            false,
        ),
    ];
    for (index, (kind, title, content, source, method, explicit)) in knowledge.iter().enumerate() {
        store
            .save_knowledge(&KnowledgeRecord {
                id: format!("knowledge-{index}"),
                repository_id: Some("repo-0001".into()),
                kind: (*kind).into(),
                title: (*title).into(),
                content: (*content).into(),
                source_object: (*source).into(),
                source_version: Some("demo@2026-10".into()),
                source_url: Some(format!(
                    "https://github.com/acme/control-plane/blob/main/{source}"
                )),
                confidence: if *explicit { 0.98 } else { 0.72 },
                extraction_method: (*method).into(),
                explicit_fact: *explicit,
                superseded_by: None,
                extracted_at: now - Duration::hours(2),
                observed_at: now - Duration::hours(3),
            })
            .await?;
    }

    let notifications = [
        (
            "review_requested",
            Severity::Medium,
            "Review requested on #412",
            "Maya requested your review of tenant boundary checks.",
            "pr:412:review",
        ),
        (
            "ci_failed",
            Severity::High,
            "CI failing on #176",
            "Backpressure ingestion has failing unit tests.",
            "pr:176:ci",
        ),
        (
            "stale_pr",
            Severity::Low,
            "#291 needs attention",
            "Draft PR has been open for 9 days.",
            "pr:291:stale",
        ),
    ];
    for (kind, severity, title, body, dedupe) in notifications {
        store
            .add_notification(&Notification {
                id: Uuid::new_v4().to_string(),
                repository_id: None,
                kind: kind.into(),
                severity,
                title: title.into(),
                body: body.into(),
                created_at: now - Duration::hours(1),
                read_at: None,
                snoozed_until: None,
                dedupe_key: dedupe.into(),
                url: None,
            })
            .await?;
    }
    store.set_setting("demo_seeded", "true").await?;
    Ok(())
}

trait DemoFiles {
    fn files(&self) -> Vec<DiffFile>;
}

impl DemoFiles
    for (
        &str,
        &str,
        i64,
        &str,
        &str,
        &str,
        CheckState,
        ReviewState,
        MergeableState,
        bool,
        i64,
        i64,
        i64,
        Vec<&str>,
        Vec<&str>,
        i64,
    )
{
    fn files(&self) -> Vec<DiffFile> {
        let path = "src/auth/workspace.rs";
        let patch = format!(
            "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -41,7 +41,12 @@ impl Workspace {{\n pub fn invite(&self, actor: &User) -> Result<Invite> {{\n-    require_member(actor, &self.id)?;\n+    let role = self.effective_role(actor)?;\n+    if !role.can_administer() {{\n+        return Err(Error::Forbidden);\n+    }}\n     let invite = Invite::new(self.id.clone());\n+    self.audit_invite(actor, &invite);\n     Ok(invite)\n }}\n"
        );
        let base =
            crate::diff::parse_file_patch(path, "modified", None, &patch).unwrap_or_else(|_| {
                DiffFile {
                    path: path.into(),
                    old_path: None,
                    status: DiffStatus::Modified,
                    additions: 4,
                    deletions: 1,
                    binary: false,
                    generated: false,
                    hunks: vec![],
                    raw: Some(patch),
                }
            });
        let tests = crate::diff::parse_file_patch(
            "src/auth/workspace_tests.rs",
            "modified",
            None,
            "diff --git a/src/auth/workspace_tests.rs b/src/auth/workspace_tests.rs\n--- a/src/auth/workspace_tests.rs\n+++ b/src/auth/workspace_tests.rs\n@@ -90,3 +90,9 @@ fn existing_member_can_view() {\n }\n+\n+#[test]\n+fn suspended_member_cannot_invite() {\n+    assert_forbidden(invite_as(\"suspended\"));\n+}\n",
        )
        .unwrap_or_else(|_| DiffFile {
            path: "src/auth/workspace_tests.rs".into(),
            old_path: None,
            status: DiffStatus::Modified,
            additions: 6,
            deletions: 0,
            binary: false,
            generated: false,
            hunks: vec![],
            raw: None,
        });
        vec![base, tests]
    }
}
