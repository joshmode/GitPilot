use crate::components;
use crate::state::{UiEvent, UiState};
use crate::theme::Tokens;
use eframe::egui::{self, Align, Layout, RichText};
use gitpilot_core::AppService;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;

pub fn show(
    ui: &mut egui::Ui,
    state: &mut UiState,
    tokens: &Tokens,
    service: &Arc<AppService>,
    runtime: &tokio::runtime::Handle,
    tx: &UnboundedSender<UiEvent>,
) {
    if state.automations.is_empty() {
        let service = Arc::clone(service);
        let tx = tx.clone();
        runtime.spawn(async move {
            let (automations, runs) =
                tokio::join!(service.automations(), service.automation_runs());
            if let Ok(automations) = automations {
                let _ = tx.send(UiEvent::Automations(automations));
            }
            if let Ok(runs) = runs {
                let _ = tx.send(UiEvent::AutomationRuns(runs));
            }
        });
    }
    if state.webhook_events.is_empty() {
        let service = Arc::clone(service);
        let tx = tx.clone();
        runtime.spawn(async move {
            if let Ok(events) = service.store.webhook_events(30).await {
                let _ = tx.send(UiEvent::WebhookEvents(events));
            }
        });
    }
    components::page_header(
        ui,
        tokens,
        "Automations",
        "Event-driven workflows with dry-run, idempotency and explicit permission gates",
        |ui| {
            if ui.button("Create starter automation").clicked() {
                let service = Arc::clone(service);
                let tx = tx.clone();
                runtime.spawn(async move {
                    match service.create_default_automation().await {
                        Ok(_) => {
                            let _ = tx.send(UiEvent::BootstrapRefreshed);
                        }
                        Err(error) => {
                            let _ = tx.send(UiEvent::Error(error.user_message()));
                        }
                    }
                });
            }
        },
    );
    components::panel(tokens).show(ui, |ui| {
        ui.label(
            RichText::new("Automation builder")
                .size(13.0)
                .strong()
                .color(tokens.text),
        );
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.add(
                egui::TextEdit::singleline(&mut state.automation_name)
                    .desired_width(220.0)
                    .hint_text("Automation name"),
            );
            ui.label("Trigger");
            egui::ComboBox::from_id_salt("automation_trigger")
                .selected_text(&state.automation_trigger)
                .show_ui(ui, |ui| {
                    for trigger in [
                        "pr_opened",
                        "pr_synchronized",
                        "pr_ready_for_review",
                        "review_submitted",
                        "ci_failed",
                        "ci_succeeded",
                        "issue_assigned",
                        "label_added",
                        "release_published",
                        "scheduled_check",
                        "manual",
                    ] {
                        ui.selectable_value(
                            &mut state.automation_trigger,
                            trigger.into(),
                            trigger,
                        );
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label("Conditions");
            ui.add(
                egui::TextEdit::singleline(&mut state.automation_conditions)
                    .desired_width(300.0)
                    .hint_text("repository:any,label:security"),
            );
            ui.label("Actions");
            ui.add(
                egui::TextEdit::singleline(&mut state.automation_actions)
                    .desired_width(300.0)
                    .hint_text("create_notification,run_ai_review"),
            );
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut state.automation_enabled, "Enabled");
            ui.checkbox(&mut state.automation_dry_run, "Dry run");
            if ui.button("Save automation").clicked() {
                let automation = gitpilot_core::Automation {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: state.automation_name.trim().into(),
                    enabled: state.automation_enabled,
                    trigger: state.automation_trigger.clone(),
                    conditions: split_csv(&state.automation_conditions),
                    actions: split_csv(&state.automation_actions),
                    dry_run: state.automation_dry_run,
                    requires_approval: true,
                    repository_id: state.repository_scope.clone(),
                    policy: state.bootstrap.permissions.clone(),
                    budget_usd: 0.0,
                    timeout_seconds: 300,
                    created_at: chrono::Utc::now(),
                    last_run_at: None,
                };
                let service = Arc::clone(service);
                let tx = tx.clone();
                runtime.spawn(async move {
                    match service.save_automation(&automation).await {
                        Ok(()) => {
                            let _ = tx.send(UiEvent::Toast("Automation saved.".into()));
                            let _ = tx.send(UiEvent::BootstrapRefreshed);
                        }
                        Err(error) => {
                            let _ = tx.send(UiEvent::Error(error.user_message()));
                        }
                    }
                });
            }
        });
        ui.label(
            RichText::new(
                "Write actions remain blocked unless both the application and automation policies allow them. Approval-required actions are prepared but never executed automatically.",
            )
            .size(10.0)
            .color(tokens.text_faint),
        );
    });
    ui.add_space(8.0);
    components::inset(tokens).show(ui, |ui| {
        ui.label(
            RichText::new(
                "Triggers: PR opened/synchronized/ready, review submitted, CI failed/passed, issue assigned, label added, release published, schedule, manual. Polling is always available. The optional standalone `gitpilot-webhook` receiver verifies GitHub HMAC signatures and persists deduplicated deliveries.",
            )
            .size(10.5)
            .color(tokens.text_muted),
        );
        if ui
            .button("Process verified webhook deliveries")
            .on_hover_text("Enqueue local workflow evaluation for unprocessed verified events")
            .clicked()
        {
            let service = Arc::clone(service);
            let tx = tx.clone();
            runtime.spawn(async move {
                match service.store.webhook_events(100).await {
                    Ok(events) => {
                        for event in events
                            .into_iter()
                            .filter(|event| event.processed_at.is_none())
                            .take(20)
                        {
                            if let Err(error) = service
                                .enqueue_webhook_processing(&event.delivery_id)
                                .await
                            {
                                tracing::warn!(%error, delivery_id = %event.delivery_id, "webhook enqueue failed");
                            }
                        }
                        let _ = tx.send(UiEvent::Toast(
                            "Verified webhook deliveries queued for local workflow evaluation."
                                .into(),
                        ));
                    }
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.user_message()));
                    }
                }
            });
        }
        if ui
            .button("Run scheduled checks now")
            .on_hover_text("Runs at most one scheduled event per automation per UTC hour")
            .clicked()
        {
            let service = Arc::clone(service);
            let tx = tx.clone();
            runtime.spawn(async move {
                match service.run_scheduled_automations().await {
                    Ok(count) => {
                        let _ = tx.send(UiEvent::Toast(format!(
                            "Scheduled check evaluated {count} automation run(s)."
                        )));
                        let _ = tx.send(UiEvent::BootstrapRefreshed);
                    }
                    Err(error) => {
                        let _ = tx.send(UiEvent::Error(error.user_message()));
                    }
                }
            });
        }
    });
    ui.add_space(8.0);
    if state.automations.is_empty() {
        components::empty_state(
            ui,
            tokens,
            "No automations",
            "Create a starter automation to see the typed condition/action model. Writes stay blocked unless separately authorized.",
            None,
        );
    }
    for automation in &state.automations {
        components::panel(tokens).show(ui, |ui| {
            ui.horizontal(|ui| {
                components::status_dot(
                    ui,
                    if automation.enabled {
                        tokens.success
                    } else {
                        tokens.text_faint
                    },
                );
                ui.label(
                    RichText::new(&automation.name)
                        .size(13.0)
                        .strong()
                        .color(tokens.text),
                );
                components::badge(
                    ui,
                    tokens,
                    if automation.dry_run {
                        "DRY RUN"
                    } else {
                        "ENABLED"
                    },
                    if automation.dry_run {
                        tokens.info
                    } else {
                        tokens.warning
                    },
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("trigger: {}", automation.trigger))
                            .size(10.0)
                            .color(tokens.text_faint),
                    );
                });
            });
            ui.horizontal_wrapped(|ui| {
                for action in &automation.actions {
                    components::badge(ui, tokens, action, tokens.text_muted);
                }
                if automation.requires_approval {
                    components::badge(ui, tokens, "approval required", tokens.warning);
                }
                ui.label(
                    RichText::new(format!(
                        "budget ${:.2} · timeout {}s",
                        automation.budget_usd, automation.timeout_seconds
                    ))
                    .size(10.0)
                    .color(tokens.text_faint),
                );
            });
        });
        ui.add_space(5.0);
    }
    components::section_header(
        ui,
        tokens,
        "Run history",
        Some("Unique event keys prevent duplicate executions"),
    );
    if state.automation_runs.is_empty() {
        ui.label(
            RichText::new("No automation runs have been recorded.")
                .size(11.0_f32)
                .color(tokens.text_muted),
        );
    }
    for run in state.automation_runs.iter().take(20) {
        ui.horizontal(|ui| {
            components::badge(ui, tokens, &run.status, tokens.info);
            ui.label(
                RichText::new(&run.summary)
                    .size(10.5)
                    .color(tokens.text_muted),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(components::short_time(run.started_at))
                        .size(10.0)
                        .color(tokens.text_faint),
                );
            });
        });
    }
    components::section_header(
        ui,
        tokens,
        "Verified webhook deliveries",
        Some("Only events with a valid X-Hub-Signature-256 enter this table"),
    );
    if state.webhook_events.is_empty() {
        ui.label(
            RichText::new(
                "No verified webhook deliveries are cached. Polling remains the fallback and no public listener runs inside the desktop app.",
            )
            .size(10.5)
            .color(tokens.text_faint),
        );
    }
    for event in state.webhook_events.iter().take(20) {
        ui.horizontal(|ui| {
            components::badge(ui, tokens, &event.event, tokens.info);
            if let Some(action) = &event.action {
                components::badge(ui, tokens, action, tokens.text_muted);
            }
            ui.label(
                RichText::new(
                    event
                        .repository_full_name
                        .as_deref()
                        .unwrap_or("repository unavailable"),
                )
                .size(10.5)
                .color(tokens.text),
            );
            if let Some(processed) = event.processed_at {
                components::badge(
                    ui,
                    tokens,
                    &format!("processed {}", components::short_time(processed)),
                    tokens.success,
                );
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(components::short_time(event.received_at))
                        .size(10.0)
                        .color(tokens.text_faint),
                );
            });
        });
    }
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}
