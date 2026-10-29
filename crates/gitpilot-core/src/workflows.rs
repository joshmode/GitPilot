//! Event-driven automation with deterministic matching, idempotency, dry-run
//! and explicit permission gates. No automation can publish or merge unless its
//! policy grants that action and a user approved the run.

use crate::domain::*;
use crate::error::{GitPilotError, Result};
use crate::store::Store;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    PrOpened,
    PrSynchronized,
    PrReadyForReview,
    ReviewSubmitted,
    CiFailed,
    CiSucceeded,
    IssueAssigned,
    LabelAdded,
    ReleasePublished,
    ScheduledCheck,
    Manual,
}

impl Trigger {
    pub fn parse(value: &str) -> Result<Self> {
        serde_json::from_value(serde_json::Value::String(value.into()))
            .map_err(|_| GitPilotError::InvalidInput(format!("unknown trigger `{value}`")))
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PrOpened => "pr_opened",
            Self::PrSynchronized => "pr_synchronized",
            Self::PrReadyForReview => "pr_ready_for_review",
            Self::ReviewSubmitted => "review_submitted",
            Self::CiFailed => "ci_failed",
            Self::CiSucceeded => "ci_succeeded",
            Self::IssueAssigned => "issue_assigned",
            Self::LabelAdded => "label_added",
            Self::ReleasePublished => "release_published",
            Self::ScheduledCheck => "scheduled_check",
            Self::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub key: String,
    pub trigger: Trigger,
    pub repository_id: Option<String>,
    pub object_id: String,
    pub labels: Vec<String>,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    RunDeterministicReview,
    RunAiReview,
    CreateNotification,
    CreateIssue,
    AddLabel,
    PublishReviewSummary,
    PublishReviewComments,
    SubmitApproval,
    SubmitChangesRequested,
    PreparePatch,
    Noop,
}

impl Action {
    pub fn parse(value: &str) -> Result<Self> {
        serde_json::from_value(serde_json::Value::String(value.into()))
            .map_err(|_| GitPilotError::InvalidInput(format!("unknown action `{value}`")))
    }

    pub fn requires_write(&self) -> bool {
        matches!(
            self,
            Self::CreateIssue
                | Self::AddLabel
                | Self::PublishReviewSummary
                | Self::PublishReviewComments
                | Self::SubmitApproval
                | Self::SubmitChangesRequested
        )
    }

    pub fn requires_approval(&self) -> bool {
        matches!(
            self,
            Self::PublishReviewComments | Self::SubmitApproval | Self::SubmitChangesRequested
        )
    }
}

#[derive(Clone)]
pub struct WorkflowEngine {
    store: Store,
}

#[derive(Debug, Clone)]
pub struct WorkflowOutcome {
    pub automation_id: String,
    pub matched: bool,
    pub executed_actions: Vec<String>,
    pub dry_run: bool,
    pub summary: String,
}

impl WorkflowEngine {
    pub fn new(store: Store) -> Self {
        Self { store }
    }

    pub async fn evaluate(&self, event: &Event) -> Result<Vec<WorkflowOutcome>> {
        let automations = self.store.automations().await?;
        let application_policy = self.store.load_permissions().await?;
        let mut outcomes = Vec::new();
        for automation in automations {
            if !automation.enabled || Trigger::parse(&automation.trigger)? != event.trigger {
                continue;
            }
            if let Some(repo) = &automation.repository_id
                && event.repository_id.as_deref() != Some(repo)
            {
                continue;
            }
            let matched = automation
                .conditions
                .iter()
                .all(|condition| evaluate_condition(condition, event));
            if !matched {
                continue;
            }
            let event_key = format!("{}:{}", event.trigger.as_str(), event.key);
            let run_id = Uuid::new_v4().to_string();
            let actions = automation
                .actions
                .iter()
                .filter_map(|value| Action::parse(value).ok())
                .collect::<Vec<_>>();
            let mut executed = Vec::new();
            let mut summary = if automation.dry_run {
                format!("Dry run matched {} action(s).", actions.len())
            } else {
                "Matched conditions.".into()
            };
            for action in &actions {
                if action.requires_write()
                    && crate::permissions::require_scoped(
                        &application_policy,
                        &automation.policy,
                        crate::permissions::Permission::ModifyGitHub,
                    )
                    .is_err()
                {
                    summary = format!(
                        "Blocked `{:?}`: GitHub writes are not authorized by both application and workflow policy.",
                        action
                    );
                    break;
                }
                if action.requires_approval() && automation.requires_approval {
                    summary = format!("`{:?}` is prepared and awaits explicit approval.", action);
                    continue;
                }
                match action {
                    Action::RunDeterministicReview | Action::RunAiReview => {
                        summary = format!(
                            "{} queued for {}{}.",
                            if matches!(action, Action::RunAiReview) {
                                "AI review"
                            } else {
                                "Deterministic review"
                            },
                            event.object_id,
                            if automation.dry_run { " (dry run)" } else { "" }
                        );
                    }
                    Action::CreateNotification => {
                        if !automation.dry_run {
                            self.store
                                .add_notification(&Notification {
                                    id: Uuid::new_v4().to_string(),
                                    repository_id: event.repository_id.clone(),
                                    kind: "automation".into(),
                                    severity: Severity::Info,
                                    title: automation.name.clone(),
                                    body: format!("Triggered for {}", event.object_id),
                                    created_at: Utc::now(),
                                    read_at: None,
                                    snoozed_until: None,
                                    dedupe_key: format!("automation:{}:{run_id}", event_key),
                                    url: None,
                                })
                                .await?;
                        }
                    }
                    Action::Noop => {}
                    _ => {}
                }
                executed.push(format!("{action:?}"));
            }
            let run = AutomationRun {
                id: run_id,
                automation_id: automation.id.clone(),
                event_key,
                status: if automation.dry_run {
                    "dry_run".into()
                } else {
                    "completed".into()
                },
                summary: summary.clone(),
                started_at: Utc::now(),
                completed_at: Some(Utc::now()),
                error: None,
            };
            let inserted = self.store.record_automation_run(&run).await?;
            outcomes.push(WorkflowOutcome {
                automation_id: automation.id,
                matched: inserted,
                executed_actions: executed,
                dry_run: automation.dry_run,
                summary,
            });
        }
        Ok(outcomes)
    }

    pub async fn create_default(&self) -> Result<Automation> {
        let automation = Automation {
            id: Uuid::new_v4().to_string(),
            name: "Notify on failing PR checks".into(),
            enabled: false,
            trigger: Trigger::CiFailed.as_str().into(),
            conditions: vec!["repository:any".into()],
            actions: vec!["create_notification".into()],
            dry_run: true,
            requires_approval: true,
            repository_id: None,
            policy: PermissionPolicy::default(),
            budget_usd: 0.0,
            timeout_seconds: 300,
            created_at: Utc::now(),
            last_run_at: None,
        };
        self.store.save_automation(&automation).await?;
        Ok(automation)
    }
}

fn evaluate_condition(condition: &str, event: &Event) -> bool {
    let condition = condition.trim();
    if condition == "repository:any" || condition.is_empty() {
        return true;
    }
    if let Some(repo) = condition.strip_prefix("repository:") {
        return event.repository_id.as_deref() == Some(repo);
    }
    if let Some(label) = condition.strip_prefix("label:") {
        return event.labels.iter().any(|l| l == label);
    }
    if let Some(field) = condition.strip_prefix("payload.")
        && let Some((key, expected)) = field.split_once('=')
    {
        return event
            .payload
            .get(key)
            .and_then(|v| v.as_str())
            .is_some_and(|value| value == expected);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_actions_require_write_permission() {
        assert!(Action::CreateIssue.requires_write());
        assert!(!Action::CreateNotification.requires_write());
    }

    #[test]
    fn conditions_are_deterministic() {
        let event = Event {
            key: "1".into(),
            trigger: Trigger::LabelAdded,
            repository_id: Some("r1".into()),
            object_id: "p1".into(),
            labels: vec!["security".into()],
            payload: serde_json::json!({"priority":"high"}),
        };
        assert!(evaluate_condition("label:security", &event));
        assert!(evaluate_condition("payload.priority=high", &event));
        assert!(!evaluate_condition("label:docs", &event));
    }
}
