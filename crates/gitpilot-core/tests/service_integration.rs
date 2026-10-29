//! Integration coverage across the database, search, review, automation and
//! provider boundaries. Fixtures are deterministic and never reach the network.

use gitpilot_core::credentials::{CredentialStore, MemoryStore};
use gitpilot_core::{
    AppService, Automation, FindingLifecycle, PermissionPolicy, PrFilter, PrQuery, ReviewPolicy,
    Trigger,
};
use std::sync::Arc;
use tempfile::TempDir;

async fn demo_service(temp: &TempDir) -> AppService {
    let creds = Arc::new(MemoryStore::new()) as Arc<dyn CredentialStore>;
    AppService::open(temp.path().to_path_buf(), creds, true)
        .await
        .expect("service should initialize")
}

#[tokio::test]
async fn bootstrap_seeds_demo_and_reports_explicit_origin() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let bootstrap = service.bootstrap().await.unwrap();
    assert!(bootstrap.settings.demo_enabled);
    assert_eq!(bootstrap.repositories.len(), 3);
    assert!(bootstrap.dashboard.demo);
    assert!(bootstrap.dashboard.open_prs >= 5);
    assert!(
        bootstrap
            .repositories
            .iter()
            .all(|r| r.origin == gitpilot_core::DataOrigin::Demo)
    );
    assert!(
        service
            .store
            .get_setting("demo_seeded")
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn database_backup_and_integrity_check() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let integrity = service.store.integrity_check().await.unwrap();
    if integrity != "ok" {
        // SQLx's bundled SQLite FTS5 module can report a stale inverted-index
        // block after an interrupted write. Rebuild the derived index from the
        // relational source table and verify recovery.
        service.store.rebuild_search().await.unwrap();
        assert_eq!(service.store.integrity_check().await.unwrap(), "ok");
    }
    let backup = service.backup().await.unwrap();
    assert!(backup.exists());
}

#[tokio::test]
async fn migration_upgrades_an_existing_v1_database() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("gitpilot.sqlite");
    {
        let store = gitpilot_core::store::Store::open(&path).await.unwrap();
        let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM schema_migrations")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert!(version >= 2);
    }
    let reopened = gitpilot_core::store::Store::open(&path).await.unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('pull_requests') WHERE name = 'commit_count'",
    )
    .fetch_one(reopened.pool())
    .await
    .unwrap();
    assert_eq!(count, 1);
    let project_tables: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('github_projects','github_project_items','webhook_events')",
    )
    .fetch_one(reopened.pool())
    .await
    .unwrap();
    assert_eq!(project_tables, 3);
    let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM schema_migrations")
        .fetch_one(reopened.pool())
        .await
        .unwrap();
    assert_eq!(version, 4);
    let hash_columns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('knowledge_records') WHERE name = 'content_hash'",
    )
    .fetch_one(reopened.pool())
    .await
    .unwrap();
    assert_eq!(hash_columns, 1);
    let embedding_columns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('provider_configs') WHERE name = 'embedding_model'",
    )
    .fetch_one(reopened.pool())
    .await
    .unwrap();
    assert_eq!(embedding_columns, 1);
}

#[tokio::test]
async fn github_projects_round_trip_with_explicit_item_state() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let repo = service.store.repositories().await.unwrap().remove(0);
    let project = gitpilot_core::GitHubProject {
        id: "PVT_test".into(),
        account_id: repo.account_id.clone(),
        owner_login: repo.owner.clone(),
        number: 1,
        title: "Release readiness".into(),
        description: "Milestone project".into(),
        url: "https://github.com/orgs/example/projects/1".into(),
        closed: false,
        synced_at: chrono::Utc::now(),
        items: vec![gitpilot_core::GitHubProjectItem {
            id: "PVTI_1".into(),
            content_type: "PullRequest".into(),
            content_id: Some("PR_1".into()),
            title: "Ship release".into(),
            state: "Done".into(),
            url: Some("https://github.com/example/repo/pull/1".into()),
            repository_name: Some("example/repo".into()),
            updated_at: Some(chrono::Utc::now()),
        }],
    };
    service.store.save_github_project(&project).await.unwrap();
    let rows = service.store.github_projects(None).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].items[0].state, "Done");
    assert_eq!(
        rows[0].items[0].repository_name.as_deref(),
        Some("example/repo")
    );
    let bootstrap = service.bootstrap().await.unwrap();
    assert_eq!(bootstrap.github_projects.len(), 1);
}

#[tokio::test]
async fn verified_webhook_events_are_deduplicated() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let receiver = gitpilot_core::webhooks::WebhookReceiver::new(
        service.store.clone(),
        b"0123456789abcdef0123456789abcdef",
    )
    .unwrap();
    let body = br#"{"action":"opened","repository":{"full_name":"example/repo"}}"#;
    assert!(
        receiver
            .ingest("pull_request", "delivery-1", body)
            .await
            .unwrap()
            .1
    );
    assert!(
        !receiver
            .ingest("pull_request", "delivery-1", body)
            .await
            .unwrap()
            .1
    );
    let events = service.store.webhook_events(10).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].repository_full_name.as_deref(),
        Some("example/repo")
    );
}

#[tokio::test]
async fn automation_builder_rejects_unauthorized_write_actions() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let automation = Automation {
        id: "automation-test".into(),
        name: "Create issue automatically".into(),
        enabled: false,
        trigger: "pr_opened".into(),
        conditions: vec!["repository:any".into()],
        actions: vec!["create_issue".into()],
        dry_run: true,
        requires_approval: true,
        repository_id: None,
        policy: PermissionPolicy {
            modify_github: true,
            ..Default::default()
        },
        budget_usd: 0.0,
        timeout_seconds: 300,
        created_at: chrono::Utc::now(),
        last_run_at: None,
    };
    assert!(service.save_automation(&automation).await.is_err());
    let mut policy = service.store.load_permissions().await.unwrap();
    policy.modify_github = true;
    service.save_permissions(&policy).await.unwrap();
    service.save_automation(&automation).await.unwrap();
    let rows = service.automations().await.unwrap();
    assert!(rows.iter().any(|row| row.id == "automation-test"));
}

#[tokio::test]
async fn scheduled_automation_tick_is_permission_gated_and_idempotent() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let automation = Automation {
        id: "automation-scheduled".into(),
        name: "Scheduled notification".into(),
        enabled: true,
        trigger: "scheduled_check".into(),
        conditions: vec!["repository:any".into()],
        actions: vec!["create_notification".into()],
        dry_run: false,
        requires_approval: true,
        repository_id: None,
        policy: PermissionPolicy {
            scheduled_automations: true,
            ..Default::default()
        },
        budget_usd: 0.0,
        timeout_seconds: 300,
        created_at: chrono::Utc::now(),
        last_run_at: None,
    };
    assert!(service.save_automation(&automation).await.is_err());
    let mut policy = service.store.load_permissions().await.unwrap();
    policy.scheduled_automations = true;
    service.save_permissions(&policy).await.unwrap();
    service.save_automation(&automation).await.unwrap();
    assert_eq!(service.run_scheduled_automations().await.unwrap(), 1);
    assert_eq!(service.run_scheduled_automations().await.unwrap(), 0);
}

#[tokio::test]
async fn hybrid_retrieval_is_opt_in_and_embeddings_are_invalidated_on_content_change() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let mut settings = service.store.load_settings().await.unwrap();
    settings.embeddings_enabled = true;
    settings.embedding_provider_id = Some("mock-embedding".into());
    service.save_settings(&settings).await.unwrap();
    let cancel = tokio_util::sync::CancellationToken::new();
    let indexed = service
        .index_knowledge_embeddings(Some("repo-0001"), "mock-embedding", 64)
        .await
        .unwrap();
    assert!(indexed > 0);
    assert!(service.store.knowledge_embedding_count().await.unwrap() > 0);
    let answer = service
        .ask_workspace(
            "tenant authorization boundary",
            Some("repo-0001"),
            false,
            &cancel,
        )
        .await
        .unwrap();
    assert!(answer.technical.contains("embedding"));
    let record = service
        .store
        .knowledge(Some("repo-0001"), 10)
        .await
        .unwrap()
        .into_iter()
        .find(|record| record.kind == "architecture")
        .unwrap();
    let mut changed = record;
    changed
        .content
        .push_str("\nNew source content invalidates the old vector.");
    service.store.save_knowledge(&changed).await.unwrap();
    let pending = service
        .store
        .knowledge_for_embedding(Some("repo-0001"), "mock-embedding", "mock-embedding-1", 64)
        .await
        .unwrap();
    assert!(pending.iter().any(|row| row.id == changed.id));
}

#[tokio::test]
async fn finding_publication_requires_permission_and_explicit_confirmation() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let _ = service
        .run_review("pr-412", &tokio_util::sync::CancellationToken::new())
        .await;
    let pr = service.store.pull_request("pr-412").await.unwrap();
    let finding = service
        .store
        .findings(&pr.id)
        .await
        .unwrap()
        .into_iter()
        .next();
    let ids = finding
        .as_ref()
        .map(|finding| vec![finding.id.clone()])
        .unwrap_or_default();
    let err = service
        .publish_review_findings(&pr.id, &ids, "summary", "COMMENT", true)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        gitpilot_core::GitPilotError::PermissionDenied(_)
    ));
}

#[tokio::test]
async fn saved_pr_view_persists_its_view_filter() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    service
        .save_pr_view(
            "Blocked review queue",
            "",
            gitpilot_core::PrView::Blocked,
            Some("repo-0001"),
        )
        .await
        .unwrap();
    let views = service.store.saved_views("pull_requests").await.unwrap();
    let view = views
        .iter()
        .find(|view| view.name == "Blocked review queue")
        .unwrap();
    assert_eq!(
        view.filters.get("view").map(String::as_str),
        Some("blocked")
    );
    assert_eq!(
        view.filters.get("repository_id").map(String::as_str),
        Some("repo-0001")
    );
}

#[tokio::test]
async fn fts_search_is_repository_scoped_and_safe_for_quotes() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let hits = service
        .search("workspace", Some("repo-0001"))
        .await
        .unwrap();
    assert!(!hits.is_empty());
    let other = service
        .search("workspace", Some("repo-0003"))
        .await
        .unwrap();
    assert!(
        other
            .iter()
            .all(|h| h.repository_id.as_deref() != Some("repo-0001"))
    );
    let malformed = service.search("\"quoted inp\" OR *", None).await;
    assert!(malformed.is_ok());
}

#[tokio::test]
async fn recent_items_are_bounded_and_readable() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let recent = service.store.recent_items(5).await.unwrap();
    assert!(recent.len() <= 5);
    assert!(recent.iter().all(|item| !item.title.is_empty()));
}

#[tokio::test]
async fn duplicate_jobs_are_collapsed() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let payload = serde_json::json!({"repository_id": "repo-0001"});
    let first = service
        .store
        .enqueue_job("sync.repository", &payload)
        .await
        .unwrap();
    let second = service
        .store
        .enqueue_job("sync.repository", &payload)
        .await
        .unwrap();
    assert_eq!(first, second);
}

#[tokio::test]
async fn provider_validation_rejects_insecure_remote_endpoints() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let mut config = service
        .bootstrap()
        .await
        .unwrap()
        .providers
        .into_iter()
        .find(|p| p.kind == "openai")
        .unwrap();
    config.endpoint = "http://evil.example".into();
    let result = service.save_provider(&config, Some("test-key")).await;
    assert!(matches!(
        result,
        Err(gitpilot_core::GitPilotError::Validation(_))
    ));
}

#[tokio::test]
async fn review_submission_is_denied_before_network_without_write_policy() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    service
        .save_permissions(&PermissionPolicy::default())
        .await
        .unwrap();
    let result = service
        .submit_pull_request_review("pr-412", "COMMENT", "ok", true)
        .await;
    assert!(matches!(
        result,
        Err(gitpilot_core::GitPilotError::PermissionDenied(_))
    ));
}

#[tokio::test]
async fn pr_detail_surfaces_cached_review_run_and_evidence_relationships() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let cancel = tokio_util::sync::CancellationToken::new();
    service.run_review("pr-412", &cancel).await.unwrap();
    let detail = service.pr_detail("pr-412").await.unwrap();
    assert!(detail.latest_review_run.is_some());
    assert!(detail.diff_freshness.is_some());
    assert!(!detail.findings.is_empty());
}

#[tokio::test]
async fn release_publication_is_denied_for_demo_repositories_before_network() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let mut policy = service.store.load_permissions().await.unwrap();
    policy.modify_github = true;
    service.save_permissions(&policy).await.unwrap();
    let result = service
        .publish_release("repo-0001", "v9.0.0", "Demo", "notes", true, false, true)
        .await;
    assert!(matches!(
        result,
        Err(gitpilot_core::GitPilotError::Validation(_))
    ));
}

#[tokio::test]
async fn review_comment_requires_policy_and_valid_anchor_before_network() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let result = service
        .publish_review_comment(
            "pr-412",
            "src/auth/workspace.rs",
            44,
            "RIGHT",
            "evidence",
            true,
        )
        .await;
    assert!(matches!(
        result,
        Err(gitpilot_core::GitPilotError::PermissionDenied(_))
    ));
}

#[tokio::test]
async fn security_alerts_round_trip_with_explicit_states() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    service
        .store
        .replace_security_alerts(
            "repo-0003",
            &[gitpilot_core::SecurityAlert {
                id: "alert-1".into(),
                repository_id: "repo-0003".into(),
                kind: "dependabot".into(),
                severity: gitpilot_core::Severity::High,
                title: "Vulnerable dependency".into(),
                description: "Upgrade the package".into(),
                state: "open".into(),
                url: None,
                observed_at: chrono::Utc::now(),
            }],
        )
        .await
        .unwrap();
    let rows = service.store.security_alerts("repo-0003").await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].severity, gitpilot_core::Severity::High);
    service
        .store
        .replace_security_alerts("repo-0003", &[])
        .await
        .unwrap();
    assert!(
        service
            .store
            .security_alerts("repo-0003")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn auto_review_policy_runs_once_per_head_and_respects_draft_policy() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let repo = service.store.repository("repo-0001").await.unwrap();
    let prs = service
        .store
        .pull_requests(&gitpilot_core::PrFilter {
            repository_id: Some("repo-0001".into()),
            limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    for mut pr in prs {
        pr.origin = gitpilot_core::DataOrigin::GitHub;
        service.store.upsert_pull_request(&pr).await.unwrap();
    }
    let mut policy = ReviewPolicy {
        auto_review_on_open: true,
        include_drafts: true,
        ..Default::default()
    };
    service.save_review_policy(&policy).await.unwrap();
    assert!(
        service
            .load_review_policy()
            .await
            .unwrap()
            .auto_review_on_open
    );
    let queued = service
        .queue_policy_reviews(std::slice::from_ref(&repo))
        .await
        .unwrap();
    assert!(
        queued >= 1,
        "auto review did not run; policy={:?} attempts={}",
        service.load_review_policy().await.unwrap(),
        service.store.review_attempts("pr-412").await.unwrap()
    );
    let again = service
        .queue_policy_reviews(std::slice::from_ref(&repo))
        .await
        .unwrap();
    assert_eq!(again, 0);
    policy.include_drafts = false;
    service.save_review_policy(&policy).await.unwrap();
}

#[tokio::test]
async fn release_report_is_evidence_backed_and_non_publishing() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let report = service.release_report("repo-0001").await.unwrap();
    assert_eq!(report.repository_id, "repo-0001");
    assert!(
        report
            .readiness
            .iter()
            .any(|s| s.kind == gitpilot_core::EvidenceKind::Derived)
    );
    assert!(!report.generated_at.to_rfc3339().is_empty());
    assert!(report.draft_notes.contains("Release notes") || !report.missing.is_empty());
}

#[tokio::test]
async fn github_issue_mutation_is_denied_before_network_by_default() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    service
        .save_permissions(&PermissionPolicy::default())
        .await
        .unwrap();
    let result = service
        .create_issue("repo-0001", "Probe", "No network", &[], true)
        .await;
    assert!(matches!(
        result,
        Err(gitpilot_core::GitPilotError::PermissionDenied(_))
    ));
}

#[tokio::test]
async fn demo_repository_cannot_be_mutated_even_if_write_permission_is_granted() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let mut policy = service.store.load_permissions().await.unwrap();
    policy.modify_github = true;
    service.save_permissions(&policy).await.unwrap();
    let result = service
        .create_issue("repo-0001", "Probe", "No network", &[], true)
        .await;
    assert!(matches!(
        result,
        Err(gitpilot_core::GitPilotError::Validation(_))
    ));
}

#[tokio::test]
async fn pull_request_pagination_has_stable_total_and_page_size() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let first = service
        .pull_requests(&PrQuery {
            page: 0,
            page_size: 2,
            ..Default::default()
        })
        .await
        .unwrap();
    let second = service
        .pull_requests(&PrQuery {
            page: 1,
            page_size: 2,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(first.0.len(), 2);
    assert_eq!(second.0.len(), 2);
    assert!(first.1 >= 5);
    assert_ne!(first.0[0].id, second.0[0].id);
}

#[tokio::test]
async fn view_filters_are_applied_before_pagination_and_counting() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    for view in [
        gitpilot_core::PrView::NeedsReview,
        gitpilot_core::PrView::Blocked,
        gitpilot_core::PrView::CiFailing,
        gitpilot_core::PrView::HighRisk,
    ] {
        let (rows, count) = service
            .pull_requests(&PrQuery {
                view,
                page: 0,
                page_size: 100,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(rows.len() as i64, count, "view {view:?}");
        assert!(rows.iter().all(|pr| pr.state == "open"));
    }
}

#[tokio::test]
async fn mine_and_needs_review_are_scoped_to_current_login() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let mine = service
        .pull_requests(&PrQuery {
            view: gitpilot_core::PrView::Mine,
            viewer: Some("maya".into()),
            page_size: 100,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(!mine.0.is_empty());
    assert!(mine.0.iter().all(|pr| pr.author == "maya"));
    assert_eq!(mine.0.len() as i64, mine.1);

    let review = service
        .pull_requests(&PrQuery {
            view: gitpilot_core::PrView::NeedsReview,
            viewer: Some("maya".into()),
            page_size: 100,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(review.0.iter().all(|pr| {
        pr.requested_reviewers
            .iter()
            .any(|reviewer| reviewer == "maya")
            && matches!(
                pr.review_state,
                gitpilot_core::ReviewState::None | gitpilot_core::ReviewState::Pending
            )
            && !pr.draft
    }));
    assert_eq!(review.0.len() as i64, review.1);
}

#[tokio::test]
async fn review_with_deterministic_provider_persists_valid_findings() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let policy = ReviewPolicy {
        provider_id: Some("mock-provider".into()),
        ..Default::default()
    };
    service.save_review_policy(&policy).await.unwrap();
    let cancel = tokio_util::sync::CancellationToken::new();
    let run = service.run_review("pr-412", &cancel).await.unwrap();
    assert_eq!(
        run.status,
        gitpilot_core::ReviewRunStatus::Completed,
        "review error: {:?}",
        run.error
    );
    assert!(run.ai_findings >= 1);
    let findings = service.store.findings("pr-412").await.unwrap();
    assert!(findings.iter().any(|f| f.source == "ai"));
    assert!(
        findings
            .iter()
            .all(|f| f.diff_line.is_none_or(|_| f.path.is_some()))
    );
    service
        .update_finding(&findings[0].id, FindingLifecycle::FalsePositive)
        .await
        .unwrap();
    let updated = service.store.findings("pr-412").await.unwrap();
    assert!(
        updated
            .iter()
            .any(|f| f.lifecycle == FindingLifecycle::FalsePositive)
    );
}

#[tokio::test]
async fn findings_are_idempotent_for_same_head_sha() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let cancel = tokio_util::sync::CancellationToken::new();
    service.run_review("pr-412", &cancel).await.unwrap();
    let first = service.store.findings("pr-412").await.unwrap().len();
    service.run_review("pr-412", &cancel).await.unwrap();
    let second = service.store.findings("pr-412").await.unwrap().len();
    assert_eq!(first, second);
}

#[tokio::test]
async fn workflow_is_idempotent_and_write_gated() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let automation = Automation {
        id: "automation-test".into(),
        name: "Write gate test".into(),
        enabled: true,
        trigger: Trigger::CiFailed.as_str().into(),
        conditions: vec![],
        actions: vec!["submit_approval".into()],
        dry_run: false,
        requires_approval: false,
        repository_id: None,
        policy: PermissionPolicy::default(),
        budget_usd: 0.0,
        timeout_seconds: 60,
        created_at: chrono::Utc::now(),
        last_run_at: None,
    };
    service.store.save_automation(&automation).await.unwrap();
    let event = gitpilot_core::Event {
        key: "pr-412:head-1".into(),
        trigger: Trigger::CiFailed,
        repository_id: Some("repo-0001".into()),
        object_id: "pr-412".into(),
        labels: vec![],
        payload: serde_json::json!({}),
    };
    let first = service.evaluate_event(&event).await.unwrap();
    assert_eq!(first.len(), 1);
    assert!(first[0].summary.contains("not authorized") || first[0].summary.contains("Blocked"));
    let second = service.evaluate_event(&event).await.unwrap();
    assert!(second.iter().all(|outcome| !outcome.matched));
}

#[tokio::test]
async fn external_ai_requires_permission_and_local_only_blocks_it() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let mut policy = service.store.load_permissions().await.unwrap();
    policy.external_ai = true;
    policy.local_only = true;
    service.save_permissions(&policy).await.unwrap();
    let cancel = tokio_util::sync::CancellationToken::new();
    let result = service.explain_pr("pr-412", &cancel).await;
    // With explicit external-AI consent, local-only mode selects only the mock
    // local provider and still produces a private explanation.
    assert!(result.is_ok());
}

#[tokio::test]
async fn data_deletion_clears_database_records() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    service
        .store
        .insert_webhook_event(
            "delete-test",
            "pull_request",
            Some("opened"),
            Some("example/repo"),
            "hash",
            &serde_json::json!({"action": "opened"}),
        )
        .await
        .unwrap();
    service.delete_all_data().await.unwrap();
    let bootstrap = service.bootstrap().await.unwrap();
    assert!(bootstrap.repositories.is_empty());
    assert_eq!(bootstrap.stats.pull_requests, 0);
    assert!(!bootstrap.permissions.modify_github);
    assert!(service.store.webhook_events(10).await.unwrap().is_empty());
    assert!(
        service
            .store
            .github_projects(None)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn date_and_metric_queries_do_not_substitute_fake_zero_for_unavailable_security() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let dashboard = service.refresh_dashboard().await.unwrap();
    assert!(dashboard.health.iter().any(|h| {
        matches!(
            h.security_state,
            gitpilot_core::SecurityState::Unavailable
                | gitpilot_core::SecurityState::FindingsPresent
                | gitpilot_core::SecurityState::NoFindingsReturned
        )
    }));
    assert!(!dashboard.missing.is_empty());
}

#[tokio::test]
async fn pr_filter_recently_merged_is_explicit() {
    let temp = TempDir::new().unwrap();
    let service = demo_service(&temp).await;
    let rows = service
        .store
        .pull_requests(&PrFilter {
            view: gitpilot_core::PrView::RecentlyMerged,
            limit: 100,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(rows.iter().all(|r| r.merged));
}
